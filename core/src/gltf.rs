//! Writes units as binary glTF (GLB), which Blender imports as is.
//!
//! Bitsquid is Z-up with Y forward; glTF is Y-up, so (x, y, z) becomes (x, z, -y). Blender turns it back
//! on import, so the model lands in Blender the way the game has it.

use anyhow::Result;
use serde_json::{Value, json};

use crate::unit::{Geometry, Unit, format_size};

/// An image to embed, used as the base colour (or other map) of a material.
pub struct Image {
    pub name: String,
    pub png: Vec<u8>,
}

/// What each material slot shows, by slot name.
#[derive(Default)]
pub struct MaterialMaps {
    pub slots: std::collections::HashMap<String, SlotMaps>,
}

#[derive(Default, Clone)]
pub struct SlotMaps {
    pub base: Option<usize>,
    pub normal: Option<usize>,
}

struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn align(&mut self) {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
    }

    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align();
        let mut v = json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(v);
        self.views.len() - 1
    }

    fn floats(&mut self, data: &[f32], comps: usize, minmax: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962));
        let ty = ["SCALAR", "VEC2", "VEC3", "VEC4"][comps - 1];
        let mut a = json!({ "bufferView": view, "componentType": 5126, "count": data.len() / comps, "type": ty });
        if minmax && !data.is_empty() {
            let mut lo = vec![f32::MAX; comps];
            let mut hi = vec![f32::MIN; comps];
            for c in data.chunks(comps) {
                for i in 0..comps {
                    lo[i] = lo[i].min(c[i]);
                    hi[i] = hi[i].max(c[i]);
                }
            }
            a["min"] = json!(lo);
            a["max"] = json!(hi);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn indices(&mut self, idx: &[u32]) -> usize {
        let wide = idx.iter().any(|&i| i > 0xffff);
        let bytes: Vec<u8> = if wide { idx.iter().flat_map(|i| i.to_le_bytes()).collect() } else { idx.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect() };
        let view = self.view(&bytes, Some(34963));
        self.accessors.push(json!({ "bufferView": view, "componentType": if wide { 5125 } else { 5123 }, "count": idx.len(), "type": "SCALAR" }));
        self.accessors.len() - 1
    }
}

/// One channel of a geometry as floats, `comps` per vertex.
pub fn channel(g: &Geometry, semantic: u32, set: u32) -> Option<(Vec<f32>, usize)> {
    let mut off = 0usize;
    for c in &g.channels {
        let size = format_size(c.format)? as usize;
        if c.semantic == semantic && c.set == set {
            let comps = size / 4;
            let mut out = Vec::with_capacity(g.vertex_count as usize * comps);
            for v in 0..g.vertex_count as usize {
                let at = v * g.stride as usize + off;
                for k in 0..comps {
                    let b = &g.vertices[at + k * 4..at + k * 4 + 4];
                    out.push(f32::from_le_bytes(b.try_into().unwrap()));
                }
            }
            return Some((out, comps));
        }
        off += size;
    }
    None
}

fn to_y_up(v: &mut [f32]) {
    for p in v.chunks_mut(3) {
        let (y, z) = (p[1], p[2]);
        p[1] = z;
        p[2] = -y;
    }
}

pub fn indices(g: &Geometry) -> Vec<u32> {
    let per = if g.index_count > 0 { g.indices.len() / g.index_count as usize } else { 2 };
    if per == 4 {
        g.indices.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect()
    } else {
        g.indices.chunks_exact(2).map(|b| u16::from_le_bytes(b.try_into().unwrap()) as u32).collect()
    }
}

/// The unit as a GLB file. `name` labels the scene; `images` are embedded PNGs that `maps` points into.
pub fn unit_to_glb(u: &Unit, name: &str, images: &[Image], maps: &MaterialMaps) -> Result<Vec<u8>> {
    let mut b = Builder { bin: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let mut materials: Vec<Value> = Vec::new();
    let mut material_ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    for (gi, g) in u.geometries.iter().enumerate() {
        let Some((mut pos, _)) = channel(g, 0, 0) else { continue };
        to_y_up(&mut pos);
        let mut attrs = serde_json::Map::new();
        attrs.insert("POSITION".into(), json!(b.floats(&pos, 3, true)));
        if let Some((mut n, 3)) = channel(g, 1, 0) {
            to_y_up(&mut n);
            for v in n.chunks_mut(3) {
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                if l > 1e-6 {
                    v.iter_mut().for_each(|x| *x /= l);
                } else {
                    v.copy_from_slice(&[0.0, 1.0, 0.0]);
                }
            }
            attrs.insert("NORMAL".into(), json!(b.floats(&n, 3, false)));
        }
        for set in 0..4 {
            if let Some((uv, 2)) = channel(g, 4, set) {
                attrs.insert(format!("TEXCOORD_{set}"), json!(b.floats(&uv, 2, false)));
            }
        }
        if let Some((col, 4)) = channel(g, 5, 0) {
            let col: Vec<f32> = col.iter().map(|c| c.clamp(0.0, 1.0)).collect();
            attrs.insert("COLOR_0".into(), json!(b.floats(&col, 4, false)));
        }
        let idx = indices(g);
        let mut prims = Vec::new();
        let batches: Vec<(u32, u32, u32)> = if g.batches.is_empty() { vec![(0, 0, idx.len() as u32 / 3)] } else { g.batches.iter().map(|x| (x.material, x.start, x.triangles)).collect() };
        for (slot, start, tris) in batches {
            let (s, e) = ((start * 3) as usize, ((start + tris) * 3) as usize);
            if tris == 0 || e > idx.len() {
                continue;
            }
            let acc = b.indices(&idx[s..e]);
            let slot_name = g.materials.get(slot as usize).cloned().unwrap_or_else(|| format!("slot_{slot}"));
            let mid = *material_ids.entry(slot_name.clone()).or_insert_with(|| {
                let mut m = json!({ "name": slot_name, "pbrMetallicRoughness": { "metallicFactor": 0.0, "roughnessFactor": 0.8 } });
                if let Some(sm) = maps.slots.get(&slot_name) {
                    if let Some(t) = sm.base {
                        m["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": t });
                    }
                    if let Some(t) = sm.normal {
                        m["normalTexture"] = json!({ "index": t });
                    }
                }
                materials.push(m);
                materials.len() - 1
            });
            prims.push(json!({ "attributes": attrs.clone(), "indices": acc, "material": mid, "mode": 4 }));
        }
        meshes.push(json!({ "name": format!("geometry_{gi}"), "primitives": prims }));
        nodes.push(json!({ "name": format!("geometry_{gi}"), "mesh": meshes.len() - 1 }));
    }
    let mut gltf_images = Vec::new();
    let mut textures = Vec::new();
    for img in images {
        let view = b.view(&img.png, None);
        gltf_images.push(json!({ "name": img.name, "bufferView": view, "mimeType": "image/png" }));
        textures.push(json!({ "source": gltf_images.len() - 1, "sampler": 0 }));
    }
    b.align();
    let node_ids: Vec<usize> = (0..nodes.len()).collect();
    let mut doc = json!({
        "asset": { "version": "2.0", "generator": concat!("Minediver ", env!("CARGO_PKG_VERSION")) },
        "scene": 0,
        "scenes": [{ "name": name, "nodes": node_ids }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": b.accessors,
        "bufferViews": b.views,
        "buffers": [{ "byteLength": b.bin.len() }],
    });
    if !textures.is_empty() {
        doc["images"] = json!(gltf_images);
        doc["textures"] = json!(textures);
        doc["samplers"] = json!([{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }]);
    }
    let mut js = serde_json::to_vec(&doc)?;
    while js.len() % 4 != 0 {
        js.push(b' ');
    }
    let total = 12 + 8 + js.len() + 8 + b.bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend(b"glTF");
    out.extend(2u32.to_le_bytes());
    out.extend((total as u32).to_le_bytes());
    out.extend((js.len() as u32).to_le_bytes());
    out.extend(b"JSON");
    out.extend(js);
    out.extend((b.bin.len() as u32).to_le_bytes());
    out.extend(b"BIN\0");
    out.extend(b.bin);
    Ok(out)
}
