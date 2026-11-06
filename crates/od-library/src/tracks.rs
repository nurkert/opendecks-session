//! Tracks, their analysis data and cues.

use crate::blob;
use crate::journal::EntityState;
use crate::{Library, LibraryError, NOW, Result, record_change};
use od_core::{
    AudioFormat, Beatgrid, ContentHash, Cue, CueColor, CueKind, DataSource, MusicalKey, Phrase,
    PhraseMood, TrackAnalysis, TrackId, TrackTags,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::PathBuf;

/// A track to add to the library.
#[derive(Clone, Debug)]
pub struct NewTrack {
    pub path: PathBuf,
    pub content_hash: ContentHash,
    pub file_size: u64,
    pub format: Option<AudioFormat>,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_rate: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub tags: TrackTags,
    pub artwork: Option<(Vec<u8>, String)>,
}

/// A track as stored in the library.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackRecord {
    pub id: TrackId,
    pub path: PathBuf,
    pub content_hash: ContentHash,
    pub file_size: u64,
    pub format: Option<AudioFormat>,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_rate: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub tags: TrackTags,
    pub date_added: String,
    pub play_count: u32,
    pub last_played: Option<String>,
    pub missing: bool,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    pub analysis_version: u32,
}

/// One play in a history list.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HistoryPlay {
    /// History list (one per performance session or imported player history).
    pub session: String,
    pub played_at: String,
    /// `None` if the track was removed from the library since.
    pub track: Option<TrackId>,
    /// "opendecks" or "device".
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    DateAdded,
    Title,
    Artist,
    Album,
    Genre,
    Bpm,
    Key,
    Rating,
    Duration,
    PlayCount,
}

impl SortKey {
    fn column(self) -> &'static str {
        match self {
            SortKey::DateAdded => "date_added",
            SortKey::Title => "title COLLATE NOCASE",
            SortKey::Artist => "artist COLLATE NOCASE",
            SortKey::Album => "album COLLATE NOCASE",
            SortKey::Genre => "genre COLLATE NOCASE",
            SortKey::Bpm => "bpm",
            SortKey::Key => "musical_key",
            SortKey::Rating => "rating",
            SortKey::Duration => "duration",
            SortKey::PlayCount => "play_count",
        }
    }
}

pub(crate) const TRACK_COLUMNS: &str = "id, path, content_hash, file_size, format, duration, sample_rate, channels, \
     bit_rate, bits_per_sample, title, artist, album, genre, label, remixer, composer, comment, year, \
     track_number, rating, color, date_added, play_count, last_played, missing, bpm, musical_key, \
     analysis_version";

pub(crate) fn track_from_row(r: &Row) -> rusqlite::Result<TrackRecord> {
    let format: Option<String> = r.get(4)?;
    let color: Option<String> = r.get(21)?;
    let key: Option<String> = r.get(27)?;
    Ok(TrackRecord {
        id: TrackId(r.get(0)?),
        path: PathBuf::from(r.get::<_, String>(1)?),
        content_hash: ContentHash(r.get(2)?),
        file_size: r.get::<_, i64>(3)? as u64,
        format: format.as_deref().and_then(AudioFormat::parse),
        duration: r.get(5)?,
        sample_rate: r.get(6)?,
        channels: r.get(7)?,
        bit_rate: r.get(8)?,
        bits_per_sample: r.get(9)?,
        tags: TrackTags {
            title: r.get(10)?,
            artist: r.get(11)?,
            album: r.get(12)?,
            genre: r.get(13)?,
            label: r.get(14)?,
            remixer: r.get(15)?,
            composer: r.get(16)?,
            comment: r.get(17)?,
            year: r.get(18)?,
            track_number: r.get(19)?,
            rating: r.get(20)?,
            color: color.as_deref().and_then(CueColor::from_hex),
        },
        date_added: r.get(22)?,
        play_count: r.get(23)?,
        last_played: r.get(24)?,
        missing: r.get(25)?,
        bpm: r.get(26)?,
        key: key.as_deref().and_then(MusicalKey::parse),
        analysis_version: r.get(28)?,
    })
}

fn key_text(k: Option<MusicalKey>) -> Option<String> {
    k.map(|k| k.format(od_core::KeyNotation::Camelot))
}

fn write_tags(conn: &Connection, id: TrackId, t: &TrackTags) -> Result<()> {
    let n = conn.execute(
        "UPDATE track SET title=?2, artist=?3, album=?4, genre=?5, label=?6, remixer=?7, composer=?8,
            comment=?9, year=?10, track_number=?11, rating=?12, color=?13 WHERE id=?1",
        params![
            id.0, t.title, t.artist, t.album, t.genre, t.label, t.remixer, t.composer, t.comment,
            t.year, t.track_number, t.rating.min(5), t.color.map(|c| c.to_hex())
        ],
    )?;
    if n == 0 {
        return Err(LibraryError::NotFound(format!("track {id}")));
    }
    Ok(())
}

pub(crate) fn read_tags(conn: &Connection, id: TrackId) -> Result<TrackTags> {
    Ok(get_track(conn, id)?.tags)
}

pub(crate) fn get_track(conn: &Connection, id: TrackId) -> Result<TrackRecord> {
    conn.query_row(&format!("SELECT {TRACK_COLUMNS} FROM track WHERE id = ?1"), [id.0], track_from_row)
        .optional()?
        .ok_or_else(|| LibraryError::NotFound(format!("track {id}")))
}

pub(crate) fn read_cues(conn: &Connection, id: TrackId) -> Result<Vec<Cue>> {
    let mut stmt = conn.prepare_cached(
        "SELECT kind, slot, start, loop_end, label, color, source FROM cue
         WHERE track_id = ?1 ORDER BY kind, slot, start",
    )?;
    let rows = stmt.query_map([id.0], |r| {
        let kind: String = r.get(0)?;
        let slot: u8 = r.get(1)?;
        let color: Option<String> = r.get(5)?;
        let source: String = r.get(6)?;
        Ok(Cue {
            kind: if kind == "hot" { CueKind::Hot(slot) } else { CueKind::Memory },
            start: r.get(2)?,
            loop_end: r.get(3)?,
            label: r.get(4)?,
            color: color.as_deref().and_then(CueColor::from_hex),
            source: DataSource::parse(&source).unwrap_or(DataSource::Manual),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn write_cues(conn: &Connection, id: TrackId, cues: &[Cue]) -> Result<()> {
    conn.execute("DELETE FROM cue WHERE track_id = ?1", [id.0])?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO cue (track_id, kind, slot, start, loop_end, label, color, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    for c in cues {
        let (kind, slot) = match c.kind {
            CueKind::Hot(s) => ("hot", s),
            CueKind::Memory => ("memory", 0),
        };
        stmt.execute(params![
            id.0,
            kind,
            slot,
            c.start,
            c.loop_end,
            c.label,
            c.color.map(|c| c.to_hex()),
            c.source.as_str()
        ])?;
    }
    Ok(())
}

/// Beatgrid with its source, as stored.
pub(crate) fn read_grid(conn: &Connection, id: TrackId) -> Result<Option<(Beatgrid, DataSource)>> {
    let row: Option<(Option<Vec<u8>>, Option<String>, bool)> = conn
        .query_row(
            "SELECT grid, grid_source, grid_locked FROM analysis WHERE track_id = ?1",
            [id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((Some(bytes), source, locked)) => Some((
            blob::decode_grid(&bytes, locked),
            source.as_deref().and_then(DataSource::parse).unwrap_or(DataSource::Analysis),
        )),
        _ => None,
    })
}

fn ensure_analysis_row(conn: &Connection, id: TrackId) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO analysis (track_id, version) VALUES (?1, 0)", [id.0])?;
    Ok(())
}

pub(crate) fn write_grid(conn: &Connection, id: TrackId, grid: Option<&(Beatgrid, DataSource)>) -> Result<()> {
    ensure_analysis_row(conn, id)?;
    match grid {
        Some((g, src)) => {
            conn.execute(
                "UPDATE analysis SET grid = ?2, grid_source = ?3, grid_locked = ?4 WHERE track_id = ?1",
                params![id.0, blob::encode_grid(g), src.as_str(), g.locked],
            )?;
            conn.execute("UPDATE track SET bpm = ?2 WHERE id = ?1", params![id.0, g.dominant_bpm()])?;
        }
        None => {
            conn.execute(
                "UPDATE analysis SET grid = NULL, grid_source = NULL, grid_locked = 0 WHERE track_id = ?1",
                [id.0],
            )?;
            conn.execute("UPDATE track SET bpm = NULL WHERE id = ?1", [id.0])?;
        }
    }
    Ok(())
}

pub(crate) fn read_key(conn: &Connection, id: TrackId) -> Result<Option<(MusicalKey, DataSource)>> {
    let key: Option<String> =
        conn.query_row("SELECT musical_key FROM track WHERE id = ?1", [id.0], |r| r.get(0)).optional()?.flatten();
    let source: Option<String> = conn
        .query_row("SELECT key_source FROM analysis WHERE track_id = ?1", [id.0], |r| r.get(0))
        .optional()?
        .flatten();
    Ok(key.as_deref().and_then(MusicalKey::parse).map(|k| {
        (k, source.as_deref().and_then(DataSource::parse).unwrap_or(DataSource::Analysis))
    }))
}

pub(crate) fn write_key(conn: &Connection, id: TrackId, key: Option<(MusicalKey, DataSource)>) -> Result<()> {
    ensure_analysis_row(conn, id)?;
    conn.execute(
        "UPDATE track SET musical_key = ?2 WHERE id = ?1",
        params![id.0, key_text(key.map(|k| k.0))],
    )?;
    conn.execute(
        "UPDATE analysis SET key_source = ?2 WHERE track_id = ?1",
        params![id.0, key.map(|k| k.1.as_str())],
    )?;
    Ok(())
}

impl Library {
    /// Adds a track, or returns the existing id if the path is already known.
    pub fn add_track(&mut self, t: &NewTrack) -> Result<TrackId> {
        let path = t.path.to_string_lossy().into_owned();
        self.tx(|tx| {
            if let Some(id) = tx
                .query_row("SELECT id FROM track WHERE path = ?1", [&path], |r| r.get::<_, i64>(0))
                .optional()?
            {
                return Ok(TrackId(id));
            }
            tx.execute(
                &format!(
                    "INSERT INTO track (path, content_hash, file_size, format, duration, sample_rate,
                        channels, bit_rate, bits_per_sample, date_added)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, {NOW})"
                ),
                params![
                    path,
                    t.content_hash.0,
                    t.file_size as i64,
                    t.format.map(|f| f.as_str()),
                    t.duration,
                    t.sample_rate,
                    t.channels,
                    t.bit_rate,
                    t.bits_per_sample
                ],
            )?;
            let id = TrackId(tx.last_insert_rowid());
            write_tags(tx, id, &t.tags)?;
            if let Some((data, media_type)) = &t.artwork {
                tx.execute(
                    "INSERT INTO artwork (track_id, media_type, data) VALUES (?1, ?2, ?3)",
                    params![id.0, media_type, data],
                )?;
            }
            record_change(tx, "track_added", Some(id), None)?;
            Ok(id)
        })
    }

    pub fn track(&self, id: TrackId) -> Result<TrackRecord> {
        get_track(&self.conn, id)
    }

    pub fn track_by_path(&self, path: &std::path::Path) -> Result<Option<TrackRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {TRACK_COLUMNS} FROM track WHERE path = ?1"),
                [path.to_string_lossy()],
                track_from_row,
            )
            .optional()?)
    }

    pub fn tracks_by_hash(&self, hash: &ContentHash) -> Result<Vec<TrackRecord>> {
        let mut stmt =
            self.conn.prepare_cached(&format!("SELECT {TRACK_COLUMNS} FROM track WHERE content_hash = ?1"))?;
        Ok(stmt.query_map([&hash.0], track_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn track_count(&self) -> Result<u64> {
        Ok(self.conn.query_row("SELECT count(*) FROM track", [], |r| r.get::<_, i64>(0))? as u64)
    }

    /// All tracks in the given order.
    pub fn tracks(&self, sort: SortKey, descending: bool) -> Result<Vec<TrackRecord>> {
        let dir = if descending { "DESC" } else { "ASC" };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TRACK_COLUMNS} FROM track ORDER BY {} {dir}, id {dir}",
            sort.column()
        ))?;
        Ok(stmt.query_map([], track_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn tracks_by_ids(&self, ids: &[TrackId]) -> Result<Vec<TrackRecord>> {
        ids.iter().map(|&id| self.track(id)).collect()
    }

    /// Full-text search over title, artist, album, genre, label, remixer and
    /// comment. Every word must match as a prefix.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<TrackRecord>> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{w}\"*"))
            .collect();
        if terms.is_empty() {
            let mut all = self.tracks(SortKey::DateAdded, true)?;
            all.truncate(limit);
            return Ok(all);
        }
        let cols = TRACK_COLUMNS.split(", ").map(|c| format!("t.{c}")).collect::<Vec<_>>().join(", ");
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {cols} FROM track_fts f JOIN track t ON t.id = f.rowid
             WHERE track_fts MATCH ?1 ORDER BY rank LIMIT ?2"
        ))?;
        Ok(stmt
            .query_map(params![terms.join(" "), limit as i64], track_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn artwork(&self, id: TrackId) -> Result<Option<(Vec<u8>, String)>> {
        Ok(self
            .conn
            .query_row("SELECT data, media_type FROM artwork WHERE track_id = ?1", [id.0], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    /// Removes a track from the library (the file is not touched).
    pub fn remove_track(&mut self, id: TrackId) -> Result<()> {
        self.tx(|tx| {
            // Record the change for every playlist that contained it, so that
            // sticks holding those playlists get updated.
            let nodes: Vec<i64> = tx
                .prepare("SELECT DISTINCT node_id FROM entry WHERE track_id = ?1")?
                .query_map([id.0], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            tx.execute("DELETE FROM track WHERE id = ?1", [id.0])?;
            for n in nodes {
                record_change(tx, "entries", None, Some(crate::NodeId(n)))?;
            }
            record_change(tx, "track_removed", Some(id), None)
        })
    }

    pub fn cues(&self, id: TrackId) -> Result<Vec<Cue>> {
        read_cues(&self.conn, id)
    }

    pub fn beatgrid(&self, id: TrackId) -> Result<Option<Beatgrid>> {
        Ok(read_grid(&self.conn, id)?.map(|(g, _)| g))
    }

    pub fn beatgrid_with_source(&self, id: TrackId) -> Result<Option<(Beatgrid, DataSource)>> {
        read_grid(&self.conn, id)
    }

    pub fn key_with_source(&self, id: TrackId) -> Result<Option<(MusicalKey, DataSource)>> {
        read_key(&self.conn, id)
    }

    /// User edit: replace tags.
    pub fn set_tags(&mut self, id: TrackId, tags: TrackTags) -> Result<()> {
        self.edit("Edit tags", vec![EntityState::Tags { track: id, tags }])
    }

    /// User edit: replace all cues of a track.
    pub fn set_cues(&mut self, id: TrackId, mut cues: Vec<Cue>) -> Result<()> {
        for c in &mut cues {
            c.source = DataSource::Manual;
        }
        self.edit("Edit cues", vec![EntityState::Cues { track: id, cues }])
    }

    /// User edit: replace the beatgrid (marks it as manual).
    pub fn set_beatgrid(&mut self, id: TrackId, grid: Beatgrid) -> Result<()> {
        self.edit("Edit beatgrid", vec![EntityState::Grid { track: id, grid: Some((grid, DataSource::Manual)) }])
    }

    /// User edit: set or clear the key (marks it as manual).
    pub fn set_key(&mut self, id: TrackId, key: Option<MusicalKey>) -> Result<()> {
        self.edit(
            "Edit key",
            vec![EntityState::Key { track: id, key: key.map(|k| (k, DataSource::Manual)) }],
        )
    }

    /// Stores an analysis result. Beatgrid and key are only replaced when the
    /// stored data has the same or lower priority than `source` and the grid
    /// is not locked. Waveform, phrases, loudness and energy are always
    /// replaced. Not journaled (not a user edit).
    pub fn store_analysis(&mut self, id: TrackId, a: &TrackAnalysis, source: DataSource) -> Result<()> {
        self.tx(|tx| {
            ensure_analysis_row(tx, id)?;
            let existing = read_grid(tx, id)?;
            let replace_grid = !a.beatgrid.is_empty()
                && existing.as_ref().is_none_or(|(g, s)| !g.locked && *s <= source);
            if replace_grid {
                write_grid(tx, id, Some(&(a.beatgrid.clone(), source)))?;
            }
            let existing_key = read_key(tx, id)?;
            if a.key.is_some() && existing_key.is_none_or(|(_, s)| s <= source) {
                write_key(tx, id, a.key.map(|k| (k, source)))?;
            }
            tx.execute(
                "UPDATE analysis SET version = ?2, waveform = ?3, waveform_rate = ?4, phrases = ?5,
                    phrase_mood = ?6, loudness = ?7, peak = ?8, energy = ?9 WHERE track_id = ?1",
                params![
                    id.0,
                    a.analysis_version,
                    blob::encode_waveform(&a.waveform),
                    a.waveform.columns_per_second,
                    serde_json::to_string(&a.phrases)?,
                    serde_json::to_string(&a.phrase_mood)?,
                    a.loudness_lufs,
                    a.peak,
                    blob::encode_f32s(&a.energy)
                ],
            )?;
            tx.execute(
                "UPDATE track SET analysis_version = ?2 WHERE id = ?1",
                params![id.0, a.analysis_version],
            )?;
            record_change(tx, "analysis", Some(id), None)
        })
    }

    /// Stores imported cues: replaces cues that came from analysis or an
    /// earlier import; manual cues are kept.
    pub fn store_imported_cues(&mut self, id: TrackId, cues: &[Cue]) -> Result<()> {
        self.tx(|tx| {
            let mut kept: Vec<Cue> =
                read_cues(tx, id)?.into_iter().filter(|c| c.source == DataSource::Manual).collect();
            for c in cues {
                let taken = kept.iter().any(|k| matches!(k.kind, CueKind::Hot(_)) && k.kind == c.kind);
                if !taken {
                    kept.push(Cue { source: DataSource::Imported, ..c.clone() });
                }
            }
            write_cues(tx, id, &kept)?;
            record_change(tx, "cues", Some(id), None)
        })
    }

    /// Loads the full analysis of a track (empty defaults if not analysed).
    pub fn analysis(&self, id: TrackId) -> Result<TrackAnalysis> {
        let row = self
            .conn
            .query_row(
                "SELECT version, waveform, waveform_rate, phrases, phrase_mood, loudness, peak, energy
                 FROM analysis WHERE track_id = ?1",
                [id.0],
                |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, Option<Vec<u8>>>(1)?,
                        r.get::<_, Option<f64>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<f32>>(5)?,
                        r.get::<_, Option<f32>>(6)?,
                        r.get::<_, Option<Vec<u8>>>(7)?,
                    ))
                },
            )
            .optional()?;
        let mut a = TrackAnalysis::default();
        if let Some((version, wf, rate, phrases, mood, loudness, peak, energy)) = row {
            a.analysis_version = version;
            if let Some(wf) = wf {
                a.waveform = blob::decode_waveform(&wf, rate.unwrap_or(od_core::Waveform::DETAIL_RATE));
            }
            if let Some(p) = phrases {
                // Stored phrases may come from older analyses or a rekordbox
                // import; readers always get DJ-readable sections.
                a.phrases = od_core::normalize_phrases(&serde_json::from_str::<Vec<Phrase>>(&p)?);
            }
            if let Some(m) = mood {
                a.phrase_mood = serde_json::from_str::<PhraseMood>(&m)?;
            }
            a.loudness_lufs = loudness;
            a.peak = peak.unwrap_or(0.0);
            a.energy = energy.map(|e| blob::decode_f32s(&e)).unwrap_or_default();
        }
        a.beatgrid = self.beatgrid(id)?.unwrap_or_default();
        a.key = read_key(&self.conn, id)?.map(|k| k.0);
        Ok(a)
    }

    /// Tracks whose analysis is older than `version` (or missing).
    pub fn tracks_needing_analysis(&self, version: u32) -> Result<Vec<TrackId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM track WHERE analysis_version < ?1 AND missing = 0 ORDER BY id")?;
        Ok(stmt.query_map([version], |r| Ok(TrackId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_missing(&mut self, id: TrackId, missing: bool) -> Result<()> {
        self.conn.execute("UPDATE track SET missing = ?2 WHERE id = ?1", params![id.0, missing])?;
        Ok(())
    }

    /// Points a track at a new file location (relocation, consolidation).
    pub fn set_track_path(&mut self, id: TrackId, path: &std::path::Path) -> Result<()> {
        self.tx(|tx| {
            tx.execute(
                "UPDATE track SET path = ?2, missing = 0 WHERE id = ?1",
                params![id.0, path.to_string_lossy()],
            )?;
            record_change(tx, "path", Some(id), None)
        })
    }

    /// Mean analysed energy (0..=1) of every analysed track. Reads only the
    /// energy curves, not waveforms.
    pub fn energy_means(&self) -> Result<std::collections::HashMap<TrackId, f32>> {
        let mut stmt = self.conn.prepare_cached("SELECT track_id, energy FROM analysis WHERE energy IS NOT NULL")?;
        let mut out = std::collections::HashMap::new();
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))? {
            let (id, bytes) = row?;
            let e = blob::decode_f32s(&bytes);
            if !e.is_empty() {
                out.insert(TrackId(id), e.iter().sum::<f32>() / e.len() as f32);
            }
        }
        Ok(out)
    }

    /// History lists, newest first: (session, first play, number of plays).
    pub fn history_sessions(&self) -> Result<Vec<(String, String, u32)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT session, min(played_at), count(*) FROM history GROUP BY session ORDER BY min(played_at) DESC",
        )?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    /// Plays in play order; one session or (for `None`) the most recent
    /// `limit` plays of all sessions.
    pub fn history(&self, session: Option<&str>, limit: usize) -> Result<Vec<HistoryPlay>> {
        let row = |r: &Row| {
            Ok(HistoryPlay {
                session: r.get(0)?,
                played_at: r.get(1)?,
                track: r.get::<_, Option<i64>>(2)?.map(TrackId),
                source: r.get(3)?,
            })
        };
        let mut out: Vec<HistoryPlay> = match session {
            Some(s) => self
                .conn
                .prepare_cached(
                    "SELECT session, played_at, track_id, source FROM history WHERE session = ?1
                     ORDER BY played_at, id LIMIT ?2",
                )?
                .query_map(params![s, limit as i64], row)?
                .collect::<rusqlite::Result<_>>()?,
            None => {
                let mut v: Vec<HistoryPlay> = self
                    .conn
                    .prepare_cached(
                        "SELECT session, played_at, track_id, source FROM history
                         ORDER BY played_at DESC, id DESC LIMIT ?1",
                    )?
                    .query_map([limit as i64], row)?
                    .collect::<rusqlite::Result<_>>()?;
                v.reverse();
                v
            }
        };
        out.truncate(limit);
        Ok(out)
    }

    /// Records a play from a performance or a device history.
    pub fn record_play(&mut self, id: TrackId, session: &str, source: &str) -> Result<()> {
        self.tx(|tx| {
            tx.execute(
                &format!("UPDATE track SET play_count = play_count + 1, last_played = {NOW} WHERE id = ?1"),
                [id.0],
            )?;
            tx.execute(
                &format!("INSERT INTO history (session, played_at, track_id, source) VALUES (?1, {NOW}, ?2, ?3)"),
                params![session, id.0, source],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;

    #[test]
    fn add_get_search() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "Deep Night", Some(124.0), Some("8A"));
        let b = track(&mut lib, "Sunrise Café", None, None);
        assert_eq!(lib.track_count().unwrap(), 2);
        assert_eq!(lib.track(a).unwrap().key, MusicalKey::parse("8A"));
        assert_eq!(lib.search("deep ni", 10).unwrap()[0].id, a);
        assert_eq!(lib.search("cafe", 10).unwrap()[0].id, b);
        assert!(lib.search("nothing", 10).unwrap().is_empty());
        // Adding the same path again returns the same id.
        let again = lib
            .add_track(&NewTrack {
                path: "/music/Deep Night.mp3".into(),
                content_hash: ContentHash("x".into()),
                file_size: 1,
                format: None,
                duration: 0.0,
                sample_rate: 0,
                channels: 2,
                bit_rate: None,
                bits_per_sample: None,
                tags: TrackTags::default(),
                artwork: None,
            })
            .unwrap();
        assert_eq!(again, a);
    }

    #[test]
    fn tag_edit_updates_search_index() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "Old", None, None);
        let mut tags = lib.track(a).unwrap().tags;
        tags.title = "Brand New".into();
        lib.set_tags(a, tags).unwrap();
        assert!(lib.search("old", 10).unwrap().is_empty());
        assert_eq!(lib.search("brand", 10).unwrap().len(), 1);
    }

    #[test]
    fn analysis_respects_priority() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "T", None, None);
        let mut an = TrackAnalysis {
            analysis_version: 1,
            beatgrid: Beatgrid::constant(128.0, 0.0, 1, 10.0),
            key: MusicalKey::parse("5A"),
            ..Default::default()
        };
        lib.store_analysis(a, &an, DataSource::Imported).unwrap();
        assert_eq!(lib.track(a).unwrap().bpm, Some(128.0));
        // Analysis must not override imported data.
        an.beatgrid = Beatgrid::constant(64.0, 0.0, 1, 10.0);
        an.key = MusicalKey::parse("1B");
        lib.store_analysis(a, &an, DataSource::Analysis).unwrap();
        assert_eq!(lib.track(a).unwrap().bpm, Some(128.0));
        assert_eq!(lib.track(a).unwrap().key, MusicalKey::parse("5A"));
        // Manual edits win.
        lib.set_beatgrid(a, Beatgrid::constant(126.0, 0.0, 1, 10.0)).unwrap();
        assert_eq!(lib.beatgrid_with_source(a).unwrap().unwrap().1, DataSource::Manual);
        assert_eq!(lib.track(a).unwrap().bpm, Some(126.0));
        assert_eq!(lib.analysis(a).unwrap().analysis_version, 1);
        assert!(lib.tracks_needing_analysis(2).unwrap().contains(&a));
    }

    #[test]
    fn imported_cues_keep_manual_hot_cues() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "T", None, None);
        lib.set_cues(a, vec![Cue::hot(0, 1.0)]).unwrap();
        lib.store_imported_cues(a, &[Cue::hot(0, 5.0), Cue::hot(1, 9.0), Cue::memory(2.0)]).unwrap();
        let cues = lib.cues(a).unwrap();
        assert_eq!(cues.len(), 3);
        assert!(cues.iter().any(|c| c.kind == CueKind::Hot(0) && c.start == 1.0));
        assert!(cues.iter().any(|c| c.kind == CueKind::Hot(1) && c.source == DataSource::Imported));
    }
}
