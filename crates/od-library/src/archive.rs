//! The whole library as one archive file (`.odlib`, a zstd-compressed tar):
//! the database with every analysis, cue, playlist and set, and optionally
//! the audio files. Restoring rewrites track paths to the restored audio,
//! so a library moves to another computer in one step.

use crate::{Library, LibraryError, Result};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

const DB_NAME: &str = "library.sqlite";
const MANIFEST: &str = "manifest.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct Manifest {
    format: u32,
    /// Track id → audio path inside the archive.
    audio: HashMap<i64, String>,
}

fn io(e: std::io::Error) -> LibraryError {
    LibraryError::Io(e)
}

impl Library {
    /// Writes the archive to `dest`. With `include_audio`, every existing
    /// audio file goes in under `audio/<id>.<ext>`.
    pub fn export_archive(
        &self,
        dest: &Path,
        include_audio: bool,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<usize> {
        // A consistent copy of the database next to the archive, removed after.
        let db = dest.with_extension("odlib-db.tmp");
        let _ = std::fs::remove_file(&db);
        self.backup_to(&db)?;
        let result = self.write_archive(dest, &db, include_audio, &mut progress);
        let _ = std::fs::remove_file(&db);
        result
    }

    fn write_archive(
        &self,
        dest: &Path,
        db: &Path,
        include_audio: bool,
        progress: &mut impl FnMut(usize, usize),
    ) -> Result<usize> {
        let enc = zstd::Encoder::new(File::create(dest).map_err(io)?, 3)
            .map_err(io)?
            .auto_finish();
        let mut tar = tar::Builder::new(enc);
        tar.append_path_with_name(db, DB_NAME).map_err(io)?;
        let mut manifest = Manifest {
            format: 1,
            audio: HashMap::new(),
        };
        if include_audio {
            let tracks = self.tracks(crate::SortKey::DateAdded, false)?;
            let total = tracks.len();
            for (i, t) in tracks.iter().enumerate() {
                progress(i, total);
                if !t.path.is_file() {
                    continue;
                }
                let ext = t
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let name = format!("audio/{}.{ext}", t.id.0);
                tar.append_path_with_name(&t.path, &name).map_err(io)?;
                manifest.audio.insert(t.id.0, name);
            }
        }
        let json =
            serde_json::to_vec_pretty(&manifest).map_err(|e| io(std::io::Error::other(e)))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, MANIFEST, json.as_slice())
            .map_err(io)?;
        tar.finish().map_err(io)?;
        Ok(manifest.audio.len())
    }

    /// Restores an archive into `dir`: `dir/library.sqlite` plus `dir/audio`
    /// when the archive carries audio (track paths then point there).
    /// Returns the opened library.
    pub fn import_archive(archive: &Path, dir: &Path) -> Result<Library> {
        std::fs::create_dir_all(dir).map_err(io)?;
        let dec = zstd::Decoder::new(File::open(archive).map_err(io)?).map_err(io)?;
        let mut tar = tar::Archive::new(dec);
        for entry in tar.entries().map_err(io)? {
            let mut entry = entry.map_err(io)?;
            let path = entry.path().map_err(io)?.into_owned();
            // Only the names this format writes; never follow absolute or ../ paths.
            let ok = path == Path::new(DB_NAME)
                || path == Path::new(MANIFEST)
                || (path.starts_with("audio") && path.components().count() == 2);
            if ok {
                let target = dir.join(&path);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                entry.unpack(target).map_err(io)?;
            }
        }
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(dir.join(MANIFEST)).map_err(io)?)
                .map_err(|e| io(std::io::Error::other(e)))?;
        let mut lib = Library::open(&dir.join(DB_NAME))?;
        for (id, rel) in &manifest.audio {
            let p: PathBuf = dir.join(rel);
            lib.set_track_path(od_core::TrackId(*id), &p)?;
        }
        let _ = std::fs::remove_file(dir.join(MANIFEST));
        Ok(lib)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::track;
    use crate::{Library, NodeKind};

    #[test]
    fn archive_round_trip_with_audio() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::open(&dir.path().join("a/library.sqlite")).unwrap();
        let t = track(&mut lib, "Summa", Some(123.0), Some("8A"));
        let audio = dir.path().join("Summa.mp3");
        std::fs::write(&audio, b"audio bytes").unwrap();
        lib.set_track_path(t, &audio).unwrap();
        let pl = lib.create_node(None, NodeKind::Playlist, "Friday").unwrap();
        lib.add_to_node(pl, &[t], None).unwrap();

        let archive = dir.path().join("lib.odlib");
        assert_eq!(lib.export_archive(&archive, true, |_, _| {}).unwrap(), 1);
        let restored = Library::import_archive(&archive, &dir.path().join("restored")).unwrap();
        let back = restored.track(t).unwrap();
        assert_eq!(back.tags.title, "Summa");
        assert_eq!(back.bpm, Some(123.0));
        assert!(back.path.starts_with(dir.path().join("restored/audio")));
        assert_eq!(std::fs::read(&back.path).unwrap(), b"audio bytes");
        assert_eq!(restored.node_tracks(pl).unwrap().len(), 1);
    }
}
