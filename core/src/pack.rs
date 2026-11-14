//! Builds an Obsidian mod folder from edited files. Minediver never writes the game's own files: the mod
//! folder goes to Obsidian, which deploys it as `.patch_N` overlays and can purge it again.
//!
//! A mod project is a folder with `mod.json`:
//!
//! ```json
//! { "id": "author.new_rifle", "name": "New rifle", "version": "0.1.0", "authors": ["Author"],
//!   "description": "...",
//!   "replace": [
//!     { "type": "unit", "name": "equipment/wep_assaultrifle_01", "file": "rifle.glb" },
//!     { "type": "texture", "name": "0123456789abcdef", "file": "skin.png" } ] }
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::archive::{Archive, Asset};
use crate::glb_read::{self, Prim};
use crate::hash::{self, NameDb};
use crate::unit::{self, Geometry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub replace: Vec<Replace>,
}

fn default_version() -> String {
    "0.1.0".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replace {
    #[serde(rename = "type")]
    pub ty: String,
    /// Resource name or 16-digit hash.
    pub name: String,
    /// Path inside the project folder.
    pub file: String,
    /// Hash of the file as Minediver wrote it. A build skips the entry while the file still matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub out: String,
    pub resources: usize,
    pub notes: Vec<String>,
}

fn parse_name(s: &str) -> u64 {
    let h = s.trim_start_matches('#');
    if h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()) { u64::from_str_radix(h, 16).unwrap() } else { hash::hash(s) }
}

pub fn build(ar: &Archive, names: &NameDb, project_dir: &Path, out: &Path) -> Result<Report> {
    let p: Project = serde_json::from_str(&std::fs::read_to_string(project_dir.join("mod.json")).context("read mod.json")?).context("mod.json")?;
    ensure!(!p.id.is_empty() && p.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b)), "mod id must use a-z, 0-9, '.', '_' or '-'");
    std::fs::create_dir_all(out.join("res"))?;
    let mut resources = Vec::new();
    let mut notes = Vec::new();
    for (i, r) in p.replace.iter().enumerate() {
        let th = hash::hash(&r.ty);
        let nh = parse_name(&r.name);
        let copies: Vec<&Asset> = ar.assets.iter().filter(|a| a.type_hash == th && a.name_hash == nh && a.patch.is_none()).collect();
        let first = copies.first().with_context(|| format!("{} {} is not in the game", r.ty, r.name))?;
        let src = std::fs::read(project_dir.join(&r.file)).with_context(|| format!("read {}", r.file))?;
        if r.start.as_deref() == Some(hash::hex(hash::murmur64(&src)).as_str()) {
            notes.push(format!("{} is unchanged, left out", r.file));
            continue;
        }
        let (orig, _) = ar.read_variant(first, 0)?;
        let bytes = match r.ty.as_str() {
            "unit" => {
                let ext = Path::new(&r.file).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if ext == "unit" { src } else { rebuild_unit(&orig, &glb_read::read(&src)?, &mut notes)? }
            }
            "texture" => {
                let ext = Path::new(&r.file).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                match ext.as_str() {
                    "dds" => src,
                    "png" => png_to_dds_like(&src, &orig)?,
                    _ => bail!("{}: textures take .png or .dds", r.file),
                }
            }
            _ => src,
        };
        if copies.iter().any(|a| a.variants.len() > 1) {
            notes.push(format!("{} has language variants; the new file replaces all of them", r.name));
        }
        let leaf = format!("res/{i:03}_{}.{}", r.name.rsplit('/').next().unwrap_or("x").trim_start_matches('#'), r.ty);
        std::fs::write(out.join(&leaf), &bytes)?;
        let ty_field = if ["lua", "texture", "material", "font", "timpani_bank", "bik"].contains(&r.ty.as_str()) { r.ty.clone() } else { format!("#{th:016x}") };
        let name_field = names.get(nh).unwrap_or_else(|| format!("#{nh:016x}"));
        let mut bundles: Vec<u64> = copies.iter().map(|a| a.bundle).collect();
        bundles.sort_unstable();
        bundles.dedup();
        for b in bundles {
            resources.push(json!({ "type": ty_field, "name": name_field, "bundle": format!("{b:016x}"), "data": leaf }));
        }
    }
    let manifest = json!({
        "obsidian": 1,
        "id": p.id,
        "version": p.version,
        "name": { "en": p.name },
        "description": { "en": if p.description.is_empty() { "Replaces game models and textures.".to_string() } else { p.description.clone() } },
        "authors": p.authors,
        "scope": "client",
        "resources": resources,
    });
    let text = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(out.join("obsidian.json"), text + "\n")?;
    Ok(Report { out: out.display().to_string(), resources: manifest["resources"].as_array().map_or(0, |a| a.len()), notes })
}

/// Uncompressed BGRA8 DDS with every mip, from a PNG. Larger than BC3, but the game reads it and nothing
/// is lost.
pub fn png_to_dds(png_bytes: &[u8]) -> Result<Vec<u8>> {
    let (w, h, mips) = png_mips(png_bytes)?;
    Ok(bgra_dds(w, h, &mips))
}

/// The PNG decoded, with a box-filtered chain of mips down to 1x1.
fn png_mips(png_bytes: &[u8]) -> Result<(u32, u32, Vec<(u32, u32, Vec<u8>)>)> {
    let dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut r = dec.read_info()?;
    let mut buf = vec![0; r.output_buffer_size().context("png size")?];
    let info = r.next_frame(&mut buf)?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => px.to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) => px.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        (png::ColorType::Grayscale, png::BitDepth::Eight) => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => px.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        other => bail!("PNG must be 8-bit RGB, RGBA or grey (got {other:?})"),
    };
    let mut mips = vec![(w, h, rgba)];
    while let Some((pw, ph, prev)) = mips.last().filter(|m| m.0 > 1 || m.1 > 1) {
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut acc = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(pw - 1);
                        let sy = (y * 2 + dy).min(ph - 1);
                        acc += prev[((sy * pw + sx) * 4 + k) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + k) as usize] = (acc / 4) as u8;
                }
            }
        }
        mips.push((nw, nh, next));
    }
    Ok((w, h, mips))
}

fn bgra_dds(w: u32, h: u32, mips: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = vec![0u8; 128];
    out[..4].copy_from_slice(b"DDS ");
    let put = |o: &mut Vec<u8>, at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, 4, 124);
    put(&mut out, 8, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000 | 0x20000);
    put(&mut out, 12, h);
    put(&mut out, 16, w);
    put(&mut out, 20, w * 4);
    put(&mut out, 28, mips.len() as u32);
    put(&mut out, 76, 32);
    put(&mut out, 80, 0x41);
    put(&mut out, 88, 32);
    put(&mut out, 92, 0x00ff0000);
    put(&mut out, 96, 0x0000ff00);
    put(&mut out, 100, 0x000000ff);
    put(&mut out, 104, 0xff000000);
    put(&mut out, 108, 0x1000 | 0x8 | 0x400000);
    for (_, _, m) in mips {
        for p in m.chunks(4) {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    out
}

/// A PNG as a DDS in the format of the game texture it replaces (BC1, BC2, BC3, BC4 or BC5 with the
/// same header layout and mips down to 1x1), so the file stays as small as the original. Any other
/// original format gets uncompressed BGRA8.
pub fn png_to_dds_like(png_bytes: &[u8], original: &[u8]) -> Result<Vec<u8>> {
    let (w, h, mips) = png_mips(png_bytes)?;
    let fourcc = original.get(84..88).unwrap_or(b"    ");
    let flags = original.get(80..84).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    let fmt = if original.len() >= 128 && &original[..4] == b"DDS " && flags & 4 != 0 {
        match fourcc {
            b"DXT1" => Some(texpresso::Format::Bc1),
            b"DXT2" | b"DXT3" => Some(texpresso::Format::Bc2),
            b"DXT4" | b"DXT5" => Some(texpresso::Format::Bc3),
            b"ATI1" | b"BC4U" => Some(texpresso::Format::Bc4),
            b"ATI2" | b"BC5U" => Some(texpresso::Format::Bc5),
            _ => None,
        }
    } else {
        None
    };
    let Some(fmt) = fmt else { return Ok(bgra_dds(w, h, &mips)) };
    let mut out = original[..128].to_vec();
    let put = |o: &mut Vec<u8>, at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, 8, 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000 | 0x80000);
    put(&mut out, 12, h);
    put(&mut out, 16, w);
    put(&mut out, 20, fmt.compressed_size(w as usize, h as usize) as u32);
    put(&mut out, 28, mips.len() as u32);
    let caps = u32::from_le_bytes(out[108..112].try_into().unwrap()) | 0x1000 | 0x8 | 0x400000;
    put(&mut out, 108, caps);
    let params = texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, ..Default::default() };
    for (mw, mh, px) in &mips {
        let mut block = vec![0u8; fmt.compressed_size(*mw as usize, *mh as usize)];
        fmt.compress(px, *mw as usize, *mh as usize, params, &mut block);
        out.extend(block);
    }
    Ok(out)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 1e-8 { [v[0] / l, v[1] / l, v[2] / l] } else { [1., 0., 0.] }
}

/// Per-vertex tangents from positions and UVs, when the file has none.
fn uv_tangents(pos: &[[f32; 3]], uv: &[[f32; 2]], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut t = vec![[0f32; 3]; pos.len()];
    for tri in idx.chunks_exact(3) {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if a >= pos.len() || b >= pos.len() || c >= pos.len() || a >= uv.len() || b >= uv.len() || c >= uv.len() {
            continue;
        }
        let (e1, e2) = (sub(pos[b], pos[a]), sub(pos[c], pos[a]));
        let (du1, dv1, du2, dv2) = (uv[b][0] - uv[a][0], uv[b][1] - uv[a][1], uv[c][0] - uv[a][0], uv[c][1] - uv[a][1]);
        let r = du1 * dv2 - du2 * dv1;
        if r.abs() < 1e-12 {
            continue;
        }
        let f = 1.0 / r;
        let tv = [f * (dv2 * e1[0] - dv1 * e2[0]), f * (dv2 * e1[1] - dv1 * e2[1]), f * (dv2 * e1[2] - dv1 * e2[2])];
        for v in [a, b, c] {
            for k in 0..3 {
                t[v][k] += tv[k];
            }
        }
    }
    t.into_iter().map(normalize).collect()
}

/// Which channel holds the UV tangent in the game's layout: 2 or 3. Measured on the original geometry.
fn tangent_semantic(g: &Geometry) -> u32 {
    let (Some((p, _)), Some((uv, _)), Some((c2, _)), Some((c3, _))) =
        (crate::gltf::channel(g, 0, 0), crate::gltf::channel(g, 4, 0), crate::gltf::channel(g, 2, 0), crate::gltf::channel(g, 3, 0))
    else {
        return 3;
    };
    let pos: Vec<[f32; 3]> = p.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
    let uvs: Vec<[f32; 2]> = uv.chunks(2).map(|c| [c[0], c[1]]).collect();
    let t = uv_tangents(&pos, &uvs, &crate::gltf::indices(g));
    let (mut s2, mut s3) = (0f32, 0f32);
    for (i, tv) in t.iter().enumerate().take(2000) {
        s2 += dot(*tv, [c2[i * 3], c2[i * 3 + 1], c2[i * 3 + 2]]).abs();
        s3 += dot(*tv, [c3[i * 3], c3[i * 3 + 1], c3[i * 3 + 2]]).abs();
    }
    if s2 > s3 { 2 } else { 3 }
}

fn slot_of(name: &str, slots: &[String]) -> Option<usize> {
    let base = name.split('.').next().unwrap_or(name);
    slots.iter().position(|s| s == name || s == base)
}

/// The geometry rebuilt from new primitives, in the original vertex layout.
fn rebuild_geometry(orig: &Geometry, prims: &[Prim], notes: &mut Vec<String>, gi: usize) -> Result<Geometry> {
    if orig.channels.iter().any(|c| c.semantic == 6 || c.semantic == 7) {
        bail!("geometry {gi} is skinned; repacking skinned meshes is not supported yet");
    }
    let tsem = tangent_semantic(orig);
    // Group primitives by slot so each slot is one batch.
    let mut by_slot: Vec<(usize, Vec<&Prim>)> = Vec::new();
    for p in prims {
        let s = slot_of(&p.material, &orig.materials).unwrap_or_else(|| {
            notes.push(format!("geometry {gi}: material '{}' matches no slot, used '{}'", p.material, orig.materials.first().cloned().unwrap_or_default()));
            0
        });
        match by_slot.iter_mut().find(|(x, _)| *x == s) {
            Some((_, v)) => v.push(p),
            None => by_slot.push((s, vec![p])),
        }
    }
    by_slot.sort_by_key(|(s, _)| *s);
    let mut verts: Vec<u8> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut batches = Vec::new();
    let (mut min, mut max, mut radius) = ([f32::MAX; 3], [f32::MIN; 3], 0f32);
    let mut count = 0u32;
    for (slot, list) in &by_slot {
        let start_tri = idx.len() as u32 / 3;
        for p in list {
            let n = p.positions.len();
            let normals: Vec<[f32; 3]> = if p.normals.len() == n { p.normals.clone() } else { vec![[0., 0., 1.]; n] };
            let uv0 = p.uvs.first().filter(|u| u.len() == n).cloned().unwrap_or_else(|| vec![[0., 0.]; n]);
            let (tangents, signs): (Vec<[f32; 3]>, Vec<f32>) = if p.tangents.len() == n {
                (p.tangents.iter().map(|t| [t[0], t[1], t[2]]).collect(), p.tangents.iter().map(|t| if t[3] < 0.0 { -1.0 } else { 1.0 }).collect())
            } else {
                (uv_tangents(&p.positions, &uv0, &p.indices), vec![1.0; n])
            };
            for v in 0..n {
                let pos = p.positions[v];
                for k in 0..3 {
                    min[k] = min[k].min(pos[k]);
                    max[k] = max[k].max(pos[k]);
                }
                radius = radius.max(dot(pos, pos).sqrt());
                let nrm = normals[v];
                // Keep the tangent at right angles to the normal.
                let t = normalize(sub(tangents[v], [nrm[0] * dot(nrm, tangents[v]), nrm[1] * dot(nrm, tangents[v]), nrm[2] * dot(nrm, tangents[v])]));
                let bi = cross(nrm, t).map(|x| x * signs[v]);
                for c in &orig.channels {
                    let vals: Vec<f32> = match (c.semantic, c.format) {
                        (0, _) => pos.to_vec(),
                        (1, _) => nrm.to_vec(),
                        (s, _) if s == tsem => t.to_vec(),
                        (2 | 3, _) => bi.to_vec(),
                        (4, _) => p.uvs.get(c.set as usize).filter(|u| u.len() == n).map_or(uv0[v], |u| u[v]).to_vec(),
                        (5, _) => p.colors.get(v).copied().unwrap_or([1., 1., 1., 1.]).to_vec(),
                        _ => vec![],
                    };
                    let size = unit::format_size(c.format).context("unknown channel format")? as usize / 4;
                    for k in 0..size {
                        verts.extend(vals.get(k).copied().unwrap_or(0.0).to_le_bytes());
                    }
                }
            }
            idx.extend(p.indices.iter().map(|i| i + count));
            count += n as u32;
        }
        let tris = idx.len() as u32 / 3 - start_tri;
        let extra = orig.batches.iter().find(|b| b.material as usize == *slot).map_or(0, |b| b.extra);
        batches.push(unit::Batch { material: *slot as u32, start: start_tri, triangles: tris, extra });
    }
    let wide = orig.index_count > 0 && orig.indices.len() / orig.index_count as usize == 4;
    ensure!(wide || count <= 65535, "geometry {gi} has {count} vertices; this part takes at most 65,535 (split it, or add it to a bigger part)");
    ensure!(verts.len() == count as usize * orig.stride as usize, "vertex layout mismatch");
    let indices: Vec<u8> = if wide { idx.iter().flat_map(|i| i.to_le_bytes()).collect() } else { idx.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect() };
    Ok(Geometry {
        vertices: verts,
        vertex_count: count,
        stride: orig.stride,
        channels: orig.channels.clone(),
        index_format: orig.index_format,
        index_count: idx.len() as u32,
        indices,
        batches,
        min,
        max,
        radius,
        materials: orig.materials.clone(),
        head: orig.head,
    })
}

/// The unit with geometries replaced from a GLB. Meshes named `geometry_N` replace geometry N; other
/// meshes join geometry 0. Geometries the GLB leaves out stay as they were. Everything after the
/// geometry section (nodes, physics, flow) is kept byte for byte.
pub fn rebuild_unit(orig: &[u8], meshes: &[glb_read::Mesh], notes: &mut Vec<String>) -> Result<Vec<u8>> {
    let u = unit::parse(orig)?;
    ensure!(!u.geometries.is_empty(), "the unit has no geometry to replace");
    let mut assigned: HashMap<usize, Vec<Prim>> = HashMap::new();
    for m in meshes {
        let gi = m.name.strip_prefix("geometry_").and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse::<usize>().ok()).filter(|&n| n < u.geometries.len());
        let gi = gi.unwrap_or_else(|| {
            notes.push(format!("mesh '{}' has no geometry_N name; added to geometry 0", m.name));
            0
        });
        assigned.entry(gi).or_default().extend(m.prims.iter().cloned());
    }
    let mut geoms = Vec::new();
    for (gi, g) in u.geometries.iter().enumerate() {
        match assigned.get(&gi) {
            Some(p) => geoms.push(rebuild_geometry(g, p, notes, gi)?),
            None => geoms.push(g.clone()),
        }
    }
    let mut out = Vec::with_capacity(orig.len());
    out.extend(unit::VERSION.to_le_bytes());
    out.extend((geoms.len() as u32).to_le_bytes());
    for g in &geoms {
        unit::write_geometry(&mut out, g);
    }
    out.extend_from_slice(&orig[u.rest_at..]);
    Ok(out)
}

/// Starts a mod project in `dir` from game resources: each model as GLB (with the textures it uses as
/// PNG), each texture as PNG, all listed in `mod.json` so a build replaces them. Edited files keep
/// their names; anything left untouched can be dropped from `replace`.
pub fn start(ex: &crate::export::Exporter, assets: &[&Asset], dir: &Path, id: &str, name: &str) -> Result<Project> {
    std::fs::create_dir_all(dir)?;
    let mut p = match std::fs::read_to_string(dir.join("mod.json")) {
        Ok(t) => serde_json::from_str(&t)?,
        Err(_) => Project { id: id.into(), name: name.into(), version: default_version(), authors: vec![], description: String::new(), replace: vec![] },
    };
    let add = |p: &mut Project, ty: &str, name: String, file: String| {
        if !p.replace.iter().any(|r| r.ty == ty && r.name == name) {
            let start = std::fs::read(dir.join(&file)).ok().map(|b| hash::hex(hash::murmur64(&b)));
            p.replace.push(Replace { ty: ty.into(), name, file, start });
        }
    };
    for a in assets {
        let shown = crate::export::display_name(ex.names, a);
        let leaf = shown.rsplit('/').next().unwrap_or(&shown).to_string();
        match crate::export::kind_of(a.type_hash) {
            crate::export::Kind::Model => {
                let (glb, t) = ex.model_glb(a, true)?;
                std::fs::write(dir.join(format!("{leaf}.glb")), glb)?;
                add(&mut p, "unit", shown.clone(), format!("{leaf}.glb"));
                for (slot, param, tex) in &t.list {
                    let Some(ta) = ex.ar.find(hash::hash("texture"), *tex) else { continue };
                    let (td, _) = ex.ar.read_variant(ta, 0)?;
                    let tname = crate::export::display_name(ex.names, ta);
                    let file = format!("textures/{leaf}_{slot}_{param}.png");
                    std::fs::create_dir_all(dir.join("textures"))?;
                    std::fs::write(dir.join(&file), crate::texture::to_png(&td)?)?;
                    add(&mut p, "texture", tname, file);
                }
            }
            crate::export::Kind::Texture => {
                let (td, _) = ex.ar.read_variant(a, 0)?;
                let file = format!("textures/{leaf}.png");
                std::fs::create_dir_all(dir.join("textures"))?;
                std::fs::write(dir.join(&file), crate::texture::to_png(&td)?)?;
                add(&mut p, "texture", shown, file);
            }
            _ => bail!("only models and textures can start a mod project for now"),
        }
    }
    std::fs::write(dir.join("mod.json"), serde_json::to_string_pretty(&p)? + "\n")?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_png_becomes_a_dds_the_decoder_reads_back_the_same() {
        let (w, h) = (8u32, 4u32);
        let px: Vec<u8> = (0..w * h * 4).map(|i| (i * 37 % 251) as u8).collect();
        let png = crate::texture::encode_png(w, h, &px).unwrap();
        let dds = super::png_to_dds(&png).unwrap();
        let info = crate::texture::info(&dds).unwrap();
        assert_eq!((info.width, info.height, info.mips), (8, 4, 4));
        let (_, _, back) = crate::texture::decode_rgba(&dds).unwrap();
        assert_eq!(back, px);
    }

    #[test]
    fn a_png_takes_the_bc_format_of_the_texture_it_replaces() {
        let (w, h) = (16u32, 8u32);
        let px: Vec<u8> = (0..w * h).flat_map(|i| [((i % w) * 16) as u8, 120, ((i / w) * 32) as u8, 255]).collect();
        let png = crate::texture::encode_png(w, h, &px).unwrap();
        let mut original = vec![0u8; 128];
        original[..4].copy_from_slice(b"DDS ");
        original[4..8].copy_from_slice(&124u32.to_le_bytes());
        original[76..80].copy_from_slice(&32u32.to_le_bytes());
        original[80..84].copy_from_slice(&4u32.to_le_bytes());
        original[84..88].copy_from_slice(b"DXT5");
        let dds = super::png_to_dds_like(&png, &original).unwrap();
        let info = crate::texture::info(&dds).unwrap();
        assert_eq!((info.width, info.height, info.mips, info.format.as_str()), (16, 8, 5, "BC3 (DXT5)"));
        let (_, _, back) = crate::texture::decode_rgba(&dds).unwrap();
        let err: u64 = back.iter().zip(&px).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
        assert!(err / (px.len() as u64) < 12, "mean error {}", err as f64 / px.len() as f64);
        // 16x8 BC3 with mips 8x4, 4x2, 2x1, 1x1: 128 + 32 + 16 + 16 + 16 bytes after the header.
        assert_eq!(dds.len(), 128 + 128 + 32 + 16 + 16 + 16);
    }
}
