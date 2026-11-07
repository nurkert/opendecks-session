//! Cover art for tracks that have none yet, found in the background: the
//! picture embedded in the file, a cover image in the track's folder
//! (`cover.jpg`, `folder.jpg`, …) or the cover rekordbox keeps for it. Runs
//! once per session after start-up (and again after an import) on its own
//! library connection; every cover found is stored and shown in the list at
//! once.

use crate::app::App;
use od_core::TrackId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Base names (lower case, without extension) of folder cover images.
const FOLDER_IMAGES: [&str; 6] = ["cover", "folder", "front", "albumart", "album", "artwork"];

/// A cover image next to the audio file, if the folder has one.
pub(crate) fn folder_image(audio: &Path) -> Option<(Vec<u8>, String)> {
    let dir = audio.parent()?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            let stem = p.file_stem().and_then(|s| s.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
            matches!(ext.as_deref(), Some("jpg" | "jpeg" | "png")) && FOLDER_IMAGES.iter().any(|n| stem == *n || stem.starts_with("albumart"))
        })
        .collect();
    found.sort_by_key(|p| FOLDER_IMAGES.iter().position(|n| p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(n))).unwrap_or(99));
    let path = found.into_iter().next()?;
    let data = std::fs::read(&path).ok()?;
    let png = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
    Some((data, if png { "image/png" } else { "image/jpeg" }.to_string()))
}

/// The cover of one file: embedded picture first, then the folder's image,
/// then the one rekordbox keeps (`rekordbox`: audio path → cover).
pub(crate) fn find_cover(audio: &Path, rekordbox: &HashMap<PathBuf, (Vec<u8>, String)>) -> Option<(Vec<u8>, String)> {
    od_decode::probe(audio)
        .ok()
        .and_then(|p| p.artwork)
        .or_else(|| folder_image(audio))
        .or_else(|| rekordbox.get(audio).cloned())
}

/// Starts the search for the tracks of the library that have no cover.
pub(crate) fn spawn_backfill(app: &App) {
    let st = app.state.borrow();
    let Some(path) = st.lib.path().map(Path::to_path_buf) else { return };
    let tracks: Vec<(TrackId, PathBuf)> = st.lib.tracks_without_artwork().unwrap_or_default();
    drop(st);
    if tracks.is_empty() {
        return;
    }
    let tx = crate::browser::art_sender(app);
    let size = crate::browser::art_px(app);
    // The app's own rekordbox library, when there is one (the tests point
    // it at nothing).
    let rb_dir = if app.state.borrow().config.headless { None } else { od_import_rekordbox::default_rekordbox_dir() };
    std::thread::Builder::new()
        .name("od-covers".into())
        .spawn(move || {
            let rekordbox: HashMap<PathBuf, (Vec<u8>, String)> = rb_dir
                .filter(|d| d.join("master.db").is_file())
                .and_then(|dir| od_import_rekordbox::source_covers(&od_import_rekordbox::Source::RekordboxLibrary { dir }).ok())
                .map(|v| v.into_iter().map(|(p, d, m)| (p, (d, m))).collect())
                .unwrap_or_default();
            let Ok(mut lib) = od_library::Library::open(&path) else { return };
            let mut found = 0usize;
            for (id, audio) in tracks {
                let Some((data, media)) = find_cover(&audio, &rekordbox) else { continue };
                if lib.set_artwork_if_missing(id, &data, &media).unwrap_or(false) {
                    found += 1;
                    let px = crate::render::artwork_pixels(&data, size);
                    if tx.send(vec![((false, id), px)]).is_err() {
                        return;
                    }
                }
            }
            tracing::info!(found, "cover art filled in");
        })
        .ok();
}
