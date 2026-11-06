//! Preferences: the settings window (SPEC §12.2) — Audio, View, Analysis,
//! Controller, Keyboard, Recording, Advanced. Opens with ⌘, / Ctrl+, (the
//! platform menu and the keymap) and from the gear in the top bar; Esc or
//! ⌘W / Ctrl+W closes it. Settings are stored in the library (see
//! [`crate::settings`]) and applied live where possible.

use crate::app::{App, handler};
use crate::keymap::{self, Chord, Group, Layout, Platform};
use crate::models::model;
use crate::settings::{BPM_RANGES, BUFFER_SIZES, GlobalConfig, SAMPLE_RATES, SCALES, Settings};
use crate::workers::AnalysisControl;
use crate::{KeyRow, PreferencesWindow, shell};
use slint::{ComponentHandle, SharedString};
use std::path::PathBuf;
use std::rc::Rc;

/// Runtime state of the window.
pub(crate) struct Prefs {
    pub window: Option<PreferencesWindow>,
    pub layout: Layout,
    /// Settings when the app started (for "applies after restart").
    pub started: Option<Settings>,
    /// The action whose binding is being recorded.
    pub recording: Option<String>,
    /// A recorded binding that conflicts, waiting for REPLACE.
    pub pending: Option<(String, Chord)>,
    pub search: String,
    /// The action selected with the arrow keys on the Keyboard page.
    pub selected: Option<String>,
    pub message: String,
    /// Output devices offered on the Audio page (names).
    pub devices: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            window: None,
            layout: Layout::detect(),
            started: None,
            recording: None,
            pending: None,
            search: String::new(),
            selected: None,
            message: String::new(),
            devices: Vec::new(),
        }
    }
}

/// The analysis worker's settings.
pub(crate) fn analysis_control(s: &Settings, lite: bool) -> AnalysisControl {
    AnalysisControl { options: s.analysis_options(lite), detect_key: s.analysis.detect_key, auto: s.analysis.auto_analyse, requested: Vec::new() }
}

/// Callbacks of the main window are wired in `shell`; the Preferences
/// window wires its own when first opened.
pub(crate) fn wire(app: &Rc<App>) {
    let started = app.state.borrow().settings.clone();
    app.prefs.borrow_mut().started = Some(started);
    // Closing the main window ends the app, Preferences included.
    let weak = Rc::downgrade(app);
    app.ui.window().on_close_requested(move || {
        if let Some(a) = weak.upgrade() {
            close(&a);
        }
        slint::CloseRequestResponse::HideWindow
    });
}

/// Opens (or raises) the Preferences window.
pub(crate) fn open(app: &Rc<App>) {
    let existing = app.prefs.borrow().window.as_ref().map(|w| w.as_weak());
    if let Some(w) = existing.and_then(|w| w.upgrade()) {
        refresh(app);
        let _ = w.show();
        w.invoke_focus_keys();
        return;
    }
    let w = match PreferencesWindow::new() {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "preferences window");
            return;
        }
    };
    {
        let st = app.state.borrow();
        shell::apply_theme(&w.global::<crate::Theme>(), &st.settings, st.config.lite);
    }
    w.set_platform_note(
        if Platform::current() == Platform::Mac { od_i18n::tr("OPEN WITH ⌘,  ·  CLOSE WITH ESC OR ⌘W") } else { od_i18n::tr("OPEN WITH CTRL+,  ·  CLOSE WITH ESC OR CTRL+W") }.into(),
    );
    wire_window(app, &w);
    app.prefs.borrow_mut().window = Some(w);
    refresh(app);
    if let Some(w) = app.prefs.borrow().window.as_ref() {
        let _ = w.show();
        w.invoke_focus_keys();
    }
}

fn close(app: &App) {
    let mut p = app.prefs.borrow_mut();
    p.recording = None;
    p.pending = None;
    p.message.clear();
    if let Some(w) = p.window.as_ref() {
        let _ = w.hide();
    }
}

fn with_window(app: &App, f: impl FnOnce(&PreferencesWindow)) {
    let w = app.prefs.borrow().window.as_ref().map(|w| w.as_weak());
    if let Some(w) = w.and_then(|w| w.upgrade()) {
        f(&w);
    }
}

fn index_of<T: PartialEq>(list: &[T], v: &T) -> i32 {
    list.iter().position(|x| x == v).map(|i| i as i32).unwrap_or(0)
}

fn strings(v: Vec<String>) -> slint::ModelRc<SharedString> {
    model(v.into_iter().map(SharedString::from).collect())
}

/// Pushes every setting into the window.
pub(crate) fn refresh(app: &App) {
    let devices: Vec<String> = od_audio_io::list_devices(od_audio_io::Backend::Default)
        .map(|d| d.into_iter().filter(|d| d.max_output_channels > 0).map(|d| d.name).collect())
        .unwrap_or_default();
    app.prefs.borrow_mut().devices = devices.clone();
    let st = app.state.borrow();
    let s = &st.settings;
    let started = app.prefs.borrow().started.clone().unwrap_or_else(|| s.clone());
    let engine_line = app.engine.borrow().message.clone();
    with_window(app, |w| {
        // Audio.
        let mut names = vec![od_i18n::tr("Automatic").to_string()];
        names.extend(devices.iter().cloned());
        let mut sel = devices.iter().position(|d| *d == s.audio.device).map(|i| i as i32 + 1).unwrap_or(0);
        if !s.audio.device.is_empty() && sel == 0 {
            names.push(od_i18n::trf("{} (not connected)", &[&s.audio.device]));
            sel = names.len() as i32 - 1;
        }
        w.set_audio_devices(strings(names));
        w.set_audio_device(sel);
        w.set_buffer_options(strings(BUFFER_SIZES.iter().map(|b| if *b == 0 { od_i18n::tr("AUTO").to_string() } else { b.to_string() }).collect()));
        w.set_buffer(index_of(&BUFFER_SIZES, &s.audio.buffer));
        w.set_rate_options(strings(
            SAMPLE_RATES.iter().map(|r| if *r == 0 { od_i18n::tr("DEVICE").to_string() } else { format!("{:.1} kHz", *r as f64 / 1000.0).replace(".0 ", " ") }).collect(),
        ));
        w.set_rate(index_of(&SAMPLE_RATES, &s.audio.sample_rate));
        w.set_headphones(index_of(&["auto", "34", "off"], &s.audio.headphones.as_str()));
        w.set_audio_status(engine_line.into());
        w.set_audio_pending(s.audio != started.audio);
        // View.
        w.set_language(index_of(&["", "en", "de"], &s.view.language.as_str()));
        w.set_notation(index_of(&["camelot", "openkey", "musical"], &s.view.key_notation.as_str()));
        w.set_scale_options(strings(SCALES.iter().map(|f| format!("{}%", (f * 100.0).round())).collect()));
        w.set_scale(SCALES.iter().position(|f| (f - s.view.scale).abs() < 0.01).map(|i| i as i32).unwrap_or(1));
        w.set_scale_pending((s.view.scale - started.view.scale).abs() > 0.01);
        w.set_colorblind(s.view.colorblind_safe);
        w.set_high_contrast(s.view.high_contrast);
        w.set_reduced_motion(s.view.reduced_motion);
        w.set_waveform(index_of(&["3band", "blue"], &s.view.waveform.as_str()));
        w.set_phrases(s.view.phrases);
        // Analysis.
        w.set_bpm_options(strings(BPM_RANGES.iter().map(|(a, b)| format!("{a}–{b}")).collect()));
        w.set_bpm_range(index_of(&BPM_RANGES, &(s.analysis.bpm_min, s.analysis.bpm_max)));
        w.set_detect_key(s.analysis.detect_key);
        w.set_detect_phrases(s.analysis.detect_phrases);
        w.set_auto_analyse(s.analysis.auto_analyse);
        // Controller.
        let mut controllers: Vec<String> = Vec::new();
        if let Some(n) = app.controller.borrow().name.clone() {
            controllers.push(od_i18n::trf("{} — connected", &[&n]));
        }
        if let Ok(ports) = od_controller::io::list_ports() {
            controllers.extend(ports.inputs.into_iter().map(|p| od_i18n::trf("MIDI input: {}", &[&p])));
        }
        w.set_controllers(strings(controllers));
        w.set_mappings(strings(crate::controller::mapping_names()));
        w.set_mapping_file(s.controller.mapping_file.clone().into());
        w.set_mapping_pending(s.controller.mapping_file != started.controller.mapping_file);
        // Recording.
        w.set_rec_format(index_of(&["wav", "flac", "wav+mp3"], &s.recording.format.as_str()));
        w.set_rec_folder(s.recordings_dir().display().to_string().into());
        w.set_auto_split(s.recording.auto_split);
        w.set_silence_start(s.recording.silence_start);
        w.set_rec_note(
            match (s.recording.format.as_str(), crate::recorder::encoder_available()) {
                ("wav", _) => "",
                (_, true) => od_i18n::tr("Recorded as WAV, then converted with ffmpeg when the recording stops."),
                (_, false) => od_i18n::tr("Needs ffmpeg on the PATH; without it the recording stays a WAV file."),
            }
            .into(),
        );
        // Advanced.
        w.set_library_path(st.config.library_path.display().to_string().into());
        w.set_library_pending(GlobalConfig::load().library.is_some_and(|p| p != st.config.library_path));
        let backups = st.lib.backups().unwrap_or_default();
        w.set_backups_text(match backups.first() {
            Some(b) => od_i18n::trf("{} backups · newest {}", &[&backups.len(), &b.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]),
            None => od_i18n::tr("No backups yet").to_string(),
        }
        .into());
        w.set_backup_on_start(s.advanced.backup_on_start);
    });
    drop(st);
    refresh_keys(app);
}

/// Rows of the Keyboard page: matching actions under group headers.
pub(crate) fn key_rows(keymap: &keymap::Keymap, search: &str) -> Vec<KeyRow> {
    let platform = keymap.platform();
    let q = search.trim().to_lowercase();
    let mut rows = Vec::new();
    let mut group: Option<Group> = None;
    for a in keymap::registry() {
        let bindings: Vec<String> = keymap.bindings(&a.id).iter().map(|c| c.display(platform)).collect();
        let binding = bindings.join("  ·  ");
        let title = a.title();
        if !q.is_empty() && !title.to_lowercase().contains(&q) && !a.id.contains(&q) && !binding.to_lowercase().contains(&q) {
            continue;
        }
        if group != Some(a.group) {
            group = Some(a.group);
            rows.push(KeyRow { title: group_title(a.group).to_uppercase().into(), header: true, ..Default::default() });
        }
        let scope = match a.context {
            keymap::Context::Global => "",
            keymap::Context::Prepare => od_i18n::tr("PREPARE"),
            keymap::Context::Perform => od_i18n::tr("PERFORM"),
        };
        rows.push(KeyRow { id: a.id.clone().into(), title: title.into(), binding: binding.into(), scope: scope.into(), custom: !keymap.is_default(&a.id), header: false });
    }
    rows
}

fn group_title(g: Group) -> String {
    match g {
        Group::Deck(n) => od_i18n::trf("Deck {}", &[&n]),
        g => od_i18n::tr(&g.title()).to_string(),
    }
}

fn refresh_keys(app: &App) {
    let p = app.prefs.borrow();
    let st = app.state.borrow();
    let rows = key_rows(&st.keymap, &p.search);
    with_window(app, |w| {
        w.set_key_rows(model(rows));
        w.set_key_count(keymap::registry().len() as i32);
        w.set_recording_id(p.recording.clone().unwrap_or_default().into());
        w.set_key_selected(p.selected.clone().unwrap_or_default().into());
        w.set_key_message(p.message.clone().into());
        w.set_key_conflict(p.pending.is_some());
    });
}

fn set_message(app: &App, m: String) {
    app.prefs.borrow_mut().message = m;
    refresh_keys(app);
}

/// A key press in the Preferences window: records a binding, or closes the
/// window on Esc / ⌘W.
fn key_event(app: &App, text: &str, control: bool, meta: bool, alt: bool, shift: bool) -> bool {
    let layout = app.prefs.borrow().layout;
    let Some(chord) = Chord::from_event(text, control, meta, alt, shift, layout) else { return false };
    let plain = chord.mods == keymap::Mods::default();
    let recording = app.prefs.borrow().recording.clone();
    if let Some(id) = recording {
        app.prefs.borrow_mut().recording = None;
        if plain && chord.key == "Escape" {
            set_message(app, String::new());
            return true;
        }
        if plain && (chord.key == "Backspace" || chord.key == "Delete") {
            clear_binding(app, &id);
            return true;
        }
        record(app, &id, chord);
        return true;
    }
    // A conflict waits: Return replaces, Esc cancels.
    if plain && app.prefs.borrow().pending.is_some() && (chord.key == "Return" || chord.key == "Escape") {
        act(app, if chord.key == "Return" { "keys.replace" } else { "keys.cancel" });
        return true;
    }
    let primary_only = chord.mods == keymap::Mods { primary: true, ..Default::default() };
    if (plain && chord.key == "Escape") || (primary_only && chord.key == "W") {
        close(app);
        return true;
    }
    let page = with_page(app);
    // ← → walk the categories.
    if plain && (chord.key == "Left" || chord.key == "Right") {
        let i = PAGES.iter().position(|p| *p == page).unwrap_or(0);
        let next = if chord.key == "Left" { (i + PAGES.len() - 1) % PAGES.len() } else { (i + 1) % PAGES.len() };
        select_page(app, PAGES[next]);
        return true;
    }
    // Keyboard page: ↑ ↓ pick an action, Return records, Backspace clears.
    if page == "keyboard" && plain {
        match chord.key.as_str() {
            "Up" | "Down" => {
                step_selected(app, chord.key == "Down");
                return true;
            }
            "Return" | "Space" => {
                let sel = app.prefs.borrow().selected.clone();
                if let Some(id) = sel {
                    start_recording(app, &id);
                }
                return true;
            }
            "Backspace" | "Delete" => {
                let sel = app.prefs.borrow().selected.clone();
                if let Some(id) = sel {
                    clear_binding(app, &id);
                }
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Moves the Keyboard page selection and keeps it in view.
fn step_selected(app: &App, down: bool) {
    let rows = {
        let st = app.state.borrow();
        key_rows(&st.keymap, &app.prefs.borrow().search)
    };
    let ids: Vec<&str> = rows.iter().filter(|r| !r.header).map(|r| r.id.as_str()).collect();
    if ids.is_empty() {
        return;
    }
    let cur = app.prefs.borrow().selected.clone();
    let i = cur.and_then(|c| ids.iter().position(|x| *x == c));
    let next = match i {
        None => 0,
        Some(i) if down => (i + 1).min(ids.len() - 1),
        Some(i) => i.saturating_sub(1),
    };
    let id = ids[next].to_string();
    // Row offsets: headers 36 px, actions 28 px.
    let mut y = 0.0f32;
    for r in &rows {
        if r.id.as_str() == id {
            break;
        }
        y += if r.header { 36.0 } else { 28.0 };
    }
    app.prefs.borrow_mut().selected = Some(id);
    with_window(app, |w| {
        // Keep a margin for the message line that recording may add.
        let view = (w.get_key_view_height() - 40.0).max(28.0);
        let top = -w.get_key_scroll();
        if y < top {
            w.set_key_scroll(-(y - if next == 0 { 36.0 } else { 0.0 }).max(0.0));
        } else if y + 28.0 > top + view {
            w.set_key_scroll(-(y + 28.0 - view));
        }
    });
    refresh_keys(app);
}

fn start_recording(app: &App, id: &str) {
    {
        let mut p = app.prefs.borrow_mut();
        p.recording = Some(id.to_string());
        p.selected = Some(id.to_string());
        p.pending = None;
    }
    let title = keymap::action(id).map(|x| x.title()).unwrap_or_default();
    set_message(app, od_i18n::trf("Press the new keys for {}", &[&title]));
}

pub(crate) const PAGES: [&str; 7] = ["audio", "view", "analysis", "controller", "keyboard", "recording", "advanced"];

fn select_page(app: &App, page: &str) {
    app.prefs.borrow_mut().recording = None;
    with_window(app, |w| w.set_page(page.into()));
    refresh(app);
}

/// Binds `chord` to `id`, or asks before taking it from another action.
fn record(app: &App, id: &str, chord: Chord) {
    let platform = Platform::current();
    let Some(action) = keymap::action(id) else { return };
    let conflicts = app.state.borrow().keymap.conflicts(id, &chord);
    if conflicts.is_empty() {
        {
            let mut st = app.state.borrow_mut();
            st.keymap.set(id, vec![chord.clone()]);
            st.save_keymap();
        }
        set_message(app, od_i18n::trf("{} now on {}", &[&action.title(), &chord.display(platform)]));
    } else {
        let names: Vec<String> = conflicts.iter().map(|a| format!("“{}”", a.title())).collect();
        app.prefs.borrow_mut().pending = Some((id.to_string(), chord.clone()));
        set_message(app, od_i18n::trf("{} is already used by {}. Replace it there?", &[&chord.display(platform), &names.join(", ")]));
    }
}

fn clear_binding(app: &App, id: &str) {
    {
        let mut st = app.state.borrow_mut();
        st.keymap.set(id, Vec::new());
        st.save_keymap();
    }
    let title = keymap::action(id).map(|a| a.title()).unwrap_or_default();
    set_message(app, od_i18n::trf("{} has no key now", &[&title]));
}

/// Applies everything that can change live; the rest shows "applies after
/// restart".
fn apply_live(app: &App) {
    let (settings, lite) = {
        let st = app.state.borrow();
        (st.settings.clone(), st.config.lite)
    };
    shell::apply_language(&settings.language());
    shell::apply_theme(&app.ui.global::<crate::Theme>(), &settings, lite);
    with_window(app, |w| shell::apply_theme(&w.global::<crate::Theme>(), &settings, lite));
    app.ui.set_show_phrases(settings.view.phrases);
    {
        let mut c = app.analysis.control.lock().unwrap_or_else(|e| e.into_inner());
        let requested = std::mem::take(&mut c.requested);
        *c = analysis_control(&settings, lite);
        c.requested = requested;
    }
    let _ = app.analysis.wake.send(());
    crate::engine::show_message(app);
    app.refresh_all();
    crate::decks::refresh_analysis(app);
}

fn change(app: &App, f: impl Fn(&mut Settings)) {
    app.state.borrow_mut().change_settings(f);
    apply_live(app);
    refresh(app);
}

fn choose(app: &App, key: &str, i: i32) {
    let i = i.max(0) as usize;
    match key {
        "audio.device" => {
            let name = if i == 0 { String::new() } else { app.prefs.borrow().devices.get(i - 1).cloned().unwrap_or_default() };
            if i > 0 && name.is_empty() {
                return; // the "not connected" entry: keep it
            }
            change(app, |s| s.audio.device = name.clone());
        }
        "audio.buffer" => change(app, |s| s.audio.buffer = BUFFER_SIZES.get(i).copied().unwrap_or(0)),
        "audio.rate" => change(app, |s| s.audio.sample_rate = SAMPLE_RATES.get(i).copied().unwrap_or(0)),
        "audio.headphones" => change(app, |s| s.audio.headphones = ["auto", "34", "off"].get(i).unwrap_or(&"auto").to_string()),
        "view.language" => change(app, |s| s.view.language = ["", "en", "de"].get(i).unwrap_or(&"").to_string()),
        "view.notation" => change(app, |s| s.view.key_notation = ["camelot", "openkey", "musical"].get(i).unwrap_or(&"camelot").to_string()),
        "view.scale" => change(app, |s| s.view.scale = SCALES.get(i).copied().unwrap_or(1.0)),
        "view.waveform" => change(app, |s| s.view.waveform = ["3band", "blue"].get(i).unwrap_or(&"3band").to_string()),
        "analysis.bpm" => {
            let (a, b) = BPM_RANGES.get(i).copied().unwrap_or((87, 175));
            change(app, |s| {
                s.analysis.bpm_min = a;
                s.analysis.bpm_max = b;
            })
        }
        "rec.format" => change(app, |s| s.recording.format = ["wav", "flac", "wav+mp3"].get(i).unwrap_or(&"wav").to_string()),
        _ => tracing::warn!(key, "unknown choice"),
    }
}

fn flag(app: &App, key: &str, v: bool) {
    match key {
        "view.phrases" => change(app, |s| s.view.phrases = v),
        "view.colorblind" => change(app, |s| s.view.colorblind_safe = v),
        "view.contrast" => change(app, |s| s.view.high_contrast = v),
        "view.motion" => change(app, |s| s.view.reduced_motion = v),
        "analysis.key" => change(app, |s| s.analysis.detect_key = v),
        "analysis.phrases" => change(app, |s| s.analysis.detect_phrases = v),
        "analysis.auto" => change(app, |s| s.analysis.auto_analyse = v),
        "rec.split" => change(app, |s| s.recording.auto_split = v),
        "rec.silence" => {
            change(app, |s| s.recording.silence_start = v);
            crate::recorder::arm(app, v);
        }
        "adv.backup" => change(app, |s| s.advanced.backup_on_start = v),
        _ => tracing::warn!(key, "unknown flag"),
    }
}

fn reveal(path: &std::path::Path) {
    let (opener, args) = if cfg!(target_os = "macos") {
        ("open", vec!["-R".to_string(), path.display().to_string()])
    } else {
        ("xdg-open", vec![path.parent().unwrap_or(path).display().to_string()])
    };
    let _ = std::process::Command::new(opener).args(args).spawn();
}

fn act(app: &App, what: &str) {
    match what {
        "reset-page" => {
            let page = with_page(app);
            change(app, |s| {
                let d = Settings::default();
                match page.as_str() {
                    "audio" => s.audio = d.audio.clone(),
                    "view" => s.view = d.view.clone(),
                    "analysis" => s.analysis = d.analysis.clone(),
                    "controller" => s.controller = d.controller.clone(),
                    "recording" => s.recording = d.recording.clone(),
                    "advanced" => s.advanced = d.advanced.clone(),
                    _ => {}
                }
            });
        }
        "mapping.choose" => {
            if let Some(f) = rfd::FileDialog::new().set_title(od_i18n::tr("Controller mapping file")).add_filter("TOML", &["toml"]).pick_file() {
                change(app, |s| s.controller.mapping_file = f.display().to_string());
            }
        }
        "mapping.clear" => change(app, |s| s.controller.mapping_file.clear()),
        "rec.folder" => {
            if let Some(d) = rfd::FileDialog::new().set_title(od_i18n::tr("Folder for recordings")).pick_folder() {
                change(app, |s| s.recording.folder = d.display().to_string());
            }
        }
        "library.choose" => {
            let picked = rfd::FileDialog::new().set_title(od_i18n::tr("Library file (library.sqlite)")).add_filter("SQLite", &["sqlite"]).pick_file();
            if let Some(f) = picked {
                let mut c = GlobalConfig::load();
                c.library = Some(f);
                match c.save() {
                    Ok(()) => app.status("Library location saved — applies after restart"),
                    Err(e) => app.status(format!("{e}")),
                }
                refresh(app);
            }
        }
        "library.show" => reveal(&app.state.borrow().config.library_path),
        "backup.now" => {
            let r = app.state.borrow().lib.backup_now("manual");
            match r {
                Ok(p) => app.status(format!("Backup saved: {}", p.display())),
                Err(e) => app.status(format!("{e}")),
            }
            refresh(app);
        }
        "backup.show" => {
            let first = app.state.borrow().lib.backups().ok().and_then(|b| b.first().cloned());
            if let Some(p) = first {
                reveal(&p);
            }
        }
        "keys.reset-all" => {
            {
                let mut st = app.state.borrow_mut();
                st.keymap.reset_all();
                st.save_keymap();
            }
            set_message(app, od_i18n::tr("All keys are back to their defaults").into());
        }
        "keys.replace" => {
            let pending = app.prefs.borrow_mut().pending.take();
            if let Some((id, chord)) = pending {
                {
                    let mut st = app.state.borrow_mut();
                    st.keymap.assign(&id, chord.clone());
                    st.save_keymap();
                }
                let title = keymap::action(&id).map(|a| a.title()).unwrap_or_default();
                set_message(app, od_i18n::trf("{} now on {}", &[&title, &chord.display(Platform::current())]));
            }
        }
        "keys.cancel" => {
            app.prefs.borrow_mut().pending = None;
            set_message(app, String::new());
        }
        "keys.export" => {
            let file = rfd::FileDialog::new().set_title(od_i18n::tr("Export keymap")).set_file_name("opendecks-keymap.toml").add_filter("TOML", &["toml"]).save_file();
            if let Some(f) = file {
                let text = app.state.borrow().keymap.export_toml();
                let m = match std::fs::write(&f, text) {
                    Ok(()) => od_i18n::trf("Keymap exported to {}", &[&f.display()]),
                    Err(e) => format!("{e}"),
                };
                set_message(app, m);
            }
        }
        "keys.import" => {
            let file = rfd::FileDialog::new().set_title(od_i18n::tr("Import keymap")).add_filter("TOML", &["toml"]).pick_file();
            if let Some(f) = file {
                let m = import_keymap(app, &f);
                set_message(app, m);
            }
        }
        _ => tracing::warn!(what, "unknown preference action"),
    }
}

fn import_keymap(app: &App, f: &PathBuf) -> String {
    let text = match std::fs::read_to_string(f) {
        Ok(t) => t,
        Err(e) => return format!("{e}"),
    };
    let mut st = app.state.borrow_mut();
    let mut km = st.keymap.clone();
    match km.import_toml(&text) {
        Ok(r) => {
            let conflicts = km.all_conflicts();
            if let Some((a, b, c)) = conflicts.first() {
                let t = |id: &str| keymap::action(id).map(|x| x.title()).unwrap_or_default();
                return od_i18n::trf("Not imported: {} is used by both {} and {}", &[&c.display(km.platform()), &t(a), &t(b)]);
            }
            st.keymap = km;
            st.save_keymap();
            let mut m = od_i18n::trf("Imported {} bindings", &[&r.applied]);
            if !r.unknown.is_empty() || !r.invalid.is_empty() {
                m.push_str(&od_i18n::trf(" · skipped {} unknown, {} invalid", &[&r.unknown.len(), &r.invalid.len()]));
            }
            m
        }
        Err(e) => od_i18n::trf("Not a keymap file: {}", &[&e]),
    }
}

fn with_page(app: &App) -> String {
    let mut page = String::from("audio");
    with_window(app, |w| page = w.get_page().to_string());
    page
}

fn wire_window(app: &Rc<App>, w: &PreferencesWindow) {
    w.on_select_page(handler!(app, |a, p: SharedString| select_page(&a, p.as_str())));
    w.on_choose(handler!(app, |a, k: SharedString, i: i32| choose(&a, k.as_str(), i)));
    w.on_flag(handler!(app, |a, k: SharedString, v: bool| flag(&a, k.as_str(), v)));
    w.on_act(handler!(app, |a, k: SharedString| act(&a, k.as_str())));
    w.on_search_keys(handler!(app, |a, q: SharedString| {
        a.prefs.borrow_mut().search = q.to_string();
        refresh_keys(&a);
    }));
    w.on_record_key(handler!(app, |a, id: SharedString| start_recording(&a, id.as_str())));
    w.on_reset_key(handler!(app, |a, id: SharedString| {
        {
            let mut st = a.state.borrow_mut();
            st.keymap.reset(&id);
            // A reset may collide with a binding made since.
            let conflicts: Vec<(String, String, Chord)> = st.keymap.all_conflicts().into_iter().filter(|(x, y, _)| x == id.as_str() || y == id.as_str()).collect();
            for (x, y, c) in conflicts {
                let other = if x == id.as_str() { y } else { x };
                let rest: Vec<Chord> = st.keymap.bindings(&other).into_iter().filter(|b| *b != c).collect();
                st.keymap.set(&other, rest);
            }
            st.save_keymap();
        }
        let title = keymap::action(&id).map(|x| x.title()).unwrap_or_default();
        set_message(&a, od_i18n::trf("{} is back to its default", &[&title]));
    }));
    w.on_clear_key(handler!(app, |a, id: SharedString| clear_binding(&a, &id)));
    let weak = Rc::downgrade(app);
    w.on_key_event(move |text, control, meta, alt, shift| {
        let Some(app) = weak.upgrade() else { return false };
        key_event(&app, text.as_str(), control, meta, alt, shift)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_rows_group_and_filter() {
        let k = keymap::Keymap::new(Platform::Mac);
        let all = key_rows(&k, "");
        assert_eq!(all.iter().filter(|r| !r.header).count(), keymap::registry().len());
        assert!(all[0].header);
        let hits = key_rows(&k, "undo");
        assert_eq!(hits.iter().filter(|r| !r.header).map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["library.undo"]);
        // Searching a key finds its actions.
        let z = key_rows(&k, "⌘z");
        assert!(z.iter().any(|r| r.id == "library.undo"));
        assert!(key_rows(&k, "no such action at all").is_empty());
    }

    #[test]
    fn custom_rows_are_marked() {
        let mut k = keymap::Keymap::new(Platform::Linux);
        k.set("deck1.play", vec![Chord::parse("Space", Platform::Linux).unwrap()]);
        let rows = key_rows(&k, "deck1.play");
        let r = rows.iter().find(|r| r.id == "deck1.play").unwrap();
        assert!(r.custom);
        assert_eq!(r.binding, "Space");
    }

    #[test]
    fn settings_tables_hold_the_defaults() {
        let d = Settings::default();
        assert!(BUFFER_SIZES.contains(&d.audio.buffer));
        assert!(SAMPLE_RATES.contains(&d.audio.sample_rate));
        assert!(SCALES.contains(&d.view.scale));
        assert!(BPM_RANGES.contains(&(d.analysis.bpm_min, d.analysis.bpm_max)));
    }
}
