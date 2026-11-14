//! HD1 textures are DDS files as they are, sometimes with a few bytes after the last mip.

use anyhow::{Result, bail, ensure};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub format: String,
    pub cube: bool,
    pub depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fmt {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc6u,
    Bc6s,
    Bc7,
    Bgra8,
    Rgba8,
    Bgrx8,
    L8,
    A8,
}

impl Fmt {
    fn block(self) -> Option<usize> {
        match self {
            Fmt::Bc1 | Fmt::Bc4 => Some(8),
            Fmt::Bc2 | Fmt::Bc3 | Fmt::Bc5 | Fmt::Bc6u | Fmt::Bc6s | Fmt::Bc7 => Some(16),
            _ => None,
        }
    }
    fn bpp(self) -> usize {
        match self {
            Fmt::L8 | Fmt::A8 => 1,
            _ => 4,
        }
    }
}

fn word(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

struct Header {
    info: Info,
    fmt: Option<Fmt>,
    data_at: usize,
}

fn header(d: &[u8]) -> Result<Header> {
    ensure!(d.len() >= 128 && &d[..4] == b"DDS ", "not a DDS texture");
    let height = word(d, 12);
    let width = word(d, 16);
    let depth = word(d, 24).max(1);
    let mips = word(d, 28).max(1);
    let pf_flags = word(d, 80);
    let fourcc = &d[84..88];
    let bits = word(d, 88);
    let rmask = word(d, 92);
    let caps2 = word(d, 112);
    let cube = caps2 & 0x200 != 0;
    let mut data_at = 128;
    let (fmt, name) = if pf_flags & 4 != 0 {
        match fourcc {
            b"DXT1" => (Some(Fmt::Bc1), "BC1 (DXT1)".to_string()),
            b"DXT2" | b"DXT3" => (Some(Fmt::Bc2), "BC2 (DXT3)".to_string()),
            b"DXT4" | b"DXT5" => (Some(Fmt::Bc3), "BC3 (DXT5)".to_string()),
            b"ATI1" | b"BC4U" => (Some(Fmt::Bc4), "BC4 (ATI1)".to_string()),
            b"ATI2" | b"BC5U" => (Some(Fmt::Bc5), "BC5 (ATI2)".to_string()),
            b"DX10" => {
                ensure!(d.len() >= 148, "DX10 header is cut short");
                data_at = 148;
                let dxgi = word(d, 128);
                let f = match dxgi {
                    70..=72 => Some(Fmt::Bc1),
                    73..=75 => Some(Fmt::Bc2),
                    76..=78 => Some(Fmt::Bc3),
                    79..=81 => Some(Fmt::Bc4),
                    82..=84 => Some(Fmt::Bc5),
                    94 | 95 => Some(Fmt::Bc6u),
                    96 => Some(Fmt::Bc6s),
                    97..=99 => Some(Fmt::Bc7),
                    27..=29 => Some(Fmt::Rgba8),
                    87 | 90 | 91 => Some(Fmt::Bgra8),
                    _ => None,
                };
                (f, format!("DXGI {dxgi}"))
            }
            other => (None, format!("FourCC {}", String::from_utf8_lossy(other))),
        }
    } else if bits == 32 {
        let f = if rmask == 0x00ff0000 { if pf_flags & 1 != 0 { Fmt::Bgra8 } else { Fmt::Bgrx8 } } else { Fmt::Rgba8 };
        (Some(f), "32-bit RGBA".to_string())
    } else if bits == 8 {
        let f = if pf_flags & 2 != 0 { Fmt::A8 } else { Fmt::L8 };
        (Some(f), "8-bit".to_string())
    } else {
        (None, format!("{bits}-bit, flags {pf_flags:#x}"))
    };
    Ok(Header { info: Info { width, height, mips, format: name, cube, depth }, fmt, data_at })
}

pub fn info(d: &[u8]) -> Result<Info> {
    Ok(header(d)?.info)
}

/// The top mip of the first face as RGBA8.
pub fn decode_rgba(d: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let h = header(d)?;
    let Some(fmt) = h.fmt else { bail!("texture format {} is not supported", h.info.format) };
    let (w, ht) = (h.info.width as usize, h.info.height as usize);
    ensure!(w > 0 && ht > 0 && w <= 16384 && ht <= 16384, "bad texture size {w}x{ht}");
    let src = &d[h.data_at..];
    let mut rgba = vec![0u8; w * ht * 4];
    if let Some(block) = fmt.block() {
        let need = w.div_ceil(4) * ht.div_ceil(4) * block;
        ensure!(src.len() >= need, "texture data is cut short");
        let mut px = vec![0u32; w * ht];
        let r = match fmt {
            Fmt::Bc1 => texture2ddecoder::decode_bc1a(src, w, ht, &mut px),
            Fmt::Bc2 => texture2ddecoder::decode_bc2(src, w, ht, &mut px),
            Fmt::Bc3 => texture2ddecoder::decode_bc3(src, w, ht, &mut px),
            Fmt::Bc4 => texture2ddecoder::decode_bc4(src, w, ht, &mut px),
            Fmt::Bc5 => texture2ddecoder::decode_bc5(src, w, ht, &mut px),
            Fmt::Bc6u => texture2ddecoder::decode_bc6_unsigned(src, w, ht, &mut px),
            Fmt::Bc6s => texture2ddecoder::decode_bc6_signed(src, w, ht, &mut px),
            Fmt::Bc7 => texture2ddecoder::decode_bc7(src, w, ht, &mut px),
            _ => unreachable!(),
        };
        if let Err(e) = r {
            bail!("decode failed: {e}");
        }
        for (i, p) in px.iter().enumerate() {
            rgba[i * 4] = (p >> 16) as u8;
            rgba[i * 4 + 1] = (p >> 8) as u8;
            rgba[i * 4 + 2] = *p as u8;
            rgba[i * 4 + 3] = (p >> 24) as u8;
        }
        if fmt == Fmt::Bc5 {
            // Two-channel normal maps: rebuild blue so the picture reads as a normal map.
            for p in rgba.chunks_mut(4) {
                let x = p[0] as f32 / 127.5 - 1.0;
                let y = p[1] as f32 / 127.5 - 1.0;
                p[2] = (((1.0 - x * x - y * y).max(0.0).sqrt() * 0.5 + 0.5) * 255.0) as u8;
                p[3] = 255;
            }
        }
    } else {
        let need = w * ht * fmt.bpp();
        ensure!(src.len() >= need, "texture data is cut short");
        for i in 0..w * ht {
            let s = &src[i * fmt.bpp()..];
            let o = &mut rgba[i * 4..i * 4 + 4];
            match fmt {
                Fmt::Bgra8 => o.copy_from_slice(&[s[2], s[1], s[0], s[3]]),
                Fmt::Bgrx8 => o.copy_from_slice(&[s[2], s[1], s[0], 255]),
                Fmt::Rgba8 => o.copy_from_slice(&s[..4]),
                Fmt::L8 => o.copy_from_slice(&[s[0], s[0], s[0], 255]),
                Fmt::A8 => o.copy_from_slice(&[255, 255, 255, s[0]]),
                _ => unreachable!(),
            }
        }
    }
    Ok((h.info.width, h.info.height, rgba))
}

pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::Fast);
        let mut wr = e.write_header()?;
        wr.write_image_data(rgba)?;
    }
    Ok(out)
}

pub fn to_png(d: &[u8]) -> Result<Vec<u8>> {
    let (w, h, rgba) = decode_rgba(d)?;
    encode_png(w, h, &rgba)
}

/// A smaller copy for previews, at most `max` pixels on the long side (box filter).
pub fn thumbnail(d: &[u8], max: u32) -> Result<(u32, u32, Vec<u8>)> {
    let (w, h, rgba) = decode_rgba(d)?;
    let scale = (w.max(h) as f32 / max as f32).ceil().max(1.0) as u32;
    if scale == 1 {
        return Ok((w, h, rgba));
    }
    let (nw, nh) = ((w / scale).max(1), (h / scale).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            for dy in 0..scale {
                for dx in 0..scale {
                    let i = (((y * scale + dy) * w + x * scale + dx) * 4) as usize;
                    for k in 0..4 {
                        acc[k] += rgba[i + k] as u32;
                    }
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for k in 0..4 {
                out[o + k] = (acc[k] / (scale * scale)) as u8;
            }
        }
    }
    Ok((nw, nh, out))
}
