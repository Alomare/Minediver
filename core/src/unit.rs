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
