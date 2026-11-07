//! The OpenDecks Session music library: one SQLite database holding tracks,
//! analysis results, cues, the playlist/set tree, USB devices, recordings
//! and history.
//!
//! User edits go through snapshot-based [`journal`] entries so that every
//! edit can be undone and redone. Every write that changes what ends up on a
//! USB stick appends to the change log, which [`devices`] uses to decide
//! whether a stick is out of date.

mod blob;
pub mod archive;
pub mod backup;
pub mod devices;
pub mod duplicates;
pub mod journal;
pub mod mirror;
pub mod mytags;
pub mod nodes;
pub mod relocate;
pub mod sets;
pub mod smart;
pub mod smart_text;
pub mod tracks;

pub use devices::{Device, DeviceFormats, PendingChanges};
pub use journal::{EntityState, JournalEntry};
pub use mirror::MirrorTrack;
pub use mytags::MyTag;
pub use nodes::{Entry, Node, NodeId, NodeKind, SetMeta, Subtree};
pub use smart::{Field, Op, Rule};
pub use tracks::{HistoryPlay, NewTrack, SortKey, TrackRecord};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::cell::Cell;
use std::path::{Path, PathBuf};

/// Version 2 adds the `my_tag` table.
pub const SCHEMA_VERSION: i64 = 3;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("library schema version {0} is newer than this application supports")]
    TooNew(i64),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid operation: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

pub(crate) const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

/// An open library database.
pub struct Library {
    conn: Connection,
    path: Option<PathBuf>,
    /// `PRAGMA data_version` at the last [`Library::external_change`] check.
    seen_data_version: Cell<i64>,
}

impl Library {
    /// Opens (or creates) the library at `path`, migrating it if needed.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path, false)
    }

    /// Opens a library on removable media: rollback journal instead of WAL
    /// and full syncs, so that pulling the stick never leaves side files
    /// that another machine would need.
    pub fn open_portable(path: &Path) -> Result<Self> {
        Self::open_with(path, true)
    }

    fn open_with(path: &Path, portable: bool) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let existed = path.exists();
        let conn = if portable {
            // FAT/exFAT do not support POSIX byte-range locks; lock with a
            // dot-file instead.
            Connection::open_with_flags_and_vfs(path, rusqlite::OpenFlags::default(), c"unix-dotfile")?
        } else {
            Connection::open(path)?
        };
        let mut lib = Self { conn, path: Some(path.to_path_buf()), seen_data_version: Cell::new(0) };
        lib.configure(portable)?;
        if existed && lib.schema_version()? < SCHEMA_VERSION && lib.schema_version()? > 0 {
            backup::backup_before_migration(&lib)?;
        }
        lib.migrate()?;
        lib.seen_data_version.set(lib.data_version()?);
        Ok(lib)
    }

    /// Opens a private in-memory library (tests, previews).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut lib = Self { conn, path: None, seen_data_version: Cell::new(0) };
        lib.configure(false)?;
        lib.migrate()?;
        Ok(lib)
    }

    /// The default location of the library database for this platform.
    pub fn default_path() -> PathBuf {
        data_dir().join("library.sqlite")
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Portable (removable media): rollback journal and full syncs, never
    /// WAL, whose shared-memory file does not work on FAT/exFAT sticks.
    fn configure(&mut self, portable: bool) -> Result<()> {
        let journal = if portable {
            "PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL;"
        } else {
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;"
        };
        self.conn.execute_batch(journal)?;
        self.conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        Ok(())
    }

    fn schema_version(&self) -> Result<i64> {
        schema_version(&self.conn)
    }

    fn migrate(&mut self) -> Result<()> {
        let version = self.schema_version()?;
        if version > SCHEMA_VERSION {
            return Err(LibraryError::TooNew(version));
        }
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Re-read inside the write lock: another process may have migrated.
        let version = schema_version(&tx)?;
        if version == 0 {
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.execute("INSERT INTO meta (key, value) VALUES ('schema_version', '1')", [])?;
        }
        if version < 2 {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS my_tag (
                    track_id INTEGER NOT NULL REFERENCES track (id) ON DELETE CASCADE,
                    category TEXT    NOT NULL,
                    value    TEXT    NOT NULL,
                    PRIMARY KEY (track_id, category, value)
                 );
                 CREATE INDEX IF NOT EXISTS my_tag_value ON my_tag (category, value);",
            )?;
        }
        // Version 3: active loops (a fresh schema already has the column).
        let has_active: i64 = tx.query_row("SELECT COUNT(*) FROM pragma_table_info('cue') WHERE name = 'active'", [], |r| r.get(0))?;
        if has_active == 0 {
            tx.execute_batch("ALTER TABLE cue ADD COLUMN active INTEGER NOT NULL DEFAULT 0;")?;
        }
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'schema_version'", [SCHEMA_VERSION.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    /// Runs `f` inside a write transaction. The write lock is taken up front
    /// (`BEGIN IMMEDIATE`) so that another process writing to the same file
    /// (the app and the MCP server) waits for the busy timeout instead of
    /// failing on a lock upgrade.
    pub(crate) fn tx<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// `PRAGMA data_version`: changes whenever *another* connection (another
    /// process such as the MCP server, or a second window) commits to the
    /// database. Commits on this connection do not change it.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    /// Cheap poll for writes made by other connections since the previous
    /// call (or since opening). One pragma read, no table access, so it can
    /// run every UI frame or on a short timer; when it returns `true`, reload
    /// whatever views are on screen (tree, track list, undo state).
    pub fn external_change(&self) -> Result<bool> {
        let v = self.data_version()?;
        Ok(self.seen_data_version.replace(v) != v)
    }

    /// Current head of the change log.
    pub fn change_seq(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT coalesce(max(seq), 0) FROM change", [], |r| r.get(0))?)
    }
}

fn schema_version(conn: &Connection) -> Result<i64> {
    let has_meta: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
        [],
        |r| r.get(0),
    )?;
    if !has_meta {
        return Ok(0);
    }
    let v: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
        .optional()?;
    Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
}

pub(crate) fn record_change(
    tx: &Connection,
    kind: &str,
    track: Option<od_core::TrackId>,
    node: Option<NodeId>,
) -> Result<()> {
    tx.execute(
        &format!("INSERT INTO change (kind, track_id, node_id, at) VALUES (?1, ?2, ?3, {NOW})"),
        rusqlite::params![kind, track.map(|t| t.0), node.map(|n| n.0)],
    )?;
    Ok(())
}

/// Platform data directory of OpenDecks Session.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPENDECKS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OpenDecks Session")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("opendecks-session")
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;
    use od_core::{AudioFormat, ContentHash, TrackTags};

    pub fn track(lib: &mut Library, name: &str, bpm: Option<f64>, key: Option<&str>) -> od_core::TrackId {
        let id = lib
            .add_track(&NewTrack {
                path: PathBuf::from(format!("/music/{name}.mp3")),
                content_hash: ContentHash(format!("hash-{name}")),
                file_size: 1000,
                format: Some(AudioFormat::Mp3),
                duration: 300.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: Some(320),
                bits_per_sample: None,
                tags: TrackTags { title: name.into(), artist: "Artist".into(), ..Default::default() },
                artwork: None,
            })
            .unwrap();
        if let Some(bpm) = bpm {
            lib.conn.execute("UPDATE track SET bpm = ?1 WHERE id = ?2", rusqlite::params![bpm, id.0]).unwrap();
        }
        if let Some(k) = key {
            lib.conn.execute("UPDATE track SET musical_key = ?1 WHERE id = ?2", rusqlite::params![k, id.0]).unwrap();
        }
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_and_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("lib/library.sqlite");
        {
            let lib = Library::open(&p).unwrap();
            assert_eq!(lib.schema_version().unwrap(), SCHEMA_VERSION);
        }
        let lib = Library::open(&p).unwrap();
        assert_eq!(lib.change_seq().unwrap(), 0);
    }

    #[test]
    fn migrates_version_1_to_2() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("library.sqlite");
        {
            let conn = Connection::open(&p).unwrap();
            conn.execute_batch(include_str!("schema.sql")).unwrap();
            conn.execute("INSERT INTO meta (key, value) VALUES ('schema_version', '1')", []).unwrap();
        }
        let lib = Library::open(&p).unwrap();
        assert_eq!(lib.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(lib.my_tag_catalog().unwrap().is_empty());
    }

    #[test]
    fn migrates_version_2_to_3_active_loops() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("library.sqlite");
        {
            let conn = Connection::open(&p).unwrap();
            // A version 2 cue table has no `active` column.
            let old = include_str!("schema.sql").replace(",\n    active   INTEGER NOT NULL DEFAULT 0   -- active loop (schema 3)", "");
            assert!(!old.contains("active   INTEGER"));
            conn.execute_batch(&old).unwrap();
            conn.execute("INSERT INTO meta (key, value) VALUES ('schema_version', '2')", []).unwrap();
        }
        let mut lib = Library::open(&p).unwrap();
        assert_eq!(lib.schema_version().unwrap(), 3);
        let t = crate::test_util::track(&mut lib, "Looped", Some(120.0), None);
        let looped = od_core::Cue { loop_end: Some(12.0), active: true, ..od_core::Cue::memory(8.0) };
        let plain = od_core::Cue { active: true, ..od_core::Cue::memory(20.0) };
        lib.set_cues(t, vec![looped, plain]).unwrap();
        let back = lib.cues(t).unwrap();
        assert!(back.iter().any(|c| c.start == 8.0 && c.active), "an active loop keeps its flag");
        assert!(back.iter().any(|c| c.start == 20.0 && !c.active), "only loops can be active");
    }

    #[test]
    fn external_writes_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("library.sqlite");
        let app = Library::open(&p).unwrap();
        let mut other = Library::open(&p).unwrap();
        assert!(!app.external_change().unwrap());
        // Own writes do not count.
        let _ = other.external_change().unwrap();
        other.create_node(None, NodeKind::Playlist, "From MCP").unwrap();
        assert!(!other.external_change().unwrap());
        assert!(app.external_change().unwrap());
        assert!(!app.external_change().unwrap());
        assert_eq!(app.all_nodes().unwrap().len(), 1);
    }

    #[test]
    fn edit_with_is_atomic_and_one_step() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = test_util::track(&mut lib, "A", None, None);
        let id = lib
            .edit_with("MCP: Create set", |l| {
                let node = l.new_node(None, NodeKind::Set, "Friday")?;
                let id = node.id;
                let tree = Subtree { nodes: vec![node], entries: vec![(id, vec![Entry::new(a)])], device_links: vec![] };
                Ok((vec![EntityState::Subtree { root: id, tree: Some(tree) }], id))
            })
            .unwrap();
        assert_eq!(lib.entries(id).unwrap().len(), 1);
        assert_eq!(lib.journal(1).unwrap()[0].label, "MCP: Create set");
        let failed: Result<()> = lib.edit_with("MCP: broken", |_| Err(LibraryError::Invalid("no".into())));
        assert!(failed.is_err());
        assert_eq!(lib.undo().unwrap().as_deref(), Some("MCP: Create set"));
        assert!(lib.node(id).is_err());
    }
}
