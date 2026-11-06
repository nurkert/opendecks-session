//! The OpenDecks stick format.
//!
//! Layout on the stick:
//!
//! ```text
//! /OPENDECKS/device.json      identity of the stick (written once at registration)
//! /OPENDECKS/library.sqlite   library with the od-library schema; track ids match the
//!                             source library, paths are relative to the stick root
//! /OPENDECKS/sync.json        present only while a sync is running (crash detection)
//! /Contents/...               audio files, shared with the Pioneer export
//! ```

use od_library::Library;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;
pub const DIR: &str = "OPENDECKS";
const MARKER: &str = "device.json";
const LIBRARY: &str = "library.sqlite";
const SYNC_JOURNAL: &str = "sync.json";

#[derive(Debug, thiserror::Error)]
pub enum NativeError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid stick data: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Library(#[from] od_library::LibraryError),
    #[error("stick format version {0} is newer than this application supports")]
    TooNew(u32),
}

pub type Result<T> = std::result::Result<T, NativeError>;

/// Identity of a registered stick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceMarker {
    pub format_version: u32,
    /// Random identifier generated at registration.
    pub device_uuid: String,
    pub name: String,
    pub created_by: String,
}

/// Written at the start of a sync and removed at the end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncJournal {
    pub started_at: String,
    pub full: bool,
}

pub fn dir(root: &Path) -> PathBuf {
    root.join(DIR)
}

pub fn library_path(root: &Path) -> PathBuf {
    dir(root).join(LIBRARY)
}

/// Writes `bytes` to `path` atomically: temporary file, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn read_marker(root: &Path) -> Result<Option<DeviceMarker>> {
    let p = dir(root).join(MARKER);
    if !p.exists() {
        return Ok(None);
    }
    let m: DeviceMarker = serde_json::from_slice(&std::fs::read(p)?)?;
    if m.format_version > FORMAT_VERSION {
        return Err(NativeError::TooNew(m.format_version));
    }
    Ok(Some(m))
}

pub fn write_marker(root: &Path, marker: &DeviceMarker) -> Result<()> {
    write_atomic(&dir(root).join(MARKER), &serde_json::to_vec_pretty(marker)?)?;
    Ok(())
}

pub fn begin_sync(root: &Path, journal: &SyncJournal) -> Result<()> {
    write_atomic(&dir(root).join(SYNC_JOURNAL), &serde_json::to_vec(journal)?)?;
    Ok(())
}

pub fn end_sync(root: &Path) -> Result<()> {
    match std::fs::remove_file(dir(root).join(SYNC_JOURNAL)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// A sync journal left behind by an interrupted sync.
pub fn interrupted_sync(root: &Path) -> Result<Option<SyncJournal>> {
    let p = dir(root).join(SYNC_JOURNAL);
    if !p.exists() {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&std::fs::read(p)?).ok())
}

/// Opens (or creates) the stick library for writing.
pub fn open_library(root: &Path) -> Result<Library> {
    let mut lib = Library::open_portable(&library_path(root))?;
    if lib.meta("stick_format_version")?.is_none() {
        lib.set_meta("stick_format_version", &FORMAT_VERSION.to_string())?;
    }
    Ok(lib)
}

/// A stick library opened for playback (e.g. on the Raspberry Pi).
pub struct Stick {
    pub root: PathBuf,
    pub marker: Option<DeviceMarker>,
    pub library: Library,
}

impl Stick {
    /// Opens the OpenDecks data of a mounted stick, if present.
    pub fn open(root: &Path) -> Result<Option<Stick>> {
        if !library_path(root).exists() {
            return Ok(None);
        }
        let marker = read_marker(root)?;
        let library = Library::open_portable(&library_path(root))?;
        Ok(Some(Stick { root: root.to_path_buf(), marker, library }))
    }

    /// Absolute path of a track file stored with a stick-relative path.
    pub fn resolve(&self, relative: &Path) -> PathBuf {
        let rel = relative.strip_prefix("/").unwrap_or(relative);
        self.root.join(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_journal_and_library() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_marker(root.path()).unwrap().is_none());
        let m = DeviceMarker {
            format_version: FORMAT_VERSION,
            device_uuid: "abc".into(),
            name: "Red".into(),
            created_by: "test".into(),
        };
        write_marker(root.path(), &m).unwrap();
        assert_eq!(read_marker(root.path()).unwrap(), Some(m));

        begin_sync(root.path(), &SyncJournal { started_at: "now".into(), full: false }).unwrap();
        assert!(interrupted_sync(root.path()).unwrap().is_some());
        end_sync(root.path()).unwrap();
        end_sync(root.path()).unwrap();
        assert!(interrupted_sync(root.path()).unwrap().is_none());

        assert!(Stick::open(root.path()).unwrap().is_none());
        drop(open_library(root.path()).unwrap());
        let stick = Stick::open(root.path()).unwrap().unwrap();
        assert_eq!(stick.resolve(Path::new("/Contents/a.mp3")), root.path().join("Contents/a.mp3"));
        // No WAL side files on the stick.
        assert!(!dir(root.path()).join("library.sqlite-wal").exists());
    }

    #[test]
    fn newer_marker_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir(root.path())).unwrap();
        std::fs::write(
            dir(root.path()).join(MARKER),
            r#"{"format_version":99,"device_uuid":"x","name":"n","created_by":"c"}"#,
        )
        .unwrap();
        assert!(matches!(read_marker(root.path()), Err(NativeError::TooNew(99))));
    }
}
