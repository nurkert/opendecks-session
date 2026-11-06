//! Writing a mirror of (part of) a library into another library database.
//!
//! Used for the OpenDecks stick format: the stick holds a library with the
//! same schema whose track ids match the source library and whose paths are
//! relative to the stick root. Mirror writes bypass the undo journal.

use crate::nodes::{Entry, Node, write_entries};
use crate::tracks::{TrackRecord, write_cues, write_grid, write_key};
use crate::{Library, NOW, Result, blob};
use od_core::{Cue, DataSource, TrackAnalysis, TrackId};
use rusqlite::params;
use std::collections::HashSet;

/// Everything stored about one track.
#[derive(Clone, Debug)]
pub struct MirrorTrack {
    pub record: TrackRecord,
    pub analysis: TrackAnalysis,
    pub grid_source: DataSource,
    pub cues: Vec<Cue>,
    pub artwork: Option<(Vec<u8>, String)>,
}

impl Library {
    /// Collects the mirror data of a track from this library.
    pub fn mirror_track_data(&self, id: TrackId) -> Result<MirrorTrack> {
        let grid_source = self.beatgrid_with_source(id)?.map(|g| g.1).unwrap_or(DataSource::Analysis);
        Ok(MirrorTrack {
            record: self.track(id)?,
            analysis: self.analysis(id)?,
            grid_source,
            cues: self.cues(id)?,
            artwork: self.artwork(id)?,
        })
    }

    /// Inserts or replaces a track with the given id and data.
    pub fn mirror_put_track(&mut self, t: &MirrorTrack) -> Result<()> {
        let r = &t.record;
        self.tx(|tx| {
            tx.execute(
                "INSERT INTO track (id, path, content_hash, file_size, format, duration, sample_rate, channels,
                    bit_rate, bits_per_sample, title, artist, album, genre, label, remixer, composer, comment,
                    year, track_number, rating, color, date_added, play_count, last_played, missing)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                    ?19, ?20, ?21, ?22, ?23, ?24, ?25, 0)
                 ON CONFLICT (id) DO UPDATE SET path = excluded.path, content_hash = excluded.content_hash,
                    file_size = excluded.file_size, format = excluded.format, duration = excluded.duration,
                    sample_rate = excluded.sample_rate, channels = excluded.channels, bit_rate = excluded.bit_rate,
                    bits_per_sample = excluded.bits_per_sample, title = excluded.title, artist = excluded.artist,
                    album = excluded.album, genre = excluded.genre, label = excluded.label,
                    remixer = excluded.remixer, composer = excluded.composer, comment = excluded.comment,
                    year = excluded.year, track_number = excluded.track_number, rating = excluded.rating,
                    color = excluded.color, date_added = excluded.date_added, play_count = excluded.play_count,
                    last_played = excluded.last_played, missing = 0",
                params![
                    r.id.0,
                    r.path.to_string_lossy(),
                    r.content_hash.0,
                    r.file_size as i64,
                    r.format.map(|f| f.as_str()),
                    r.duration,
                    r.sample_rate,
                    r.channels,
                    r.bit_rate,
                    r.bits_per_sample,
                    r.tags.title,
                    r.tags.artist,
                    r.tags.album,
                    r.tags.genre,
                    r.tags.label,
                    r.tags.remixer,
                    r.tags.composer,
                    r.tags.comment,
                    r.tags.year,
                    r.tags.track_number,
                    r.tags.rating,
                    r.tags.color.map(|c| c.to_hex()),
                    r.date_added,
                    r.play_count,
                    r.last_played
                ],
            )?;
            let a = &t.analysis;
            tx.execute("DELETE FROM analysis WHERE track_id = ?1", [r.id.0])?;
            tx.execute(
                "INSERT INTO analysis (track_id, version, waveform, waveform_rate, phrases, phrase_mood,
                    loudness, peak, energy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    r.id.0,
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
            let grid = (!a.beatgrid.is_empty()).then(|| (a.beatgrid.clone(), t.grid_source));
            write_grid(tx, r.id, grid.as_ref())?;
            write_key(tx, r.id, a.key.map(|k| (k, DataSource::Analysis)))?;
            tx.execute(
                "UPDATE track SET analysis_version = ?2 WHERE id = ?1",
                params![r.id.0, a.analysis_version],
            )?;
            write_cues(tx, r.id, &t.cues)?;
            tx.execute("DELETE FROM artwork WHERE track_id = ?1", [r.id.0])?;
            if let Some((data, media)) = &t.artwork {
                tx.execute(
                    "INSERT INTO artwork (track_id, media_type, data) VALUES (?1, ?2, ?3)",
                    params![r.id.0, media, data],
                )?;
            }
            Ok(())
        })
    }

    /// Removes all tracks whose id is not in `keep`. Returns removed records.
    pub fn mirror_retain_tracks(&mut self, keep: &HashSet<TrackId>) -> Result<Vec<TrackRecord>> {
        let removed: Vec<TrackRecord> = self
            .tracks(crate::SortKey::DateAdded, false)?
            .into_iter()
            .filter(|t| !keep.contains(&t.id))
            .collect();
        self.tx(|tx| {
            for t in &removed {
                tx.execute("DELETE FROM track WHERE id = ?1", [t.id.0])?;
            }
            Ok(())
        })?;
        Ok(removed)
    }

    /// Replaces the whole node tree. Parents must precede children.
    pub fn mirror_replace_tree(&mut self, nodes: &[(Node, Vec<Entry>)]) -> Result<()> {
        self.tx(|tx| {
            tx.execute("DELETE FROM node", [])?;
            for (n, entries) in nodes {
                tx.execute(
                    "INSERT INTO node (id, parent_id, kind, name, sort_order, smart_rule, set_meta, created)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        n.id.0,
                        n.parent.map(|p| p.0),
                        n.kind.as_str(),
                        n.name,
                        n.sort_order,
                        n.smart_rule.as_ref().map(serde_json::to_string).transpose()?,
                        n.set_meta.as_ref().map(serde_json::to_string).transpose()?,
                        n.created
                    ],
                )?;
                write_entries(tx, n.id, entries)?;
            }
            Ok(())
        })
    }

    /// Reads a value from the `meta` table.
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Timestamp in the library's format.
    pub fn now(&self) -> Result<String> {
        Ok(self.conn.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;
    use crate::test_util::track;
    use od_core::{Beatgrid, Cue};

    #[test]
    fn mirror_roundtrip() {
        let mut src = Library::open_in_memory().unwrap();
        let a = track(&mut src, "A", None, None);
        let b = track(&mut src, "B", None, None);
        src.set_cues(a, vec![Cue::hot(2, 3.5)]).unwrap();
        src.set_beatgrid(a, Beatgrid::constant(125.0, 0.2, 1, 30.0)).unwrap();
        let pl = src.create_node(None, NodeKind::Playlist, "P").unwrap();
        src.add_to_node(pl, &[b, a], None).unwrap();

        let mut dst = Library::open_in_memory().unwrap();
        for id in [a, b] {
            let mut t = src.mirror_track_data(id).unwrap();
            t.record.path = format!("Contents/{id}.mp3").into();
            dst.mirror_put_track(&t).unwrap();
        }
        dst.mirror_replace_tree(&[(src.node(pl).unwrap(), src.entries(pl).unwrap())]).unwrap();
        assert_eq!(dst.cues(a).unwrap(), src.cues(a).unwrap());
        assert_eq!(dst.beatgrid_with_source(a).unwrap(), src.beatgrid_with_source(a).unwrap());
        assert_eq!(dst.track(a).unwrap().bpm, Some(125.0));
        assert_eq!(dst.node_tracks(pl).unwrap().iter().map(|t| t.id).collect::<Vec<_>>(), vec![b, a]);
        // Updating keeps one row.
        let t = src.mirror_track_data(a).unwrap();
        dst.mirror_put_track(&t).unwrap();
        assert_eq!(dst.track_count().unwrap(), 2);
        let removed = dst.mirror_retain_tracks(&[a].into_iter().collect()).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(dst.node_tracks(pl).unwrap().len(), 1);
    }
}
