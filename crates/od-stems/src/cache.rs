//! On-disk stem cache: one `.odstem` file per track content hash.
//!
//! # File format (version 1, little endian)
//!
//! | offset | size | field                                              |
//! |-------:|-----:|----------------------------------------------------|
//! | 0      | 6    | magic `ODSTEM`                                     |
//! | 6      | 2    | format version (`1`)                               |
//! | 8      | 4    | sample rate (Hz) of the stems (= the track's rate) |
//! | 12     | 8    | frames per stem                                    |
//! | 20     | 1    | stem count (`3`: vocals, drums, bass)              |
//! | 21     | 1    | codec (`1`: delta-coded 16-bit PCM, zstd)          |
//! | 22     | 2    | reserved (0)                                       |
//! | 24     | 12   | `f32` dequantisation scale per stem                |
//! | 36     | 8    | compressed payload length                          |
//! | 44     | …    | payload                                            |
//!
//! The payload is one zstd frame. Uncompressed it holds, per stem and per
//! channel (L then R), `frames` 16-bit samples, first-order delta coded
//! (`x[n] − x[n−1]`, wrapping) and byte-split (all low bytes, then all high
//! bytes) — which roughly halves the size compared to plain PCM in zstd.
//! "Inst" is not stored; it is `mix − vocals − drums − bass`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use od_core::ContentHash;

use crate::{Error, Stems};

const MAGIC: &[u8; 6] = b"ODSTEM";
/// Current `.odstem` format version.
pub const FORMAT_VERSION: u16 = 1;
const CODEC_DELTA_ZSTD: u8 = 1;
const HEADER_LEN: usize = 44;
const ZSTD_LEVEL: i32 = 3;

/// Header of a cached stem file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StemFileInfo {
    pub version: u16,
    pub sample_rate: u32,
    pub frames: u64,
}

/// Stem cache directory (`data_dir()/stems` by default).
#[derive(Clone, Debug)]
pub struct StemCache {
    dir: PathBuf,
}

impl StemCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `od_library::data_dir()/stems`.
    pub fn default_dir() -> PathBuf {
        od_library::data_dir().join("stems")
    }

    /// The cache in [`StemCache::default_dir`].
    pub fn open_default() -> Self {
        Self::new(Self::default_dir())
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// File for a track hash (hashes are hex; anything else is sanitised).
    pub fn path(&self, hash: &ContentHash) -> PathBuf {
        let name: String = hash.0.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        self.dir.join(format!("{name}.odstem"))
    }

    pub fn exists(&self, hash: &ContentHash) -> bool {
        self.path(hash).is_file()
    }

    /// Reads only the header (cheap; for UI badges and validation).
    pub fn info(&self, hash: &ContentHash) -> Result<Option<StemFileInfo>, Error> {
        let p = self.path(hash);
        let mut f = match fs::File::open(&p) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut h = [0u8; HEADER_LEN];
        io::Read::read_exact(&mut f, &mut h)?;
        parse_header(&h).map(|(i, ..)| Some(i))
    }

    /// Loads the stems of `hash`, `Ok(None)` if not cached.
    pub fn load(&self, hash: &ContentHash) -> Result<Option<Stems>, Error> {
        match fs::read(self.path(hash)) {
            Ok(bytes) => decode(&bytes).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Writes the stems of `hash` (atomically: temp file + rename).
    pub fn store(&self, hash: &ContentHash, stems: &Stems) -> Result<PathBuf, Error> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path(hash);
        let tmp = path.with_extension("odstem.part");
        {
            let mut f = io::BufWriter::new(fs::File::create(&tmp)?);
            f.write_all(&encode(stems)?)?;
            f.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    pub fn remove(&self, hash: &ContentHash) -> Result<(), Error> {
        match fs::remove_file(self.path(hash)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

fn parse_header(h: &[u8]) -> Result<(StemFileInfo, [f32; 3], usize), Error> {
    let bad = |m: &str| Error::CacheFormat(m.to_string());
    if h.len() < HEADER_LEN || &h[0..6] != MAGIC {
        return Err(bad("not an .odstem file"));
    }
    let u16_at = |o: usize| u16::from_le_bytes([h[o], h[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(h[o..o + 4].try_into().unwrap());
    let u64_at = |o: usize| u64::from_le_bytes(h[o..o + 8].try_into().unwrap());
    let version = u16_at(6);
    if version != FORMAT_VERSION {
        return Err(bad(&format!("unsupported version {version}")));
    }
    if h[20] != 3 || h[21] != CODEC_DELTA_ZSTD {
        return Err(bad("unsupported stem layout or codec"));
    }
    let scale = [
        f32::from_bits(u32_at(24)),
        f32::from_bits(u32_at(28)),
        f32::from_bits(u32_at(32)),
    ];
    let info = StemFileInfo { version, sample_rate: u32_at(8), frames: u64_at(12) };
    Ok((info, scale, u64_at(36) as usize))
}

/// Serialises stems into the `.odstem` format.
pub fn encode(stems: &Stems) -> Result<Vec<u8>, Error> {
    let parts = [&stems.vocals, &stems.drums, &stems.bass];
    let frames = parts.iter().map(|p| p.len() / 2).max().unwrap_or(0);
    let mut scale = [0f32; 3];
    // Planar, delta-coded, byte-split: [stem][channel][lo bytes | hi bytes].
    let mut raw = vec![0u8; frames * 2 * 2 * 3];
    for (s, p) in parts.iter().enumerate() {
        let peak = p.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-9);
        scale[s] = peak / 32767.0;
        let inv = 32767.0 / peak;
        for c in 0..2 {
            let base = (s * 2 + c) * frames * 2;
            let (lo, hi) = raw[base..base + frames * 2].split_at_mut(frames);
            let mut prev = 0i16;
            for f in 0..frames {
                let v = p.get(f * 2 + c).copied().unwrap_or(0.0);
                let q = (v * inv).round().clamp(-32767.0, 32767.0) as i16;
                let d = q.wrapping_sub(prev) as u16;
                prev = q;
                lo[f] = d as u8;
                hi[f] = (d >> 8) as u8;
            }
        }
    }
    let mut enc = zstd::bulk::Compressor::new(ZSTD_LEVEL)?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()) as u32;
    let _ = enc.multithread(threads.min(8)); // fine if unsupported
    let payload = enc.compress(&raw)?;
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&stems.sample_rate.to_le_bytes());
    out.extend_from_slice(&(frames as u64).to_le_bytes());
    out.extend_from_slice(&[3, CODEC_DELTA_ZSTD, 0, 0]);
    for s in scale {
        out.extend_from_slice(&s.to_bits().to_le_bytes());
    }
    out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Parses an `.odstem` file.
pub fn decode(bytes: &[u8]) -> Result<Stems, Error> {
    let (info, scale, len) = parse_header(bytes)?;
    let payload = bytes
        .get(HEADER_LEN..HEADER_LEN + len)
        .ok_or_else(|| Error::CacheFormat("truncated payload".into()))?;
    let frames = usize::try_from(info.frames).map_err(|_| Error::CacheFormat("too long".into()))?;
    let expect = frames * 2 * 2 * 3;
    let raw = zstd::bulk::decompress(payload, expect)?;
    if raw.len() != expect {
        return Err(Error::CacheFormat("payload size mismatch".into()));
    }
    let mut parts: [Vec<f32>; 3] = std::array::from_fn(|_| vec![0f32; frames * 2]);
    for (s, p) in parts.iter_mut().enumerate() {
        let k = scale[s];
        for c in 0..2 {
            let base = (s * 2 + c) * frames * 2;
            let (lo, hi) = raw[base..base + frames * 2].split_at(frames);
            let mut prev = 0i16;
            for f in 0..frames {
                let d = (lo[f] as u16 | (hi[f] as u16) << 8) as i16;
                prev = prev.wrapping_add(d);
                p[f * 2 + c] = prev as f32 * k;
            }
        }
    }
    let [vocals, drums, bass] = parts;
    Ok(Stems { sample_rate: info.sample_rate, vocals, drums, bass })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems(frames: usize) -> Stems {
        let t = |f: f64, a: f32| -> Vec<f32> {
            (0..frames)
                .flat_map(|i| {
                    let v = (i as f64 * f).sin() as f32 * a;
                    [v, -v * 0.5]
                })
                .collect()
        };
        Stems { sample_rate: 44_100, vocals: t(0.01, 0.8), drums: t(0.2, 1.3), bass: vec![0.0; frames * 2] }
    }

    #[test]
    fn round_trip_within_16_bit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = StemCache::new(dir.path().join("stems"));
        let h = ContentHash("abc123".into());
        assert!(!cache.exists(&h));
        assert!(cache.load(&h).unwrap().is_none());
        let s = stems(10_000);
        cache.store(&h, &s).unwrap();
        assert!(cache.exists(&h));
        let info = cache.info(&h).unwrap().unwrap();
        assert_eq!((info.sample_rate, info.frames, info.version), (44_100, 10_000, FORMAT_VERSION));
        let back = cache.load(&h).unwrap().unwrap();
        assert_eq!(back.sample_rate, 44_100);
        for (a, b, peak) in [(&s.vocals, &back.vocals, 0.8f32), (&s.drums, &back.drums, 1.3), (&s.bass, &back.bass, 1.0)] {
            assert_eq!(a.len(), b.len());
            let err = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).fold(0f32, f32::max);
            assert!(err <= peak / 32767.0, "error {err}");
        }
        cache.remove(&h).unwrap();
        assert!(!cache.exists(&h));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"nope").is_err());
        let mut b = encode(&stems(100)).unwrap();
        b.truncate(b.len() - 3);
        assert!(decode(&b).is_err());
    }
}
