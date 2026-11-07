//! Duplicate track check: the same file content added twice, or the same
//! artist and title at (almost) the same length — e.g. a WAV and an MP3 of
//! one release.

use crate::{Library, Result, TrackRecord};
use od_core::TrackId;
use std::collections::BTreeMap;

/// Why tracks are grouped as duplicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateKind {
    /// Byte-identical audio (same content hash).
    SameFile,
    /// Same artist and title, lengths within two seconds.
    SameRelease,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateGroup {
    pub kind: DuplicateKind,
    /// Oldest first (the one to keep, usually).
    pub tracks: Vec<TrackId>,
}

/// Artist + title folded for comparison: case, accents of common Latin
/// letters, punctuation and spacing don't matter.
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'Á' | 'À' | 'Â' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ß' => 's',
            c => c.to_ascii_lowercase(),
        })
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Groups of duplicate tracks, given all tracks (pure, for tests).
pub fn group_duplicates(tracks: &[TrackRecord]) -> Vec<DuplicateGroup> {
    let mut by_hash: BTreeMap<&str, Vec<&TrackRecord>> = BTreeMap::new();
    for t in tracks {
        by_hash
            .entry(t.content_hash.0.as_str())
            .or_default()
            .push(t);
    }
    let mut out = Vec::new();
    let mut in_file_group = std::collections::HashSet::new();
    for group in by_hash.into_values().filter(|g| g.len() > 1) {
        let mut ids: Vec<TrackId> = group.iter().map(|t| t.id).collect();
        ids.sort();
        in_file_group.extend(ids.iter().copied());
        out.push(DuplicateGroup {
            kind: DuplicateKind::SameFile,
            tracks: ids,
        });
    }
    let mut by_name: BTreeMap<String, Vec<&TrackRecord>> = BTreeMap::new();
    for t in tracks.iter().filter(|t| !t.tags.title.trim().is_empty()) {
        by_name
            .entry(format!(
                "{}\u{0}{}",
                fold(&t.tags.artist),
                fold(&t.tags.title)
            ))
            .or_default()
            .push(t);
    }
    for mut group in by_name.into_values().filter(|g| g.len() > 1) {
        group.sort_by(|a, b| a.duration.total_cmp(&b.duration));
        // Split into runs whose neighbours are within two seconds.
        let mut run: Vec<&TrackRecord> = Vec::new();
        let mut flush = |run: &mut Vec<&TrackRecord>| {
            let mut ids: Vec<TrackId> = run.iter().map(|t| t.id).collect();
            ids.sort();
            // Skip runs that are entirely one same-file group already.
            if ids.len() > 1 && !ids.iter().all(|i| in_file_group.contains(i)) {
                out.push(DuplicateGroup {
                    kind: DuplicateKind::SameRelease,
                    tracks: ids,
                });
            }
            run.clear();
        };
        for t in group {
            if run
                .last()
                .is_some_and(|l: &&TrackRecord| t.duration - l.duration > 2.0)
            {
                flush(&mut run);
            }
            run.push(t);
        }
        flush(&mut run);
    }
    out
}

impl Library {
    /// Duplicate groups in the whole collection.
    pub fn find_duplicates(&self) -> Result<Vec<DuplicateGroup>> {
        Ok(group_duplicates(
            &self.tracks(crate::SortKey::DateAdded, false)?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{ContentHash, TrackTags};

    fn t(id: i64, hash: &str, artist: &str, title: &str, duration: f64) -> TrackRecord {
        TrackRecord {
            id: TrackId(id),
            content_hash: ContentHash(hash.into()),
            duration,
            tags: TrackTags {
                artist: artist.into(),
                title: title.into(),
                ..Default::default()
            },
            ..TrackRecord::test_default()
        }
    }

    #[test]
    fn finds_same_file_and_same_release() {
        let tracks = vec![
            t(1, "aaa", "Frink", "Summa (Original Mix)", 407.0),
            t(2, "aaa", "Frink", "Summa (Original Mix)", 407.0),
            t(3, "bbb", "Rüfüs Du Sol", "Innerbloom", 577.2),
            t(4, "ccc", "RUFUS DU SOL", "Innerbloom!", 578.5),
            t(5, "ddd", "Rufus du Sol", "Innerbloom", 300.0), // a radio edit: different length
            t(6, "eee", "Other", "Track", 200.0),
        ];
        let g = group_duplicates(&tracks);
        assert_eq!(g.len(), 2, "{g:?}");
        assert_eq!(
            g[0],
            DuplicateGroup {
                kind: DuplicateKind::SameFile,
                tracks: vec![TrackId(1), TrackId(2)]
            }
        );
        assert_eq!(
            g[1],
            DuplicateGroup {
                kind: DuplicateKind::SameRelease,
                tracks: vec![TrackId(3), TrackId(4)]
            }
        );
    }
}
