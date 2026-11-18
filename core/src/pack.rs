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
//!
//! A project may also list custom weapons. The build then writes a `main.lua` that makes each one through
//! Obsidian API's `api.weapons.define`, usually drawn with a new unit of the same project:
//!
//! ```json
//!   "weapons": [ { "name": "author_breaker", "base": "weapon_assaultrifle", "title": "Breaker",
//!     "model": "author/breaker", "stats": { "power": 120 }, "unlock": "free" } ]
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
    /// Custom weapons the mod makes through Obsidian API (`api.weapons.define`). Each is a JSON object
    /// with the fields that call takes: name, base, title, description, model, icon, stats, attributes,
    /// unlock.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weapons: Vec<serde_json::Value>,
}

/// The Obsidian API version that has `api.weapons`.
pub const WEAPONS_API: &str = "1.54.0";

/// A JSON value as a Lua literal: keys that are plain names bare, any other key in brackets.
fn lua_value(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::Null => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => lua_string(s),
        Value::Array(a) => format!("{{ {} }}", a.iter().map(lua_value).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    let bare = !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && !k.as_bytes()[0].is_ascii_digit();
                    let key = if bare { k.clone() } else { format!("[{}]", lua_string(k)) };
                    format!("{key} = {}", lua_value(&o[k]))
                })
                .collect();
            format!("{{ {} }}", parts.join(", "))
        }
    }
}

/// A Lua string literal: printable ASCII as it is, quotes and backslashes escaped, every other byte as
/// a decimal escape, so UTF-8 text survives any editor.
fn lua_string(s: &str) -> String {
    let mut out = String::from("\"");
    for b in s.bytes() {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b}")),
        }
    }
    out.push('"');
    out
}

/// The entry script of a mod that makes custom weapons: one `api.weapons.define` per weapon, each
/// logged when it fails, and nothing at all with an Obsidian API too old to have them.
pub fn weapons_script(name: &str, weapons: &[serde_json::Value]) -> String {
    let name = name.replace(['\n', '\r'], " ").replace('"', "'");
    let mut out = String::new();
    out.push_str(&format!("-- {name}: made by Minediver. The loader runs this once when the game boots.\n"));
    out.push_str("local api = obsidian.import(\"obsidian.api\")\n");
    out.push_str("if not (api and api.weapons and api.weapons.define) then\n");
    out.push_str(&format!("\tobsidian.log({})\n", lua_string(&format!("{name} needs Obsidian API {WEAPONS_API} or later"))));
    out.push_str("\treturn\nend\n\nlocal WEAPONS = {\n");
    for w in weapons {
        out.push_str(&format!("\t{},\n", lua_value(w)));
    }
    out.push_str("}\n\nfor _, def in ipairs(WEAPONS) do\n");
    out.push_str("\tlocal made, why = api.weapons.define(def)\n");
    out.push_str("\tif not made then obsidian.log(\"weapon \" .. tostring(def.name) .. \" not made: \" .. tostring(why)) end\n");
    out.push_str("end\n");
    out
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
    /// Set for a new resource: the game resource it is made from. `name` is then the new name, the
    /// resource goes into the bundles of the base, and a new unit gets a copy of the base material under
    /// its own name, pointing at the new textures of the project that name their base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// Every 8-byte run of `from` in `data` (at any offset) becomes `to`. Returns how many.
fn swap_hash(data: &mut [u8], from: u64, to: u64) -> usize {
    let (f, t) = (from.to_le_bytes(), to.to_le_bytes());
    let mut n = 0;
    let mut i = 0;
    while i + 8 <= data.len() {
        if data[i..i + 8] == f {
            data[i..i + 8].copy_from_slice(&t);
            n += 1;
            i += 8;
        } else {
            i += 1;
        }
    }
    n
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
    // New textures by the hash of their base, for the materials of new units.
    let changed = |r: &Replace| -> bool {
        let now = std::fs::read(project_dir.join(&r.file)).ok().map(|b| hash::hex(hash::murmur64(&b)));
        r.start.is_none() || now != r.start
    };
    // Only new textures that were edited: an untouched one is left out and the material keeps the game's.
    let retarget: HashMap<u64, u64> = p.replace.iter().filter(|r| r.ty == "texture" && changed(r)).filter_map(|r| r.base.as_ref().map(|b| (parse_name(b), parse_name(&r.name)))).collect();
    for (i, r) in p.replace.iter().enumerate() {
        let th = hash::hash(&r.ty);
        let nh = parse_name(&r.name);
        let base_h = r.base.as_deref().map(parse_name).unwrap_or(nh);
        let copies: Vec<&Asset> = ar.assets.iter().filter(|a| a.type_hash == th && a.name_hash == base_h && a.patch.is_none()).collect();
        let first = copies.first().with_context(|| format!("{} {} is not in the game", r.ty, r.base.as_deref().unwrap_or(&r.name)))?;
        let src = std::fs::read(project_dir.join(&r.file)).with_context(|| format!("read {}", r.file))?;
        // A new unit is always built (it is the point of the entry); anything else unchanged is left out.
        let new_unit = r.base.is_some() && r.ty == "unit";
        if !new_unit && r.start.as_deref() == Some(hash::hex(hash::murmur64(&src)).as_str()) {
            notes.push(format!("{} is unchanged, left out", r.file));
            continue;
        }
        let (orig, _) = ar.read_variant(first, 0)?;
        let bytes = match r.ty.as_str() {
            "unit" => {
                let ext = Path::new(&r.file).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if ext == "unit" {
                    src
                } else {
                    let bone_names = ar.find(hash::hash("bones"), base_h).and_then(|b| ar.read_variant(b, 0).ok()).map(|(bd, _)| unit::bone_names(&bd)).unwrap_or_default();
                    rebuild_unit(&orig, &glb_read::read(&src)?, &bone_names, &mut notes)?
                }
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
        let mut bytes = bytes;
        if r.base.is_some() && r.ty == "unit" {
            // The unit names its material by its own name: point it at the copy made below.
            let u = unit::parse(&bytes)?;
            let n = swap_hash(&mut bytes[u.rest_at..], base_h, nh);
            let mat_th = hash::hash("material");
            let mat = ar.assets.iter().find(|a| a.type_hash == mat_th && a.name_hash == base_h && a.patch.is_none());
            match mat {
                Some(m) => {
                    let (mut md, _) = ar.read_variant(m, 0)?;
                    let linker = crate::links::Linker::new(ar);
                    for st in crate::links::material_textures(&linker, &md) {
                        if let Some(&new) = retarget.get(&st.texture) {
                            swap_hash(&mut md, st.texture, new);
                        }
                    }
                    let mleaf = format!("res/{i:03}_{}.material", r.name.rsplit('/').next().unwrap_or("x"));
                    std::fs::write(out.join(&mleaf), &md)?;
                    let mut mb: Vec<u64> = copies.iter().map(|a| a.bundle).collect();
                    mb.sort_unstable();
                    mb.dedup();
                    for b in mb {
                        resources.push(json!({ "type": "material", "name": r.name, "bundle": format!("{b:016x}"), "data": mleaf }));
                    }
                }
                None => notes.push(format!("{}: the base unit has no material of its own name; the new unit uses the base material", r.name)),
            }
            if n == 0 {
                notes.push(format!("{}: the base unit does not name its material; the new unit draws with the base material", r.name));
            }
        }
        let leaf = format!("res/{i:03}_{}.{}", r.name.rsplit('/').next().unwrap_or("x").trim_start_matches('#'), r.ty);
        std::fs::write(out.join(&leaf), &bytes)?;
        let ty_field = if ["lua", "texture", "material", "font", "timpani_bank", "bik"].contains(&r.ty.as_str()) { r.ty.clone() } else { format!("#{th:016x}") };
        let name_field = if r.base.is_some() { r.name.clone() } else { names.get(nh).unwrap_or_else(|| format!("#{nh:016x}")) };
        let mut bundles: Vec<u64> = copies.iter().map(|a| a.bundle).collect();
        bundles.sort_unstable();
        bundles.dedup();
        for b in bundles {
            resources.push(json!({ "type": ty_field, "name": name_field, "bundle": format!("{b:016x}"), "data": leaf }));
        }
    }
    let mut manifest = json!({
        "obsidian": 1,
        "id": p.id,
        "version": p.version,
        "name": { "en": p.name },
        "description": { "en": if p.description.is_empty() { "Replaces game models and textures.".to_string() } else { p.description.clone() } },
        "authors": p.authors,
        "scope": "client",
        "resources": resources,
    });
    if p.weapons.is_empty() {
        let _ = std::fs::remove_file(out.join("main.lua"));
    } else {
        for w in &p.weapons {
            ensure!(w.get("name").and_then(|v| v.as_str()).is_some() && w.get("base").and_then(|v| v.as_str()).is_some(), "each weapon needs a name and a base");
        }
        std::fs::write(out.join("main.lua"), weapons_script(&p.name, &p.weapons))?;
        // A custom weapon has its own numbers, which the other players' games play against: a session mod.
        manifest["scope"] = json!("session");
        manifest["entry"] = json!("main.lua");
        manifest["requires"] = json!(["obsidian.api"]);
        notes.push(format!("main.lua makes {} custom weapon(s) and needs Obsidian API {WEAPONS_API}", p.weapons.len()));
    }
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
    // Every game texture ends with a 4-byte word after the last mip (a category the game reads); the new
    // file keeps the original's. Without it the game hangs on the loading screen (seen 2026-11-14).
    let tail = original.get(original.len().saturating_sub(4)..).unwrap_or(&[]).to_vec();
    let single = original.len() >= 32 && u32::from_le_bytes(original[28..32].try_into().unwrap()) <= 1;
    let mips = if single { mips.into_iter().take(1).collect() } else { mips };
    let Some(fmt) = fmt else {
        let mut out = bgra_dds(w, h, &mips);
        out.extend(tail);
        return Ok(out);
    };
    // The original's header, flags included, with the new size, first-level size and mip count.
    let mut out = original[..128].to_vec();
    let put = |o: &mut Vec<u8>, at: usize, v: u32| o[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut out, 12, h);
    put(&mut out, 16, w);
    put(&mut out, 20, fmt.compressed_size(w as usize, h as usize) as u32);
    put(&mut out, 24, 0);
    put(&mut out, 28, mips.len() as u32);
    let params = texpresso::Params { algorithm: texpresso::Algorithm::ClusterFit, ..Default::default() };
    for (mw, mh, px) in &mips {
        let mut block = vec![0u8; fmt.compressed_size(*mw as usize, *mh as usize)];
        fmt.compress(px, *mw as usize, *mh as usize, params, &mut block);
        out.extend(block);
    }
    out.extend(tail);
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

/// What a skinned geometry needs to write its blend channels: its skin, and the skin bone of each
/// joint name.
struct SkinCtx<'a> {
    skin: &'a unit::Skin,
    bone_of: HashMap<String, Vec<usize>>,
}

/// Blend indices and weights of one vertex, in the bone set of its batch. Joints outside the set are
/// dropped (counted in `dropped`) and the rest weigh 1 together.
fn blend(p: &Prim, v: usize, ctx: &SkinCtx, set: &[u32], comps: usize, dropped: &mut usize) -> ([f32; 4], [f32; 4]) {
    let mut infl: Vec<(f32, f32)> = Vec::new();
    if let (Some(j), Some(w)) = (p.joints.get(v), p.weights.get(v)) {
        for k in 0..4 {
            if w[k] <= 0.0 {
                continue;
            }
            let bones = p.joint_names.get(j[k] as usize).and_then(|n| ctx.bone_of.get(n));
            match bones.and_then(|bs| bs.iter().find_map(|b| set.iter().position(|&s| s as usize == *b))) {
                Some(local) => infl.push((local as f32, w[k])),
                None => *dropped += 1,
            }
        }
    }
    infl.sort_by(|a, b| b.1.total_cmp(&a.1));
    infl.truncate(comps.clamp(1, 4));
    let sum: f32 = infl.iter().map(|i| i.1).sum();
    let (mut bi, mut bw) = ([0f32; 4], [0f32; 4]);
    if sum <= 1e-6 {
        bw[0] = 1.0;
    } else {
        for (k, (b, w)) in infl.iter().enumerate() {
            bi[k] = *b;
            bw[k] = w / sum;
        }
    }
    (bi, bw)
}

/// The geometry rebuilt from new primitives, in the original vertex layout. A skinned geometry gets one
/// batch per (slot, bone set): each triangle goes to the first set of the skin that holds every bone
/// it uses (or the one that holds most of them), and a vertex shared by two sets is written twice,
/// since blend indices count within a set.
fn rebuild_geometry(orig: &Geometry, prims: &[Prim], notes: &mut Vec<String>, gi: usize, ctx: Option<&SkinCtx>) -> Result<Geometry> {
    let skinned = orig.channels.iter().any(|c| c.semantic == 6 || c.semantic == 7);
    if skinned && ctx.is_none() {
        bail!("geometry {gi} is skinned and the skeleton of the unit was not found");
    }
    if skinned && prims.iter().any(|p| p.joints.len() != p.positions.len()) {
        bail!("geometry {gi} is skinned: keep the armature and export the mesh with skinning (JOINTS_0 and WEIGHTS_0)");
    }
    let comps_of = |s: u32| orig.channels.iter().find(|c| c.semantic == s).and_then(|c| unit::format_size(c.format)).map_or(1, |b| b as usize / 4);
    let blend_comps = comps_of(6).min(comps_of(7));
    let tsem = tangent_semantic(orig);
    let mut dropped = 0usize;

    // Per primitive: slot, and the frame of every vertex.
    struct Frame {
        normals: Vec<[f32; 3]>,
        tangents: Vec<[f32; 3]>,
        bitangents: Vec<[f32; 3]>,
        uv0: Vec<[f32; 2]>,
    }
    let mut slots = Vec::new();
    let mut frames = Vec::new();
    for p in prims {
        let s = slot_of(&p.material, &orig.materials).unwrap_or_else(|| {
            notes.push(format!("geometry {gi}: material '{}' matches no slot, used '{}'", p.material, orig.materials.first().cloned().unwrap_or_default()));
            0
        });
        slots.push(s);
        let n = p.positions.len();
        let normals: Vec<[f32; 3]> = if p.normals.len() == n { p.normals.clone() } else { vec![[0., 0., 1.]; n] };
        let uv0 = p.uvs.first().filter(|u| u.len() == n).cloned().unwrap_or_else(|| vec![[0., 0.]; n]);
        let (raw, signs): (Vec<[f32; 3]>, Vec<f32>) = if p.tangents.len() == n {
            (p.tangents.iter().map(|t| [t[0], t[1], t[2]]).collect(), p.tangents.iter().map(|t| if t[3] < 0.0 { -1.0 } else { 1.0 }).collect())
        } else {
            (uv_tangents(&p.positions, &uv0, &p.indices), vec![1.0; n])
        };
        let mut tangents = Vec::with_capacity(n);
        let mut bitangents = Vec::with_capacity(n);
        for v in 0..n {
            let nrm = normals[v];
            // Keep the tangent at right angles to the normal.
            let d = dot(nrm, raw[v]);
            let t = normalize(sub(raw[v], [nrm[0] * d, nrm[1] * d, nrm[2] * d]));
            tangents.push(t);
            bitangents.push(cross(nrm, t).map(|x| x * signs[v]));
        }
        frames.push(Frame { normals, tangents, bitangents, uv0 });
    }

    // The skin bones a vertex leans on.
    let bones_of = |p: &Prim, v: usize| -> Vec<usize> {
        let (Some(c), Some(j), Some(w)) = (ctx, p.joints.get(v), p.weights.get(v)) else { return Vec::new() };
        (0..4).filter(|&k| w[k] > 0.0).filter_map(|k| p.joint_names.get(j[k] as usize).and_then(|n| c.bone_of.get(n)).and_then(|bs| bs.first()).copied()).collect()
    };
    let pick_set = |need: &[usize]| -> usize {
        let Some(c) = ctx else { return 0 };
        let holds = |s: &Vec<u32>| need.iter().filter(|b| s.iter().any(|x| *x as usize == **b)).count();
        c.skin.sets.iter().position(|s| holds(s) == need.len()).unwrap_or_else(|| {
            (0..c.skin.sets.len()).max_by_key(|&i| holds(&c.skin.sets[i])).unwrap_or(0)
        })
    };

    // Triangles by (slot, set), in slot order then set order.
    let mut groups: std::collections::BTreeMap<(usize, usize), Vec<(usize, [u32; 3])>> = Default::default();
    for (pi, p) in prims.iter().enumerate() {
        for t in p.indices.chunks_exact(3) {
            if t.iter().any(|&v| v as usize >= p.positions.len()) {
                continue;
            }
            let set = if skinned {
                let mut need: Vec<usize> = t.iter().flat_map(|&v| bones_of(p, v as usize)).collect();
                need.sort_unstable();
                need.dedup();
                pick_set(&need)
            } else {
                0
            };
            groups.entry((slots[pi], set)).or_default().push((pi, [t[0], t[1], t[2]]));
        }
    }

    let mut verts: Vec<u8> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut batches = Vec::new();
    let (mut min, mut max, mut radius) = ([f32::MAX; 3], [f32::MIN; 3], 0f32);
    let mut count = 0u32;
    for ((slot, set), tris) in &groups {
        let start_tri = idx.len() as u32 / 3;
        let set_bones: &[u32] = ctx.and_then(|c| c.skin.sets.get(*set)).map_or(&[], |s| s.as_slice());
        let mut placed: HashMap<(usize, u32), u32> = HashMap::new();
        for (pi, t) in tris {
            for &v in t {
                if let Some(&at) = placed.get(&(*pi, v)) {
                    idx.push(at);
                    continue;
                }
                let p = &prims[*pi];
                let f = &frames[*pi];
                let vu = v as usize;
                let pos = p.positions[vu];
                for k in 0..3 {
                    min[k] = min[k].min(pos[k]);
                    max[k] = max[k].max(pos[k]);
                }
                radius = radius.max(dot(pos, pos).sqrt());
                let (bidx, bwt) = match ctx {
                    Some(c) if skinned => blend(p, vu, c, set_bones, blend_comps, &mut dropped),
                    _ => ([0.0; 4], [1.0, 0.0, 0.0, 0.0]),
                };
                for c in &orig.channels {
                    let vals: Vec<f32> = match c.semantic {
                        0 => pos.to_vec(),
                        1 => f.normals[vu].to_vec(),
                        s if s == tsem => f.tangents[vu].to_vec(),
                        2 | 3 => f.bitangents[vu].to_vec(),
                        4 => p.uvs.get(c.set as usize).filter(|u| u.len() == p.positions.len()).map_or(f.uv0[vu], |u| u[vu]).to_vec(),
                        5 => p.colors.get(vu).copied().unwrap_or([1., 1., 1., 1.]).to_vec(),
                        6 => bidx.to_vec(),
                        7 => bwt.to_vec(),
                        _ => vec![],
                    };
                    let size = unit::format_size(c.format).context("unknown channel format")? as usize / 4;
                    for k in 0..size {
                        verts.extend(vals.get(k).copied().unwrap_or(0.0).to_le_bytes());
                    }
                }
                placed.insert((*pi, v), count);
                idx.push(count);
                count += 1;
            }
        }
        let tri_count = idx.len() as u32 / 3 - start_tri;
        let extra = if skinned { *set as u32 } else { orig.batches.iter().find(|b| b.material as usize == *slot).map_or(0, |b| b.extra) };
        batches.push(unit::Batch { material: *slot as u32, start: start_tri, triangles: tri_count, extra });
    }
    if dropped > 0 {
        notes.push(format!("geometry {gi}: {dropped} bone weights pointed at bones this part cannot use and were left out"));
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
pub fn rebuild_unit(orig: &[u8], meshes: &[glb_read::Mesh], bone_names: &HashMap<u32, String>, notes: &mut Vec<String>) -> Result<Vec<u8>> {
    let u = unit::parse(orig)?;
    ensure!(!u.geometries.is_empty(), "the unit has no geometry to replace");
    let (skins, skins_end) = unit::skins(orig, u.rest_at)?;
    let hashes: Vec<u32> = bone_names.keys().copied().collect();
    let graph = if skins.is_empty() { None } else { unit::find_graph(orig, skins_end, &skins, &hashes) };
    // Joint names as Minediver exports them: the bone name, or node_<index> without one.
    let ctxs: Vec<SkinCtx> = match &graph {
        Some(nodes) => skins
            .iter()
            .map(|skin| {
                let mut bone_of = HashMap::new();
                for (i, n) in nodes.iter().enumerate() {
                    let label = bone_names.get(&n.name).cloned().unwrap_or_else(|| format!("node_{i}"));
                    for (b, _) in skin.nodes.iter().enumerate().filter(|(_, s)| **s as usize == i) {
                        bone_of.entry(label.clone()).or_insert_with(Vec::new).push(b);
                    }
                }
                SkinCtx { skin, bone_of }
            })
            .collect(),
        None => Vec::new(),
    };
    let mut skin_k = 0usize;
    let mut ctx_of: Vec<Option<usize>> = Vec::new();
    for g in &u.geometries {
        if g.channels.iter().any(|c| c.semantic == 6) {
            ctx_of.push((skin_k < ctxs.len()).then_some(skin_k));
            skin_k += 1;
        } else {
            ctx_of.push(None);
        }
    }
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
            Some(p) => {
                let ctx = ctx_of[gi].map(|k| &ctxs[k]);
                let mut p = p.clone();
                if let (Some(c), Some(nodes)) = (ctx, &graph) {
                    into_part_space(&mut p, c, nodes);
                }
                geoms.push(rebuild_geometry(g, &p, notes, gi, ctx)?)
            }
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
/// With `new_prefix`, the project adds new resources instead (named `<prefix><file name>`, each with the
/// game resource it comes from as `base`), so the game keeps the originals.
pub fn start(ex: &crate::export::Exporter, assets: &[&Asset], dir: &Path, id: &str, name: &str, new_prefix: Option<&str>) -> Result<Project> {
    std::fs::create_dir_all(dir)?;
    let mut p = match std::fs::read_to_string(dir.join("mod.json")) {
        Ok(t) => serde_json::from_str(&t)?,
        Err(_) => Project { id: id.into(), name: name.into(), version: default_version(), authors: vec![], description: String::new(), replace: vec![], weapons: vec![] },
    };
    let add = |p: &mut Project, ty: &str, name: String, file: String| {
        let (name, base) = match new_prefix {
            Some(prefix) => {
                let stem = Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("new").to_string();
                (format!("{prefix}{stem}"), Some(name))
            }
            None => (name, None),
        };
        if !p.replace.iter().any(|r| r.ty == ty && r.name == name) {
            let start = std::fs::read(dir.join(&file)).ok().map(|b| hash::hex(hash::murmur64(&b)));
            p.replace.push(Replace { ty: ty.into(), name, file, start, base });
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
mod weapon_tests {
    use super::*;

    #[test]
    fn a_weapon_becomes_a_define_call_with_its_fields_as_lua() {
        let w = json!({ "name": "me_breaker", "base": "weapon_assaultrifle", "title": { "en": "Breaker", "pt-BR": "Quebra" },
            "model": "me/breaker", "stats": { "power": 120, "magazine_capacity": 12 }, "unlock": "free" });
        let lua = weapons_script("My \"pack\"", &[w]);
        assert!(lua.starts_with("-- My 'pack': made by Minediver"), "{lua}");
        assert!(lua.contains("local api = obsidian.import(\"obsidian.api\")"));
        assert!(lua.contains("name = \"me_breaker\""), "{lua}");
        assert!(lua.contains("stats = { magazine_capacity = 12, power = 120 }"), "{lua}");
        assert!(lua.contains("[\"pt-BR\"] = \"Quebra\""), "{lua}");
        assert!(lua.contains("api.weapons.define(def)"));
    }

    #[test]
    fn lua_strings_escape_what_lua_needs() {
        assert_eq!(lua_string("a\\b\"c\nd"), "\"a\\\\b\\\"c\\10d\"");
        assert_eq!(lua_string("caf\u{e9}"), "\"caf\\195\\169\"");
        assert_eq!(lua_value(&json!([1, true, null])), "{ 1, true, nil }");
    }
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
        original[28..32].copy_from_slice(&5u32.to_le_bytes());
        let dds = super::png_to_dds_like(&png, &original).unwrap();
        let info = crate::texture::info(&dds).unwrap();
        assert_eq!((info.width, info.height, info.mips, info.format.as_str()), (16, 8, 5, "BC3 (DXT5)"));
        let (_, _, back) = crate::texture::decode_rgba(&dds).unwrap();
        let err: u64 = back.iter().zip(&px).map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
        assert!(err / (px.len() as u64) < 12, "mean error {}", err as f64 / px.len() as f64);
        // 16x8 BC3 with mips 8x4, 4x2, 2x1, 1x1: 128 + 32 + 16 + 16 + 16 bytes after the header.
        assert_eq!(dds.len(), 128 + 128 + 32 + 16 + 16 + 16 + 4, "header, five mips, the original tail");
        assert_eq!(dds[dds.len() - 4..], original[124..128]);
    }
}

type M4 = [f32; 16];

fn mul4(a: &M4, b: &M4) -> M4 {
    let mut o = [0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

/// Inverse of an affine column-major 4x4.
fn invert_affine(m: &M4) -> Option<M4> {
    let (a, b, c) = ([m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]);
    let det = dot(a, cross(b, c));
    if det.abs() < 1e-12 {
        return None;
    }
    // Rows of the inverse 3x3 are the cross products of the columns, over the determinant.
    let r0 = cross(b, c).map(|x| x / det);
    let r1 = cross(c, a).map(|x| x / det);
    let r2 = cross(a, b).map(|x| x / det);
    let t = [m[12], m[13], m[14]];
    let it = [-dot(r0, t), -dot(r1, t), -dot(r2, t)];
    Some([r0[0], r1[0], r2[0], 0.0, r0[1], r1[1], r2[1], 0.0, r0[2], r1[2], r2[2], 0.0, it[0], it[1], it[2], 1.0])
}

fn apply4(m: &M4, p: [f32; 3], w: f32) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r] * w)
}

/// Moves a skinned part from the bind space of the GLB into the space the game keeps the part in. A
/// vertex goes through its strongest joint: out by that joint's bind transform in the file, into glTF to
/// game axes, back by the bind transform of the same bone in the game. For a file Minediver wrote this
/// changes nothing; for one Blender wrote it holds while the rest pose is the one exported.
fn into_part_space(prims: &mut [Prim], ctx: &SkinCtx, nodes: &[unit::Node]) {
    const TO_GAME: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    for p in prims {
        let mut cache: HashMap<usize, Option<M4>> = HashMap::new();
        let mut to_part = |k: usize| -> Option<M4> {
            *cache.entry(k).or_insert_with(|| {
                let bone = *p.joint_names.get(k).and_then(|n| ctx.bone_of.get(n))?.first()?;
                let node = nodes.get(*ctx.skin.nodes.get(bone)? as usize)?;
                let game = invert_affine(&mul4(&node.world, ctx.skin.inverse_bind.get(bone)?))?;
                Some(mul4(&game, &mul4(&TO_GAME, p.joint_binds.get(k)?)))
            })
        };
        for v in 0..p.positions.len() {
            let (Some(j), Some(w)) = (p.joints.get(v), p.weights.get(v)) else { continue };
            let k = (0..4).max_by(|a, b| w[*a].total_cmp(&w[*b])).unwrap_or(0);
            let Some(m) = to_part(j[k] as usize) else { continue };
            p.positions[v] = apply4(&m, p.positions[v], 1.0);
            if let Some(n) = p.normals.get_mut(v) {
                *n = normalize(apply4(&m, *n, 0.0));
            }
            if let Some(t) = p.tangents.get_mut(v) {
                let d = normalize(apply4(&m, [t[0], t[1], t[2]], 0.0));
                *t = [d[0], d[1], d[2], t[3]];
            }
        }
    }
}
