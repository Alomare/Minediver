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

    fn joints(&mut self, data: &[u16]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(34962));
        self.accessors.push(json!({ "bufferView": view, "componentType": 5123, "count": data.len() / 4, "type": "VEC4" }));
        self.accessors.len() - 1
    }

    fn matrices(&mut self, data: &[f32]) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, None);
        self.accessors.push(json!({ "bufferView": view, "componentType": 5126, "count": data.len() / 16, "type": "MAT4" }));
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
    for p in v.chunks_exact_mut(3) {
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
    unit_to_glb_rigged(u, name, images, maps, None)
}

/// The skeleton of a unit: its scene graph, its skins, and bone names by 32-bit hash.
pub struct Rig {
    pub nodes: Vec<crate::unit::Node>,
    pub skins: Vec<crate::unit::Skin>,
    pub names: std::collections::HashMap<u32, String>,
}

/// `unit_to_glb` with bones. With a rig, every vertex stays in game axes and one root node turns the
/// whole unit Y-up, so joints, inverse bind matrices and vertices share one space.
pub fn unit_to_glb_rigged(u: &Unit, name: &str, images: &[Image], maps: &MaterialMaps, rig: Option<&Rig>) -> Result<Vec<u8>> {
    let skinned = |g: &Geometry| g.channels.iter().any(|c| c.semantic == 6) && g.channels.iter().any(|c| c.semantic == 7);
    let rig = rig.filter(|r| !r.skins.is_empty() && u.geometries.iter().any(|g| skinned(g)));
    let mut skin_of_geometry: Vec<Option<usize>> = Vec::new();
    let mut k = 0;
    for g in &u.geometries {
        skin_of_geometry.push(if skinned(g) { k += 1; Some(k - 1) } else { None });
    }
    let mut b = Builder { bin: Vec::new(), views: Vec::new(), accessors: Vec::new() };
    let mut materials: Vec<Value> = Vec::new();
    let mut material_ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    for (gi, g) in u.geometries.iter().enumerate() {
        let Some((mut pos, 3)) = channel(g, 0, 0) else { continue };
        if rig.is_none() {
            to_y_up(&mut pos);
        }
        let mut attrs = serde_json::Map::new();
        attrs.insert("POSITION".into(), json!(b.floats(&pos, 3, true)));
        if let Some((mut n, 3)) = channel(g, 1, 0) {
            if rig.is_none() {
                to_y_up(&mut n);
            }
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
        let skin = rig.and_then(|r| skin_of_geometry[gi].and_then(|k| r.skins.get(k)));
        if let (Some(skin), Some((bi, bn)), Some((bw, wn))) = (skin, channel(g, 6, 0), channel(g, 7, 0)) {
            // Each batch picks a bone set; a blend index is a place in that set.
            let count = g.vertex_count as usize;
            let mut set_of = vec![0usize; count];
            for b in &g.batches {
                let (s, e) = ((b.start * 3) as usize, ((b.start + b.triangles) * 3) as usize);
                for &v in idx.get(s..e.min(idx.len())).unwrap_or(&[]) {
                    if (v as usize) < count {
                        set_of[v as usize] = b.extra as usize;
                    }
                }
            }
            let mut joints = vec![0u16; count * 4];
            let mut weights = vec![0f32; count * 4];
            for v in 0..count {
                let set = skin.sets.get(set_of[v]).or(skin.sets.first());
                let mut sum = 0.0;
                for j in 0..bn.min(4) {
                    let w = if j < wn { bw[v * wn + j].max(0.0) } else { 0.0 };
                    let local = bi[v * bn + j].round().max(0.0) as usize;
                    let bone = set.and_then(|s| s.get(local)).copied().unwrap_or(local as u32);
                    joints[v * 4 + j] = bone.min(skin.nodes.len().saturating_sub(1) as u32) as u16;
                    weights[v * 4 + j] = w;
                    sum += w;
                }
                if sum > 1e-6 {
                    for j in 0..4 {
                        weights[v * 4 + j] /= sum;
                    }
                } else {
                    weights[v * 4] = 1.0;
                }
            }
            attrs.insert("JOINTS_0".into(), json!(b.joints(&joints)));
            attrs.insert("WEIGHTS_0".into(), json!(b.floats(&weights, 4, false)));
        }
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
        let mut node = json!({ "name": format!("geometry_{gi}"), "mesh": meshes.len() - 1 });
        if let (Some(_), Some(k)) = (rig, skin_of_geometry[gi]) {
            node["skin"] = json!(k);
        }
        nodes.push(node);
    }
    let mut gltf_images = Vec::new();
    let mut textures = Vec::new();
    for img in images {
        let view = b.view(&img.png, None);
        gltf_images.push(json!({ "name": img.name, "bufferView": view, "mimeType": "image/png" }));
        textures.push(json!({ "source": gltf_images.len() - 1, "sampler": 0 }));
    }
    b.align();
    let mut node_ids: Vec<usize> = (0..nodes.len()).collect();
    let mut skins_json = Vec::new();
    if let Some(r) = rig {
        let base = nodes.len();
        for (i, n) in r.nodes.iter().enumerate() {
            let (rot, s, p) = (n.rotation, n.scale, n.position);
            let m = [
                rot[0] * s[0], rot[1] * s[0], rot[2] * s[0], 0.0, rot[3] * s[1], rot[4] * s[1], rot[5] * s[1], 0.0,
                rot[6] * s[2], rot[7] * s[2], rot[8] * s[2], 0.0, p[0], p[1], p[2], 1.0,
            ];
            let label = r.names.get(&n.name).cloned().unwrap_or_else(|| format!("node_{i}"));
            let children: Vec<usize> = r.nodes.iter().enumerate().filter(|(_, c)| c.parent == Some(i as u16)).map(|(j, _)| base + j).collect();
            let mut node = json!({ "name": label, "matrix": m });
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            nodes.push(node);
        }
        for s in &r.skins {
            let ibm: Vec<f32> = s.inverse_bind.iter().flatten().copied().collect();
            let acc = b.matrices(&ibm);
            skins_json.push(json!({ "joints": s.nodes.iter().map(|&n| base + n as usize).collect::<Vec<_>>(), "inverseBindMatrices": acc }));
        }
        // Skinned parts sit at the scene root: their joints carry the turn to Y-up already.
        let skinned_nodes: Vec<usize> = node_ids.iter().copied().filter(|&i| nodes[i].get("skin").is_some()).collect();
        let mut children: Vec<usize> = node_ids.iter().copied().filter(|i| !skinned_nodes.contains(i)).collect();
        children.extend(r.nodes.iter().enumerate().filter(|(_, n)| n.parent.is_none()).map(|(j, _)| base + j));
        // -90 degrees about X: game Z-up to glTF Y-up.
        nodes.push(json!({ "name": name, "rotation": [-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2], "children": children }));
        node_ids = skinned_nodes;
        node_ids.push(nodes.len() - 1);
    }
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
    if !skins_json.is_empty() {
        doc["skins"] = json!(skins_json);
    }
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
