//! Where each track's audio file lives on a stick.
//!
//! Stored in `/OPENDECKS/placement.json` so that paths stay stable across
//! syncs regardless of which export formats are enabled.

use od_core::{ContentHash, TrackId};
use od_library::TrackRecord;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// FAT/exFAT allow 255 UTF-16 units per component; stay well below.
const MAX_COMPONENT: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    /// Stick-relative path starting with `/Contents/`.
    pub path: String,
    pub hash: ContentHash,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementMap {
    pub tracks: BTreeMap<TrackId, Placement>,
}

impl PlacementMap {
    fn file(root: &Path) -> std::path::PathBuf {
        od_format_native::dir(root).join("placement.json")
    }

    pub fn load(root: &Path) -> Self {
        std::fs::read(Self::file(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        od_format_native::write_atomic(&Self::file(root), &bytes)
    }

    pub fn taken_paths(&self) -> HashSet<String> {
        self.tracks
            .values()
            .map(|p| p.path.to_lowercase())
            .collect()
    }
}

fn component(s: &str, fallback: &str) -> String {
    let s = od_library::relocate::sanitize(if s.trim().is_empty() { fallback } else { s });
    if s.chars().count() > MAX_COMPONENT {
        s.chars()
            .take(MAX_COMPONENT)
            .collect::<String>()
            .trim()
            .to_string()
    } else {
        s
    }
}

/// Stick path for a track: `/Contents/<Artist>/<Album>/<file name>`, made
/// unique (case-insensitively, as FAT is) against `taken`.
pub fn contents_path(t: &TrackRecord, taken: &HashSet<String>) -> String {
    let artist = component(&t.tags.artist, "Unknown Artist");
    let album = component(&t.tags.album, "Unknown Album");
    let file = t
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{}", t.id));
    let stem = Path::new(&file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = Path::new(&file)
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    let stem = component(&stem, "track");
    let mut candidate = format!("/Contents/{artist}/{album}/{stem}{ext}");
    let mut n = 2;
    while taken.contains(&candidate.to_lowercase()) {
        candidate = format!("/Contents/{artist}/{album}/{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::TrackTags;

    fn rec(path: &str, artist: &str, album: &str) -> TrackRecord {
        TrackRecord {
            id: TrackId(7),
            path: path.into(),
            content_hash: ContentHash("h".into()),
            file_size: 1,
            format: None,
            duration: 1.0,
            sample_rate: 44_100,
            channels: 2,
            bit_rate: None,
            bits_per_sample: None,
            tags: TrackTags {
                artist: artist.into(),
                album: album.into(),
                ..Default::default()
            },
            date_added: String::new(),
            play_count: 0,
            last_played: None,
            missing: false,
            bpm: None,
            key: None,
            analysis_version: 0,
        }
    }

    #[test]
    fn paths_are_sanitised_and_unique() {
        let t = rec("/music/x/Track: One.mp3", "AC/DC", "");
        let mut taken = HashSet::new();
        let p = contents_path(&t, &taken);
        assert_eq!(p, "/Contents/AC_DC/Unknown Album/Track_ One.mp3");
        taken.insert(p.to_lowercase());
        assert_eq!(
            contents_path(&t, &taken),
            "/Contents/AC_DC/Unknown Album/Track_ One (2).mp3"
        );
    }

    #[test]
    fn map_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let mut m = PlacementMap::default();
        m.tracks.insert(
            TrackId(1),
            Placement {
                path: "/Contents/a.mp3".into(),
                hash: ContentHash("h".into()),
                size: 3,
            },
        );
        m.save(root.path()).unwrap();
        assert_eq!(PlacementMap::load(root.path()), m);
    }
}
