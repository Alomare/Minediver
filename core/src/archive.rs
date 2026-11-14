//! The game's `contents` folder: every bundle, every resource, found once and kept as a flat list.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Serialize;

use crate::bundle::{self, VariantHead};
use crate::hash;

#[derive(Debug, Clone, Serialize)]
pub struct Asset {
    pub id: u32,
    /// The bundle file name (16 hex digits) as a number.
    pub bundle: u64,
    /// `Some(n)` for a `.patch_n` file, which a mod manager wrote.
    pub patch: Option<u16>,
    pub type_hash: u64,
    pub name_hash: u64,
    pub flags: u32,
    pub variants: Vec<VariantHead>,
}

impl Asset {
    pub fn size(&self) -> u64 {
        self.variants.iter().map(|v| v.size as u64 + v.stream_size as u64).sum()
    }
    pub fn file_name(&self) -> String {
        bundle_file_name(self.bundle, self.patch)
    }
}

pub fn bundle_file_name(bundle: u64, patch: Option<u16>) -> String {
    match patch {
        Some(n) => format!("{bundle:016x}.patch_{n}"),
        None => format!("{bundle:016x}"),
    }
}

pub struct Archive {
    pub contents: PathBuf,
    pub assets: Vec<Asset>,
    /// Bundles that could not be read, with why.
    pub errors: Vec<(String, String)>,
}

/// A bundle file name: 16 hex digits, maybe with `.patch_<n>`.
fn parse_bundle_name(name: &str) -> Option<(u64, Option<u16>)> {
    let (base, patch) = match name.split_once(".patch_") {
        Some((b, n)) => (b, Some(n.parse().ok()?)),
        None => (name, None),
    };
    if base.len() != 16 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((u64::from_str_radix(base, 16).ok()?, patch))
}

/// Finds `contents` under a game folder, or takes the folder itself when it holds bundles.
pub fn resolve_contents(dir: &Path) -> Option<PathBuf> {
    for cand in [dir.join("contents"), dir.to_path_buf(), dir.join("data")] {
        let Ok(rd) = std::fs::read_dir(&cand) else { continue };
        let has = rd.flatten().any(|e| e.file_name().to_str().and_then(parse_bundle_name).is_some());
        if has {
            return Some(cand);
        }
    }
    None
}

/// Steam library folders where HD1 may be installed.
pub fn detect_game() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for drive in ["C", "D", "E", "F", "G", "X"] {
            roots.push(PathBuf::from(format!("{drive}:\\Program Files (x86)\\Steam")));
            roots.push(PathBuf::from(format!("{drive}:\\Program Files\\Steam")));
            roots.push(PathBuf::from(format!("{drive}:\\Steam")));
            roots.push(PathBuf::from(format!("{drive}:\\SteamLibrary")));
        }
    } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    // Extra libraries listed by Steam itself.
    let mut extra = Vec::new();
    for r in &roots {
        if let Ok(vdf) = std::fs::read_to_string(r.join("steamapps").join("libraryfolders.vdf")) {
            for line in vdf.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("\"path\"") {
                    let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
                    extra.push(PathBuf::from(p));
                }
            }
        }
    }
    roots.extend(extra);
    roots.into_iter().map(|r| r.join("steamapps").join("common").join("Helldivers")).find(|g| resolve_contents(g).is_some())
}

impl Archive {
    /// Reads the index of every bundle in `contents`, in parallel.
    pub fn scan(contents: &Path, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Archive> {
        let mut files: Vec<(PathBuf, u64, Option<u16>)> = std::fs::read_dir(contents)
            .with_context(|| format!("read {}", contents.display()))?
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                let (b, p) = parse_bundle_name(&name)?;
                Some((e.path(), b, p))
            })
            .collect();
        files.sort_by_key(|f| (f.1, f.2));
        let total = files.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let results: Vec<_> = files
            .par_iter()
            .map(|(path, b, p)| {
                let r = bundle::Reader::open(path).and_then(|mut r| r.heads());
                let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                progress(n, total);
                (*b, *p, r)
            })
            .collect();
        let mut assets = Vec::new();
        let mut errors = Vec::new();
        for (b, p, r) in results {
            match r {
                Ok(heads) => {
                    for h in heads {
                        assets.push(Asset {
                            id: assets.len() as u32,
                            bundle: b,
                            patch: p,
                            type_hash: h.type_hash,
                            name_hash: h.name_hash,
                            flags: h.flags,
                            variants: h.variants,
                        })
                    }
                }
                Err(e) => errors.push((bundle_file_name(b, p), format!("{e:#}"))),
            }
        }
        Ok(Archive { contents: contents.to_path_buf(), assets, errors })
    }

    pub fn bundle_path(&self, a: &Asset) -> PathBuf {
        self.contents.join(a.file_name())
    }

    /// The bytes of one variant: the bundle part and the stream part.
    pub fn read_variant(&self, a: &Asset, index: usize) -> Result<(Vec<u8>, Vec<u8>)> {
        let v = a.variants.get(index).context("no such variant")?;
        let path = self.bundle_path(a);
        let mut r = bundle::Reader::open(&path)?;
        let data = r.read(v.at, v.size as usize)?;
        let stream = bundle::read_stream(&path, v.stream_at, v.stream_size)?;
        Ok((data, stream))
    }

    /// The first vanilla copy of a resource, by type and name.
    pub fn find(&self, type_hash: u64, name_hash: u64) -> Option<&Asset> {
        self.assets.iter().filter(|a| a.type_hash == type_hash && a.name_hash == name_hash).min_by_key(|a| a.patch.is_some())
    }

    pub fn find_named(&self, ty: &str, name: &str) -> Option<&Asset> {
        self.find(hash::hash(ty), hash::hash(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_names() {
        assert_eq!(parse_bundle_name("9e13b2414b41b842"), Some((0x9e13b2414b41b842, None)));
        assert_eq!(parse_bundle_name("9e13b2414b41b842.patch_2"), Some((0x9e13b2414b41b842, Some(2))));
        assert_eq!(parse_bundle_name("9e13b2414b41b842.stream"), None);
        assert_eq!(parse_bundle_name("boot"), None);
    }
}
