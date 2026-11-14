//! Reads meshes back from a GLB file (as Blender exports it), in game axes (Z-up).

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

#[derive(Debug, Default, Clone)]
pub struct Prim {
    pub material: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uvs: Vec<Vec<[f32; 2]>>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Skinned: four joints and weights per vertex, and the name of each joint of the skin.
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub joint_names: Vec<String>,
    /// Skinned: positions, normals and tangents stay as the file has them (bind space), and this is each
    /// joint's world matrix times its inverse bind matrix, in glTF axes.
    pub joint_binds: Vec<M4>,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub name: String,
    pub prims: Vec<Prim>,
}

struct Doc<'a> {
    json: Value,
    bin: &'a [u8],
}

fn comps(ty: &str) -> usize {
    match ty {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT4" => 16,
        _ => 0,
    }
}

impl Doc<'_> {
    /// An accessor as floats (normalized integers scaled to 0..1 or -1..1).
    fn floats(&self, i: usize) -> Result<(Vec<f32>, usize)> {
        let a = &self.json["accessors"][i];
        let n = comps(a["type"].as_str().unwrap_or_default());
        let count = a["count"].as_u64().context("accessor count")? as usize;
        let ct = a["componentType"].as_u64().unwrap_or(5126);
        let norm = a["normalized"].as_bool().unwrap_or(false);
        let Some(vi) = a["bufferView"].as_u64() else { return Ok((vec![0.0; count * n], n)) };
        let v = &self.json["bufferViews"][vi as usize];
        ensure!(v["buffer"].as_u64().unwrap_or(0) == 0, "only the GLB's own buffer is read");
        let size = match ct {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => bail!("component type {ct}"),
        };
        let base = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = v["byteStride"].as_u64().map(|s| s as usize).unwrap_or(n * size);
        let mut out = Vec::with_capacity(count * n);
        for e in 0..count {
            for k in 0..n {
                let at = base + e * stride + k * size;
                ensure!(at + size <= self.bin.len(), "accessor runs past the buffer");
                let b = &self.bin[at..at + size];
                let x = match ct {
                    5126 => f32::from_le_bytes(b.try_into().unwrap()),
                    5121 => b[0] as f32 / if norm { 255.0 } else { 1.0 },
                    5120 => (b[0] as i8) as f32 / if norm { 127.0 } else { 1.0 },
                    5123 => u16::from_le_bytes(b.try_into().unwrap()) as f32 / if norm { 65535.0 } else { 1.0 },
                    5122 => i16::from_le_bytes(b.try_into().unwrap()) as f32 / if norm { 32767.0 } else { 1.0 },
                    _ => u32::from_le_bytes(b.try_into().unwrap()) as f32,
                };
                out.push(x);
            }
        }
        Ok((out, n))
    }

    fn uints(&self, i: usize) -> Result<Vec<u32>> {
        let a = &self.json["accessors"][i];
        let count = a["count"].as_u64().unwrap_or(0) as usize;
        let ct = a["componentType"].as_u64().unwrap_or(5125);
        let vi = a["bufferView"].as_u64().context("index buffer view")? as usize;
        let v = &self.json["bufferViews"][vi];
        let base = v["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let size = match ct {
            5121 => 1,
            5123 => 2,
            _ => 4,
        };
        ensure!(base + count * size <= self.bin.len(), "indices run past the buffer");
        Ok((0..count)
            .map(|e| {
                let b = &self.bin[base + e * size..base + e * size + size];
                match size {
                    1 => b[0] as u32,
                    2 => u16::from_le_bytes(b.try_into().unwrap()) as u32,
                    _ => u32::from_le_bytes(b.try_into().unwrap()),
                }
            })
            .collect())
    }
}

pub type M4 = [f32; 16];
const IDENT: M4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

fn mul(a: &M4, b: &M4) -> M4 {
    let mut o = [0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

fn node_matrix(n: &Value) -> M4 {
    if let Some(m) = n["matrix"].as_array() {
        let mut o = IDENT;
        for (i, x) in m.iter().enumerate().take(16) {
            o[i] = x.as_f64().unwrap_or(0.0) as f32;
        }
        return o;
    }
    let get = |k: &str, d: &[f32]| -> Vec<f32> { n[k].as_array().map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_else(|| d.to_vec()) };
    let t = get("translation", &[0., 0., 0.]);
    let r = get("rotation", &[0., 0., 0., 1.]);
    let s = get("scale", &[1., 1., 1.]);
    let (x, y, z, w) = (r[0], r[1], r[2], r[3]);
    let rot = [
        1. - 2. * (y * y + z * z), 2. * (x * y + z * w), 2. * (x * z - y * w), 0.,
        2. * (x * y - z * w), 1. - 2. * (x * x + z * z), 2. * (y * z + x * w), 0.,
        2. * (x * z + y * w), 2. * (y * z - x * w), 1. - 2. * (x * x + y * y), 0.,
        t[0], t[1], t[2], 1.,
    ];
    let mut o = rot;
    for c in 0..3 {
        for r in 0..3 {
            o[c * 4 + r] *= s[c];
        }
    }
    o
}

fn apply(m: &M4, p: [f32; 3], w: f32) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r] * w)
}

/// glTF Y-up back to game Z-up: (x, y, z) -> (x, -z, y).
fn to_z_up(p: [f32; 3]) -> [f32; 3] {
    [p[0], -p[2], p[1]]
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-8 { [v[0] / l, v[1] / l, v[2] / l] } else { [0., 0., 1.] }
}

pub fn read(bytes: &[u8]) -> Result<Vec<Mesh>> {
    ensure!(bytes.len() >= 20 && &bytes[..4] == b"glTF", "not a GLB file (export from Blender as glTF Binary)");
    let jl = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    ensure!(&bytes[16..20] == b"JSON" && 20 + jl <= bytes.len(), "GLB JSON chunk is broken");
    let json: Value = serde_json::from_slice(&bytes[20..20 + jl])?;
    let rest = &bytes[20 + jl..];
    let bin: &[u8] = if rest.len() >= 8 && &rest[4..8] == b"BIN\0" {
        let bl = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        &rest[8..(8 + bl).min(rest.len())]
    } else {
        &[]
    };
    let doc = Doc { json, bin };
    let mut out = Vec::new();
    let scene = doc.json["scene"].as_u64().unwrap_or(0) as usize;
    let roots: Vec<usize> = doc.json["scenes"][scene]["nodes"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()).unwrap_or_else(|| (0..doc.json["nodes"].as_array().map_or(0, |a| a.len())).collect());
    let count = doc.json["nodes"].as_array().map_or(0, |a| a.len());
    let mut worlds = vec![IDENT; count];
    let mut order = Vec::new();
    let mut stack: Vec<(usize, M4)> = roots.into_iter().map(|r| (r, IDENT)).collect();
    while let Some((ni, parent)) = stack.pop() {
        if ni >= count {
            continue;
        }
        let n = &doc.json["nodes"][ni];
        let m = mul(&parent, &node_matrix(n));
        worlds[ni] = m;
        order.push(ni);
        if let Some(kids) = n["children"].as_array() {
            stack.extend(kids.iter().filter_map(|k| k.as_u64()).map(|k| (k as usize, m)));
        }
    }
    for ni in order {
        let n = &doc.json["nodes"][ni];
        let mut m = worlds[ni];
        let Some(mi) = n["mesh"].as_u64() else { continue };
        let mut joint_names = Vec::new();
        let mut joint_binds = Vec::new();
        if let Some(si) = n["skin"].as_u64() {
            // A skinned mesh keeps its vertices in bind space; each vertex is placed later through its own
            // joint (the rebuilder knows the game bone of each joint).
            let skin = &doc.json["skins"][si as usize];
            let joints: Vec<usize> = skin["joints"].as_array().into_iter().flatten().filter_map(|j| j.as_u64().map(|j| j as usize)).collect();
            joint_names = joints.iter().map(|&j| doc.json["nodes"][j]["name"].as_str().unwrap_or("").to_string()).collect();
            let ib = match skin["inverseBindMatrices"].as_u64() {
                Some(i) => doc.floats(i as usize)?.0,
                None => IDENT.repeat(joints.len()),
            };
            joint_binds = joints.iter().enumerate().map(|(k, &j)| {
                let inv: M4 = ib.get(k * 16..k * 16 + 16).map_or(IDENT, |s| s.try_into().unwrap());
                mul(&worlds.get(j).copied().unwrap_or(IDENT), &inv)
            }).collect();
            m = IDENT;
        }
        let skinned_node = !joint_binds.is_empty();
        let mesh = &doc.json["meshes"][mi as usize];
        let name = n["name"].as_str().or(mesh["name"].as_str()).unwrap_or("mesh").to_string();
        let mut prims = Vec::new();
        for p in mesh["primitives"].as_array().into_iter().flatten() {
            if p["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let at = &p["attributes"];
            let Some(pi) = at["POSITION"].as_u64() else { continue };
            let (pos, _) = doc.floats(pi as usize)?;
            let mut prim = Prim::default();
            let axes = |p: [f32; 3]| if skinned_node { p } else { to_z_up(p) };
            prim.positions = pos.chunks(3).map(|c| axes(apply(&m, [c[0], c[1], c[2]], 1.0))).collect();
            if let Some(i) = at["NORMAL"].as_u64() {
                prim.normals = doc.floats(i as usize)?.0.chunks(3).map(|c| norm(axes(apply(&m, [c[0], c[1], c[2]], 0.0)))).collect();
            }
            if let Some(i) = at["TANGENT"].as_u64() {
                prim.tangents = doc.floats(i as usize)?.0.chunks(4).map(|c| { let t = norm(axes(apply(&m, [c[0], c[1], c[2]], 0.0))); [t[0], t[1], t[2], c[3]] }).collect();
            }
            for set in 0..4 {
                if let Some(i) = at[format!("TEXCOORD_{set}")].as_u64() {
                    prim.uvs.push(doc.floats(i as usize)?.0.chunks(2).map(|c| [c[0], c[1]]).collect());
                }
            }
            if let Some(i) = at["COLOR_0"].as_u64() {
                let (c, n) = doc.floats(i as usize)?;
                prim.colors = c.chunks(n).map(|c| [c[0], c[1], c[2], if n == 4 { c[3] } else { 1.0 }]).collect();
            }
            prim.indices = match p["indices"].as_u64() {
                Some(i) => doc.uints(i as usize)?,
                None => (0..prim.positions.len() as u32).collect(),
            };
            if let (Some(ji), Some(wi)) = (at["JOINTS_0"].as_u64(), at["WEIGHTS_0"].as_u64()) {
                prim.joints = doc.floats(ji as usize)?.0.chunks(4).map(|c| [c[0] as u16, c[1] as u16, c[2] as u16, c[3] as u16]).collect();
                prim.weights = doc.floats(wi as usize)?.0.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
                prim.joint_names = joint_names.clone();
                prim.joint_binds = joint_binds.clone();
            }
            prim.material = p["material"].as_u64().and_then(|mi| doc.json["materials"][mi as usize]["name"].as_str()).unwrap_or("").to_string();
            // A mirrored node flips winding.
            let det = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2]);
            if det < 0.0 {
                for t in prim.indices.chunks_mut(3) {
                    if t.len() == 3 {
                        t.swap(1, 2);
                    }
                }
            }
            compact(&mut prim);
            prims.push(prim);
        }
        out.push(Mesh { name, prims });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Keeps only the vertices the indices use, in first-use order. Primitives that share one vertex list
/// (as Minediver writes them) come back the size they were.
fn compact(p: &mut Prim) {
    let n = p.positions.len();
    let mut map = vec![u32::MAX; n];
    let mut order = Vec::new();
    for i in p.indices.iter_mut() {
        let v = *i as usize;
        if v >= n {
            continue;
        }
        if map[v] == u32::MAX {
            map[v] = order.len() as u32;
            order.push(v);
        }
        *i = map[v];
    }
    if order.len() == n && order.iter().enumerate().all(|(a, b)| a == *b) {
        return;
    }
    fn pick<T: Copy>(v: &[T], order: &[usize], n: usize) -> Vec<T> {
        if v.len() == n { order.iter().map(|&i| v[i]).collect() } else { v.to_vec() }
    }
    p.positions = pick(&p.positions, &order, n);
    p.normals = pick(&p.normals, &order, n);
    p.tangents = pick(&p.tangents, &order, n);
    p.colors = pick(&p.colors, &order, n);
    p.joints = pick(&p.joints, &order, n);
    p.weights = pick(&p.weights, &order, n);
    p.uvs = p.uvs.iter().map(|u| pick(u, &order, n)).collect();
}
