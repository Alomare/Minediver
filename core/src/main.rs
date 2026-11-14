use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use minediver::{archive::Archive, hash};

#[derive(Parser)]
#[command(name = "minediver-cli", version, about = "Helldivers (2015) asset extractor")]
struct Cli {
    /// Game folder or its contents folder. Found through Steam when left out.
    #[arg(long, global = true)]
    game: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Counts resources by type.
    Census,
    /// Dumps raw resources of one type into a folder, for research.
    Dump { ty: String, out: PathBuf, #[arg(long, default_value_t = 20)] limit: usize, #[arg(long)] name: Option<String> },
    /// Finds resource names in the game files and writes them as a name list.
    Harvest { out: PathBuf },
    /// Parses every unit and reports what layouts appear.
    UnitStats,
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
    eprintln!("indexed {} resources in {:?}, {} errors", ar.assets.len(), t.elapsed(), ar.errors.len());
    for (b, e) in &ar.errors {
        eprintln!("  {b}: {e}");
    }
    let names = hash::NameDb::builtin();
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
        Cmd::UnitStats => unit_stats(&ar)?,
        Cmd::Harvest { out } => {
            let t = std::time::Instant::now();
            let found = minediver::harvest::harvest(&ar, &|_, _| {})?;
            let text: String = found.iter().map(|(h, n)| format!("{h:016x} {n}\n")).collect();
            std::fs::write(&out, text)?;
            let fresh = hash::NameDb::builtin();
            let before = ar.assets.iter().filter(|a| fresh.contains(a.name_hash)).count();
            fresh.add_list(&std::fs::read_to_string(&out)?);
            let after = ar.assets.iter().filter(|a| fresh.contains(a.name_hash)).count();
            println!("{} names in {:?}; resources named {} -> {} of {}", found.len(), t.elapsed(), before, after, ar.assets.len());
        }
        Cmd::Dump { ty, out, limit, name } => {
            std::fs::create_dir_all(&out)?;
            let th = u64::from_str_radix(&ty, 16).ok().filter(|_| ty.len() == 16).unwrap_or_else(|| hash::hash(&ty));
            for a in ar.assets.iter().filter(|a| a.type_hash == th && a.patch.is_none() && name.as_ref().is_none_or(|n| names.get(a.name_hash).is_some_and(|x| x.contains(n.as_str())))).take(limit) {
                let (d, s) = ar.read_variant(a, 0)?;
                let base = names.get(a.name_hash).map(|n| n.replace('/', "__")).unwrap_or_else(|| hash::hex(a.name_hash));
                std::fs::write(out.join(format!("{base}.{ty}")), &d)?;
                if !s.is_empty() {
                    std::fs::write(out.join(format!("{base}.{ty}.stream")), &s)?;
                }
            }
        }
    }
    Ok(())
}

fn unit_stats(ar: &Archive) -> Result<()> {
    use std::collections::BTreeMap;
    let th = hash::hash("unit");
    let mut ok = 0;
    let mut errs: BTreeMap<String, usize> = BTreeMap::new();
    let mut layouts: BTreeMap<String, usize> = BTreeMap::new();
    let mut misc: BTreeMap<String, usize> = BTreeMap::new();
    for a in ar.assets.iter().filter(|a| a.type_hash == th && a.patch.is_none()) {
        let (d, _) = ar.read_variant(a, 0)?;
        match minediver::unit::parse(&d) {
            Ok(u) => {
                ok += 1;
                for g in &u.geometries {
                    let l: Vec<String> = g.channels.iter().map(|c| format!("{}:{}:{}:{}:{}", c.semantic, c.format, c.set, c.extra, c.flag)).collect();
                    *layouts.entry(format!("stride {} [{}]", g.stride, l.join(" "))).or_default() += 1;
                    *misc.entry(format!("idx {:?} head {:?}", g.index_format, g.head)).or_default() += 1;
                    *misc.entry(format!("ibytes/count {}", if g.index_count > 0 { g.indices.len() as u32 / g.index_count } else { 0 })).or_default() += 1;
                    for _m in &g.materials {
                    }
                    if g.batches.len() > 2 && misc.len() < 400 { *misc.entry(format!("multi {:?} idx {} mats {:?}", g.batches.iter().map(|b| (b.material, b.start, b.triangles, b.extra)).collect::<Vec<_>>(), g.index_count, g.materials)).or_default() += 1; }
                    for b in &g.batches {
                        *misc.entry(format!("batch extra {}", b.extra)).or_default() += 1;
                    }
                }
            }
            Err(e) => {
                let k = format!("{e:#}");
                let k: String = k.chars().filter(|c| !c.is_ascii_digit()).collect();
                *errs.entry(k).or_default() += 1;
            }
        }
    }
    println!("ok {ok}");
    for (k, v) in errs { println!("ERR {v:5} {k}"); }
    for (k, v) in layouts { println!("LAYOUT {v:5} {k}"); }
    for (k, v) in misc { println!("MISC {v:5} {k}"); }
    Ok(())
}
