//! HD1 `.unit` resources. Layout worked out from the game files (version 144):
//!
//! ```text
//! u32 version (144)  u32 geometry count
//! per geometry:
//!   u32 vertex bytes, vertex bytes
//!   u32 ?, u32 ?                       (always 0 so far)
//!   u32 vertex count, u32 stride, u32 channel count
//!   channel × n: u32 semantic, u32 format, u32 set, u32 ?, u8 ?   (17 bytes, packed)
//!   u32 ?, u32 ?, u32 ?                (index format and flags, 0 = u16)
//!   u32 index count, u32 index bytes, indices
//!   u32 batch count, batch × n: u32 material slot, u32 first triangle, u32 triangle count, u32 ? (bone set?)
//!   f32 × 3 min, f32 × 3 max, f32 radius
//!   u32 material count, (u32 length, name) × n          material slot names
//! then scene graph, physics, flow and the rest, not read yet.
//! ```
//! Checked on all 5,307 units of Steam build 6001293: every geometry section parses. TODO(verify): fields
//! marked `?`. Semantics: 0 position, 1 normal, 2 binormal, 3 tangent, 4 uv (set = layer), 5 colour,
//! 6 blend indices, 7 blend weights. Formats: 0..3 = 1..4 floats.

use anyhow::{Result, bail, ensure};
use serde::Serialize;

pub const VERSION: u32 = 144;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Hash, PartialOrd, Ord)]
pub struct Channel {
    pub semantic: u32,
    pub format: u32,
    pub set: u32,
    pub extra: u32,
    pub flag: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    pub material: u32,
    pub start: u32,
    pub triangles: u32,
    pub extra: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Geometry {
    #[serde(skip)]
    pub vertices: Vec<u8>,
    pub vertex_count: u32,
    pub stride: u32,
    pub channels: Vec<Channel>,
    pub index_format: [u32; 3],
    #[serde(skip)]
    pub indices: Vec<u8>,
    pub index_count: u32,
    pub batches: Vec<Batch>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub radius: f32,
    pub materials: Vec<String>,
    pub head: [u32; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct Unit {
    pub geometries: Vec<Geometry>,
    /// Where the part after the meshes starts.
    pub rest_at: usize,
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(self.p + n <= self.d.len(), "unit is cut short at {}", self.p);
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
}

/// Size in bytes of one element of a channel format. TODO(verify) beyond formats 1 and 2.
pub fn format_size(format: u32) -> Option<u32> {
    Some(match format {
        0 => 4,
        1 => 8,
        2 => 12,
        3 => 16,
        4 => 4,
        _ => return None,
    })
}

pub fn parse(d: &[u8]) -> Result<Unit> {
    let mut c = Cur { d, p: 0 };
    let version = c.u32()?;
    ensure!(version == VERSION, "unit version {version}, expected {VERSION}");
    let n = c.u32()?;
    ensure!(n < 4096, "implausible geometry count {n}");
    let mut geometries = Vec::new();
    for _ in 0..n {
        let vbytes = c.u32()? as usize;
        let vertices = c.take(vbytes)?.to_vec();
        let head = [c.u32()?, c.u32()?];
        let vertex_count = c.u32()?;
        let stride = c.u32()?;
        let nch = c.u32()?;
        ensure!(nch < 32, "implausible channel count {nch}");
        let mut channels = Vec::new();
        for _ in 0..nch {
            channels.push(Channel { semantic: c.u32()?, format: c.u32()?, set: c.u32()?, extra: c.u32()?, flag: c.take(1)?[0] });
        }
        if vertex_count as usize * stride as usize != vbytes {
            bail!("vertex bytes {vbytes} != {vertex_count} x {stride}");
        }
        let index_format = [c.u32()?, c.u32()?, c.u32()?];
        let index_count = c.u32()?;
        let ibytes = c.u32()? as usize;
        let indices = c.take(ibytes)?.to_vec();
        let nb = c.u32()?;
        ensure!(nb < 4096, "implausible batch count {nb}");
        let mut batches = Vec::new();
        for _ in 0..nb {
            batches.push(Batch { material: c.u32()?, start: c.u32()?, triangles: c.u32()?, extra: c.u32()? });
        }
        let min = [c.f32()?, c.f32()?, c.f32()?];
        let max = [c.f32()?, c.f32()?, c.f32()?];
        let radius = c.f32()?;
        let nm = c.u32()?;
        ensure!(nm < 4096, "implausible material count {nm}");
        let mut materials = Vec::new();
        for _ in 0..nm {
            let len = c.u32()? as usize;
            ensure!(len < 256, "implausible material name length {len}");
            materials.push(String::from_utf8_lossy(c.take(len)?).into_owned());
        }
        geometries.push(Geometry { vertices, vertex_count, stride, channels, index_format, indices, index_count, batches, min, max, radius, materials, head });
    }
    Ok(Unit { geometries, rest_at: c.p })
}

/// Writes one geometry the way `parse` reads it.
pub fn write_geometry(out: &mut Vec<u8>, g: &Geometry) {
    let u = |out: &mut Vec<u8>, v: u32| out.extend(v.to_le_bytes());
    u(out, g.vertices.len() as u32);
    out.extend_from_slice(&g.vertices);
    u(out, g.head[0]);
    u(out, g.head[1]);
    u(out, g.vertex_count);
    u(out, g.stride);
    u(out, g.channels.len() as u32);
    for c in &g.channels {
        u(out, c.semantic);
        u(out, c.format);
        u(out, c.set);
        u(out, c.extra);
        out.push(c.flag);
    }
    for w in g.index_format {
        u(out, w);
    }
    u(out, g.index_count);
    u(out, g.indices.len() as u32);
    out.extend_from_slice(&g.indices);
    u(out, g.batches.len() as u32);
    for b in &g.batches {
        u(out, b.material);
        u(out, b.start);
        u(out, b.triangles);
        u(out, b.extra);
    }
    for f in g.min.iter().chain(g.max.iter()).chain(std::iter::once(&g.radius)) {
        out.extend(f.to_le_bytes());
    }
    u(out, g.materials.len() as u32);
    for m in &g.materials {
        u(out, m.len() as u32);
        out.extend_from_slice(m.as_bytes());
    }
}

/// The unit written back from its parts: the same bytes as the original.
pub fn write(u: &Unit, orig: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(orig.len());
    out.extend(VERSION.to_le_bytes());
    out.extend((u.geometries.len() as u32).to_le_bytes());
    for g in &u.geometries {
        write_geometry(&mut out, g);
    }
    out.extend_from_slice(&orig[u.rest_at..]);
    out
}

/// One node of the unit's scene graph.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// 3x3 rotation (rows), position, scale, relative to the parent.
    pub rotation: [f32; 9],
    pub position: [f32; 3],
    pub scale: [f32; 3],
    /// Object-space transform, column-major 4x4 with the translation in 12..15.
    pub world: [f32; 16],
    pub parent: Option<u16>,
    /// Top 32 bits of murmur64 of the node name (the names are in the `bones` resource).
    pub name: u32,
}

/// A skin: the bones a skinned geometry follows.
#[derive(Debug, Clone, Serialize)]
pub struct Skin {
    /// Inverse bind matrices, column-major, one per bone.
    pub inverse_bind: Vec<[f32; 16]>,
    /// Scene graph node of each bone.
    pub nodes: Vec<u32>,
    /// Bone sets: a vertex blend index `i` means bone `sets[set][i]` of this skin.
    pub sets: Vec<Vec<u32>>,
}

fn f32s<const N: usize>(d: &[u8], at: usize) -> [f32; N] {
    std::array::from_fn(|i| f32::from_le_bytes(d[at + i * 4..at + i * 4 + 4].try_into().unwrap()))
}

fn word_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

/// The skins right after the geometries: u32 count; per skin u32 n, n inverse bind matrices, u32 n,
/// n node indices, u32 set count, per set u32 m and m bone indices.
pub fn skins(d: &[u8], rest_at: usize) -> Result<(Vec<Skin>, usize)> {
    let mut c = Cur { d, p: rest_at };
    let n = c.u32()?;
    ensure!(n < 256, "implausible skin count {n}");
    let mut out = Vec::new();
    for _ in 0..n {
        let bones = c.u32()? as usize;
        ensure!(bones < 1024, "implausible bone count {bones}");
        let at = c.p;
        c.take(bones * 64)?;
        let inverse_bind = (0..bones).map(|i| f32s::<16>(d, at + i * 64)).collect();
        let n2 = c.u32()? as usize;
        ensure!(n2 == bones, "skin node count {n2} differs from bone count {bones}");
        let nodes = (0..n2).map(|_| c.u32()).collect::<Result<Vec<_>>>()?;
        let sets_n = c.u32()?;
        ensure!(sets_n < 256, "implausible bone set count {sets_n}");
        let mut sets = Vec::new();
        for _ in 0..sets_n {
            let m = c.u32()? as usize;
            ensure!(m <= bones.max(1) * 4, "implausible bone set size {m}");
            let set = (0..m).map(|_| c.u32()).collect::<Result<Vec<_>>>()?;
            ensure!(set.iter().all(|&b| (b as usize) < bones), "bone set points past the skin");
            sets.push(set);
        }
        out.push(Skin { inverse_bind, nodes, sets });
    }
    Ok((out, c.p))
}

fn is_transform(m: &[f32; 16]) -> bool {
    if m.iter().any(|x| !x.is_finite() || x.abs() > 1e5) {
        return false;
    }
    if m[3].abs() > 1e-4 || m[7].abs() > 1e-4 || m[11].abs() > 1e-4 || (m[15] - 1.0).abs() > 1e-4 {
        return false;
    }
    // Columns of the 3x3 part are not zero (scale can be anything else).
    (0..3).all(|c| (0..3).map(|r| m[c * 4 + r] * m[c * 4 + r]).sum::<f32>() > 1e-8)
}

/// The scene graph: u32 count, count locals (9 + 3 + 3 floats), count world matrices, count parent
/// words (`parent << 16 | has_parent`), count name hashes. Found by shape after `from`, since the
/// sections between the skins and the graph are not read yet.
pub fn scene_graph(d: &[u8], from: usize) -> Option<(Vec<Node>, usize)> {
    scene_graph_where(d, from, |_| true)
}

/// `scene_graph`, taking the first candidate `accept` agrees with.
pub fn scene_graph_where(d: &[u8], from: usize, accept: impl Fn(&[Node]) -> bool) -> Option<(Vec<Node>, usize)> {
    let mut p = from;
    while p + 4 <= d.len() {
        let n = word_at(d, p) as usize;
        if (1..=2048).contains(&n) {
            let locals = p + 4;
            let worlds = locals + n * 60;
            let parents = worlds + n * 64;
            let names = parents + n * 4;
            if names + n * 4 <= d.len() && word_at(d, parents) == 0 {
                let ok = (0..n).all(|i| {
                    let w = word_at(d, parents + i * 4);
                    let has = w & 0xffff;
                    let parent = (w >> 16) as usize;
                    (i == 0 && w == 0) || (i > 0 && ((has == 1 && parent < i) || w == 0))
                }) && (0..n).all(|i| is_transform(&f32s::<16>(d, worlds + i * 64)));
                if ok {
                    let nodes = (0..n)
                        .map(|i| {
                            let l: [f32; 15] = f32s(d, locals + i * 60);
                            let w = word_at(d, parents + i * 4);
                            Node {
                                rotation: l[..9].try_into().unwrap(),
                                position: l[9..12].try_into().unwrap(),
                                scale: l[12..15].try_into().unwrap(),
                                world: f32s(d, worlds + i * 64),
                                parent: (w & 0xffff == 1).then_some((w >> 16) as u16),
                                name: word_at(d, names + i * 4),
                            }
                        })
                        .collect::<Vec<Node>>();
                    if accept(&nodes) {
                        return Some((nodes, p));
                    }
                }
            }
        }
        p += 1;
    }
    None
}

/// Names of a `bones` resource by their 32-bit hash: u32 count, u32 ?, count hashes, u32 count,
/// then the names, each ending in a zero byte.
pub fn bone_names(d: &[u8]) -> std::collections::HashMap<u32, String> {
    let mut out = std::collections::HashMap::new();
    if d.len() < 8 {
        return out;
    }
    let n = word_at(d, 0) as usize;
    let names_at = 8 + n * 4 + 4;
    if names_at > d.len() {
        return out;
    }
    for (i, name) in d[names_at..].split(|&b| b == 0).take(n).enumerate() {
        if 8 + i * 4 + 4 <= d.len() {
            out.insert(word_at(d, 8 + i * 4), String::from_utf8_lossy(name).into_owned());
        }
    }
    out
}

/// The scene graph of a unit, checked against its skins and, when given, the names of its `bones`
/// resource.
pub fn find_graph(d: &[u8], skins_end: usize, skins: &[Skin], bone_hashes: &[u32]) -> Option<Vec<Node>> {
    let need = skins.iter().flat_map(|s| s.nodes.iter()).map(|&n| n as usize + 1).max().unwrap_or(0);
    scene_graph_where(d, skins_end, |nodes| nodes.len() >= need && bone_hashes.iter().all(|h| nodes.iter().any(|n| n.name == *h)))
        .or_else(|| {
            // Some units (helmets) use a part of a bigger skeleton: most of their nodes are known bones.
            scene_graph_where(d, skins_end, |nodes| {
                let known = nodes.iter().filter(|n| bone_hashes.contains(&n.name)).count();
                nodes.len() >= need && (bone_hashes.is_empty() || known * 2 >= nodes.len())
            })
        })
        .map(|(n, _)| n)
}
