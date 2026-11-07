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

// ── Files dropped from Finder / the file manager ────────────────────────

/// The audio files among dropped paths: files with a supported extension
/// and every audio file below a dropped folder, in drop order, each once.
pub(crate) fn dropped_audio(paths: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for p in paths {
        let found = if p.is_dir() {
            crate::workers::scan(p)
        } else if p.is_file() && od_core::AudioFormat::is_supported_extension(p) {
            vec![p.clone()]
        } else {
            Vec::new()
        };
        for f in found {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// The list dropped files also go into: the open playlist or set of the
/// library (not a stick, the Explorer or a virtual list).
pub(crate) fn drop_list(app: &App) -> Option<od_library::NodeId> {
    let st = app.state.borrow();
    if st.read_only() || st.explorer.open.is_some() || st.browser.special.is_some() {
        return None;
    }
    st.browser.node.filter(|n| st.lib.node(*n).is_ok_and(|n| matches!(n.kind, od_library::NodeKind::Playlist | od_library::NodeKind::Set)))
}

/// Files dropped on the window: onto a deck (`deck` 1-based) the first file
/// is added to the collection and loaded there; anywhere else every audio
/// file (folders included) is added to the collection and to the open
/// playlist or set, in the background.
pub(crate) fn files_dropped(app: &App, paths: Vec<std::path::PathBuf>, deck: i32) {
    let files = dropped_audio(&paths);
    if files.is_empty() {
        app.status("No audio files in what was dropped");
        return;
    }
    if deck >= 1 {
        let d = (deck as usize - 1).min(crate::decks::DECKS - 1);
        crate::explorer::import_and_load(app, files[0].clone(), d);
        return;
    }
    let list = drop_list(app);
    let library = app.state.borrow().config.library_path.clone();
    let (tx, wake) = (app.worker_tx.clone(), app.analysis.wake.clone());
    app.status(od_i18n::trf("Adding {} dropped file(s)…", &[&files.len()]));
    std::thread::spawn(move || {
        let (added, failed) = match od_library::Library::open(&library) {
            Ok(mut lib) => add_files(&mut lib, &files, list),
            Err(e) => {
                let _ = tx.send(WorkerEvent::Message(format!("Cannot open library: {e}")));
                return;
            }
        };
        let _ = tx.send(WorkerEvent::ImportFinished { added, failed });
        let _ = wake.send(());
    });
}

/// Adds `files` to the collection (known paths are kept) and, when `list`
/// is given, appends them to that list in order. Returns (added, failed).
pub(crate) fn add_files(lib: &mut od_library::Library, files: &[std::path::PathBuf], list: Option<od_library::NodeId>) -> (usize, usize) {
    let (mut added, mut failed) = (0, 0);
    let mut ids = Vec::new();
    for f in files {
        let known = lib.track_by_path(f).ok().flatten().map(|t| t.id);
        match known.map(Ok).unwrap_or_else(|| crate::workers::import_file(lib, f).inspect(|_| added += 1)) {
            Ok(id) => ids.push(id),
            Err(e) => {
                failed += 1;
                tracing::warn!(path = %f.display(), error = %e, "dropped file not added");
            }
        }
    }
    if let (Some(n), false) = (list, ids.is_empty()) {
        if let Err(e) = lib.add_to_node(n, &ids, None) {
            tracing::warn!(error = %e, "dropped files not added to the list");
        }
    }
    (added, failed)
}

/// Native file drops (winit: Finder / the file manager). The OS reports one
/// event per file; they are collected and handled together. Platforms whose
/// Slint backend delivers OS drops to `DropArea` use that path instead.
pub(crate) fn wire_native_drop(app: &std::rc::Rc<App>) {
    use slint::winit_030::{WinitWindowAccessor, winit::event::WindowEvent};
    let pending: std::rc::Rc<std::cell::RefCell<Vec<std::path::PathBuf>>> = Default::default();
    let weak = std::rc::Rc::downgrade(app);
    let timer = std::rc::Rc::new(slint::Timer::default());
    app.ui.window().on_winit_window_event(move |_, event| {
        match event {
            WindowEvent::HoveredFile(_) => {
                if let Some(a) = weak.upgrade() {
                    a.ui.set_file_drag(true);
                }
            }
            WindowEvent::HoveredFileCancelled => {
                if let Some(a) = weak.upgrade() {
                    a.ui.set_file_drag(false);
                }
            }
            WindowEvent::DroppedFile(path) => {
                pending.borrow_mut().push(path.clone());
                let (weak, pending) = (weak.clone(), pending.clone());
                timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_millis(120), move || {
                    let paths = std::mem::take(&mut *pending.borrow_mut());
                    if let Some(a) = weak.upgrade() {
                        a.ui.set_file_drag(false);
                        files_dropped(&a, paths, 0);
                    }
                });
            }
            _ => {}
        }
        slint::winit_030::EventResult::Propagate
    });
}
