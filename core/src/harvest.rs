//! Finds resource names inside the game's own files. Scripts, levels and units mention many paths as
//! text; any piece of such text that hashes to a resource or bundle in the archive is a real name.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rayon::prelude::*;

use crate::archive::Archive;
use crate::hash::{self, murmur64};

fn is_path_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'/' || b == b'.' || b == b'-'
}

/// Runs of path-like text that hold at least one `/` or `_`.
fn runs(data: &[u8], out: &mut Vec<Vec<u8>>) {
    let mut i = 0;
    while i < data.len() {
        if !is_path_byte(data[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < data.len() && is_path_byte(data[i]) {
            i += 1;
        }
        let run = &data[start..i];
        if run.len() >= 4 && run.len() <= 240 && run.iter().any(|&b| b == b'/' || b == b'_') {
            out.push(run.to_vec());
        }
    }
}

/// Every piece of `run` that hashes to one of `wanted`. Text often sits right after a length byte that
/// happens to be printable, so every start is tried, not only the first.
fn matches(run: &[u8], wanted: &HashSet<u64>, found: &mut HashMap<u64, String>) {
    for s in 0..run.len() {
        if !run[s].is_ascii_lowercase() && !run[s].is_ascii_digit() && run[s] != b'_' {
            continue;
        }
        for e in (s + 2)..=run.len() {
            let piece = &run[s..e];
            let h = murmur64(piece);
            if wanted.contains(&h) {
                found.entry(h).or_insert_with(|| String::from_utf8_lossy(piece).into_owned());
            }
        }
    }
}

/// Types whose bytes are pictures, sound or shaders, where text is noise.
fn skip_type(t: u64) -> bool {
    ["texture", "timpani_bank", "shader_library", "bik", "animation"].iter().any(|n| hash::hash(n) == t)
}

/// Names for resources of `ar`, as `(hash, name)`. Reads every text-bearing resource once.
pub fn harvest(ar: &Archive, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<(u64, String)>> {
    let mut wanted: HashSet<u64> = ar.assets.iter().map(|a| a.name_hash).collect();
    wanted.extend(ar.assets.iter().map(|a| a.bundle));
    let mut by_bundle: HashMap<String, Vec<&crate::archive::Asset>> = HashMap::new();
    for a in ar.assets.iter().filter(|a| a.patch.is_none() && !skip_type(a.type_hash)) {
        by_bundle.entry(a.file_name()).or_default().push(a);
    }
    let total = by_bundle.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let parts: Vec<HashMap<u64, String>> = by_bundle
        .par_iter()
        .map(|(file, assets)| {
            let mut found = HashMap::new();
            if let Ok(mut r) = crate::bundle::Reader::open(&ar.contents.join(file)) {
                let mut text = Vec::new();
                for a in assets {
                    for v in &a.variants {
                        if let Ok(d) = r.read(v.at, v.size as usize) {
                            text.clear();
                            runs(&d, &mut text);
                            for run in &text {
                                matches(run, &wanted, &mut found);
                            }
                        }
                    }
                }
            }
            progress(done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1, total);
            found
        })
        .collect();
    let mut all: HashMap<u64, String> = HashMap::new();
    for p in parts {
        for (h, n) in p {
            // Prefer the shortest text when two runs give the same hash (they can't differ in practice).
            all.entry(h).and_modify(|o| if n.len() < o.len() { *o = n.clone() }).or_insert(n);
        }
    }
    let mut v: Vec<_> = all.into_iter().collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_behind_a_printable_length_byte_is_found() {
        let mut runs_out = Vec::new();
        runs(b"\x00\x006units/weapons/rifle_01\x00", &mut runs_out);
        let wanted: HashSet<u64> = [hash::hash("units/weapons/rifle_01")].into();
        let mut found = HashMap::new();
        matches(&runs_out[0], &wanted, &mut found);
        assert_eq!(found.values().next().map(String::as_str), Some("units/weapons/rifle_01"));
    }
}
