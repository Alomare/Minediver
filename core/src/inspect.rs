//! What the details pane shows about a resource.

use anyhow::Result;
use serde_json::{Value, json};

use crate::archive::Asset;
use crate::export::{Exporter, Kind, display_name, kind_of};
use crate::{audio, hash, texture, unit};

pub fn semantic_name(s: u32) -> &'static str {
    match s {
        0 => "position",
        1 => "normal",
        2 => "binormal",
        3 => "tangent",
        4 => "uv",
        5 => "color",
        6 => "blend indices",
        7 => "blend weights",
        _ => "unknown",
    }
}

pub fn inspect(ex: &Exporter, a: &Asset) -> Result<Value> {
    let kind = kind_of(a.type_hash);
    let mut v = json!({
        "name": display_name(ex.names, a),
        "named": ex.names.contains(a.name_hash),
        "hash": hash::hex(a.name_hash),
        "type": hash::type_label(a.type_hash),
        "kind": kind,
        "bundle": a.file_name(),
        "bundle_name": ex.names.get(a.bundle),
        "flags": a.flags,
        "size": a.size(),
        "variants": a.variants.iter().map(|x| json!({ "lang": x.lang, "size": x.size, "stream": x.stream_size })).collect::<Vec<_>>(),
        "copies": ex.ar.assets.iter().filter(|o| o.type_hash == a.type_hash && o.name_hash == a.name_hash).count(),
    });
    let (data, _) = ex.ar.read_variant(a, 0)?;
    match kind {
        Kind::Model => match unit::parse(&data) {
            Ok(u) => {
                let tex = ex.model_textures(a, &u, false);
                let geoms: Vec<Value> = u
                    .geometries
                    .iter()
                    .map(|g| {
                        json!({
                            "vertices": g.vertex_count,
                            "triangles": g.index_count / 3,
                            "stride": g.stride,
                            "channels": g.channels.iter().map(|c| if c.set > 0 { format!("{} {}", semantic_name(c.semantic), c.set) } else { semantic_name(c.semantic).to_string() }).collect::<Vec<_>>(),
                            "slots": g.materials,
                            "skinned": g.channels.iter().any(|c| c.semantic == 6),
                            "min": g.min, "max": g.max,
                        })
                    })
                    .collect();
                v["model"] = json!({
                    "geometries": geoms,
                    "vertices": u.geometries.iter().map(|g| g.vertex_count).sum::<u32>(),
                    "triangles": u.geometries.iter().map(|g| g.index_count / 3).sum::<u32>(),
                    "material": ex.unit_material(a).map(|m| m.file_name()),
                    "textures": tex.list.iter().map(|(s, p, t)| json!({ "slot": s, "param": p, "texture": hash::hex(*t), "name": ex.names.get(*t) })).collect::<Vec<_>>(),
                });
            }
            Err(e) => v["error"] = json!(format!("{e:#}")),
        },
        Kind::Texture => match texture::info(&data) {
            Ok(i) => v["texture"] = json!(i),
            Err(e) => v["error"] = json!(format!("{e:#}")),
        },
        Kind::Audio if hash::type_name(a.type_hash) == Some("timpani_bank") => match audio::clips(&data) {
            Ok(c) => {
                v["audio"] = json!({
                    "clips": c.len(),
                    "list": c.iter().take(500).map(|c| json!({ "id": hash::hex(c.id), "codec": c.codec_name(), "channels": c.channels, "rate": c.rate, "seconds": c.duration() })).collect::<Vec<_>>(),
                })
            }
            Err(e) => v["error"] = json!(format!("{e:#}")),
        },
        _ => {}
    }
    Ok(v)
}
