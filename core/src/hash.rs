//! Resource name hashing (MurmurHash64A, seed 0) and the name database.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

const M: u64 = 0xc6a4a7935bd1e995;
const R: u32 = 47;

/// MurmurHash64A with seed 0, the hash Bitsquid uses for resource names and types.
pub fn murmur64(data: &[u8]) -> u64 {
    let mut h = (data.len() as u64).wrapping_mul(M);
    let mut blocks = data.chunks_exact(8);
    for block in &mut blocks {
        let mut k = u64::from_le_bytes(block.try_into().unwrap());
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h ^= k;
        h = h.wrapping_mul(M);
    }
    let tail = blocks.remainder();
    if !tail.is_empty() {
        let mut buf = [0u8; 8];
        buf[..tail.len()].copy_from_slice(tail);
        h ^= u64::from_le_bytes(buf);
        h = h.wrapping_mul(M);
    }
    h ^= h >> R;
    h = h.wrapping_mul(M);
    h ^= h >> R;
    h
}

pub fn hash(name: &str) -> u64 {
    murmur64(name.as_bytes())
}

/// Bitsquid resource types. Types not listed here show as a hash.
pub const TYPE_NAMES: &[&str] = &[
    "animation", "animation_curves", "apb", "bik", "bones", "config", "crypto", "data", "entity", "flow",
    "font", "ivf", "level", "lua", "material", "mod", "mouse_cursor", "navdata", "network_config", "package",
    "particles", "physics_properties", "render_config", "rt_pipeline", "scene", "script_flow_nodes", "shader",
    "shader_library", "shader_library_group", "shading_environment", "shading_environment_mapping", "slug",
    "slug_album", "sound_environment", "state_machine", "strings", "surface_properties", "texture",
    "timpani_bank", "timpani_master", "tome", "unit", "vector_field", "wav", "wwise_bank", "wwise_dep",
    "wwise_metadata", "wwise_stream", "ini", "mesh", "texture_atlas",
];

pub fn type_name(h: u64) -> Option<&'static str> {
    static MAP: std::sync::OnceLock<HashMap<u64, &'static str>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| TYPE_NAMES.iter().map(|t| (hash(t), *t)).collect()).get(&h).copied()
}

pub fn type_label(h: u64) -> String {
    type_name(h).map_or_else(|| hex(h), str::to_string)
}

pub fn hex(h: u64) -> String {
    format!("{h:016x}")
}

/// Names found in the game files by `minediver-cli harvest`, one `<hash> <name>` per line.
const BUILTIN: &str = include_str!("../data/names.txt");

/// Every known name, by hash. Starts with the built-in list; imports add to it.
pub struct NameDb {
    map: RwLock<HashMap<u64, String>>,
}

impl NameDb {
    pub fn builtin() -> NameDb {
        let db = NameDb { map: RwLock::new(HashMap::new()) };
        db.add_list(BUILTIN);
        db
    }

    /// Adds a list. Each line is `<16 hex> <name>` or a bare name, which is hashed here.
    /// Returns how many lines named a hash not known before.
    pub fn add_list(&self, text: &str) -> usize {
        let mut map = self.map.write().unwrap();
        let mut added = 0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (h, name) = match line.split_once(' ') {
                Some((h, n)) if h.len() == 16 => match u64::from_str_radix(h, 16) {
                    Ok(h) => (h, n.trim()),
                    Err(_) => (hash(line), line),
                },
                _ => (hash(line), line),
            };
            if map.insert(h, name.to_string()).is_none() {
                added += 1;
            }
        }
        added
    }

    pub fn add_file(&self, path: &Path) -> std::io::Result<usize> {
        Ok(self.add_list(&std::fs::read_to_string(path)?))
    }

    pub fn get(&self, h: u64) -> Option<String> {
        self.map.read().unwrap().get(&h).cloned()
    }

    pub fn contains(&self, h: u64) -> bool {
        self.map.read().unwrap().contains_key(&h)
    }

    pub fn len(&self) -> usize {
        self.map.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The whole list, sorted by name, as `add_list` reads it.
    pub fn export(&self) -> String {
        let map = self.map.read().unwrap();
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by(|a, b| a.1.cmp(b.1));
        rows.iter().map(|(h, n)| format!("{h:016x} {n}\n")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_the_game() {
        assert_eq!(hash("lua"), 0xa14e8dfa2cd117e2);
        assert_eq!(hash("resource_packages/boot"), 0x9e13b2414b41b842);
        assert_eq!(hash("lua/boot/boot"), 0x37048a618abddc81);
        assert_eq!(type_name(hash("texture")), Some("texture"));
    }

    #[test]
    fn lists_take_hashed_and_bare_lines() {
        let db = NameDb { map: RwLock::new(HashMap::new()) };
        assert_eq!(db.add_list("a14e8dfa2cd117e2 lua\nlua/boot/boot\n# comment\n"), 2);
        assert_eq!(db.get(0x37048a618abddc81).as_deref(), Some("lua/boot/boot"));
        assert_eq!(db.add_list("lua"), 0);
    }
}
