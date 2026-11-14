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
    /// Research: blend channels of a unit (by 16-digit hash), before and after a GLB round trip.
    Blend { hash: String },
    /// Checks the repack path: every unit to GLB, back through the rebuilder, and parsed again.
    Selftest,
    /// Starts a mod project from game resources (models and textures picked like extract).
    Start {
        #[command(flatten)]
        pick: Pick,
        dir: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        /// Add new resources named <prefix><file name> instead of replacing the game's, e.g. "mymod/".
        #[arg(long)]
        new_prefix: Option<String>,
    },
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
            let mut seen = std::collections::HashSet::new();
            // One copy of each resource: the same name in several bundles would write the same file.
            let picked: Vec<_> = ar.assets.iter().filter(|a| f.matches(&names, a) && seen.insert((a.type_hash, a.name_hash))).collect();
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
        Cmd::Blend { hash } => blend_dbg(&ar, &names, u64::from_str_radix(&hash, 16)?)?,
        Cmd::Selftest => selftest(&ar)?,
        Cmd::Start { pick, dir, id, name, new_prefix } => {
            let f = Filter::new(&pick.query()?)?;
            let mut seen = std::collections::HashSet::new();
            let picked: Vec<_> = ar.assets.iter().filter(|a| f.matches(&names, a) && seen.insert((a.type_hash, a.name_hash))).collect();
            let linker = minediver::links::Linker::new(&ar);
            let ex = Exporter::new(&ar, &names, &linker);
            let p = minediver::pack::start(&ex, &picked, &dir, &id, &name, new_prefix.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&p)?);
        }
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
                let skinned = u.geometries.iter().filter(|g| g.channels.iter().any(|c| c.semantic == 6)).count();
                match minediver::unit::skins(&d, u.rest_at) {
                    Ok((s, after)) => {
                        *layouts.entry(format!("SKINS {} for {} skinned geometries", s.len(), skinned)).or_default() += 1;
                        let bone_hashes: Vec<u32> = ar.find(hash::hash("bones"), a.name_hash).and_then(|b| ar.read_variant(b, 0).ok()).map(|(bd, _)| minediver::unit::bone_names(&bd).into_keys().collect()).unwrap_or_default();
                        match minediver::unit::find_graph(&d, after, &s, &bone_hashes) {
                            Some(nodes) => {
                                let in_range = s.iter().all(|k| k.nodes.iter().all(|&n| (n as usize) < nodes.len()));
                                let bones_ok = ar.find(hash::hash("bones"), a.name_hash).and_then(|b| ar.read_variant(b, 0).ok()).map(|(bd, _)| {
                                    let names = minediver::unit::bone_names(&bd);
                                    names.keys().all(|h| nodes.iter().any(|n| n.name == *h))
                                });
                                *layouts.entry(format!("GRAPH skin nodes in range {in_range}, bones match {bones_ok:?}")).or_default() += 1;
                            }
                            None => *errs.entry(format!("no scene graph {:016x} bones {}", a.name_hash, bone_hashes.len())).or_default() += 1,
                        }
                    }
                    Err(e) => *errs.entry(format!("skins: {e:#}")).or_default() += 1,
                }
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
    let names = hash::NameDb::builtin();
    let linker = minediver::links::Linker::new(ar);
    let ex = Exporter::new(ar, &names, &linker);
    let units: Vec<_> = ar.assets.iter().filter(|a| a.type_hash == th && a.patch.is_none()).collect();
    let results: Vec<Result<bool>> = units
        .par_iter()
        .map(|a| -> Result<bool> {
            let (d, _) = ar.read_variant(a, 0)?;
            let u = minediver::unit::parse(&d)?;
            if u.geometries.is_empty() {
                return Ok(false);
            }
            let rig = ex.rig(a, &u, &d);
            let glb = minediver::gltf::unit_to_glb_rigged(&u, "t", &[], &Default::default(), rig.as_ref())?;
            let meshes = minediver::glb_read::read(&glb)?;
            let mut notes = Vec::new();
            let bone_names = rig.as_ref().map(|r| r.names.clone()).unwrap_or_default();
            let back = minediver::pack::rebuild_unit(&d, &meshes, &bone_names, &mut notes)?;
            let u2 = minediver::unit::parse(&back)?;
            anyhow::ensure!(u2.geometries.len() == u.geometries.len(), "geometry count");
            let (skins, _) = minediver::unit::skins(&d, u.rest_at)?;
            let mut k = 0;
            for (g, g2) in u.geometries.iter().zip(&u2.geometries) {
                anyhow::ensure!(g.index_count == g2.index_count, "index count {} -> {}", g.index_count, g2.index_count);
                let skin = if g.channels.iter().any(|c| c.semantic == 6) { k += 1; skins.get(k - 1) } else { None };
                // Every triangle as its corners (position, strongest skin bone), in any order: the same set
                // of triangles must come back, wherever the vertices and batches went.
                // Keyed by corner positions in millimetres, so rounding never reorders the pairs.
                let keyed = |t: Vec<[(i64, i64, i64, i64); 3]>| {
                    let mut m: std::collections::HashMap<[(i64, i64, i64); 3], Vec<[i64; 3]>> = Default::default();
                    for c in t {
                        m.entry(c.map(|x| (x.0 / 10, x.1 / 10, x.2 / 10))).or_default().push(c.map(|x| x.3));
                    }
                    m.values_mut().for_each(|v| v.sort());
                    m
                };
                let (t1, t2) = (keyed(triangles(g, skin)), keyed(triangles(g2, skin)));
                let total: usize = t1.values().map(Vec::len).sum();
                // Triangles whose key differs: match them again with 0.2 mm tolerance before calling them moved.
                let near = |a: &[(i64, i64, i64); 3], b: &[(i64, i64, i64); 3]| a.iter().zip(b.iter()).all(|(x, y)| (x.0 - y.0).abs() <= 1 && (x.1 - y.1).abs() <= 1 && (x.2 - y.2).abs() <= 1);
                let left2: Vec<&[(i64, i64, i64); 3]> = t2.keys().filter(|k| !t1.contains_key(*k)).collect();
                let missing: usize = t1.iter().filter(|(k, _)| !t2.contains_key(*k) && !left2.iter().any(|o| near(k, o))).map(|(_, v)| v.len()).sum();
                let differ: usize = t1.iter().filter(|(k, v)| t2.get(*k).is_some_and(|w| w != *v)).map(|(_, v)| v.len()).sum();
                // A corner on a millimetre edge can land in the next key; a tie between equal weights may
                // pick either bone. Both stay rare.
                anyhow::ensure!(missing * 100 <= total, "{missing} of {total} triangles moved");
                anyhow::ensure!(differ * 100 <= total, "strongest bone changed on {differ} of {total} triangles");
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
    println!("units round-tripped through GLB: {ok} ok, {skinned} skinned or empty (skipped), {bad} failed");
    Ok(())
}

/// Each triangle as three corners: position in 1e-4 steps and the skin bone the corner leans on most
/// (-1 without a skin). The batch of the triangle picks the bone set.
fn triangles(g: &minediver::unit::Geometry, skin: Option<&minediver::unit::Skin>) -> Vec<[(i64, i64, i64, i64); 3]> {
    let (p, _) = minediver::gltf::channel(g, 0, 0).unwrap_or_default();
    let blend = minediver::gltf::channel(g, 6, 0).zip(minediver::gltf::channel(g, 7, 0));
    let idx = minediver::gltf::indices(g);
    let mut set_of_tri = vec![0usize; idx.len() / 3];
    for b in &g.batches {
        for t in b.start..b.start + b.triangles {
            if let Some(s) = set_of_tri.get_mut(t as usize) {
                *s = b.extra as usize;
            }
        }
    }
    idx.chunks_exact(3)
        .enumerate()
        .map(|(ti, t)| {
            [0, 1, 2].map(|c| {
                let v = t[c] as usize;
                let q = |k: usize| (p[v * 3 + k] * 1e4).round() as i64;
                let bone = match (skin, &blend) {
                    (Some(s), Some(((bi, n), (bw, m)))) => {
                        let top = (0..(*n).min(*m)).map(|k| bw[v * m + k]).fold(0f32, f32::max);
                        (0..(*n).min(*m))
                            .filter(|&k| bw[v * m + k] >= top - 1e-3)
                            .map(|k| s.sets.get(set_of_tri[ti]).and_then(|set| set.get(bi[v * n + k].round() as usize)).and_then(|b| s.nodes.get(*b as usize)).map_or(-2, |node| *node as i64))
                            .min()
                            .unwrap_or(-2)
                    }
                    _ => -1,
                };
                (q(0), q(1), q(2), bone)
            })
        })
        .collect()
}

fn blend_dbg(ar: &Archive, names: &hash::NameDb, h: u64) -> Result<()> {
    let a = ar.find(hash::hash("unit"), h).context("no such unit")?;
    let (d, _) = ar.read_variant(a, 0)?;
    let u = minediver::unit::parse(&d)?;
    let linker = minediver::links::Linker::new(ar);
    let ex = Exporter::new(ar, names, &linker);
    let rig = ex.rig(a, &u, &d);
    let (skins, _) = minediver::unit::skins(&d, u.rest_at)?;
    for s in &skins {
        println!("skin nodes {:?} sets {:?}", s.nodes, s.sets);
    }
    let glb = minediver::gltf::unit_to_glb_rigged(&u, "t", &[], &Default::default(), rig.as_ref())?;
    let meshes = minediver::glb_read::read(&glb)?;
    let mut notes = Vec::new();
    let back = minediver::pack::rebuild_unit(&d, &meshes, &rig.as_ref().map(|r| r.names.clone()).unwrap_or_default(), &mut notes)?;
    let u2 = minediver::unit::parse(&back)?;
    println!("notes {notes:?}");
    for (g, g2) in u.geometries.iter().zip(&u2.geometries) {
        let show = |g: &minediver::unit::Geometry| {
            let (b, n) = minediver::gltf::channel(g, 6, 0).unwrap_or_default();
            let (w, m) = minediver::gltf::channel(g, 7, 0).unwrap_or_default();
            let (p, _) = minediver::gltf::channel(g, 0, 0).unwrap_or_default();
            for v in 0..6.min(g.vertex_count as usize) {
                println!("  pos {:?} idx {:?} w {:?}", &p[v * 3..v * 3 + 3], &b.get(v * n..v * n + n), &w.get(v * m..v * m + m));
            }
        };
        {
            let (p1, _) = minediver::gltf::channel(g, 0, 0).unwrap_or_default();
            let (p2, _) = minediver::gltf::channel(g2, 0, 0).unwrap_or_default();
            let mut worst = 0f32;
            for b in p2.chunks(3) {
                let best = p1.chunks(3).map(|a| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()).fold(f32::MAX, f32::min);
                worst = worst.max(best);
            }
            println!("worst distance of a rebuilt vertex to an original one: {worst}");
        }
        println!("original batches {:?}", g.batches.iter().map(|b| (b.material, b.triangles, b.extra)).collect::<Vec<_>>());
        show(g);
        println!("rebuilt batches {:?}", g2.batches.iter().map(|b| (b.material, b.triangles, b.extra)).collect::<Vec<_>>());
        show(g2);
    }
    for m in &meshes {
        for p in &m.prims {
            println!("glb {} joints {:?} first {:?} {:?}", m.name, p.joint_names, p.joints.first(), p.weights.first());
        }
    }
    Ok(())
}
