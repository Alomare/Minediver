//! Turns resources into files people can open: GLB, PNG, WAV/OGG/MP3, MP4, text, or the raw bytes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::archive::{Archive, Asset};
use crate::hash::{self, NameDb};
use crate::links::{self, Linker};
use crate::{audio, gltf, texture, unit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Model,
    Texture,
    Audio,
    Video,
    Script,
    Text,
    Material,
    Animation,
    Level,
    Other,
}

pub fn kind_of(type_hash: u64) -> Kind {
    match hash::type_name(type_hash) {
        Some("unit") => Kind::Model,
        Some("texture") => Kind::Texture,
        Some("timpani_bank" | "wav" | "wwise_bank" | "wwise_stream") => Kind::Audio,
        Some("bik" | "ivf") => Kind::Video,
        Some("lua" | "flow" | "script_flow_nodes") => Kind::Script,
        Some("strings" | "config" | "ini" | "network_config" | "render_config") => Kind::Text,
        Some("material" | "shader" | "shader_library" | "shader_library_group" | "shading_environment") => Kind::Material,
        Some("animation" | "animation_curves" | "bones" | "state_machine") => Kind::Animation,
        Some("level" | "navdata") => Kind::Level,
        _ => Kind::Other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// "glb" or "raw".
    pub model: String,
    /// "png", "dds" or "both".
    pub texture: String,
    /// "native" (WAV for PCM, OGG for Vorbis), "wav", "mp3" or "ogg". Anything but native needs ffmpeg.
    pub audio: String,
    /// "mp4" (needs ffmpeg) or "bik".
    pub video: String,
    /// Also write each texture a model uses as PNG beside the GLB.
    pub model_textures: bool,
    /// Also write the raw resource bytes.
    pub raw: bool,
    /// Every language variant, not only the first.
    pub all_variants: bool,
    pub ffmpeg: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options { model: "glb".into(), texture: "png".into(), audio: "native".into(), video: "mp4".into(), model_textures: true, raw: false, all_variants: false, ffmpeg: None }
    }
}

/// Where the archive's resources go on disk: the name with its folders, or `_unnamed/<type>/<hash>`.
pub fn base_path(names: &NameDb, a: &Asset) -> PathBuf {
    match names.get(a.name_hash) {
        Some(n) => n.split('/').filter(|s| !s.is_empty() && *s != ".." && *s != ".").collect(),
        None => PathBuf::from("_unnamed").join(hash::type_label(a.type_hash)).join(hash::hex(a.name_hash)),
    }
}

pub fn display_name(names: &NameDb, a: &Asset) -> String {
    names.get(a.name_hash).unwrap_or_else(|| hash::hex(a.name_hash))
}

fn with_ext(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

/// ffmpeg from the setting, PATH, or the usual install places.
pub fn find_ffmpeg(setting: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = setting.filter(|p| p.is_file()) {
        return Some(p.to_path_buf());
    }
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let c = dir.join(exe);
            if c.is_file() {
                return Some(c);
            }
        }
    }
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let pk = PathBuf::from(local).join("Microsoft").join("WinGet").join("Packages");
            if let Ok(rd) = std::fs::read_dir(&pk) {
                for e in rd.flatten().filter(|e| e.file_name().to_string_lossy().contains("FFmpeg")) {
                    if let Ok(inner) = std::fs::read_dir(e.path()) {
                        for i in inner.flatten() {
                            let c = i.path().join("bin").join(exe);
                            if c.is_file() {
                                return Some(c);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn ffmpeg_convert(ffmpeg: &Path, input: &Path, output: &Path, args: &[&str]) -> Result<()> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(input).args(args).arg(output);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().context("run ffmpeg")?;
    if !out.status.success() {
        bail!("ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

pub struct Exporter<'a> {
    pub ar: &'a Archive,
    pub names: &'a NameDb,
    pub linker: &'a Linker,
}

/// What a model's slots draw with, ready for the GLB.
pub struct ModelTextures {
    pub images: Vec<gltf::Image>,
    pub maps: gltf::MaterialMaps,
    /// (slot, parameter, texture asset)
    pub list: Vec<(String, String, u64)>,
}

impl<'a> Exporter<'a> {
    pub fn new(ar: &'a Archive, names: &'a NameDb, linker: &'a Linker) -> Self {
        Exporter { ar, names, linker }
    }

    /// The material resource that goes with a unit: same name, in the same bundle when it can be.
    pub fn unit_material(&self, a: &Asset) -> Option<&'a Asset> {
        let th = hash::hash("material");
        self.ar.assets.iter().filter(|m| m.type_hash == th && m.name_hash == a.name_hash).min_by_key(|m| (m.bundle != a.bundle, m.patch.is_some()))
    }

    /// Textures of each slot a unit draws with, decoded to PNG when `decode` is set.
    pub fn model_textures(&self, a: &Asset, u: &unit::Unit, decode: bool) -> ModelTextures {
        let mut out = ModelTextures { images: Vec::new(), maps: Default::default(), list: Vec::new() };
        let Some(m) = self.unit_material(a) else { return out };
        let Ok((md, _)) = self.ar.read_variant(m, 0) else { return out };
        let slot_names: HashMap<u32, String> = u.geometries.iter().flat_map(|g| g.materials.iter()).map(|s| (links::hash32(s), s.clone())).collect();
        let mut image_of: HashMap<u64, usize> = HashMap::new();
        for st in links::material_textures(self.linker, &md) {
            let slot = slot_names.get(&st.slot).cloned().unwrap_or_else(|| format!("{:08x}", st.slot));
            let param = st.param_name.map(str::to_string).unwrap_or_else(|| format!("{:08x}", st.param));
            out.list.push((slot.clone(), param.clone(), st.texture));
            if !decode {
                continue;
            }
            let idx = match image_of.get(&st.texture) {
                Some(i) => Some(*i),
                None => self.ar.find(hash::hash("texture"), st.texture).and_then(|t| self.ar.read_variant(t, 0).ok()).and_then(|(td, _)| texture::to_png(&td).ok()).map(|png| {
                    out.images.push(gltf::Image { name: self.names.get(st.texture).unwrap_or_else(|| hash::hex(st.texture)), png });
                    image_of.insert(st.texture, out.images.len() - 1);
                    out.images.len() - 1
                }),
            };
            let e = out.maps.slots.entry(slot).or_default();
            match st.param_name {
                Some("diffuse_map" | "color_map" | "albedo_map" | "base_map" | "diffuse_texture") if e.base.is_none() => e.base = idx,
                Some("normal_map" | "nm_map" | "bump_map" | "normal_texture") if e.normal.is_none() => e.normal = idx,
                _ => {}
            }
        }
        out
    }

    pub fn model_glb(&self, a: &Asset, with_textures: bool) -> Result<(Vec<u8>, ModelTextures)> {
        let (d, _) = self.ar.read_variant(a, 0)?;
        let u = unit::parse(&d)?;
        let t = self.model_textures(a, &u, with_textures);
        let glb = gltf::unit_to_glb(&u, &display_name(self.names, a), &t.images, &t.maps)?;
        Ok((glb, t))
    }

    /// Writes one resource under `out`. Returns the files written.
    pub fn export(&self, a: &Asset, out: &Path, o: &Options) -> Result<Vec<PathBuf>> {
        let base = out.join(base_path(self.names, a));
        let mut written = Vec::new();
        let ty = hash::type_label(a.type_hash);
        let variants: Vec<usize> = if o.all_variants { (0..a.variants.len()).collect() } else { vec![0] };
        for vi in variants {
            let lang = a.variants[vi].lang;
            let base = if vi == 0 { base.clone() } else { with_ext(&base, &format!(".lang{lang}")) };
            let (data, stream) = self.ar.read_variant(a, vi)?;
            if o.raw {
                let p = with_ext(&base, &format!(".{ty}"));
                write(&p, &data)?;
                written.push(p.clone());
                if !stream.is_empty() {
                    let s = with_ext(&p, ".stream");
                    write(&s, &stream)?;
                    written.push(s);
                }
            }
            match kind_of(a.type_hash) {
                Kind::Model if o.model == "glb" => {
                    let u = unit::parse(&data)?;
                    let t = self.model_textures(a, &u, true);
                    let glb = gltf::unit_to_glb(&u, &display_name(self.names, a), &t.images, &t.maps)?;
                    let p = with_ext(&base, ".glb");
                    write(&p, &glb)?;
                    written.push(p);
                    if o.model_textures {
                        for img in &t.images {
                            let leaf = img.name.rsplit('/').next().unwrap_or(&img.name).to_string();
                            let p = with_ext(&base, &format!("_textures/{leaf}.png"));
                            write(&p, &img.png)?;
                            written.push(p);
                        }
                    }
                }
                Kind::Texture => {
                    if o.texture == "dds" || o.texture == "both" {
                        let p = with_ext(&base, ".dds");
                        write(&p, &data)?;
                        written.push(p);
                    }
                    if o.texture == "png" || o.texture == "both" {
                        let p = with_ext(&base, ".png");
                        write(&p, &texture::to_png(&data)?)?;
                        written.push(p);
                    }
                }
                Kind::Audio if ty == "timpani_bank" => {
                    let ff = find_ffmpeg(o.ffmpeg.as_deref());
                    for c in audio::clips(&data)? {
                        let file = audio::clip_file(&data, &stream, &c)?;
                        let native = with_ext(&base, &format!("/{:016x}.{}", c.id, c.extension()));
                        let want = if o.audio == "native" { c.extension() } else { o.audio.as_str() };
                        if want == c.extension() {
                            write(&native, &file)?;
                            written.push(native);
                        } else {
                            let ff = ff.as_deref().context("This audio format needs ffmpeg. Set the ffmpeg path in Settings")?;
                            write(&native, &file)?;
                            let target = native.with_extension(want);
                            let r = ffmpeg_convert(ff, &native, &target, &[]);
                            std::fs::remove_file(&native).ok();
                            r?;
                            written.push(target);
                        }
                    }
                }
                Kind::Video => {
                    // The resource holds a short head; the movie itself is the stream part.
                    let movie = if stream.is_empty() { data.clone() } else { stream.clone() };
                    let ext = if ty == "ivf" { "ivf" } else { "bik" };
                    let raw = with_ext(&base, &format!(".{ext}"));
                    write(&raw, &movie)?;
                    if o.video == "mp4" {
                        let ff = find_ffmpeg(o.ffmpeg.as_deref()).context("MP4 needs ffmpeg. Set the ffmpeg path in Settings")?;
                        let mp4 = with_ext(&base, ".mp4");
                        ffmpeg_convert(&ff, &raw, &mp4, &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-c:a", "aac"])?;
                        std::fs::remove_file(&raw).ok();
                        written.push(mp4);
                    } else {
                        written.push(raw);
                    }
                }
                Kind::Text if ty == "strings" => {
                    let p = with_ext(&base, ".txt");
                    write(&p, strings_to_text(&data)?.as_bytes())?;
                    written.push(p);
                }
                _ if !o.raw => {
                    let ext = match ty.as_str() {
                        "lua" => "luac".to_string(),
                        t => t.to_string(),
                    };
                    let p = with_ext(&base, &format!(".{ext}"));
                    write(&p, &data)?;
                    written.push(p.clone());
                    if !stream.is_empty() {
                        let s = with_ext(&p, ".stream");
                        write(&s, &stream)?;
                        written.push(s);
                    }
                }
                _ => {}
            }
        }
        Ok(written)
    }
}

/// A `strings` resource as text, one `<key hash>\t<text>` line per entry (newlines written as `\n`).
/// Layout: u32 header, u32 count, (u32 key, u32 offset) per entry, then null-terminated UTF-8.
pub fn strings_to_text(d: &[u8]) -> Result<String> {
    let word = |at: usize| -> Result<u32> { Ok(u32::from_le_bytes(d.get(at..at + 4).context("strings table is cut short")?.try_into().unwrap())) };
    let count = word(4)? as usize;
    anyhow::ensure!(8 + count * 8 <= d.len(), "strings table runs past the end");
    let mut out = String::new();
    for i in 0..count {
        let key = word(8 + i * 8)?;
        let off = word(12 + i * 8)? as usize;
        anyhow::ensure!(off < d.len(), "string offset past the end");
        let end = d[off..].iter().position(|&b| b == 0).map_or(d.len(), |p| off + p);
        let text = String::from_utf8_lossy(&d[off..end]).replace('\r', "").replace('\n', "\n");
        out.push_str(&format!("{key:08x}\t{text}\n"));
    }
    Ok(out)
}
