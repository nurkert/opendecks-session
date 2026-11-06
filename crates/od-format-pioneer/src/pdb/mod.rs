//! `/PIONEER/rekordbox/export.pdb`, the DeviceSQL database of a stick.
//!
//! Reference: Deep Symmetry, "Database Exports"
//! (<https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html>),
//! `rekordbox_pdb.ksy` from crate-digger, and the table descriptions of
//! rekordcrate (menu, sort and sync-info tables). All numbers little-endian,
//! 4096 byte pages.
//!
//! The writer produces the 20 tables of a rekordbox export in rekordbox's
//! order, including the default `colors`, `columns`, menu (17) and sort (18)
//! tables and the sync-info row of table 19. `exportExt.pdb` (My Tags) is not
//! written; players work without it.

mod page;
mod rows;
mod string;

use crate::palette;
use crate::{Error, Result};
use od_core::{AudioFormat, KeyNotation, MusicalKey, TrackTags};
use rows::ts;
use std::collections::{BTreeMap, HashMap};

/// Table types of `export.pdb`.
pub mod table {
    pub const TRACKS: u32 = 0;
    pub const GENRES: u32 = 1;
    pub const ARTISTS: u32 = 2;
    pub const ALBUMS: u32 = 3;
    pub const LABELS: u32 = 4;
    pub const KEYS: u32 = 5;
    pub const COLORS: u32 = 6;
    pub const PLAYLIST_TREE: u32 = 7;
    pub const PLAYLIST_ENTRIES: u32 = 8;
    pub const HISTORY_PLAYLISTS: u32 = 11;
    pub const HISTORY_ENTRIES: u32 = 12;
    pub const ARTWORK: u32 = 13;
    pub const COLUMNS: u32 = 16;
    pub const MENU: u32 = 17;
    pub const SORT: u32 = 18;
    pub const SYNC_INFO: u32 = 19;
}

/// One track of the export.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportTrack {
    /// Row id, unique and non-zero. Players report it in status packets.
    pub id: u32,
    /// USB-relative audio path, e.g. `/Contents/Artist/Album/file.mp3`.
    pub device_path: String,
    pub tags: TrackTags,
    /// Seconds (stored as whole seconds).
    pub duration: f64,
    pub sample_rate: u32,
    pub bit_rate_kbps: u32,
    pub bits_per_sample: u16,
    pub file_size: u64,
    pub format: AudioFormat,
    /// Tempo at the start of the track (stored as BPM × 100).
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    /// USB-relative path of the 80×80 artwork, e.g.
    /// `/PIONEER/Artwork/00001/a1.jpg` (players look for `a1_m.jpg` too).
    pub artwork_path: Option<String>,
    /// USB-relative path of the `.DAT` analysis file.
    pub anlz_path: String,
    /// `YYYY-MM-DD`.
    pub date_added: String,
    pub play_count: u32,
}

/// A playlist or folder of the playlist tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportPlaylist {
    /// Non-zero id.
    pub id: u32,
    /// Id of the parent folder, 0 at the root.
    pub parent_id: u32,
    pub name: String,
    pub is_folder: bool,
    /// Position among the siblings (0-based).
    pub sort_order: u32,
    /// Tracks in play order (empty for folders).
    pub track_ids: Vec<u32>,
}

/// A history playlist ("HISTORY 001", …) as written by players.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportHistoryPlaylist {
    pub id: u32,
    pub name: String,
    pub track_ids: Vec<u32>,
}

/// Contents of `export.pdb`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportDatabase {
    pub tracks: Vec<ExportTrack>,
    pub playlists: Vec<ExportPlaylist>,
    pub history: Vec<ExportHistoryPlaylist>,
    /// Date of the export (`YYYY-MM-DD`), stored in the sync-info row and as
    /// the tracks' analysis date. Empty: the latest `date_added` is used.
    pub export_date: String,
}

/// File type codes of track rows (Deep Symmetry, "Track Rows").
fn file_type(f: AudioFormat) -> u16 {
    match f {
        AudioFormat::Mp3 => 0x01,
        AudioFormat::Aac | AudioFormat::Alac => 0x04,
        AudioFormat::Flac => 0x05,
        AudioFormat::Wav => 0x0b,
        AudioFormat::Aiff => 0x0c,
        AudioFormat::Ogg | AudioFormat::Opus => 0x00,
    }
}

fn format_from_type(t: u16) -> Option<AudioFormat> {
    Some(match t {
        0x01 => AudioFormat::Mp3,
        0x04 => AudioFormat::Aac,
        0x05 => AudioFormat::Flac,
        0x0b => AudioFormat::Wav,
        0x0c => AudioFormat::Aiff,
        _ => return None,
    })
}

/// Deterministic stand-in for the track row field rekordbox fills with an
/// apparently random 28 bit value.
fn checksum(s: &str) -> u32 {
    let h = s.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    (h & 0x0fff_ffff).max(1)
}

/// Assigns ids 1, 2, … to distinct keys in first-seen order.
struct Interner<K> {
    ids: HashMap<K, u32>,
    order: Vec<K>,
}

impl<K: std::hash::Hash + Eq + Clone> Interner<K> {
    fn new() -> Self {
        Self {
            ids: HashMap::new(),
            order: Vec::new(),
        }
    }
    fn id(&mut self, k: K) -> u32 {
        if let Some(id) = self.ids.get(&k) {
            return *id;
        }
        self.order.push(k.clone());
        let id = self.order.len() as u32;
        self.ids.insert(k, id);
        id
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

impl ExportDatabase {
    /// Serializes the database to `export.pdb` bytes.
    pub fn to_pdb_bytes(&self) -> Vec<u8> {
        use page::TableSpec;
        let export_date = if self.export_date.is_empty() {
            self.tracks
                .iter()
                .map(|t| t.date_added.clone())
                .max()
                .unwrap_or_default()
        } else {
            self.export_date.clone()
        };

        let mut tracks: Vec<&ExportTrack> = self.tracks.iter().collect();
        tracks.sort_by_key(|t| t.id);

        let mut artists = Interner::new();
        let mut albums = Interner::<(String, u32)>::new();
        let mut genres = Interner::new();
        let mut labels = Interner::new();
        let mut keys = Interner::new();
        let mut artwork = Interner::new();
        let mut opt_artist = |name: &str| {
            if name.is_empty() {
                0
            } else {
                artists.id(name.to_string())
            }
        };

        let mut track_rows = Vec::with_capacity(tracks.len());
        for t in &tracks {
            let g = &t.tags;
            let artist_id = opt_artist(&g.artist);
            let f = rows::TrackFields {
                id: t.id,
                sample_rate: t.sample_rate,
                composer_id: opt_artist(&g.composer),
                file_size: t.file_size.min(u32::MAX as u64) as u32,
                checksum: checksum(&t.device_path),
                artwork_id: t
                    .artwork_path
                    .as_ref()
                    .filter(|p| !p.is_empty())
                    .map_or(0, |p| artwork.id(p.clone())),
                key_id: t.key.map_or(0, |k| keys.id(k.format(KeyNotation::Musical))),
                orig_artist_id: 0,
                label_id: if g.label.is_empty() {
                    0
                } else {
                    labels.id(g.label.clone())
                },
                remixer_id: opt_artist(&g.remixer),
                bitrate: t.bit_rate_kbps,
                track_number: g.track_number.unwrap_or(0),
                tempo: t.bpm.map_or(0, |b| (b * 100.0).round().max(0.0) as u32),
                genre_id: if g.genre.is_empty() {
                    0
                } else {
                    genres.id(g.genre.clone())
                },
                album_id: if g.album.is_empty() {
                    0
                } else {
                    albums.id((g.album.clone(), artist_id))
                },
                artist_id,
                disc_number: 0,
                play_count: t.play_count.min(u16::MAX as u32) as u16,
                year: g.year.unwrap_or(0),
                sample_depth: t.bits_per_sample,
                duration: t.duration.round().clamp(0.0, u16::MAX as f64) as u16,
                color_id: g.color.map_or(0, palette::track_color_id),
                rating: g.rating.min(5),
                file_type: file_type(t.format),
            };
            let mut s: [String; 21] = Default::default();
            s[ts::UNKNOWN2] = "1".into();
            s[ts::UNKNOWN3] = "1".into();
            s[ts::AUTOLOAD_HOTCUES] = "ON".into();
            s[ts::DATE_ADDED] = t.date_added.clone();
            s[ts::ANALYZE_PATH] = t.anlz_path.clone();
            s[ts::ANALYZE_DATE] = if export_date.is_empty() {
                t.date_added.clone()
            } else {
                export_date.clone()
            };
            s[ts::COMMENT] = g.comment.clone();
            s[ts::TITLE] = g.title.clone();
            s[ts::FILENAME] = file_name(&t.device_path).to_string();
            s[ts::FILE_PATH] = t.device_path.clone();
            track_rows.push(rows::track(&f, &s));
        }

        let named = |order: &[String], f: fn(u32, &str) -> page::Row| {
            order
                .iter()
                .enumerate()
                .map(|(i, n)| f(i as u32 + 1, n))
                .collect::<Vec<_>>()
        };

        let mut tree: Vec<&ExportPlaylist> = self.playlists.iter().collect();
        tree.sort_by_key(|p| (p.parent_id, p.sort_order, p.id));
        let tree_rows = tree
            .iter()
            .map(|p| rows::playlist_tree(p.parent_id, p.sort_order, p.id, p.is_folder, &p.name))
            .collect();
        let mut by_id: Vec<&ExportPlaylist> =
            self.playlists.iter().filter(|p| !p.is_folder).collect();
        by_id.sort_by_key(|p| p.id);
        let entry_rows = by_id
            .iter()
            .flat_map(|p| {
                p.track_ids
                    .iter()
                    .enumerate()
                    .map(|(i, t)| rows::three_u32(i as u32 + 1, *t, p.id))
            })
            .collect();

        let mut hist: Vec<&ExportHistoryPlaylist> = self.history.iter().collect();
        hist.sort_by_key(|h| h.id);
        let hist_rows = hist.iter().map(|h| rows::id_name(h.id, &h.name)).collect();
        let hist_entry_rows = hist
            .iter()
            .flat_map(|h| {
                h.track_ids
                    .iter()
                    .enumerate()
                    .map(|(i, t)| rows::three_u32(*t, h.id, i as u32 + 1))
            })
            .collect();

        let spec = |page_type: u32, rows: Vec<page::Row>| TableSpec {
            page_type,
            rows,
            single_transaction: false,
            fixed_seq: None,
        };
        let fixed = |page_type: u32, rows: Vec<page::Row>, seq: u32| TableSpec {
            page_type,
            rows,
            single_transaction: true,
            fixed_seq: Some(seq),
        };

        let tables = vec![
            spec(table::TRACKS, track_rows),
            spec(table::GENRES, named(&genres.order, rows::id_name)),
            spec(table::ARTISTS, named(&artists.order, rows::artist)),
            spec(
                table::ALBUMS,
                albums
                    .order
                    .iter()
                    .enumerate()
                    .map(|(i, (n, a))| rows::album(i as u32 + 1, *a, n))
                    .collect(),
            ),
            spec(table::LABELS, named(&labels.order, rows::id_name)),
            spec(table::KEYS, named(&keys.order, rows::key)),
            fixed(
                table::COLORS,
                palette::TRACK_COLORS
                    .iter()
                    .enumerate()
                    .map(|(i, (n, _))| rows::color(i as u8 + 1, n))
                    .collect(),
                2,
            ),
            spec(table::PLAYLIST_TREE, tree_rows),
            spec(table::PLAYLIST_ENTRIES, entry_rows),
            spec(9, Vec::new()),
            spec(10, Vec::new()),
            spec(table::HISTORY_PLAYLISTS, hist_rows),
            spec(table::HISTORY_ENTRIES, hist_entry_rows),
            spec(table::ARTWORK, named(&artwork.order, rows::id_name)),
            spec(14, Vec::new()),
            spec(15, Vec::new()),
            fixed(
                table::COLUMNS,
                rows::COLUMNS
                    .iter()
                    .map(|(id, c, n)| rows::column(*id, *c, n))
                    .collect(),
                3,
            ),
            fixed(
                table::MENU,
                rows::MENU.iter().map(|r| rows::raw8(*r)).collect(),
                4,
            ),
            fixed(
                table::SORT,
                rows::SORT.iter().map(|r| rows::raw8(*r)).collect(),
                5,
            ),
            TableSpec {
                fixed_seq: Some(1),
                ..spec(
                    table::SYNC_INFO,
                    vec![rows::sync_info(tracks.len() as u32, &export_date, "")],
                )
            },
        ];
        page::write_database(tables)
    }

    /// Parses an `export.pdb` (written by this crate or by rekordbox).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let tables = page::read_database(bytes)?;
        let rows_of = |t: u32| {
            tables
                .iter()
                .filter(move |x| x.page_type == t)
                .flat_map(|x| x.rows.iter().copied())
        };
        let str_at = |row: &[u8], at: usize| -> Result<String> { Ok(string::decode(row, at)?.0) };
        use crate::bytes::{le16, le32, u8_at};

        let mut names: HashMap<u32, BTreeMap<u32, String>> = HashMap::new();
        for t in [table::GENRES, table::LABELS, table::ARTWORK] {
            let m = names.entry(t).or_default();
            for r in rows_of(t) {
                m.insert(le32(r, 0)?, str_at(r, 4)?);
            }
        }
        let mut artists = BTreeMap::new();
        for r in rows_of(table::ARTISTS) {
            let sub = le16(r, 0)?;
            let ofs = if sub & 4 != 0 {
                le16(r, 0x0a)? as usize
            } else {
                u8_at(r, 0x09)? as usize
            };
            artists.insert(le32(r, 4)?, str_at(r, ofs)?);
        }
        let mut albums = BTreeMap::new();
        for r in rows_of(table::ALBUMS) {
            let sub = le16(r, 0)?;
            let ofs = if sub & 4 != 0 {
                le16(r, 0x16)? as usize
            } else {
                u8_at(r, 0x15)? as usize
            };
            albums.insert(le32(r, 0x0c)?, str_at(r, ofs)?);
        }
        let mut keys = BTreeMap::new();
        for r in rows_of(table::KEYS) {
            keys.insert(le32(r, 0)?, str_at(r, 8)?);
        }
        let mut sync_date = String::new();
        for r in rows_of(table::SYNC_INFO) {
            if le16(r, 0)? == 0x0280 {
                sync_date = str_at(r, 12).unwrap_or_default();
            }
        }

        let name = |m: &BTreeMap<u32, String>, id: u32| m.get(&id).cloned().unwrap_or_default();
        let mut tracks = Vec::new();
        for r in rows_of(table::TRACKS) {
            let sub = le16(r, 0)?;
            let s = |i: usize| -> Result<String> {
                let ofs = if sub & 4 != 0 {
                    le16(r, 0x5e + 2 * i)? as usize
                } else {
                    u8_at(r, 0x5d + i)? as usize
                };
                str_at(r, ofs)
            };
            let device_path = s(ts::FILE_PATH)?;
            let ft = le16(r, 0x5a)?;
            let format = AudioFormat::from_path(std::path::Path::new(&device_path))
                .or_else(|| format_from_type(ft))
                .unwrap_or(AudioFormat::Mp3);
            let year = le16(r, 0x50)?;
            let track_number = le32(r, 0x34)?;
            let tempo = le32(r, 0x38)?;
            let artwork_id = le32(r, 0x1c)?;
            let key_id = le32(r, 0x20)?;
            let tags = TrackTags {
                title: s(ts::TITLE)?,
                artist: name(&artists, le32(r, 0x44)?),
                album: name(&albums, le32(r, 0x40)?),
                genre: name(&names[&table::GENRES], le32(r, 0x3c)?),
                label: name(&names[&table::LABELS], le32(r, 0x28)?),
                remixer: name(&artists, le32(r, 0x2c)?),
                composer: name(&artists, le32(r, 0x0c)?),
                comment: s(ts::COMMENT)?,
                year: (year != 0).then_some(year),
                track_number: (track_number != 0).then_some(track_number),
                rating: u8_at(r, 0x59)?,
                color: palette::track_color(u8_at(r, 0x58)?),
            };
            tracks.push(ExportTrack {
                id: le32(r, 0x48)?,
                device_path,
                tags,
                duration: le16(r, 0x54)? as f64,
                sample_rate: le32(r, 0x08)?,
                bit_rate_kbps: le32(r, 0x30)?,
                bits_per_sample: le16(r, 0x52)?,
                file_size: le32(r, 0x10)? as u64,
                format,
                bpm: (tempo != 0).then(|| tempo as f64 / 100.0),
                key: keys.get(&key_id).and_then(|k| MusicalKey::parse(k)),
                artwork_path: names[&table::ARTWORK].get(&artwork_id).cloned(),
                anlz_path: s(ts::ANALYZE_PATH)?,
                date_added: s(ts::DATE_ADDED)?,
                play_count: le16(r, 0x4e)? as u32,
            });
        }
        tracks.sort_by_key(|t| t.id);

        let mut entries: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for r in rows_of(table::PLAYLIST_ENTRIES) {
            entries
                .entry(le32(r, 8)?)
                .or_default()
                .push((le32(r, 0)?, le32(r, 4)?));
        }
        let mut playlists = Vec::new();
        for r in rows_of(table::PLAYLIST_TREE) {
            let id = le32(r, 0x0c)?;
            let mut e = entries.remove(&id).unwrap_or_default();
            e.sort();
            playlists.push(ExportPlaylist {
                id,
                parent_id: le32(r, 0)?,
                name: str_at(r, 0x14)?,
                is_folder: le32(r, 0x10)? != 0,
                sort_order: le32(r, 0x08)?,
                track_ids: e.into_iter().map(|(_, t)| t).collect(),
            });
        }
        playlists.sort_by_key(|p| (p.parent_id, p.sort_order, p.id));

        let mut hentries: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for r in rows_of(table::HISTORY_ENTRIES) {
            hentries
                .entry(le32(r, 4)?)
                .or_default()
                .push((le32(r, 8)?, le32(r, 0)?));
        }
        let mut history = Vec::new();
        for r in rows_of(table::HISTORY_PLAYLISTS) {
            let id = le32(r, 0)?;
            let mut e = hentries.remove(&id).unwrap_or_default();
            e.sort();
            history.push(ExportHistoryPlaylist {
                id,
                name: str_at(r, 4)?,
                track_ids: e.into_iter().map(|(_, t)| t).collect(),
            });
        }
        history.sort_by_key(|h| h.id);

        if tables.is_empty() {
            return Err(Error::Invalid("no tables".into()));
        }
        Ok(Self {
            tracks,
            playlists,
            history,
            export_date: sync_date,
        })
    }
}

#[cfg(test)]
mod tests;
