//! The OpenDecks Session music library: one SQLite database holding tracks,
//! analysis results, cues, the playlist/set tree, USB devices, recordings
//! and history.
//!
//! User edits go through snapshot-based [`journal`] entries so that every
//! edit can be undone and redone. Every write that changes what ends up on a
//! USB stick appends to the change log, which [`devices`] uses to decide
//! whether a stick is out of date.

mod blob;
pub mod backup;
pub mod devices;
pub mod journal;
pub mod mirror;
pub mod nodes;
pub mod relocate;
pub mod sets;
pub mod smart;
pub mod tracks;

pub use devices::{Device, DeviceFormats, PendingChanges};
pub use journal::{EntityState, JournalEntry};
pub use mirror::MirrorTrack;
pub use nodes::{Entry, Node, NodeId, NodeKind, SetMeta};
pub use smart::{Field, Op, Rule};
pub use tracks::{NewTrack, SortKey, TrackRecord};

use rusqlite::{Connection, OptionalExtension, Transaction};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 1;

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
        let mut lib = Self { conn, path: Some(path.to_path_buf()) };
        lib.configure(portable)?;
        if existed && lib.schema_version()? < SCHEMA_VERSION && lib.schema_version()? > 0 {
            backup::backup_before_migration(&lib)?;
        }
        lib.migrate()?;
        Ok(lib)
    }

    /// Opens a private in-memory library (tests, previews).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut lib = Self { conn, path: None };
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
        let has_meta: bool = self.conn.query_row(
            "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |r| r.get(0),
        )?;
        if !has_meta {
            return Ok(0);
        }
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
            .optional()?;
        Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    fn migrate(&mut self) -> Result<()> {
        let version = self.schema_version()?;
        if version > SCHEMA_VERSION {
            return Err(LibraryError::TooNew(version));
        }
        if version == 0 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Runs `f` inside a transaction.
    pub(crate) fn tx<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Current head of the change log.
    pub fn change_seq(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT coalesce(max(seq), 0) FROM change", [], |r| r.get(0))?)
    }
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
}
