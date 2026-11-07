//! Snapshot-based undo/redo of user edits.
//!
//! An edit is a list of entity states to apply. Before applying, the current
//! state of each entity is captured; undo re-applies the captured states and
//! redo re-applies the edit.

use crate::mytags::{self, MyTag};
use crate::nodes::{self, Entry, Node, NodeId, Subtree};
use crate::tracks::{
    read_cues, read_grid, read_key, read_phrases, read_tags, write_cues, write_grid, write_key, write_phrases,
};
use crate::{Library, NOW, Result, record_change};
use od_core::{Beatgrid, Cue, DataSource, MusicalKey, Phrase, TrackId, TrackTags};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Keep at most this many journal entries.
const JOURNAL_LIMIT: i64 = 1000;

/// The complete state of one editable entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entity", rename_all = "snake_case")]
pub enum EntityState {
    Tags { track: TrackId, tags: TrackTags },
    Cues { track: TrackId, cues: Vec<Cue> },
    Grid { track: TrackId, grid: Option<(Beatgrid, DataSource)> },
    Key { track: TrackId, key: Option<(MusicalKey, DataSource)> },
    /// Attributes of an existing node (name, parent, order, rule, set data).
    Node { node: Node },
    /// The entries of a playlist or set.
    Entries { node: NodeId, entries: Vec<Entry> },
    /// A node with all descendants and their entries; `None` = absent.
    Subtree { root: NodeId, tree: Option<Subtree> },
    /// The My Tag values of a track.
    MyTags { track: TrackId, tags: Vec<MyTag> },
    /// The phrases of a track (phrase edit).
    Phrases { track: TrackId, phrases: Vec<Phrase> },
}

impl EntityState {
    /// Reads the current state of the same entity.
    fn capture(&self, conn: &Connection) -> Result<EntityState> {
        Ok(match self {
            EntityState::Tags { track, .. } => EntityState::Tags { track: *track, tags: read_tags(conn, *track)? },
            EntityState::Cues { track, .. } => EntityState::Cues { track: *track, cues: read_cues(conn, *track)? },
            EntityState::Grid { track, .. } => EntityState::Grid { track: *track, grid: read_grid(conn, *track)? },
            EntityState::Key { track, .. } => EntityState::Key { track: *track, key: read_key(conn, *track)? },
            EntityState::Node { node } => EntityState::Node { node: nodes::read_node(conn, node.id)? },
            EntityState::Entries { node, .. } => {
                EntityState::Entries { node: *node, entries: nodes::read_entries(conn, *node)? }
            }
            EntityState::Subtree { root, .. } => {
                EntityState::Subtree { root: *root, tree: nodes::capture_subtree(conn, *root)? }
            }
            EntityState::MyTags { track, .. } => {
                EntityState::MyTags { track: *track, tags: mytags::read(conn, *track)? }
            }
            EntityState::Phrases { track, .. } => {
                EntityState::Phrases { track: *track, phrases: read_phrases(conn, *track)? }
            }
        })
    }

    fn apply(&self, conn: &Connection) -> Result<()> {
        match self {
            EntityState::Tags { track, tags } => {
                conn.execute(
                    "UPDATE track SET title=?2, artist=?3, album=?4, genre=?5, label=?6, remixer=?7,
                        composer=?8, comment=?9, year=?10, track_number=?11, rating=?12, color=?13
                     WHERE id=?1",
                    params![
                        track.0, tags.title, tags.artist, tags.album, tags.genre, tags.label,
                        tags.remixer, tags.composer, tags.comment, tags.year, tags.track_number,
                        tags.rating.min(5), tags.color.map(|c| c.to_hex())
                    ],
                )?;
                record_change(conn, "tags", Some(*track), None)
            }
            EntityState::Cues { track, cues } => {
                write_cues(conn, *track, cues)?;
                record_change(conn, "cues", Some(*track), None)
            }
            EntityState::Grid { track, grid } => {
                write_grid(conn, *track, grid.as_ref())?;
                record_change(conn, "grid", Some(*track), None)
            }
            EntityState::Key { track, key } => {
                write_key(conn, *track, *key)?;
                record_change(conn, "key", Some(*track), None)
            }
            EntityState::Node { node } => {
                nodes::write_node(conn, node)?;
                record_change(conn, "node", None, Some(node.id))
            }
            EntityState::Entries { node, entries } => {
                nodes::write_entries(conn, *node, entries)?;
                record_change(conn, "entries", None, Some(*node))
            }
            EntityState::Subtree { root, tree } => {
                nodes::delete_subtree(conn, *root)?;
                if let Some(t) = tree {
                    nodes::restore_subtree(conn, t)?;
                }
                record_change(conn, "node", None, Some(*root))
            }
            EntityState::MyTags { track, tags } => {
                mytags::write(conn, *track, tags)?;
                record_change(conn, "tags", Some(*track), None)
            }
            EntityState::Phrases { track, phrases } => {
                write_phrases(conn, *track, phrases)?;
                record_change(conn, "phrases", Some(*track), None)
            }
        }
    }
}

/// One journal entry as shown in the edit history.
#[derive(Clone, Debug, PartialEq)]
pub struct JournalEntry {
    pub id: i64,
    pub label: String,
    pub at: String,
    pub undone: bool,
}

impl Library {
    /// Applies a user edit and records it for undo.
    pub fn edit(&mut self, label: &str, after: Vec<EntityState>) -> Result<()> {
        self.tx(|tx| apply_edit(tx, label, after))
    }

    /// Applies a user edit whose new states are computed from the current
    /// ones, atomically: `f` reads the library and returns the states to
    /// apply plus a result, all under one write lock, so that a concurrent
    /// writer (the app and the MCP server share the file) cannot slip in
    /// between the read and the write. The edit becomes one undo step named
    /// `label`. Nothing is written if `f` fails or returns no states.
    pub fn edit_with<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&Library) -> Result<(Vec<EntityState>, T)>,
    ) -> Result<T> {
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let out = f(self).and_then(|(states, out)| {
            if !states.is_empty() {
                apply_edit(&self.conn, label, states)?;
            }
            Ok(out)
        });
        match out {
            Ok(v) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Undoes the most recent edit. Returns its label, or `None` if there is
    /// nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>> {
        self.tx(|tx| {
            let row: Option<(i64, String, String)> = tx
                .query_row(
                    "SELECT id, label, before FROM journal WHERE undone = 0 ORDER BY id DESC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((id, label, before)) = row else { return Ok(None) };
            let states: Vec<EntityState> = serde_json::from_str(&before)?;
            for s in states.iter().rev() {
                s.apply(tx)?;
            }
            tx.execute("UPDATE journal SET undone = 1 WHERE id = ?1", [id])?;
            Ok(Some(label))
        })
    }

    /// Redoes the most recently undone edit.
    pub fn redo(&mut self) -> Result<Option<String>> {
        self.tx(|tx| {
            let row: Option<(i64, String, String)> = tx
                .query_row(
                    "SELECT id, label, after FROM journal WHERE undone = 1 ORDER BY id ASC LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((id, label, after)) = row else { return Ok(None) };
            let states: Vec<EntityState> = serde_json::from_str(&after)?;
            for s in &states {
                s.apply(tx)?;
            }
            tx.execute("UPDATE journal SET undone = 0 WHERE id = ?1", [id])?;
            Ok(Some(label))
        })
    }

    pub fn can_undo(&self) -> Result<bool> {
        Ok(self.conn.query_row("SELECT count(*) > 0 FROM journal WHERE undone = 0", [], |r| r.get(0))?)
    }

    pub fn can_redo(&self) -> Result<bool> {
        Ok(self.conn.query_row("SELECT count(*) > 0 FROM journal WHERE undone = 1", [], |r| r.get(0))?)
    }

    /// Most recent journal entries, newest first.
    pub fn journal(&self, limit: usize) -> Result<Vec<JournalEntry>> {
        let mut stmt =
            self.conn.prepare("SELECT id, label, at, undone FROM journal ORDER BY id DESC LIMIT ?1")?;
        Ok(stmt
            .query_map([limit as i64], |r| {
                Ok(JournalEntry { id: r.get(0)?, label: r.get(1)?, at: r.get(2)?, undone: r.get(3)? })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
}

/// Applies an edit inside an open transaction.
pub(crate) fn apply_edit(tx: &Connection, label: &str, after: Vec<EntityState>) -> Result<()> {
    let mut before = Vec::with_capacity(after.len());
    for s in &after {
        before.push(s.capture(tx)?);
        s.apply(tx)?;
    }
    // A new edit discards the redo branch.
    tx.execute("DELETE FROM journal WHERE undone = 1", [])?;
    tx.execute(
        &format!("INSERT INTO journal (label, before, after, at) VALUES (?1, ?2, ?3, {NOW})"),
        params![label, serde_json::to_string(&before)?, serde_json::to_string(&after)?],
    )?;
    tx.execute(
        "DELETE FROM journal WHERE id <= (SELECT max(id) FROM journal) - ?1",
        [JOURNAL_LIMIT],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::Library;
    use crate::test_util::track;
    use od_core::{Beatgrid, Cue, DataSource};

    #[test]
    fn undo_redo_cues_and_grid() {
        let mut lib = Library::open_in_memory().unwrap();
        let t = track(&mut lib, "A", None, None);
        lib.set_cues(t, vec![Cue::hot(0, 1.0)]).unwrap();
        lib.set_cues(t, vec![Cue::hot(0, 2.0), Cue::memory(3.0)]).unwrap();
        lib.set_beatgrid(t, Beatgrid::constant(120.0, 0.0, 1, 5.0)).unwrap();
        assert!(lib.can_undo().unwrap());
        assert_eq!(lib.undo().unwrap().as_deref(), Some("Edit beatgrid"));
        assert!(lib.beatgrid(t).unwrap().is_none());
        assert_eq!(lib.undo().unwrap().as_deref(), Some("Edit cues"));
        assert_eq!(lib.cues(t).unwrap(), vec![Cue { source: DataSource::Manual, ..Cue::hot(0, 1.0) }]);
        assert_eq!(lib.redo().unwrap().as_deref(), Some("Edit cues"));
        assert_eq!(lib.cues(t).unwrap().len(), 2);
        // A new edit drops the redo branch.
        lib.set_cues(t, vec![]).unwrap();
        assert!(!lib.can_redo().unwrap());
        assert_eq!(lib.journal(10).unwrap().len(), 3);
    }

    #[test]
    fn undo_on_empty_journal() {
        let mut lib = Library::open_in_memory().unwrap();
        assert_eq!(lib.undo().unwrap(), None);
        assert_eq!(lib.redo().unwrap(), None);
    }
}
