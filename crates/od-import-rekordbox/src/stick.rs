//! Reader for a USB stick exported by rekordbox (`/PIONEER/rekordbox/export.pdb`
//! plus the `USBANLZ` analysis files), via `od-format-pioneer`.

use crate::model::{SmartFields, SrcLibrary, SrcNode, SrcTrack, anlz_family};
use crate::{ImportError, Result};
use od_format_pioneer::pdb::ExportDatabase;
use std::path::{Path, PathBuf};

pub(crate) fn pdb_path(root: &Path) -> PathBuf {
    root.join("PIONEER").join("rekordbox").join("export.pdb")
}

/// Maps a stick-relative path (`/Contents/…`) to the mounted file system.
pub(crate) fn on_stick(root: &Path, device_path: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in device_path.split(['/', '\\']).filter(|s| !s.is_empty() && *s != "." && *s != "..") {
        p.push(part);
    }
    p
}

pub(crate) fn read(root: &Path) -> Result<SrcLibrary> {
    let path = pdb_path(root);
    if !path.is_file() {
        return Err(ImportError::NotFound(path));
    }
    let bytes = std::fs::read(&path)?;
    let db = ExportDatabase::parse(&bytes)?;
    let mut lib = SrcLibrary::default();
    for t in db.tracks {
        lib.tracks.push(SrcTrack {
            src_id: t.id.to_string(),
            path: on_stick(root, &t.device_path),
            tags: t.tags,
            duration: t.duration,
            sample_rate: t.sample_rate,
            bit_rate: Some(t.bit_rate_kbps).filter(|b| *b > 0),
            bits_per_sample: Some(t.bits_per_sample as u32).filter(|b| *b > 0),
            file_size: t.file_size,
            bpm: t.bpm.filter(|b| *b > 0.0),
            key: t.key,
            db_cues: None,
            anlz: if t.anlz_path.is_empty() { Vec::new() } else { anlz_family(on_stick(root, &t.anlz_path)) },
            smart: SmartFields { play_count: t.play_count as i64, ..Default::default() },
        });
    }
    for p in db.playlists {
        lib.playlists.push(SrcNode {
            id: p.id.to_string(),
            parent: (p.parent_id != 0).then(|| p.parent_id.to_string()),
            name: p.name,
            folder: p.is_folder,
            seq: p.sort_order as i64,
            tracks: p.track_ids.iter().map(|i| i.to_string()).collect(),
            was_smart: false,
        });
    }
    for (i, h) in db.history.into_iter().enumerate() {
        lib.history.push(SrcNode {
            id: h.id.to_string(),
            parent: None,
            name: h.name,
            folder: false,
            seq: i as i64,
            tracks: h.track_ids.iter().map(|i| i.to_string()).collect(),
            was_smart: false,
        });
    }
    Ok(lib)
}
