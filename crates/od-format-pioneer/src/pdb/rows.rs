//! Row encoders and the default rows rekordbox writes into every export.
//!
//! Row layouts: Deep Symmetry "Table Rows" and `rekordbox_pdb.ksy`.
//! Padding rules (rows 4 byte aligned; artist and album rows allocated
//! `align4(header) + align4(name) + 4` bytes; track rows followed by about
//! 48 spare bytes) follow observations published by rekordcrate and
//! baken-export and match rekordbox's demo export.

use super::page::Row;
use super::string::{DString, encode, encode_wide};
use crate::bytes::*;

/// Appends a string to `row`, aligning UTF-16 strings to 4 bytes. Returns
/// the offset of the string from the row start.
fn push_str(row: &mut Vec<u8>, s: &DString) -> usize {
    if s.wide {
        row.resize(align4(row.len()), 0);
    }
    let at = row.len();
    row.extend_from_slice(&s.bytes);
    at
}

fn finish(mut bytes: Vec<u8>, extra: usize, index_shift: bool) -> Row {
    let len = align4(bytes.len()) + extra;
    bytes.resize(len, 0);
    Row { bytes, index_shift }
}

/// `id, name` rows (genres, labels, artwork, history playlists).
pub(crate) fn id_name(id: u32, name: &str) -> Row {
    let mut r = Vec::new();
    put_le32(&mut r, id);
    push_str(&mut r, &encode(name));
    finish(r, 0, false)
}

pub(crate) fn key(id: u32, name: &str) -> Row {
    let mut r = Vec::new();
    put_le32(&mut r, id);
    put_le32(&mut r, id);
    push_str(&mut r, &encode(name));
    finish(r, 0, false)
}

/// Name tail of artist and album rows: `0x03, ofs_name_near, name`.
fn near_name_row(mut r: Vec<u8>, name: &str) -> Row {
    let header = r.len() + 2;
    let s = encode(name);
    let name_at = if s.wide { align4(header) } else { header };
    r.push(0x03);
    r.push(name_at as u8);
    r.resize(name_at, 0);
    r.extend_from_slice(&s.bytes);
    let slot = align4(header) + align4(s.bytes.len()) + 4;
    let slot = slot.max(align4(r.len()));
    r.resize(slot, 0);
    Row {
        bytes: r,
        index_shift: true,
    }
}

pub(crate) fn artist(id: u32, name: &str) -> Row {
    let mut r = Vec::new();
    put_le16(&mut r, 0x0060);
    put_le16(&mut r, 0); // index_shift
    put_le32(&mut r, id);
    near_name_row(r, name)
}

pub(crate) fn album(id: u32, artist_id: u32, name: &str) -> Row {
    let mut r = Vec::new();
    put_le16(&mut r, 0x0080);
    put_le16(&mut r, 0);
    put_le32(&mut r, 0);
    put_le32(&mut r, artist_id);
    put_le32(&mut r, id);
    put_le32(&mut r, 0);
    near_name_row(r, name)
}

pub(crate) fn color(id: u8, name: &str) -> Row {
    let mut r = vec![0, 0, 0, 0, id];
    put_le16(&mut r, id as u16);
    r.push(0);
    push_str(&mut r, &encode(name));
    finish(r, 0, false)
}

pub(crate) fn playlist_tree(
    parent: u32,
    sort_order: u32,
    id: u32,
    is_folder: bool,
    name: &str,
) -> Row {
    let mut r = Vec::new();
    put_le32(&mut r, parent);
    put_le32(&mut r, 0);
    put_le32(&mut r, sort_order);
    put_le32(&mut r, id);
    put_le32(&mut r, is_folder as u32);
    push_str(&mut r, &encode(name));
    finish(r, 0, false)
}

pub(crate) fn three_u32(a: u32, b: u32, c: u32) -> Row {
    let mut r = Vec::new();
    put_le32(&mut r, a);
    put_le32(&mut r, b);
    put_le32(&mut r, c);
    finish(r, 0, false)
}

pub(crate) fn column(id: u16, code: u16, name: &str) -> Row {
    let mut r = Vec::new();
    put_le16(&mut r, id);
    put_le16(&mut r, code);
    push_str(&mut r, &encode_wide(&format!("\u{fffa}{name}\u{fffb}")));
    finish(r, 0, false)
}

pub(crate) fn raw8(b: [u8; 8]) -> Row {
    Row {
        bytes: b.to_vec(),
        index_shift: false,
    }
}

/// The single row of table 19 ("history"/sync info): number of tracks,
/// export date, format version "1000" and a device label.
pub(crate) fn sync_info(num_tracks: u32, date: &str, label: &str) -> Row {
    let mut r = Vec::new();
    put_le16(&mut r, 0x0280);
    put_le16(&mut r, 0);
    put_le32(&mut r, num_tracks);
    put_le32(&mut r, 0);
    push_str(&mut r, &encode(date));
    r.extend_from_slice(&[0x19, 0x1e]);
    push_str(&mut r, &encode("1000"));
    push_str(&mut r, &encode(label));
    finish(r, 8, true)
}

/// Numeric fields of a track row.
#[derive(Default)]
pub(crate) struct TrackFields {
    pub id: u32,
    pub sample_rate: u32,
    pub composer_id: u32,
    pub file_size: u32,
    pub checksum: u32,
    pub artwork_id: u32,
    pub key_id: u32,
    pub orig_artist_id: u32,
    pub label_id: u32,
    pub remixer_id: u32,
    pub bitrate: u32,
    pub track_number: u32,
    pub tempo: u32,
    pub genre_id: u32,
    pub album_id: u32,
    pub artist_id: u32,
    pub disc_number: u16,
    pub play_count: u16,
    pub year: u16,
    pub sample_depth: u16,
    pub duration: u16,
    pub color_id: u8,
    pub rating: u8,
    pub file_type: u16,
}

/// Indices of the 21 track strings.
pub(crate) mod ts {
    pub const UNKNOWN2: usize = 2;
    pub const UNKNOWN3: usize = 3;
    pub const AUTOLOAD_HOTCUES: usize = 7;
    pub const DATE_ADDED: usize = 10;
    pub const ANALYZE_PATH: usize = 14;
    pub const ANALYZE_DATE: usize = 15;
    pub const COMMENT: usize = 16;
    pub const TITLE: usize = 17;
    pub const FILENAME: usize = 19;
    pub const FILE_PATH: usize = 20;
}

/// Track row: 0x5e byte fixed part, the constant 3, 21 string offsets, the
/// strings, then spare bytes.
pub(crate) fn track(f: &TrackFields, strings: &[String; 21]) -> Row {
    let mut r = Vec::with_capacity(512);
    put_le16(&mut r, 0x0024);
    put_le16(&mut r, 0);
    put_le32(&mut r, 0x000c_0700);
    put_le32(&mut r, f.sample_rate);
    put_le32(&mut r, f.composer_id);
    put_le32(&mut r, f.file_size);
    put_le32(&mut r, f.checksum);
    put_le16(&mut r, 19048);
    put_le16(&mut r, 30967);
    for v in [
        f.artwork_id,
        f.key_id,
        f.orig_artist_id,
        f.label_id,
        f.remixer_id,
        f.bitrate,
        f.track_number,
        f.tempo,
        f.genre_id,
        f.album_id,
        f.artist_id,
        f.id,
    ] {
        put_le32(&mut r, v);
    }
    for v in [
        f.disc_number,
        f.play_count,
        f.year,
        f.sample_depth,
        f.duration,
        0x29,
    ] {
        put_le16(&mut r, v);
    }
    r.push(f.color_id);
    r.push(f.rating);
    put_le16(&mut r, f.file_type);
    put_le16(&mut r, 3);
    debug_assert_eq!(r.len(), 0x5e);
    r.resize(0x5e + 21 * 2, 0);
    for (i, s) in strings.iter().enumerate() {
        let at = push_str(&mut r, &encode(s));
        set_le16(&mut r, 0x5e + 2 * i, at as u16);
    }
    finish(r, 48, true)
}

/// Browse columns (table 16) as in every rekordbox export: `(id, code, name)`.
pub(crate) const COLUMNS: [(u16, u16, &str); 27] = [
    (1, 0x80, "GENRE"),
    (2, 0x81, "ARTIST"),
    (3, 0x82, "ALBUM"),
    (4, 0x83, "TRACK"),
    (5, 0x85, "BPM"),
    (6, 0x86, "RATING"),
    (7, 0x87, "YEAR"),
    (8, 0x88, "REMIXER"),
    (9, 0x89, "LABEL"),
    (10, 0x8a, "ORIGINAL ARTIST"),
    (11, 0x8b, "KEY"),
    (12, 0x8d, "CUE"),
    (13, 0x8e, "COLOR"),
    (14, 0x92, "TIME"),
    (15, 0x93, "BITRATE"),
    (16, 0x94, "FILE NAME"),
    (17, 0x84, "PLAYLIST"),
    (18, 0x98, "HOT CUE BANK"),
    (19, 0x95, "HISTORY"),
    (20, 0x91, "SEARCH"),
    (21, 0x96, "COMMENTS"),
    (22, 0x8c, "DATE ADDED"),
    (23, 0x97, "DJ PLAY COUNT"),
    (24, 0x90, "FOLDER"),
    (25, 0xa1, "DEFAULT"),
    (26, 0xa2, "ALPHABET"),
    (27, 0xaa, "MATCHING"),
];

/// Default browse menu configuration (table 17): `category (u16), content
/// (u16), kind (u8), visibility (u8), sort order (u16)`. Visible entries are
/// ARTIST, ALBUM, TRACK, KEY, PLAYLIST, HISTORY, SEARCH, FOLDER and
/// DATE ADDED (as in a default rekordbox export).
pub(crate) const MENU: [[u8; 8]; 22] = [
    [0x01, 0x00, 0x01, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x05, 0x00, 0x06, 0x00, 0x05, 0x01, 0x00, 0x00],
    [0x06, 0x00, 0x07, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x07, 0x00, 0x08, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x08, 0x00, 0x09, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x09, 0x00, 0x0a, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x0a, 0x00, 0x0b, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x0d, 0x00, 0x0f, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x0e, 0x00, 0x13, 0x00, 0x04, 0x01, 0x00, 0x00],
    [0x0f, 0x00, 0x14, 0x00, 0x06, 0x01, 0x00, 0x00],
    [0x10, 0x00, 0x15, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x12, 0x00, 0x17, 0x00, 0x63, 0x01, 0x00, 0x00],
    [0x02, 0x00, 0x02, 0x00, 0x02, 0x00, 0x01, 0x00],
    [0x03, 0x00, 0x03, 0x00, 0x03, 0x00, 0x02, 0x00],
    [0x04, 0x00, 0x04, 0x00, 0x01, 0x00, 0x03, 0x00],
    [0x0b, 0x00, 0x0c, 0x00, 0x63, 0x00, 0x04, 0x00],
    [0x11, 0x00, 0x05, 0x00, 0x63, 0x00, 0x05, 0x00],
    [0x13, 0x00, 0x16, 0x00, 0x63, 0x00, 0x06, 0x00],
    [0x14, 0x00, 0x12, 0x00, 0x63, 0x00, 0x07, 0x00],
    [0x1b, 0x00, 0x1a, 0x00, 0x63, 0x02, 0x08, 0x00],
    [0x18, 0x00, 0x11, 0x00, 0x63, 0x00, 0x09, 0x00],
    [0x16, 0x00, 0x1b, 0x00, 0x63, 0x00, 0x0a, 0x00],
];

/// Default rows of table 18 (sort configuration, not yet understood).
pub(crate) const SORT: [[u8; 8]; 17] = [
    [0x01, 0x00, 0x06, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x15, 0x00, 0x07, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x0e, 0x00, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x08, 0x00, 0x09, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x09, 0x00, 0x0a, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x0a, 0x00, 0x0b, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x0f, 0x00, 0x0d, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x0d, 0x00, 0x0f, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x17, 0x00, 0x10, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x16, 0x00, 0x11, 0x00, 0x01, 0x00, 0x00, 0x00],
    [0x19, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00],
    [0x1a, 0x00, 0x01, 0x00, 0x00, 0x02, 0x00, 0x00],
    [0x02, 0x00, 0x02, 0x00, 0x00, 0x03, 0x00, 0x00],
    [0x03, 0x00, 0x03, 0x00, 0x00, 0x04, 0x00, 0x00],
    [0x05, 0x00, 0x04, 0x00, 0x00, 0x05, 0x00, 0x00],
    [0x06, 0x00, 0x05, 0x00, 0x00, 0x06, 0x00, 0x00],
    [0x0b, 0x00, 0x0c, 0x00, 0x00, 0x07, 0x00, 0x00],
];
