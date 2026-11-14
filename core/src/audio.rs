//! Timpani sound banks (`timpani_bank`): a table of 24-byte rows (u64 clip id, offset, size, stream
//! offset, stream size) after an 8-byte head, then the clips. A clip is a 68-byte header with a
//! WAVEFORMATEX at byte 48, then the start of the audio; the rest of the audio is in the `.stream` part.
//! Clips hold PCM or a plain Ogg Vorbis stream.

use anyhow::{Result, ensure};
use serde::Serialize;

pub const CLIP_HEADER: usize = 68;
pub const PCM: u16 = 1;
pub const VORBIS: u16 = 0x566F;

#[derive(Debug, Clone, Serialize)]
pub struct Clip {
    pub id: u64,
    pub codec: u16,
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    pub block_align: u16,
    pub off: usize,
    pub size: usize,
    pub stream_off: usize,
    pub stream_size: usize,
}

impl Clip {
    pub fn codec_name(&self) -> &'static str {
        match self.codec {
            PCM => "PCM",
            VORBIS => "Ogg Vorbis",
            2 => "MS ADPCM",
            0x11 => "IMA ADPCM",
            _ => "unknown",
        }
    }
    pub fn extension(&self) -> &'static str {
        if self.codec == VORBIS { "ogg" } else { "wav" }
    }
    /// Seconds, when the codec gives a way to tell.
    pub fn duration(&self) -> Option<f32> {
        if self.codec == PCM && self.block_align > 0 && self.rate > 0 {
            Some(((self.size - CLIP_HEADER + self.stream_size) / self.block_align as usize) as f32 / self.rate as f32)
        } else {
            None
        }
    }
}

fn u32_at(d: &[u8], at: usize) -> usize {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap()) as usize
}
fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(d[at..at + 2].try_into().unwrap())
}

pub fn clips(d: &[u8]) -> Result<Vec<Clip>> {
    ensure!(d.len() >= 8, "bank is cut short");
    let count = u32_at(d, 0);
    ensure!(8 + count * 24 <= d.len(), "bank table runs past the end");
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = 8 + 24 * i;
        let id = u64::from_le_bytes(d[at..at + 8].try_into().unwrap());
        let (off, size, stream_off, stream_size) = (u32_at(d, at + 8), u32_at(d, at + 12), u32_at(d, at + 16), u32_at(d, at + 20));
        ensure!(off + size <= d.len() && size >= CLIP_HEADER, "clip {id:016x} runs past the end");
        let h = &d[off..off + CLIP_HEADER];
        out.push(Clip {
            id,
            codec: u16_at(h, 48),
            channels: u16_at(h, 50),
            rate: u32_at(h, 52) as u32,
            block_align: u16_at(h, 60),
            bits: u16_at(h, 62),
            off,
            size,
            stream_off,
            stream_size,
        });
    }
    Ok(out)
}

/// The clip's audio as a playable file: WAV for PCM and ADPCM, Ogg for Vorbis.
pub fn clip_file(bank: &[u8], stream: &[u8], c: &Clip) -> Result<Vec<u8>> {
    ensure!(c.stream_off + c.stream_size <= stream.len(), "clip {:016x} runs past the end of the stream", c.id);
    let mut audio = Vec::with_capacity(c.size - CLIP_HEADER + c.stream_size);
    audio.extend_from_slice(&bank[c.off + CLIP_HEADER..c.off + c.size]);
    audio.extend_from_slice(&stream[c.stream_off..c.stream_off + c.stream_size]);
    if c.codec == VORBIS {
        return Ok(audio);
    }
    let fmt = &bank[c.off + 48..c.off + CLIP_HEADER];
    // PCM takes the 16-byte format; others keep cbSize and extra bytes as given.
    let fmt_len = if c.codec == PCM { 16 } else { (18 + u16_at(fmt, 16) as usize).min(fmt.len()) };
    let mut out = Vec::with_capacity(audio.len() + 64);
    out.extend(b"RIFF");
    out.extend(((4 + 8 + fmt_len + 8 + audio.len()) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend((fmt_len as u32).to_le_bytes());
    out.extend_from_slice(&fmt[..fmt_len]);
    out.extend(b"data");
    out.extend((audio.len() as u32).to_le_bytes());
    out.extend(audio);
    Ok(out)
}
