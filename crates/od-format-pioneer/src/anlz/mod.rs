//! ANLZ analysis files (`ANLZ0000.DAT`, `.EXT`, `.2EX`).
//!
//! Layout reference: Deep Symmetry, "Analysis Files"
//! (<https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html>)
//! and `rekordbox_anlz.ksy` from crate-digger. All numbers are big-endian.
//!
//! A file is a `PMAI` header followed by tagged sections. Every section starts
//! with `fourcc`, `len_header` and `len_tag`; the remaining header bytes hold
//! tag specific fields. [`AnlzFile::parse`] followed by [`AnlzFile::to_bytes`]
//! reproduces a file byte for byte, including unknown fields and tags.

mod build;
mod hash;

pub use build::{AnlzInput, AnlzSet, build_anlz};
pub use hash::{AnlzSlots, anlz_dat_path_for, anlz_dir_for, anlz_hash};

use crate::bytes::*;
use crate::palette;
use crate::{Error, Result};
use od_core::{
    Beat, Beatgrid, Cue, CueKind, DataSource, Phrase, PhraseKind, PhraseMood, Waveform,
    WaveformColumn,
};

/// Header words after `len_file` as rekordbox writes them.
pub const DEFAULT_HEADER_TAIL: [u8; 16] = [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];

/// `loop_time` value of cue entries that are not loops.
pub const NO_LOOP: u32 = 0xffff_ffff;

/// A parsed analysis file.
#[derive(Clone, Debug, PartialEq)]
pub struct AnlzFile {
    /// Bytes of the `PMAI` header after `len_file` (normally 16 bytes).
    pub header_tail: Vec<u8>,
    pub tags: Vec<Tag>,
}

/// `PCOB` / `PCO2` list type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueListType {
    Memory,
    Hot,
    Other(u32),
}

impl CueListType {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Memory,
            1 => Self::Hot,
            o => Self::Other(o),
        }
    }
    fn to_u32(self) -> u32 {
        match self {
            Self::Memory => 0,
            Self::Hot => 1,
            Self::Other(o) => o,
        }
    }
}

/// One beat of a `PQTZ` grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PqtzBeat {
    /// Position in the bar, 1..=4.
    pub beat_number: u16,
    /// BPM × 100.
    pub tempo: u16,
    /// Milliseconds.
    pub time: u32,
}

/// `PCPT` entry of a `PCOB` list (56 bytes in rekordbox output).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcpt {
    pub len_header: u32,
    /// 0 for memory cues, 1 = A, 2 = B, ...
    pub hot_cue: u32,
    pub status: u32,
    pub unknown1: u32,
    pub order_first: u16,
    pub order_last: u16,
    /// 1 = point, 2 = loop.
    pub cue_type: u8,
    pub unknown2: [u8; 3],
    pub time: u32,
    pub loop_time: u32,
    /// Remaining bytes of the entry (16 zero bytes normally).
    pub tail: Vec<u8>,
}

/// `PCP2` entry of a `PCO2` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pcp2 {
    pub len_header: u32,
    pub hot_cue: u32,
    pub cue_type: u8,
    pub unknown1: [u8; 3],
    pub time: u32,
    pub loop_time: u32,
    /// Row id in the `colors` table for memory cues (0 = none).
    pub color_id: u8,
    pub unknown2: [u8; 7],
    pub loop_numerator: u16,
    pub loop_denominator: u16,
    /// Raw UTF-16BE comment including the trailing NUL; `None` if the entry
    /// ends before the length field.
    pub comment_raw: Option<Vec<u8>>,
    /// `color_code, r, g, b`; `None` if the entry ends before them.
    pub hot_color: Option<[u8; 4]>,
    /// Bytes after the colour (rekordbox writes 40 zero bytes, giving 88 byte
    /// entries without comment).
    pub trailing: Vec<u8>,
}

impl Pcp2 {
    pub fn comment(&self) -> String {
        self.comment_raw
            .as_deref()
            .map(decode_utf16be)
            .unwrap_or_default()
    }
}

/// One phrase of a `PSSI` tag (24 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PssiEntry {
    /// Phrase number starting at 1.
    pub index: u16,
    /// Beat number (1-based) at which the phrase starts.
    pub beat: u16,
    pub kind: u16,
    /// Bytes 6..24 of the entry: `?, k1, ?, k2, ?, b, beat2, beat3, beat4,
    /// ?, k3, ?, fill, beat_fill`.
    pub rest: [u8; 18],
}

impl PssiEntry {
    pub fn k1(&self) -> u8 {
        self.rest[1]
    }
    pub fn k2(&self) -> u8 {
        self.rest[3]
    }
    pub fn k3(&self) -> u8 {
        self.rest[13]
    }
    pub fn fill(&self) -> u8 {
        self.rest[15]
    }
}

/// `PSSI` song structure (phrases).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongStructure {
    pub len_entry_bytes: u32,
    /// Whether the body was XOR-masked (rekordbox 6+ exports are).
    pub masked: bool,
    /// 1 = high, 2 = mid, 3 = low.
    pub mood: u16,
    pub unknown1: [u8; 6],
    /// Beat number at which the last phrase ends.
    pub end_beat: u16,
    pub unknown2: u16,
    pub bank: u8,
    pub unknown3: u8,
    pub entries: Vec<PssiEntry>,
}

/// XOR base pattern of `PSSI` bodies; each byte is increased by the entry
/// count before use (Deep Symmetry, "Song Structure Tag").
const PSSI_MASK: [u8; 19] = [
    0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8,
    0xE9, 0xF4, 0xE1,
];

fn pssi_xor(buf: &mut [u8], len_entries: u16) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= PSSI_MASK[i % PSSI_MASK.len()].wrapping_add(len_entries as u8);
    }
}

/// A tagged section.
#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    /// `PPTH`: audio file path.
    Path(String),
    /// `PVBR`: seek index (400 byte offsets plus a trailer, as u32 values).
    Vbr { unknown: u32, entries: Vec<u32> },
    /// `PQTZ`: beat grid.
    BeatGrid {
        unknown1: u32,
        unknown2: u32,
        beats: Vec<PqtzBeat>,
    },
    /// `PCOB`: memory or hot cue list.
    CueList {
        list_type: CueListType,
        unknown: u16,
        memory_count: u32,
        cues: Vec<Pcpt>,
    },
    /// `PCO2`: extended (nxs2) cue list with colours and comments.
    ExtCueList {
        list_type: CueListType,
        unknown: u16,
        cues: Vec<Pcp2>,
    },
    /// `PWAV`: 400 column monochrome preview (5 bit height, 3 bit whiteness).
    WavePreview { unknown: u32, data: Vec<u8> },
    /// `PWV2`: 100 column tiny preview (4 bit height).
    WaveTiny { unknown: u32, data: Vec<u8> },
    /// `PWV3`: monochrome detail, 150 entries per second.
    WaveDetail { unknown: u32, data: Vec<u8> },
    /// `PWV4`: colour preview, 1200 columns of 6 bytes.
    WaveColorPreview { unknown: u32, data: Vec<[u8; 6]> },
    /// `PWV5`: colour detail, 150 entries per second (3+3+3 bit RGB, 5 bit height).
    WaveColorDetail { unknown: u32, data: Vec<u16> },
    /// `PWV6`: 3-band preview, 1200 columns of 3 bytes.
    Wave3BandPreview { data: Vec<[u8; 3]> },
    /// `PWV7`: 3-band detail, 150 entries per second of 3 bytes.
    Wave3BandDetail { unknown: u32, data: Vec<[u8; 3]> },
    /// `PWVC`: per-band thresholds of the 3-band waveform.
    VocalConfig {
        reserved: u16,
        low: u16,
        mid: u16,
        high: u16,
    },
    /// `PSSI`: song structure.
    SongStructure(SongStructure),
    /// Anything else (e.g. `PQT2`), kept verbatim.
    Unknown {
        fourcc: [u8; 4],
        header: Vec<u8>,
        body: Vec<u8>,
    },
}

impl Tag {
    pub fn fourcc(&self) -> [u8; 4] {
        match self {
            Tag::Path(_) => *b"PPTH",
            Tag::Vbr { .. } => *b"PVBR",
            Tag::BeatGrid { .. } => *b"PQTZ",
            Tag::CueList { .. } => *b"PCOB",
            Tag::ExtCueList { .. } => *b"PCO2",
            Tag::WavePreview { .. } => *b"PWAV",
            Tag::WaveTiny { .. } => *b"PWV2",
            Tag::WaveDetail { .. } => *b"PWV3",
            Tag::WaveColorPreview { .. } => *b"PWV4",
            Tag::WaveColorDetail { .. } => *b"PWV5",
            Tag::Wave3BandPreview { .. } => *b"PWV6",
            Tag::Wave3BandDetail { .. } => *b"PWV7",
            Tag::VocalConfig { .. } => *b"PWVC",
            Tag::SongStructure(_) => *b"PSSI",
            Tag::Unknown { fourcc, .. } => *fourcc,
        }
    }

    pub fn fourcc_str(&self) -> String {
        String::from_utf8_lossy(&self.fourcc()).into_owned()
    }

    fn parse(fourcc: [u8; 4], header: &[u8], body: &[u8]) -> Result<Tag> {
        let unknown = || Tag::Unknown {
            fourcc,
            header: header.to_vec(),
            body: body.to_vec(),
        };
        // Tags whose header does not have the documented size are kept raw.
        let h = |n: usize| header.len() == n;
        Ok(match &fourcc {
            b"PPTH" if h(4) => {
                let len = be32(header, 0)? as usize;
                if len != body.len() {
                    return Ok(unknown());
                }
                let s = decode_utf16be(body);
                if utf16be_nul(&s) != body {
                    return Ok(unknown());
                }
                Tag::Path(s)
            }
            b"PVBR" if h(4) && body.len().is_multiple_of(4) => Tag::Vbr {
                unknown: be32(header, 0)?,
                entries: (0..body.len() / 4)
                    .map(|i| be32(body, i * 4))
                    .collect::<Result<_>>()?,
            },
            b"PQTZ" if h(12) => {
                let n = be32(header, 8)? as usize;
                if body.len() != n * 8 {
                    return Ok(unknown());
                }
                let beats = (0..n)
                    .map(|i| {
                        Ok(PqtzBeat {
                            beat_number: be16(body, i * 8)?,
                            tempo: be16(body, i * 8 + 2)?,
                            time: be32(body, i * 8 + 4)?,
                        })
                    })
                    .collect::<Result<_>>()?;
                Tag::BeatGrid {
                    unknown1: be32(header, 0)?,
                    unknown2: be32(header, 4)?,
                    beats,
                }
            }
            b"PCOB" if h(12) => {
                let n = be16(header, 6)? as usize;
                let mut cues = Vec::with_capacity(n);
                let mut at = 0;
                for _ in 0..n {
                    let (e, len) = parse_pcpt(body, at)?;
                    cues.push(e);
                    at += len;
                }
                if at != body.len() {
                    return Ok(unknown());
                }
                Tag::CueList {
                    list_type: CueListType::from_u32(be32(header, 0)?),
                    unknown: be16(header, 4)?,
                    memory_count: be32(header, 8)?,
                    cues,
                }
            }
            b"PCO2" if h(8) => {
                let n = be16(header, 4)? as usize;
                let mut cues = Vec::with_capacity(n);
                let mut at = 0;
                for _ in 0..n {
                    let (e, len) = parse_pcp2(body, at)?;
                    cues.push(e);
                    at += len;
                }
                if at != body.len() {
                    return Ok(unknown());
                }
                Tag::ExtCueList {
                    list_type: CueListType::from_u32(be32(header, 0)?),
                    unknown: be16(header, 6)?,
                    cues,
                }
            }
            b"PWAV" | b"PWV2" if h(8) => {
                if be32(header, 0)? as usize != body.len() {
                    return Ok(unknown());
                }
                let unknown = be32(header, 4)?;
                let data = body.to_vec();
                if &fourcc == b"PWAV" {
                    Tag::WavePreview { unknown, data }
                } else {
                    Tag::WaveTiny { unknown, data }
                }
            }
            b"PWV3" | b"PWV4" | b"PWV5" | b"PWV7" if h(12) => {
                let entry = be32(header, 0)? as usize;
                let n = be32(header, 4)? as usize;
                let unknown_v = be32(header, 8)?;
                let expected = match &fourcc {
                    b"PWV3" => 1,
                    b"PWV4" => 6,
                    b"PWV5" => 2,
                    _ => 3,
                };
                if entry != expected || n * entry != body.len() {
                    return Ok(unknown());
                }
                match &fourcc {
                    b"PWV3" => Tag::WaveDetail {
                        unknown: unknown_v,
                        data: body.to_vec(),
                    },
                    b"PWV4" => Tag::WaveColorPreview {
                        unknown: unknown_v,
                        data: body.as_chunks::<6>().0.to_vec(),
                    },
                    b"PWV5" => Tag::WaveColorDetail {
                        unknown: unknown_v,
                        data: body
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|c| u16::from_be_bytes(*c))
                            .collect(),
                    },
                    _ => Tag::Wave3BandDetail {
                        unknown: unknown_v,
                        data: body.as_chunks::<3>().0.to_vec(),
                    },
                }
            }
            b"PWV6" if h(8) => {
                let entry = be32(header, 0)? as usize;
                let n = be32(header, 4)? as usize;
                if entry != 3 || n * 3 != body.len() {
                    return Ok(unknown());
                }
                Tag::Wave3BandPreview {
                    data: body.as_chunks::<3>().0.to_vec(),
                }
            }
            b"PWVC" if h(2) && body.len() == 6 => Tag::VocalConfig {
                reserved: be16(header, 0)?,
                low: be16(body, 0)?,
                mid: be16(body, 2)?,
                high: be16(body, 4)?,
            },
            b"PSSI" if h(20) => match parse_pssi(header, body)? {
                Some(s) => Tag::SongStructure(s),
                None => unknown(),
            },
            _ => unknown(),
        })
    }

    /// Header bytes after `len_tag` and the body.
    fn encode(&self) -> (Vec<u8>, Vec<u8>) {
        let mut h = Vec::new();
        let mut b = Vec::new();
        match self {
            Tag::Path(p) => {
                b = utf16be_nul(p);
                put_be32(&mut h, b.len() as u32);
            }
            Tag::Vbr { unknown, entries } => {
                put_be32(&mut h, *unknown);
                for e in entries {
                    put_be32(&mut b, *e);
                }
            }
            Tag::BeatGrid {
                unknown1,
                unknown2,
                beats,
            } => {
                put_be32(&mut h, *unknown1);
                put_be32(&mut h, *unknown2);
                put_be32(&mut h, beats.len() as u32);
                for beat in beats {
                    put_be16(&mut b, beat.beat_number);
                    put_be16(&mut b, beat.tempo);
                    put_be32(&mut b, beat.time);
                }
            }
            Tag::CueList {
                list_type,
                unknown,
                memory_count,
                cues,
            } => {
                put_be32(&mut h, list_type.to_u32());
                put_be16(&mut h, *unknown);
                put_be16(&mut h, cues.len() as u16);
                put_be32(&mut h, *memory_count);
                for c in cues {
                    let mut e = Vec::with_capacity(56);
                    e.extend_from_slice(b"PCPT");
                    put_be32(&mut e, c.len_header);
                    put_be32(&mut e, (40 + c.tail.len()) as u32);
                    put_be32(&mut e, c.hot_cue);
                    put_be32(&mut e, c.status);
                    put_be32(&mut e, c.unknown1);
                    put_be16(&mut e, c.order_first);
                    put_be16(&mut e, c.order_last);
                    e.push(c.cue_type);
                    e.extend_from_slice(&c.unknown2);
                    put_be32(&mut e, c.time);
                    put_be32(&mut e, c.loop_time);
                    e.extend_from_slice(&c.tail);
                    b.extend_from_slice(&e);
                }
            }
            Tag::ExtCueList {
                list_type,
                unknown,
                cues,
            } => {
                put_be32(&mut h, list_type.to_u32());
                put_be16(&mut h, cues.len() as u16);
                put_be16(&mut h, *unknown);
                for c in cues {
                    let mut e = Vec::new();
                    put_be32(&mut e, c.hot_cue);
                    e.push(c.cue_type);
                    e.extend_from_slice(&c.unknown1);
                    put_be32(&mut e, c.time);
                    put_be32(&mut e, c.loop_time);
                    e.push(c.color_id);
                    e.extend_from_slice(&c.unknown2);
                    put_be16(&mut e, c.loop_numerator);
                    put_be16(&mut e, c.loop_denominator);
                    if let Some(raw) = &c.comment_raw {
                        put_be32(&mut e, raw.len() as u32);
                        e.extend_from_slice(raw);
                        if let Some(col) = &c.hot_color {
                            e.extend_from_slice(col);
                            e.extend_from_slice(&c.trailing);
                        }
                    }
                    b.extend_from_slice(b"PCP2");
                    put_be32(&mut b, c.len_header);
                    put_be32(&mut b, (12 + e.len()) as u32);
                    b.extend_from_slice(&e);
                }
            }
            Tag::WavePreview { unknown, data } | Tag::WaveTiny { unknown, data } => {
                put_be32(&mut h, data.len() as u32);
                put_be32(&mut h, *unknown);
                b = data.clone();
            }
            Tag::WaveDetail { unknown, data } => {
                put_be32(&mut h, 1);
                put_be32(&mut h, data.len() as u32);
                put_be32(&mut h, *unknown);
                b = data.clone();
            }
            Tag::WaveColorPreview { unknown, data } => {
                put_be32(&mut h, 6);
                put_be32(&mut h, data.len() as u32);
                put_be32(&mut h, *unknown);
                b = data.concat();
            }
            Tag::WaveColorDetail { unknown, data } => {
                put_be32(&mut h, 2);
                put_be32(&mut h, data.len() as u32);
                put_be32(&mut h, *unknown);
                for v in data {
                    put_be16(&mut b, *v);
                }
            }
            Tag::Wave3BandPreview { data } => {
                put_be32(&mut h, 3);
                put_be32(&mut h, data.len() as u32);
                b = data.concat();
            }
            Tag::Wave3BandDetail { unknown, data } => {
                put_be32(&mut h, 3);
                put_be32(&mut h, data.len() as u32);
                put_be32(&mut h, *unknown);
                b = data.concat();
            }
            Tag::VocalConfig {
                reserved,
                low,
                mid,
                high,
            } => {
                put_be16(&mut h, *reserved);
                put_be16(&mut b, *low);
                put_be16(&mut b, *mid);
                put_be16(&mut b, *high);
            }
            Tag::SongStructure(s) => {
                let n = s.entries.len() as u16;
                put_be32(&mut h, s.len_entry_bytes);
                put_be16(&mut h, n);
                let mut rest = Vec::new();
                put_be16(&mut rest, s.mood);
                rest.extend_from_slice(&s.unknown1);
                put_be16(&mut rest, s.end_beat);
                put_be16(&mut rest, s.unknown2);
                rest.push(s.bank);
                rest.push(s.unknown3);
                for e in &s.entries {
                    put_be16(&mut rest, e.index);
                    put_be16(&mut rest, e.beat);
                    put_be16(&mut rest, e.kind);
                    rest.extend_from_slice(&e.rest);
                }
                if s.masked {
                    pssi_xor(&mut rest, n);
                }
                h.extend_from_slice(&rest[..14]);
                b.extend_from_slice(&rest[14..]);
            }
            Tag::Unknown { header, body, .. } => {
                h = header.clone();
                b = body.clone();
            }
        }
        (h, b)
    }
}

fn parse_pcpt(body: &[u8], at: usize) -> Result<(Pcpt, usize)> {
    if slice(body, at, 4)? != b"PCPT" {
        return Err(Error::BadMagic {
            expected: "PCPT",
            found: body[at..at + 4].to_vec(),
        });
    }
    let len_header = be32(body, at + 4)?;
    let len = be32(body, at + 8)? as usize;
    if len < 40 {
        return Err(Error::Invalid(format!("PCPT entry too short: {len}")));
    }
    let e = slice(body, at, len)?;
    Ok((
        Pcpt {
            len_header,
            hot_cue: be32(e, 12)?,
            status: be32(e, 16)?,
            unknown1: be32(e, 20)?,
            order_first: be16(e, 24)?,
            order_last: be16(e, 26)?,
            cue_type: e[28],
            unknown2: e[29..32].try_into().unwrap(),
            time: be32(e, 32)?,
            loop_time: be32(e, 36)?,
            tail: e[40..].to_vec(),
        },
        len,
    ))
}

fn parse_pcp2(body: &[u8], at: usize) -> Result<(Pcp2, usize)> {
    if slice(body, at, 4)? != b"PCP2" {
        return Err(Error::BadMagic {
            expected: "PCP2",
            found: body[at..at + 4].to_vec(),
        });
    }
    let len_header = be32(body, at + 4)?;
    let len = be32(body, at + 8)? as usize;
    if len < 0x28 {
        return Err(Error::Invalid(format!("PCP2 entry too short: {len}")));
    }
    let e = slice(body, at, len)?;
    let mut c = Pcp2 {
        len_header,
        hot_cue: be32(e, 0x0c)?,
        cue_type: e[0x10],
        unknown1: e[0x11..0x14].try_into().unwrap(),
        time: be32(e, 0x14)?,
        loop_time: be32(e, 0x18)?,
        color_id: e[0x1c],
        unknown2: e[0x1d..0x24].try_into().unwrap(),
        loop_numerator: be16(e, 0x24)?,
        loop_denominator: be16(e, 0x26)?,
        comment_raw: None,
        hot_color: None,
        trailing: Vec::new(),
    };
    if len >= 0x2c {
        let clen = be32(e, 0x28)? as usize;
        let raw = slice(e, 0x2c, clen)?.to_vec();
        let after = 0x2c + clen;
        c.comment_raw = Some(raw);
        if len >= after + 4 {
            c.hot_color = Some(e[after..after + 4].try_into().unwrap());
            c.trailing = e[after + 4..].to_vec();
        } else if len > after {
            return Err(Error::Invalid("PCP2 entry with partial colour".into()));
        }
    } else if len != 0x28 {
        return Err(Error::Invalid(
            "PCP2 entry with partial comment length".into(),
        ));
    }
    Ok((c, len))
}

fn parse_pssi(header: &[u8], body: &[u8]) -> Result<Option<SongStructure>> {
    let len_entry_bytes = be32(header, 0)?;
    let n = be16(header, 4)?;
    let mut rest = header[6..].to_vec();
    rest.extend_from_slice(body);
    if len_entry_bytes != 24 || rest.len() != 14 + n as usize * 24 {
        return Ok(None);
    }
    let raw_mood = u16::from_be_bytes([rest[0], rest[1]]);
    let masked = !(1..=3).contains(&raw_mood);
    if masked {
        pssi_xor(&mut rest, n);
    }
    let entries = (0..n as usize)
        .map(|i| {
            let o = 14 + i * 24;
            PssiEntry {
                index: u16::from_be_bytes([rest[o], rest[o + 1]]),
                beat: u16::from_be_bytes([rest[o + 2], rest[o + 3]]),
                kind: u16::from_be_bytes([rest[o + 4], rest[o + 5]]),
                rest: rest[o + 6..o + 24].try_into().unwrap(),
            }
        })
        .collect();
    Ok(Some(SongStructure {
        len_entry_bytes,
        masked,
        mood: u16::from_be_bytes([rest[0], rest[1]]),
        unknown1: rest[2..8].try_into().unwrap(),
        end_beat: u16::from_be_bytes([rest[8], rest[9]]),
        unknown2: u16::from_be_bytes([rest[10], rest[11]]),
        bank: rest[12],
        unknown3: rest[13],
        entries,
    }))
}

impl AnlzFile {
    /// An empty file with rekordbox's default header.
    pub fn new(tags: Vec<Tag>) -> Self {
        Self {
            header_tail: DEFAULT_HEADER_TAIL.to_vec(),
            tags,
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if slice(bytes, 0, 4)? != b"PMAI" {
            return Err(Error::BadMagic {
                expected: "PMAI",
                found: bytes[..4].to_vec(),
            });
        }
        let len_header = be32(bytes, 4)? as usize;
        let len_file = be32(bytes, 8)? as usize;
        if len_header < 12 || len_file > bytes.len() || len_header > len_file {
            return Err(Error::Invalid(format!(
                "PMAI header {len_header} / file length {len_file} / actual {}",
                bytes.len()
            )));
        }
        let header_tail = bytes[12..len_header].to_vec();
        let mut tags = Vec::new();
        let mut at = len_header;
        while at < len_file {
            let fourcc: [u8; 4] = slice(bytes, at, 4)?.try_into().unwrap();
            let lh = be32(bytes, at + 4)? as usize;
            let lt = be32(bytes, at + 8)? as usize;
            if lh < 12 || lt < lh || at + lt > len_file {
                return Err(Error::Invalid(format!(
                    "tag {} at {at:#x}: header {lh}, length {lt}",
                    String::from_utf8_lossy(&fourcc)
                )));
            }
            tags.push(Tag::parse(
                fourcc,
                &bytes[at + 12..at + lh],
                &bytes[at + lh..at + lt],
            )?);
            at += lt;
        }
        Ok(Self { header_tail, tags })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for t in &self.tags {
            let (h, b) = t.encode();
            body.extend_from_slice(&t.fourcc());
            put_be32(&mut body, (12 + h.len()) as u32);
            put_be32(&mut body, (12 + h.len() + b.len()) as u32);
            body.extend_from_slice(&h);
            body.extend_from_slice(&b);
        }
        let len_header = 12 + self.header_tail.len();
        let mut out = Vec::with_capacity(len_header + body.len());
        out.extend_from_slice(b"PMAI");
        put_be32(&mut out, len_header as u32);
        put_be32(&mut out, (len_header + body.len()) as u32);
        out.extend_from_slice(&self.header_tail);
        out.extend_from_slice(&body);
        out
    }

    /// First tag with the given four character code.
    pub fn tag(&self, fourcc: &[u8; 4]) -> Option<&Tag> {
        self.tags.iter().find(|t| &t.fourcc() == fourcc)
    }

    /// Audio path from `PPTH`.
    pub fn path(&self) -> Option<String> {
        self.tags.iter().find_map(|t| match t {
            Tag::Path(p) => Some(p.clone()),
            _ => None,
        })
    }

    /// Beat grid from `PQTZ`.
    pub fn beatgrid(&self) -> Option<Beatgrid> {
        self.tags.iter().find_map(|t| match t {
            Tag::BeatGrid { beats, .. } => Some(Beatgrid {
                beats: beats
                    .iter()
                    .map(|b| Beat {
                        time: b.time as f64 / 1000.0,
                        bpm: b.tempo as f64 / 100.0,
                        bar_position: b.beat_number.clamp(1, 4) as u8,
                    })
                    .collect(),
                locked: false,
            }),
            _ => None,
        })
    }

    /// Hot and memory cues. Uses the `PCO2` lists if the file has any (the
    /// `.EXT` file), otherwise the `PCOB` lists (the `.DAT` file).
    pub fn cues(&self) -> Vec<Cue> {
        let has_ext = self
            .tags
            .iter()
            .any(|t| matches!(t, Tag::ExtCueList { .. }));
        let mut out = Vec::new();
        for t in &self.tags {
            match t {
                Tag::ExtCueList { cues, .. } if has_ext => {
                    for c in cues {
                        let color = if c.hot_cue > 0 {
                            match c.hot_color {
                                Some([code, ..]) if code != 0 => palette::hot_cue_color(code),
                                Some([_, r, g, b]) if (r, g, b) != (0, 0, 0) => {
                                    Some(od_core::CueColor::rgb(r, g, b))
                                }
                                _ => None,
                            }
                        } else {
                            palette::track_color(c.color_id)
                        };
                        out.push(make_cue(
                            c.hot_cue,
                            c.cue_type,
                            c.time,
                            c.loop_time,
                            c.comment(),
                            color,
                        ));
                    }
                }
                Tag::CueList { cues, .. } if !has_ext => {
                    for c in cues {
                        out.push(make_cue(
                            c.hot_cue,
                            c.cue_type,
                            c.time,
                            c.loop_time,
                            String::new(),
                            None,
                        ));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Phrases from `PSSI` (beat indices are 0-based like `od_core`).
    pub fn phrases(&self) -> Option<(Vec<Phrase>, PhraseMood)> {
        let s = self.tags.iter().find_map(|t| match t {
            Tag::SongStructure(s) => Some(s),
            _ => None,
        })?;
        let mood = match s.mood {
            1 => PhraseMood::High,
            3 => PhraseMood::Low,
            _ => PhraseMood::Mid,
        };
        let phrases = s
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let end = s.entries.get(i + 1).map(|n| n.beat).unwrap_or(s.end_beat);
                Phrase {
                    start_beat: (e.beat as u32).saturating_sub(1),
                    end_beat: (end as u32).saturating_sub(1),
                    kind: phrase_kind(mood, e.kind),
                }
            })
            .collect();
        Some((phrases, mood))
    }

    /// The most detailed waveform in the file (`PWV7`, else `PWV5`, else
    /// `PWV3`) at 150 columns per second.
    pub fn waveform(&self) -> Option<Waveform> {
        let rate = Waveform::DETAIL_RATE;
        let find = |cc: &[u8; 4]| self.tag(cc);
        let columns: Vec<WaveformColumn> =
            if let Some(Tag::Wave3BandDetail { data, .. }) = find(b"PWV7") {
                data.iter()
                    .map(|&[l, m, h]| {
                        let (l, m, h) = (unscale7(l), unscale7(m), unscale7(h));
                        WaveformColumn {
                            low: l,
                            mid: m,
                            high: h,
                            peak: l.max(m).max(h),
                        }
                    })
                    .collect()
            } else if let Some(Tag::WaveColorDetail { data, .. }) = find(b"PWV5") {
                data.iter()
                    .map(|&v| {
                        let r = ((v >> 13) & 7) as u32 * 255 / 7;
                        let g = ((v >> 10) & 7) as u32 * 255 / 7;
                        let b = ((v >> 7) & 7) as u32 * 255 / 7;
                        let height = ((v >> 2) & 31) as u32 * 255 / 31;
                        WaveformColumn {
                            low: r as u8,
                            mid: g as u8,
                            high: b as u8,
                            peak: height as u8,
                        }
                    })
                    .collect()
            } else if let Some(Tag::WaveDetail { data, .. }) = find(b"PWV3") {
                data.iter()
                    .map(|&v| {
                        let p = ((v & 31) as u32 * 255 / 31) as u8;
                        WaveformColumn {
                            low: p,
                            mid: p,
                            high: p,
                            peak: p,
                        }
                    })
                    .collect()
            } else {
                return None;
            };
        Some(Waveform {
            columns_per_second: rate,
            columns,
        })
    }
}

fn unscale7(v: u8) -> u8 {
    ((v.min(127) as u32 * 255 + 63) / 127) as u8
}

fn make_cue(
    hot: u32,
    cue_type: u8,
    time: u32,
    loop_time: u32,
    label: String,
    color: Option<od_core::CueColor>,
) -> Cue {
    let kind = if hot > 0 {
        CueKind::Hot((hot - 1).min(255) as u8)
    } else {
        CueKind::Memory
    };
    let loop_end = (cue_type == 2 && loop_time != NO_LOOP).then(|| loop_time as f64 / 1000.0);
    Cue {
        kind,
        start: time as f64 / 1000.0,
        loop_end,
        label,
        color,
        source: DataSource::Imported,
    }
}

/// Phrase id of `kind` in the given mood (table "Phrase labels in each
/// mood" of the Deep Symmetry documentation).
pub(crate) fn phrase_id(mood: PhraseMood, kind: PhraseKind) -> u16 {
    match mood {
        PhraseMood::High => match kind {
            PhraseKind::Intro => 1,
            PhraseKind::Up | PhraseKind::Verse => 2,
            PhraseKind::Down | PhraseKind::Bridge => 3,
            PhraseKind::Chorus => 5,
            PhraseKind::Outro => 6,
        },
        PhraseMood::Mid | PhraseMood::Low => match kind {
            PhraseKind::Intro => 1,
            PhraseKind::Verse | PhraseKind::Up => 2,
            PhraseKind::Bridge | PhraseKind::Down => 8,
            PhraseKind::Chorus => 9,
            PhraseKind::Outro => 10,
        },
    }
}

fn phrase_kind(mood: PhraseMood, id: u16) -> PhraseKind {
    match mood {
        PhraseMood::High => match id {
            1 => PhraseKind::Intro,
            2 => PhraseKind::Up,
            3 => PhraseKind::Down,
            5 => PhraseKind::Chorus,
            6 => PhraseKind::Outro,
            _ => PhraseKind::Verse,
        },
        _ => match id {
            1 => PhraseKind::Intro,
            8 => PhraseKind::Bridge,
            9 => PhraseKind::Chorus,
            10 => PhraseKind::Outro,
            _ => PhraseKind::Verse,
        },
    }
}

#[cfg(test)]
mod tests;
