//! HD1 bundles. A bundle file is a header (version, payload size, zero) and the payload in 64 KB blocks,
//! each zlib-compressed unless it is stored whole. The payload holds a count, 256 bytes of properties,
//! an index of (type, name) pairs, then each resource: type, name, variant count, flags, a (lang, size,
//! stream size) head per variant, and the variants' bytes. Streamed parts live in `<bundle>.stream`,
//! one after another in the order the variants appear.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

pub const VERSION: u32 = 0xF000_0004;
pub const BLOCK: usize = 0x10000;
pub const PROPS: usize = 256;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VariantHead {
    pub lang: u32,
    /// Offset in the payload.
    pub at: u64,
    pub size: u32,
    /// Offset in the `.stream` file.
    pub stream_at: u64,
    pub stream_size: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntryHead {
    pub type_hash: u64,
    pub name_hash: u64,
    pub flags: u32,
    pub variants: Vec<VariantHead>,
}

/// A bundle read block by block, so a gigabyte bundle never sits in memory whole.
pub struct Reader {
    file: BufReader<File>,
    blocks: Vec<(u64, u32)>,
    total: u64,
    cache: Option<(usize, Vec<u8>)>,
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn unpack(body: &[u8]) -> Result<Vec<u8>> {
    if body.len() == BLOCK {
        return Ok(body.to_vec());
    }
    let mut out = Vec::with_capacity(BLOCK);
    flate2::read::ZlibDecoder::new(body).take(BLOCK as u64).read_to_end(&mut out).context("bad zlib block")?;
    Ok(out)
}

impl Reader {
    pub fn open(path: &Path) -> Result<Reader> {
        let mut file = BufReader::with_capacity(1 << 16, File::open(path).with_context(|| format!("open {}", path.display()))?);
        let mut head = [0u8; 12];
        file.read_exact(&mut head)?;
        let version = word(&head, 0);
        ensure!(version == VERSION, "not an HD1 bundle (version {version:#x})");
        let total = word(&head, 4) as u64;
        let len = file.get_ref().metadata()?.len();
        let mut blocks = Vec::with_capacity(total.div_ceil(BLOCK as u64) as usize);
        let mut at = 12u64;
        for _ in 0..total.div_ceil(BLOCK as u64) {
            let mut size = [0u8; 4];
            file.read_exact(&mut size)?;
            let size = u32::from_le_bytes(size);
            ensure!(at + 4 + size as u64 <= len, "bundle is cut short");
            blocks.push((at + 4, size));
            file.seek_relative(size as i64)?;
            at += 4 + size as u64;
        }
        Ok(Reader { file, blocks, total, cache: None })
    }

    pub fn payload_size(&self) -> u64 {
        self.total
    }

    /// `n` bytes of the payload from `at`.
    pub fn read(&mut self, at: u64, n: usize) -> Result<Vec<u8>> {
        ensure!(at + n as u64 <= self.total, "read past the end of the bundle");
        let mut out = Vec::with_capacity(n);
        let mut at = at as usize;
        while out.len() < n {
            let i = at / BLOCK;
            if self.cache.as_ref().is_none_or(|(have, _)| *have != i) {
                let (pos, size) = self.blocks[i];
                self.file.seek(SeekFrom::Start(pos))?;
                let mut body = vec![0u8; size as usize];
                self.file.read_exact(&mut body)?;
                self.cache = Some((i, unpack(&body)?));
            }
            let block = &self.cache.as_ref().unwrap().1;
            let from = at % BLOCK;
            let end = (from + n - out.len()).min(block.len());
            ensure!(from < end, "block is short");
            out.extend_from_slice(&block[from..end]);
            at += end - from;
        }
        Ok(out)
    }

    pub fn props(&mut self) -> Result<Vec<u8>> {
        self.read(4, PROPS)
    }

    /// Every resource in the bundle with where its bytes are.
    pub fn heads(&mut self) -> Result<Vec<EntryHead>> {
        let count = word(&self.read(0, 4)?, 0) as u64;
        ensure!(count < 1_000_000, "implausible resource count {count}");
        let mut at = 4 + PROPS as u64 + count * 16;
        let mut stream_at = 0u64;
        let mut out = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let h = self.read(at, 24)?;
            let n = word(&h, 16) as usize;
            ensure!(n < 4096, "implausible variant count {n}");
            at += 24;
            let list = self.read(at, n * 12)?;
            at += n as u64 * 12;
            let mut variants = Vec::with_capacity(n);
            for v in list.chunks(12) {
                let (size, stream_size) = (word(v, 4), word(v, 8));
                variants.push(VariantHead { lang: word(v, 0), at, size, stream_at, stream_size });
                at += size as u64;
                stream_at += stream_size as u64;
            }
            out.push(EntryHead {
                type_hash: u64::from_le_bytes(h[..8].try_into().unwrap()),
                name_hash: u64::from_le_bytes(h[8..16].try_into().unwrap()),
                flags: word(&h, 20),
                variants,
            });
        }
        Ok(out)
    }
}

/// The `.stream` file that goes with a bundle, if there is one.
pub fn stream_path(bundle: &Path) -> PathBuf {
    let mut s = bundle.as_os_str().to_owned();
    s.push(".stream");
    PathBuf::from(s)
}

pub fn read_stream(bundle: &Path, at: u64, n: u32) -> Result<Vec<u8>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let path = stream_path(bundle);
    let mut f = File::open(&path).with_context(|| format!("open {}", path.display()))?;
    f.seek(SeekFrom::Start(at))?;
    let mut out = vec![0u8; n as usize];
    f.read_exact(&mut out).context("stream file is cut short")?;
    Ok(out)
}

/// A resource to write into a new bundle.
pub struct NewEntry {
    pub type_hash: u64,
    pub name_hash: u64,
    pub flags: u32,
    /// (lang, data, stream data)
    pub variants: Vec<(u32, Vec<u8>, Vec<u8>)>,
}

/// Writes a bundle and, when any variant has stream data, the `.stream` beside it.
pub fn write(path: &Path, props: &[u8; PROPS], entries: &[NewEntry]) -> Result<()> {
    let mut payload = Vec::new();
    payload.extend((entries.len() as u32).to_le_bytes());
    payload.extend_from_slice(props);
    for e in entries {
        payload.extend(e.type_hash.to_le_bytes());
        payload.extend(e.name_hash.to_le_bytes());
    }
    let mut stream = Vec::new();
    for e in entries {
        payload.extend(e.type_hash.to_le_bytes());
        payload.extend(e.name_hash.to_le_bytes());
        payload.extend((e.variants.len() as u32).to_le_bytes());
        payload.extend(e.flags.to_le_bytes());
        for (lang, data, s) in &e.variants {
            payload.extend(lang.to_le_bytes());
            payload.extend((data.len() as u32).to_le_bytes());
            payload.extend((s.len() as u32).to_le_bytes());
        }
        for (_, data, s) in &e.variants {
            payload.extend_from_slice(data);
            stream.extend_from_slice(s);
        }
    }
    if payload.len() > u32::MAX as usize {
        bail!("bundle passes 4 GB");
    }
    let mut out = Vec::with_capacity(payload.len() / 2);
    out.extend(VERSION.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    for piece in payload.chunks(BLOCK) {
        let mut block = piece.to_vec();
        block.resize(BLOCK, 0);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
        z.write_all(&block)?;
        let z = z.finish()?;
        let body = if z.len() < BLOCK { &z } else { &block };
        out.extend((body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
    }
    std::fs::write(path, out)?;
    if !stream.is_empty() {
        std::fs::write(stream_path(path), stream)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_bundles_read_back() {
        let dir = std::env::temp_dir().join(format!("md-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("0000000000000001");
        let big: Vec<u8> = (0..150_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let entries = vec![
            NewEntry { type_hash: 1, name_hash: 2, flags: 0, variants: vec![(0, b"small".to_vec(), b"st".to_vec())] },
            NewEntry { type_hash: 3, name_hash: 4, flags: 9, variants: vec![(0, big.clone(), vec![]), (8, b"ru".to_vec(), b"ream".to_vec())] },
        ];
        write(&path, &[5; PROPS], &entries).unwrap();
        let mut r = Reader::open(&path).unwrap();
        assert_eq!(r.props().unwrap(), vec![5; PROPS]);
        let h = r.heads().unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[1].flags, 9);
        let v = &h[1].variants[0];
        assert_eq!(r.read(v.at, v.size as usize).unwrap(), big);
        let ru = h[1].variants[1].clone();
        assert_eq!(read_stream(&path, ru.stream_at, ru.stream_size).unwrap(), b"ream");
        std::fs::remove_dir_all(dir).ok();
    }
}
