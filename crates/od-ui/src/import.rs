//! One-time rekordbox migration from the UI. Runs in the background on its
//! own library connection; the UI picks up the result through the change log.

use crate::app::App;
use od_import_rekordbox::{ImportOptions, Source};
use slint::ComponentHandle;

pub(crate) fn run_rekordbox_import(app: &App) {
    let source = match od_import_rekordbox::default_rekordbox_dir() {
        Some(dir) => Source::RekordboxLibrary { dir },
        None => {
            let Some(dir) = rfd::FileDialog::new()
                .set_title("Choose your rekordbox folder (with master.db) or a rekordbox USB stick")
                .pick_folder()
            else {
                return;
            };
            if dir.join("PIONEER/rekordbox/export.pdb").exists() {
                Source::RekordboxStick { root: dir }
            } else {
                Source::RekordboxLibrary { dir }
            }
        }
    };
    let library_path = app.state.borrow().config.library_path.clone();
    let wake = app.analysis.wake.clone();
    let ui = app.ui.as_weak();
    let say = move |ui: &slint::Weak<crate::AppWindow>, text: String| {
        let _ = ui.upgrade_in_event_loop(move |ui| ui.set_status_text(text.into()));
    };
    say(&ui, "Reading rekordbox library…".into());
    std::thread::spawn(move || {
        let preview = match od_import_rekordbox::preview(&source) {
            Ok(p) => p,
            Err(e) => return say(&ui, format!("rekordbox import failed: {e}")),
        };
        say(
            &ui,
            format!(
                "Importing {} tracks, {} playlists and {} cues from rekordbox (rekordbox itself is not changed)…",
                preview.tracks, preview.playlists, preview.cues
            ),
        );
        let mut lib = match od_library::Library::open(&library_path) {
            Ok(l) => l,
            Err(e) => return say(&ui, format!("Cannot open library: {e}")),
        };
        let opts = ImportOptions { include_history: true, copy_audio_to: None, analyse_missing: true };
        let mut last = std::time::Instant::now();
        let ui2 = ui.clone();
        let result = od_import_rekordbox::import(&source, &mut lib, &opts, |p| {
            if last.elapsed().as_millis() > 200 {
                last = std::time::Instant::now();
                say(&ui2, format!("rekordbox import: {:?} {}/{}", p.phase, p.done, p.total));
            }
            true
        });
        match result {
            Ok(r) => {
                say(
                    &ui,
                    format!(
                        "rekordbox import done: {} tracks added, {} playlists, {} cues, {} beatgrids{}",
                        r.tracks_added,
                        r.playlists,
                        r.cues,
                        r.grids,
                        if r.missing_files.is_empty() { String::new() } else { format!(", {} files not found", r.missing_files.len()) }
                    ),
                );
                let _ = wake.send(());
            }
            Err(e) => say(&ui, format!("rekordbox import failed: {e}")),
        }
    });
}
