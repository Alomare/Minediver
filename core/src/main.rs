use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use minediver::archive::Archive;
use minediver::export::{self, Exporter, Options};
use minediver::filter::{Filter, Query};
use minediver::hash;
use rayon::prelude::*;

#[derive(Parser)]
#[command(name = "minediver-cli", version, about = "Reads, converts and repacks Helldivers (2015) resources")]
struct Cli {
    /// Game folder or its contents folder. Found through Steam when left out.
    #[arg(long, global = true)]
    game: Option<PathBuf>,
    /// Extra name list to load (`<hash> <name>` or bare names, one per line).
    #[arg(long, global = true)]
    names: Vec<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct Pick {
    /// Words that must appear in the name, type or hash.
    #[arg(long, default_value = "")]
    text: String,
    /// model, texture, audio, video, script, text, material, animation, level, other
    #[arg(long = "kind")]
    kinds: Vec<String>,
    /// Resource type, such as unit or texture.
    #[arg(long = "type")]
    types: Vec<String>,
    /// Glob over names, such as "equipment/wep_*".
    #[arg(long)]
    include: Vec<String>,
    #[arg(long)]
    exclude: Vec<String>,
    #[arg(long)]
    named_only: bool,
}

impl Pick {
    fn query(&self) -> Result<Query> {
        let kinds = self.kinds.iter().map(|k| serde_json::from_value(serde_json::Value::String(k.clone())).with_context(|| format!("unknown kind {k}"))).collect::<Result<_>>()?;
        Ok(Query { text: self.text.clone(), kinds, types: self.types.clone(), include: self.include.clone(), exclude: self.exclude.clone(), named_only: self.named_only, ..Default::default() })
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Counts resources by type.
    Census,
    /// Lists resources.
    List {
        #[command(flatten)]
        pick: Pick,
        #[arg(long)]
        json: bool,
    },
    /// Converts resources into files under a folder.
    Extract {
        #[command(flatten)]
        pick: Pick,
        #[arg(long, short)]
        out: PathBuf,
        /// png, dds or both
        #[arg(long, default_value = "png")]
        texture: String,
        /// native, wav, ogg or mp3
        #[arg(long, default_value = "native")]
        audio: String,
        /// mp4 or bik
        #[arg(long, default_value = "mp4")]
        video: String,
        /// glb or raw
        #[arg(long, default_value = "glb")]
        model: String,
        /// Also write the raw resource bytes.
        #[arg(long)]
        raw: bool,
        #[arg(long)]
        all_variants: bool,
        #[arg(long)]
        ffmpeg: Option<PathBuf>,
    },
    /// Shows what Minediver knows about one resource (by name or 16-digit hash).
    Info { name: String, #[arg(long, default_value = "unit")] r#type: String },
    /// Finds resource names in the game files and writes them as a name list.
    Harvest { out: PathBuf },
    /// Parses every unit and reports what layouts appear.
    UnitStats,
    /// Checks the repack path: every unit to GLB, back through the rebuilder, and parsed again.
    Selftest,
    /// Builds an Obsidian mod folder from a Minediver mod project (mod.json).
    Pack { project: PathBuf, #[arg(long, short)] out: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let game = match cli.game {
        Some(g) => g,
        None => minediver::archive::detect_game().context("game not found; pass --game")?,
    };
    let contents = minediver::archive::resolve_contents(&game).context("no bundles in that folder")?;
    let t = std::time::Instant::now();
    let ar = Archive::scan(&contents, &|_, _| {})?;
    eprintln!("indexed {} resources in {:.2?}, {} unreadable bundles", ar.assets.len(), t.elapsed(), ar.errors.len());
    for (b, e) in &ar.errors {
        eprintln!("  {b}: {e}");
    }
    let names = hash::NameDb::builtin();
    for n in &cli.names {
        names.add_file(n)?;
    }
    match cli.cmd {
        Cmd::Census => {
            let mut by: BTreeMap<String, (usize, usize, u64, u64, usize)> = BTreeMap::new();
            for a in ar.assets.iter().filter(|a| a.patch.is_none()) {
                let e = by.entry(hash::type_label(a.type_hash)).or_default();
                e.0 += 1;
                e.1 += names.contains(a.name_hash) as usize;
                e.2 += a.variants.iter().map(|v| v.size as u64).sum::<u64>();
                e.3 += a.variants.iter().map(|v| v.stream_size as u64).sum::<u64>();
                e.4 += (a.variants.len() > 1) as usize;
            }
            println!("{:28} {:>7} {:>7} {:>12} {:>12} {:>6}", "type", "count", "named", "bytes", "stream", "multi");
            for (t, (c, n, b, s, m)) in by {
                println!("{t:28} {c:7} {n:7} {b:12} {s:12} {m:6}");
            }
        }
        Cmd::List { pick, json } => {
            let f = Filter::new(&pick.query()?)?;
            for a in ar.assets.iter().filter(|a| f.matches(&names, a)) {
                if json {
                    println!("{}", serde_json::json!({ "name": export::display_name(&names, a), "type": hash::type_label(a.type_hash), "bundle": a.file_name(), "size": a.size(), "variants": a.variants.len() }));
                } else {
                    println!("{:20} {:10} {} {}", hash::type_label(a.type_hash), a.size(), a.file_name(), export::display_name(&names, a));
                }
            }
        }
        Cmd::Extract { pick, out, texture, audio, video, model, raw, all_variants, ffmpeg } => {
            let f = Filter::new(&pick.query()?)?;
            let picked: Vec<_> = ar.assets.iter().filter(|a| f.matches(&names, a)).collect();
            let linker = minediver::links::Linker::new(&ar);
            let ex = Exporter::new(&ar, &names, &linker);
            let o = Options { texture, audio, video, model, raw, all_variants, ffmpeg, ..Default::default() };
            let t = std::time::Instant::now();
            let results: Vec<_> = picked.par_iter().map(|a| (a, ex.export(a, &out, &o))).collect();
            let mut files = 0;
            let mut failed = 0;
            for (a, r) in results {
                match r {
                    Ok(w) => files += w.len(),
                    Err(e) => {
                        failed += 1;
                        eprintln!("{} {}: {e:#}", hash::type_label(a.type_hash), export::display_name(&names, a));
                    }
                }
            }
            println!("{} resources, {files} files written, {failed} failed, {:.2?}", picked.len(), t.elapsed());
        }
        Cmd::Info { name, r#type } => {
            let nh = u64::from_str_radix(&name, 16).ok().filter(|_| name.len() == 16).unwrap_or_else(|| hash::hash(&name));
            let th = hash::hash(&r#type);
            let a = ar.find(th, nh).context("no such resource")?;
            let linker = minediver::links::Linker::new(&ar);
            let ex = Exporter::new(&ar, &names, &linker);
            println!("{}", serde_json::to_string_pretty(&minediver::inspect::inspect(&ex, a)?)?);
        }
        Cmd::Harvest { out } => {
            let t = std::time::Instant::now();
            let found = minediver::harvest::harvest(&ar, &|_, _| {})?;
            let text: String = found.iter().map(|(h, n)| format!("{h:016x} {n}\n")).collect();
            std::fs::write(&out, text)?;
            let before = ar.assets.iter().filter(|a| names.contains(a.name_hash)).count();
            names.add_list(&std::fs::read_to_string(&out)?);
            let after = ar.assets.iter().filter(|a| names.contains(a.name_hash)).count();
            println!("{} names in {:.2?}; resources named {before} -> {after} of {}", found.len(), t.elapsed(), ar.assets.len());
        }
        Cmd::UnitStats => unit_stats(&ar)?,
        Cmd::Selftest => selftest(&ar)?,
        Cmd::Pack { project, out } => {
            let r = minediver::pack::build(&ar, &names, &project, &out)?;
            println!("{}", serde_json::to_string_pretty(&r)?);
        }
    }
    Ok(())
}

fn unit_stats(ar: &Archive) -> Result<()> {
    let th = hash::hash("unit");
    let mut ok = 0;
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let mut layouts: BTreeMap<String, usize> = BTreeMap::new();
    for a in ar.assets.iter().filter(|a| a.type_hash == th && a.patch.is_none()) {
        let (d, _) = ar.read_variant(a, 0)?;
        match minediver::unit::parse(&d) {
            Ok(u) => {
                ok += 1;
                if minediver::unit::write(&u, &d) != d { *errs.entry("write differs from original".into()).or_default() += 1; }
                for g in &u.geometries {
                    let l: Vec<String> = g.channels.iter().map(|c| format!("{}:{}:{}", c.semantic, c.format, c.set)).collect();
                    *layouts.entry(format!("stride {} [{}]", g.stride, l.join(" "))).or_default() += 1;
                }
            }
            Err(e) => *errs.entry(format!("{e:#}")).or_default() += 1,
        }
    }
    println!("ok {ok}");
    for (k, v) in errs {
        println!("ERR {v:5} {k}");
    }
    for (k, v) in layouts {
        println!("LAYOUT {v:5} {k}");
    }
    Ok(())
}

fn selftest(ar: &Archive) -> Result<()> {
    let th = hash::hash("unit");
    let units: Vec<_> = ar.assets.iter().filter(|a| a.type_hash == th && a.patch.is_none()).collect();
    let results: Vec<Result<bool>> = units
        .par_iter()
        .map(|a| -> Result<bool> {
            let (d, _) = ar.read_variant(a, 0)?;
            let u = minediver::unit::parse(&d)?;
            if u.geometries.iter().any(|g| g.channels.iter().any(|c| c.semantic == 6)) {
                return Ok(false);
            }
            let glb = minediver::gltf::unit_to_glb(&u, "t", &[], &Default::default())?;
            let meshes = minediver::glb_read::read(&glb)?;
            let mut notes = Vec::new();
            let back = minediver::pack::rebuild_unit(&d, &meshes, &mut notes)?;
            let u2 = minediver::unit::parse(&back)?;
            anyhow::ensure!(u2.geometries.len() == u.geometries.len(), "geometry count");
            for (g, g2) in u.geometries.iter().zip(&u2.geometries) {
                anyhow::ensure!(g.index_count == g2.index_count, "index count {} -> {}", g.index_count, g2.index_count);
                for k in 0..3 {
                    anyhow::ensure!((g.min[k] - g2.min[k]).abs() < 1e-4 && (g.max[k] - g2.max[k]).abs() < 1e-4, "bounds moved");
                }
                let (p1, _) = minediver::gltf::channel(g, 0, 0).unwrap();
                let (p2, _) = minediver::gltf::channel(g2, 0, 0).unwrap();
                let i1 = minediver::gltf::indices(g);
                let i2 = minediver::gltf::indices(g2);
                // Same triangles in the same order (by corner positions), wherever the vertices went.
                for (a, b) in i1.iter().zip(&i2) {
                    for k in 0..3 {
                        anyhow::ensure!((p1[*a as usize * 3 + k] - p2[*b as usize * 3 + k]).abs() < 1e-5, "triangle corner moved");
                    }
                }
            }
            Ok(true)
        })
        .collect();
    let (mut ok, mut skinned, mut bad) = (0, 0, 0);
    for (a, r) in units.iter().zip(results) {
        match r {
            Ok(true) => ok += 1,
            Ok(false) => skinned += 1,
            Err(e) => {
                bad += 1;
                if bad <= 10 {
                    eprintln!("{:016x}: {e:#}", a.name_hash);
                }
            }
        }
    }
    println!("units round-tripped through GLB: {ok} ok, {skinned} skinned (skipped), {bad} failed");
    Ok(())
}
