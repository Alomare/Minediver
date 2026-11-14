//! Which resources a resource refers to. Units name their materials, and materials their textures, by
//! 64-bit hash somewhere in their bytes; looking for every known hash of the right type finds them
//! without parsing the whole layout.

use std::collections::{HashMap, HashSet};

use crate::archive::Archive;
use crate::hash;

pub struct Linker {
    by_type: HashMap<u64, HashSet<u64>>,
}

/// A hash found in a resource, with the 4 bytes in front of it (often the slot or parameter id).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ref {
    pub type_hash: u64,
    pub name_hash: u64,
    pub at: usize,
    pub before: u32,
}

impl Linker {
    pub fn new(ar: &Archive) -> Linker {
        let mut by_type: HashMap<u64, HashSet<u64>> = HashMap::new();
        for a in &ar.assets {
            by_type.entry(a.type_hash).or_default().insert(a.name_hash);
        }
        Linker { by_type }
    }

    /// Every hash in `data` that names a resource of type `ty`, in order, at any byte offset.
    pub fn find(&self, data: &[u8], ty: &str) -> Vec<Ref> {
        let th = hash::hash(ty);
        let Some(set) = self.by_type.get(&th) else { return Vec::new() };
        let mut out = Vec::new();
        if data.len() < 8 {
            return out;
        }
        let mut i = 0;
        while i + 8 <= data.len() {
            let h = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
            if h != 0 && set.contains(&h) {
                let before = if i >= 4 { u32::from_le_bytes(data[i - 4..i].try_into().unwrap()) } else { 0 };
                out.push(Ref { type_hash: th, name_hash: h, at: i, before });
                i += 8;
            } else {
                i += 1;
            }
        }
        out
    }
}


/// The top 32 bits of the name hash: how materials and units name slots and parameters.
pub fn hash32(name: &str) -> u32 {
    (hash::hash(name) >> 32) as u32
}

/// Texture parameter names known so far, by their 32-bit hash.
pub fn param_name(h: u32) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "diffuse_map", "normal_map", "specular_map", "gloss_map", "emissive_map", "mask_map", "detail_map", "detail_normal_map",
        "color_map", "glow_map", "illumination_map", "damage_map", "dirt_map", "env_map", "cube_map", "reflection_map",
        "height_map", "opacity_map", "decal_map", "tint_map", "ao_map", "lightmap", "bump_map", "nm_map", "spec_map",
        "diffuse_texture", "normal_texture", "base_map", "albedo_map", "texture_map", "noise_map", "distortion_map",
        "flow_map", "fire_map", "ramp_map", "lookup_map", "color_lookup", "specular_gloss_map", "normal_specular_map",
    ];
    // Seen in the game files: the second map of most weapon materials is a normal map.
    if h == 0xa3e48458 {
        return Some("normal_map");
    }
    NAMES.iter().copied().find(|n| hash32(n) == h)
}

/// One texture a material slot uses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SlotTexture {
    pub slot: u32,
    pub param: u32,
    pub param_name: Option<&'static str>,
    pub texture: u64,
}

/// The textures of each slot of a material. A slot block ends with the slot's 32-bit name hash, two
/// words, a zero pair, a count, then (parameter hash, texture hash) pairs.
pub fn material_textures(linker: &Linker, data: &[u8]) -> Vec<SlotTexture> {
    let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    let refs = linker.find(data, "texture");
    let mut out = Vec::new();
    let mut slot = 0u32;
    let mut list_end = 0usize;
    for r in refs {
        if r.at < 8 {
            continue;
        }
        if r.at >= list_end {
            // A new list: the count sits before the first pair, the slot hash 16 bytes before that.
            let count_at = r.at - 8;
            let n = word(count_at) as usize;
            if n == 0 || n > 64 || count_at < 16 {
                continue;
            }
            slot = word(count_at - 16);
            list_end = count_at + 4 + n * 12;
        }
        out.push(SlotTexture { slot, param: r.before, param_name: param_name(r.before), texture: r.name_hash });
    }
    out
}
