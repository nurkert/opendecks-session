//! One-time rekordbox migration from the UI. Runs in the background on its
//! own library connection; the UI picks up the result through the change log.

use crate::app::App;
use crate::workers::WorkerEvent;
use od_import_rekordbox::{ImportOptions, Source};
use slint::ComponentHandle;
use std::path::Path;

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
        let _ = ui.upgrade_in_event_loop(move |ui| ui.set_status_text(od_i18n::translate_message(&text).into()));
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

/// File menu actions on the whole library: XML import / export
/// (rekordbox-compatible), library archive, consolidate and the rekordbox
/// preferences. Asks for the file or folder, then runs the step.
pub(crate) fn library_action(app: &App, id: &str) {
    let message = match id {
        "library.export.xml" => {
            let Some(f) = rfd::FileDialog::new().set_title(od_i18n::tr("Export library XML")).set_file_name("opendecks.xml").add_filter("XML", &["xml"]).save_file() else { return };
            export_xml(app, &f)
        }
        "library.import.xml" => {
            let Some(f) = rfd::FileDialog::new().set_title(od_i18n::tr("Import library XML")).add_filter("XML", &["xml"]).pick_file() else { return };
            import_xml(app, &f)
        }
        "library.export.archive" => {
            let Some(f) = rfd::FileDialog::new().set_title(od_i18n::tr("Export library archive")).set_file_name("library.odlib").add_filter("OpenDecks library", &["odlib"]).save_file() else { return };
            let audio = rfd::MessageDialog::new()
                .set_title(od_i18n::tr("Export library archive"))
                .set_description(od_i18n::tr("Include the audio files? The archive then holds everything to move the library to another computer."))
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes;
            export_archive(app, &f, audio);
            return;
        }
        "library.restore.archive" => {
            let Some(f) = rfd::FileDialog::new().set_title(od_i18n::tr("Restore library archive")).add_filter("OpenDecks library", &["odlib"]).pick_file() else { return };
            let Some(dir) = rfd::FileDialog::new().set_title(od_i18n::tr("Folder for the restored library")).pick_folder() else { return };
            restore_archive(app, &f, &dir)
        }
        "library.consolidate" => {
            let Some(dir) = rfd::FileDialog::new().set_title(od_i18n::tr("Folder to collect all tracks in")).pick_folder() else { return };
            consolidate(app, &dir);
            return;
        }
        "library.import.rekordbox-settings" => import_rekordbox_settings(app, None),
        _ => return,
    };
    app.status(message);
}

/// Writes the library as rekordbox-compatible XML.
pub(crate) fn export_xml(app: &App, path: &Path) -> String {
    match od_import_rekordbox::xml::export_xml(&app.state.borrow().lib, path) {
        Ok(n) => od_i18n::trf("Exported {} tracks to {}", &[&n, &path.display()]),
        Err(e) => od_i18n::trf("XML export failed: {}", &[&e]),
    }
}

/// Reads a rekordbox-compatible XML file into the library.
pub(crate) fn import_xml(app: &App, path: &Path) -> String {
    if app.state.borrow().read_only() {
        app.state.borrow_mut().browser.stick = None;
    }
    let r = od_import_rekordbox::xml::import_xml(&mut app.state.borrow_mut().lib, path);
    let m = match r {
        Ok(r) => {
            let _ = app.analysis.wake.send(());
            let mut m = od_i18n::trf("Imported {} tracks, {} playlists and {} cues from XML", &[&r.tracks, &r.playlists, &r.cues]);
            if !r.missing.is_empty() {
                m.push_str(&od_i18n::trf(" · {} files not found", &[&r.missing.len()]));
            }
            m
        }
        Err(e) => od_i18n::trf("XML import failed: {}", &[&e]),
    };
    app.refresh_library();
    m
}

/// Writes the library archive in the background (it can take minutes with
/// audio); the result arrives as a status message.
pub(crate) fn export_archive(app: &App, path: &Path, include_audio: bool) {
    let library = app.state.borrow().config.library_path.clone();
    let tx = app.worker_tx.clone();
    let path = path.to_path_buf();
    app.status("Writing the library archive…");
    std::thread::spawn(move || {
        let r = od_library::Library::open(&library).and_then(|lib| lib.export_archive(&path, include_audio, |_, _| {}));
        let m = match r {
            Ok(n) => od_i18n::trf("Library archive saved: {} ({} audio files)", &[&path.display(), &n]),
            Err(e) => od_i18n::trf("Library archive failed: {}", &[&e]),
        };
        let _ = tx.send(WorkerEvent::Message(m));
    });
}

/// Restores an archive into `dir` and opens that library from the next
/// start.
pub(crate) fn restore_archive(app: &App, archive: &Path, dir: &Path) -> String {
    match od_library::Library::import_archive(archive, dir) {
        Ok(lib) => {
            drop(lib);
            let db = dir.join("library.sqlite");
            let mut c = crate::settings::GlobalConfig::load();
            c.library = Some(db.clone());
            match c.save() {
                Ok(()) => od_i18n::trf("Library restored to {} — it opens at the next start", &[&db.display()]),
                Err(e) => format!("{e}"),
            }
        }
        Err(e) => {
            let _ = app;
            od_i18n::trf("Restoring the archive failed: {}", &[&e])
        }
    }
}

/// Copies every track into `dir/<Artist>/` and points the library at the
/// copies, in the background (originals stay).
pub(crate) fn consolidate(app: &App, dir: &Path) {
    let library = app.state.borrow().config.library_path.clone();
    let tx = app.worker_tx.clone();
    let dir = dir.to_path_buf();
    app.status("Consolidating the library…");
    std::thread::spawn(move || {
        let r = od_library::Library::open(&library).and_then(|mut lib| lib.consolidate(&dir, |_, _| {}));
        let m = match r {
            Ok(n) => od_i18n::trf("Consolidated: {} tracks copied to {}", &[&n, &dir.display()]),
            Err(e) => od_i18n::trf("Consolidating failed: {}", &[&e]),
        };
        let _ = tx.send(WorkerEvent::Message(m));
    });
}

/// Takes over the preferences and keyboard shortcuts of a rekordbox
/// installation (`dir`, else the user's rekordbox folder). Shortcuts that
/// would bind one key twice are left out.
pub(crate) fn import_rekordbox_settings(app: &App, dir: Option<&Path>) -> String {
    let (settings, _report) = match od_import_rekordbox::settings::read(dir) {
        Ok(r) => r,
        Err(e) => return od_i18n::trf("No rekordbox preferences found: {}", &[&e]),
    };
    let values = settings.preference_values();
    let applied = app.state.borrow().stored.clone().apply_values(&values).len();
    app.state.borrow_mut().change_settings(|s| {
        s.apply_values(&values);
    });
    let keys = apply_keymap(app, &settings.keymap_toml());
    crate::preferences::apply_all(app);
    od_i18n::trf("Took over {} rekordbox preferences and {} shortcuts", &[&applied, &keys])
}

/// Applies imported bindings one by one, skipping those that would collide
/// with a binding already in use; returns how many were taken.
fn apply_keymap(app: &App, toml_text: &str) -> usize {
    let mut st = app.state.borrow_mut();
    let mut imported = st.keymap.clone();
    if imported.import_toml(toml_text).is_err() {
        return 0;
    }
    let mut taken = 0;
    for a in crate::keymap::registry() {
        let wanted = imported.bindings(&a.id);
        if wanted == st.keymap.bindings(&a.id) {
            continue;
        }
        let mut km = st.keymap.clone();
        km.set(&a.id, wanted);
        if km.all_conflicts().is_empty() {
            st.keymap = km;
            taken += 1;
        }
    }
    if taken > 0 {
        st.save_keymap();
    }
    taken
}
