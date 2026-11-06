//! Finding moved or missing files, and consolidating files into one folder.

use crate::{Library, Result, TrackRecord};
use od_core::{ContentHash, TrackId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Result of checking all track paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissingReport {
    pub missing: Vec<TrackId>,
    pub found_again: Vec<TrackId>,
}

/// A missing track and a file that has the same content.
#[derive(Clone, Debug, PartialEq)]
pub struct Relocation {
    pub track: TrackId,
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

impl Library {
    /// Marks tracks whose file no longer exists as missing (and clears the
    /// flag for files that are back).
    pub fn check_missing(&mut self) -> Result<MissingReport> {
        let mut report = MissingReport::default();
        for t in self.tracks(crate::SortKey::DateAdded, false)? {
            let exists = t.path.exists();
            if !exists && !t.missing {
                self.set_missing(t.id, true)?;
                report.missing.push(t.id);
            } else if exists && t.missing {
                self.set_missing(t.id, false)?;
                report.found_again.push(t.id);
            }
        }
        Ok(report)
    }

    /// Searches `roots` for files whose content hash matches missing tracks.
    /// `hash` computes a file's content hash (injected so the library does
    /// not depend on the decoder). Candidates are pre-filtered by file size.
    pub fn find_relocations(
        &self,
        roots: &[PathBuf],
        hash: impl Fn(&Path) -> std::io::Result<ContentHash>,
    ) -> Result<Vec<Relocation>> {
        let missing: Vec<TrackRecord> =
            self.tracks(crate::SortKey::DateAdded, false)?.into_iter().filter(|t| t.missing).collect();
        if missing.is_empty() {
            return Ok(vec![]);
        }
        let mut by_size: HashMap<u64, Vec<&TrackRecord>> = HashMap::new();
        for t in &missing {
            by_size.entry(t.file_size).or_default().push(t);
        }
        let mut out = Vec::new();
        let mut stack: Vec<PathBuf> = roots.to_vec();
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.is_dir() {
                    stack.push(path);
                    continue;
                }
                let Some(candidates) = by_size.get(&meta.len()) else { continue };
                let Ok(h) = hash(&path) else { continue };
                for t in candidates {
                    if t.content_hash == h && !out.iter().any(|r: &Relocation| r.track == t.id) {
                        out.push(Relocation { track: t.id, old_path: t.path.clone(), new_path: path.clone() });
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn apply_relocations(&mut self, relocations: &[Relocation]) -> Result<()> {
        for r in relocations {
            self.set_track_path(r.track, &r.new_path)?;
        }
        Ok(())
    }

    /// Copies every track file into `dest/<Artist>/<file name>` and points
    /// the library at the copies. Originals are left in place.
    pub fn consolidate(&mut self, dest: &Path, mut progress: impl FnMut(usize, usize)) -> Result<usize> {
        let tracks = self.tracks(crate::SortKey::DateAdded, false)?;
        let total = tracks.len();
        let mut moved = 0;
        for (i, t) in tracks.into_iter().enumerate() {
            progress(i, total);
            if t.missing || t.path.starts_with(dest) {
                continue;
            }
            let artist = sanitize(if t.tags.artist.is_empty() { "Unknown Artist" } else { &t.tags.artist });
            let name = t.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let mut target = dest.join(&artist).join(sanitize(&name));
            let mut n = 1;
            while target.exists() {
                let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let ext = Path::new(&name).extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                target = dest.join(&artist).join(sanitize(&format!("{stem} ({n}).{ext}")));
                n += 1;
            }
            std::fs::create_dir_all(target.parent().unwrap_or(dest))?;
            std::fs::copy(&t.path, &target)?;
            self.set_track_path(t.id, &target)?;
            moved += 1;
        }
        progress(total, total);
        Ok(moved)
    }
}

/// Makes a string safe as a file name on all supported file systems
/// (including FAT32/exFAT sticks).
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() { "_".into() } else { trimmed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewTrack;
    use od_core::TrackTags;

    fn file_hash(p: &Path) -> std::io::Result<ContentHash> {
        Ok(ContentHash(format!("{:x}", std::fs::read(p)?.iter().map(|&b| b as u64).sum::<u64>())))
    }

    #[test]
    fn relocate_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old/a.mp3");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, b"abcdef").unwrap();
        let mut lib = Library::open_in_memory().unwrap();
        let id = lib
            .add_track(&NewTrack {
                path: old.clone(),
                content_hash: file_hash(&old).unwrap(),
                file_size: 6,
                format: None,
                duration: 1.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: None,
                bits_per_sample: None,
                tags: TrackTags { artist: "AC/DC".into(), ..Default::default() },
                artwork: None,
            })
            .unwrap();
        let new = dir.path().join("new/sub/renamed.mp3");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        std::fs::write(dir.path().join("new/other.mp3"), b"zzzzzz").unwrap();
        assert_eq!(lib.check_missing().unwrap().missing, vec![id]);
        let rel = lib.find_relocations(&[dir.path().join("new")], file_hash).unwrap();
        assert_eq!(rel.len(), 1);
        assert_eq!(rel[0].new_path, new);
        lib.apply_relocations(&rel).unwrap();
        assert!(!lib.track(id).unwrap().missing);

        let dest = dir.path().join("managed");
        assert_eq!(lib.consolidate(&dest, |_, _| {}).unwrap(), 1);
        assert_eq!(lib.track(id).unwrap().path, dest.join("AC_DC/renamed.mp3"));
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize("  ..."), "_");
    }
}
