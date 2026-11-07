//! Preferences: the settings window (SPEC §12.2) — Audio, View, Analysis,
//! Controller, Keyboard, Recording, Advanced. Opens with ⌘, / Ctrl+, (the
//! platform menu and the keymap) and from the gear in the top bar; Esc or
//! ⌘W / Ctrl+W closes it. Settings are stored in the library (see
//! [`crate::settings`]) and applied live where possible.

use crate::app::{App, handler};
use crate::keymap::{self, Chord, Group, Layout, Platform};
use crate::models::model;
use crate::settings::{
    BPM_RANGES, BUFFER_SIZES, CROSSFADER_CURVES, EQ_MODES, FADER_CURVES, FONT_SIZES, GlobalConfig, JOG_MODES, LINE_SPACINGS, REC_GAINS, SAMPLE_RATES, SCALES,
    Settings, TEMPO_RANGES, ZOOM_SPANS,
};
use od_engine::{ChannelCommand, Command, CrossfaderCurve, DeckCommand, EqMode, FaderCurve, JogMode, MixerCommand, TempoRange};
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
    /// MIDI monitor and learn (Controller page).
    pub midi: crate::midi_tools::MidiTools,
    /// Polls the MIDI monitor while it or learn runs.
    midi_timer: slint::Timer,
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
            midi: Default::default(),
            midi_timer: slint::Timer::default(),
        }
    }
}

/// The analysis worker's settings.
pub(crate) fn analysis_control(s: &Settings, lite: bool) -> AnalysisControl {
    AnalysisControl { options: s.analysis_options(lite), detect_key: s.analysis.detect_key, auto: s.analysis.auto_analyse, requested: Vec::new() }
}

/// Deck defaults (Preferences > Controller > Decks) as commands for one
/// deck: tempo range, jog mode and auto cue. Quantize is global, see
/// [`engine_settings`].
pub(crate) fn deck_defaults(s: &Settings) -> Vec<DeckCommand> {
    let range = match s.deck.tempo_range.as_str() {
        "six" => TempoRange::Six,
        "sixteen" => TempoRange::Sixteen,
        "wide" => TempoRange::Wide,
        _ => TempoRange::Ten,
    };
    let jog = if s.deck.jog_mode == "bend" { JogMode::Bend } else { JogMode::Vinyl };
    vec![DeckCommand::SetTempoRange(range), DeckCommand::SetJogMode(jog), DeckCommand::SetAutoCue(s.deck.auto_cue)]
}

/// Mixer settings (Preferences > Controller > Mixer) for `channels`
/// channels and the master section.
pub(crate) fn mixer_settings(s: &Settings, channels: usize) -> Vec<Command> {
    let eq = if s.mixer.eq_mode == "classic" { EqMode::Classic } else { EqMode::Isolator };
    let fader = match s.mixer.fader_curve.as_str() {
        "linear" => FaderCurve::Linear,
        "sharp" => FaderCurve::Sharp,
        _ => FaderCurve::Smooth,
    };
    let xf = match s.mixer.crossfader_curve.as_str() {
        "linear" => CrossfaderCurve::Linear,
        "cut" => CrossfaderCurve::Cut,
        _ => CrossfaderCurve::ConstantPower,
    };
    let mut out = Vec::new();
    for c in 0..channels {
        out.push(Command::Channel(c, ChannelCommand::SetEqMode(eq)));
        out.push(Command::Channel(c, ChannelCommand::SetFaderCurve(fader)));
        out.push(Command::Channel(c, ChannelCommand::SetAutoGain(s.mixer.auto_gain)));
    }
    out.push(Command::Mixer(MixerCommand::SetCrossfaderCurve(xf)));
    out
}

/// Every engine setting of the preferences for `decks` decks: deck
/// defaults, global quantize and the mixer.
pub(crate) fn engine_settings(s: &Settings, decks: usize) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::new();
    for d in 0..decks {
        out.extend(deck_defaults(s).into_iter().map(|c| Command::Deck(d, c)));
    }
    out.push(Command::SetQuantize(s.deck.quantize));
    out.extend(mixer_settings(s, decks));
    out
}

/// Starting zoom of the deck waveforms (`decks.zoom`, 1 = 8 s across) for
/// Preferences > View > Waveform zoom.
pub(crate) fn default_deck_zoom(s: &Settings) -> f64 {
    s.view.zoom_span as f64 / 8.0
}

/// Sends the deck defaults and mixer settings to the engine, if it runs.
/// Call once right after the engine starts (`engine::ensure_engine`).
pub(crate) fn apply_engine_settings(app: &App) {
    if app.engine.borrow().engine.is_none() {
        return;
    }
    let s = app.state.borrow().settings.clone();
    for c in engine_settings(&s, crate::decks::DECKS) {
        crate::engine::send(app, c);
    }
}

/// Sends only the commands of one changed deck / mixer setting (so a live
/// change does not reset what the DJ changed on the decks since).
fn apply_engine_key(app: &App, key: &str) {
    if app.engine.borrow().engine.is_none() {
        return;
    }
    let s = app.state.borrow().settings.clone();
    let decks = crate::decks::DECKS;
    let all = engine_settings(&s, decks);
    let wanted = |c: &Command| match (key, c) {
        ("deck.tempo-range", Command::Deck(_, DeckCommand::SetTempoRange(_))) => true,
        ("deck.jog-mode", Command::Deck(_, DeckCommand::SetJogMode(_))) => true,
        ("deck.auto-cue", Command::Deck(_, DeckCommand::SetAutoCue(_))) => true,
        ("deck.quantize", Command::SetQuantize(_)) => true,
        ("mixer.eq-mode", Command::Channel(_, ChannelCommand::SetEqMode(_))) => true,
        ("mixer.fader-curve", Command::Channel(_, ChannelCommand::SetFaderCurve(_))) => true,
        ("mixer.auto-gain", Command::Channel(_, ChannelCommand::SetAutoGain(_))) => true,
        ("mixer.crossfader-curve", Command::Mixer(MixerCommand::SetCrossfaderCurve(_))) => true,
        _ => false,
    };
    for c in all.into_iter().filter(wanted) {
        crate::engine::send(app, c);
    }
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
    shell::wire_tooltips(app, &w.global::<crate::TooltipState>());
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

/// Runs `f` on the Preferences window's theme, if the window exists.
pub(crate) fn with_theme(app: &App, f: impl FnOnce(&crate::Theme<'_>)) {
    with_window(app, |w| f(&w.global::<crate::Theme>()));
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
        w.set_bar_counter(index_of(&crate::settings::BAR_COUNTERS, &s.view.bar_counter.as_str()));
        w.set_appearance(index_of(&crate::settings::APPEARANCES, &s.view.appearance.as_str()));
        w.set_waveform(index_of(&["3band", "blue", "rgb"], &s.view.waveform.as_str()));
        w.set_phrases(s.view.phrases);
        w.set_zoom_span(index_of(&ZOOM_SPANS, &s.view.zoom_span));
        w.set_font_size(index_of(&FONT_SIZES, &s.browser.font_size));
        w.set_line_spacing(index_of(&LINE_SPACINGS, &s.browser.line_spacing.as_str()));
        // Controller > Decks and Mixer.
        w.set_tempo_range(index_of(&TEMPO_RANGES, &s.deck.tempo_range.as_str()));
        w.set_deck_quantize(s.deck.quantize);
        w.set_jog_mode(index_of(&JOG_MODES, &s.deck.jog_mode.as_str()));
        w.set_auto_cue(s.deck.auto_cue);
        w.set_eq_mode(index_of(&EQ_MODES, &s.mixer.eq_mode.as_str()));
        w.set_fader_curve(index_of(&FADER_CURVES, &s.mixer.fader_curve.as_str()));
        w.set_crossfader_curve(index_of(&CROSSFADER_CURVES, &s.mixer.crossfader_curve.as_str()));
        w.set_auto_gain(s.mixer.auto_gain);
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
        // MIDI monitor / learn and the FLX10 settings.
        {
            use crate::midi_tools as mt;
            let p = app.prefs.borrow();
            let m = &p.midi;
            w.set_midi_monitor(m.monitoring);
            w.set_midi_lines(strings(m.lines.iter().cloned().collect()));
            w.set_learn_active(m.learning);
            w.set_learn_actions(strings(od_controller::templates::ACTION_NAMES.iter().map(|a| a.to_string()).collect()));
            w.set_learn_action(m.action as i32);
            w.set_learn_deck(m.deck as i32);
            w.set_learn_result(m.suggestion.as_ref().map(mt::describe).unwrap_or_default().into());
            w.set_learn_entry(m.entry().unwrap_or_default().into());
            w.set_learn_file(learn_file(&st).display().to_string().into());
            let f = &s.controller.flx10;
            w.set_flx10_fader_start(index_of(&mt::FADER_STARTS, &f.fader_start.as_str()));
            w.set_flx10_rec_out(index_of(&mt::REC_OUTS, &f.rec_out.as_str()));
            w.set_flx10_time_mode(index_of(&mt::TIME_MODES, &f.time_mode.as_str()));
            w.set_flx10_slip_flashing(f.slip_flashing);
            w.set_flx10_slip_led(index_of(&mt::SLIP_LEDS, &f.slip_led.as_str()));
            w.set_flx10_virtual_deck(f.virtual_deck);
            w.set_flx10_jog_ring((f.jog_ring.clamp(1, 3) - 1) as i32);
            w.set_flx10_boost(index_of(&mt::BOOSTS, &f.boost_db));
            w.set_flx10_wire(mt::flx10_wire_text(f).into());
        }
        // Recording.
        w.set_rec_format(index_of(&["wav", "flac", "wav+mp3"], &s.recording.format.as_str()));
        w.set_rec_folder(s.recordings_dir().display().to_string().into());
        w.set_auto_split(s.recording.auto_split);
        w.set_silence_start(s.recording.silence_start);
        w.set_silence_stop(s.recording.silence_stop);
        w.set_rec_pattern(s.recording.name_pattern.clone().into());
        w.set_rec_example(crate::recorder::recording_name(&s.recording.name_pattern, "2026-11-07T21:30:00Z", 1).into());
        w.set_rec_gain_options(strings(REC_GAINS.iter().map(|g| if *g == 0.0 { "0 dB".to_string() } else { format!("{g:+} dB") }).collect()));
        w.set_rec_gain(index_of(&REC_GAINS, &s.recording.gain_db));
        w.set_rec_note(
            match s.recording.format.as_str() {
                "wav" => "",
                _ => od_i18n::tr("Recorded as WAV, then encoded when the recording stops."),
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
        w.set_update_auto(s.update.auto_check);
        w.set_update_prereleases(s.update.prereleases);
    });
    drop(st);
    refresh_updates(app);
    refresh_keys(app);
}

/// The mapping file learned controls are written to: the own mapping
/// file, else `mappings/learned.toml` next to the library.
fn learn_file(st: &crate::state::AppState) -> PathBuf {
    let own = &st.settings.controller.mapping_file;
    if own.is_empty() { crate::midi_tools::default_learn_file(&st.config.library_path) } else { PathBuf::from(own) }
}

/// Starts or stops polling the MIDI monitor.
fn midi_stream(app: &Rc<App>) {
    {
        let mut p = app.prefs.borrow_mut();
        if p.midi.monitor.is_none() {
            p.midi.monitor = crate::controller::monitor(app);
        }
        if p.midi.monitor.is_none() && (p.midi.monitoring || p.midi.learning) {
            drop(p);
            app.status("No controller layer running — connect a controller and restart");
        }
    }
    let run = {
        let p = app.prefs.borrow();
        p.midi.monitoring || p.midi.learning
    };
    let weak = Rc::downgrade(app);
    let p = app.prefs.borrow();
    if run && !p.midi_timer.running() {
        p.midi_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(60), move || {
            let Some(a) = weak.upgrade() else { return };
            let changed = a.prefs.borrow_mut().midi.poll();
            if changed {
                refresh(&a);
            }
        });
    } else if !run {
        p.midi_timer.stop();
    }
}

/// Controller page tools: the MIDI monitor, MIDI learn and the FLX10.
pub(crate) fn midi_act(app: &Rc<App>, what: &str) {
    match what {
        "midi.on" | "midi.off" => app.prefs.borrow_mut().midi.set_monitoring(what == "midi.on"),
        "midi.clear" => app.prefs.borrow_mut().midi.lines.clear(),
        "learn.start" => app.prefs.borrow_mut().midi.start_learn(),
        "learn.stop" => app.prefs.borrow_mut().midi.stop_learn(),
        "learn.save" => {
            let (entry, port) = {
                let p = app.prefs.borrow();
                (p.midi.entry(), p.midi.learn_port.clone())
            };
            let Some(entry) = entry else {
                app.status("Learn a control first");
                return;
            };
            let (path, own) = {
                let st = app.state.borrow();
                (learn_file(&st), st.settings.controller.mapping_file.clone())
            };
            match crate::midi_tools::save_entry(&path, &port, &entry) {
                Ok(()) => {
                    if own.is_empty() {
                        let f = path.display().to_string();
                        change(app, |s| s.controller.mapping_file = f.clone());
                    }
                    app.prefs.borrow_mut().midi.stop_learn();
                    app.status(od_i18n::trf("Control added to {} — used from the next start", &[&path.display()]));
                }
                Err(e) => app.status(od_i18n::trf("The control could not be added: {}", &[&e])),
            }
        }
        "flx10.apply" => crate::midi_tools::apply_flx10_now(app),
        _ => tracing::warn!(what, "unknown controller tool action"),
    }
    midi_stream(app);
    refresh(app);
}

/// Advanced > Updates: the version and when it was last checked.
pub(crate) fn refresh_updates(app: &App) {
    let text = crate::update::last_checked_text(app);
    with_window(app, |w| w.set_update_status(text.into()));
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
    shell::apply_browser_style(&app.ui, &settings, app.state.borrow().config.kiosk);
    crate::recorder::apply_gain(app);
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

/// Applies settings changed outside the window (an import) everywhere.
pub(crate) fn apply_all(app: &App) {
    apply_live(app);
    apply_engine_settings(app);
    refresh(app);
}

/// A deck or mixer setting: stored, then sent to the running engine.
fn change_engine(app: &App, key: &str, f: impl Fn(&mut Settings)) {
    change(app, f);
    apply_engine_key(app, key);
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
        "view.bar-counter" => change(app, |s| s.view.bar_counter = crate::settings::BAR_COUNTERS.get(i).unwrap_or(&"to-phrase").to_string()),
        "view.appearance" => {
            crate::appearance::set(app, crate::settings::APPEARANCES.get(i).unwrap_or(&"dark"));
            return;
        }
        "view.waveform" => change(app, |s| s.view.waveform = ["3band", "blue", "rgb"].get(i).unwrap_or(&"3band").to_string()),
        "analysis.bpm" => {
            let (a, b) = BPM_RANGES.get(i).copied().unwrap_or((87, 175));
            change(app, |s| {
                s.analysis.bpm_min = a;
                s.analysis.bpm_max = b;
            })
        }
        "rec.format" => change(app, |s| s.recording.format = ["wav", "flac", "wav+mp3"].get(i).unwrap_or(&"wav").to_string()),
        "rec.gain" => change(app, |s| s.recording.gain_db = REC_GAINS.get(i).copied().unwrap_or(0.0)),
        "view.zoom" => change(app, |s| s.view.zoom_span = ZOOM_SPANS.get(i).copied().unwrap_or(8.0)),
        "view.font-size" => change(app, |s| s.browser.font_size = FONT_SIZES.get(i).copied().unwrap_or(14)),
        "view.line-spacing" => change(app, |s| s.browser.line_spacing = LINE_SPACINGS.get(i).unwrap_or(&"normal").to_string()),
        "deck.tempo-range" => change_engine(app, key, |s| s.deck.tempo_range = TEMPO_RANGES.get(i).unwrap_or(&"ten").to_string()),
        "deck.jog-mode" => change_engine(app, key, |s| s.deck.jog_mode = JOG_MODES.get(i).unwrap_or(&"vinyl").to_string()),
        "mixer.eq-mode" => change_engine(app, key, |s| s.mixer.eq_mode = EQ_MODES.get(i).unwrap_or(&"isolator").to_string()),
        "mixer.fader-curve" => change_engine(app, key, |s| s.mixer.fader_curve = FADER_CURVES.get(i).unwrap_or(&"smooth").to_string()),
        "mixer.crossfader-curve" => change_engine(app, key, |s| s.mixer.crossfader_curve = CROSSFADER_CURVES.get(i).unwrap_or(&"constant_power").to_string()),
        "learn.action" => {
            app.prefs.borrow_mut().midi.action = i.min(od_controller::templates::ACTION_NAMES.len() - 1);
            refresh(app);
        }
        "learn.deck" => {
            app.prefs.borrow_mut().midi.deck = i.min(4) as u8;
            refresh(app);
        }
        "flx10.fader-start" => change(app, |s| s.controller.flx10.fader_start = crate::midi_tools::FADER_STARTS.get(i).unwrap_or(&"on").to_string()),
        "flx10.rec-out" => change(app, |s| s.controller.flx10.rec_out = crate::midi_tools::REC_OUTS.get(i).unwrap_or(&"with-mic").to_string()),
        "flx10.time-mode" => change(app, |s| s.controller.flx10.time_mode = crate::midi_tools::TIME_MODES.get(i).unwrap_or(&"remaining").to_string()),
        "flx10.slip-led" => change(app, |s| s.controller.flx10.slip_led = crate::midi_tools::SLIP_LEDS.get(i).unwrap_or(&"lit").to_string()),
        "flx10.jog-ring" => change(app, |s| s.controller.flx10.jog_ring = (i.min(2) + 1) as u8),
        "flx10.boost" => change(app, |s| s.controller.flx10.boost_db = crate::midi_tools::BOOSTS.get(i).copied().unwrap_or(0)),
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
        "rec.silence-stop" => change(app, |s| s.recording.silence_stop = v),
        "deck.quantize" => change_engine(app, key, |s| s.deck.quantize = v),
        "deck.auto-cue" => change_engine(app, key, |s| s.deck.auto_cue = v),
        "mixer.auto-gain" => change_engine(app, key, |s| s.mixer.auto_gain = v),
        "rec.silence" => {
            change(app, |s| s.recording.silence_start = v);
            crate::recorder::arm(app, v);
        }
        "adv.backup" => change(app, |s| s.advanced.backup_on_start = v),
        "update.auto" => change(app, |s| s.update.auto_check = v),
        "update.pre" => change(app, |s| s.update.prereleases = v),
        "flx10.slip-flashing" => change(app, |s| s.controller.flx10.slip_flashing = v),
        "flx10.virtual-deck" => change(app, |s| s.controller.flx10.virtual_deck = v),
        _ => tracing::warn!(key, "unknown flag"),
    }
}

/// A typed setting (Return in a field).
fn text(app: &App, key: &str, v: &str) {
    match key {
        "rec.pattern" => change(app, |s| s.recording.name_pattern = v.to_string()),
        _ => tracing::warn!(key, "unknown text setting"),
    }
}

pub(crate) fn reveal(path: &std::path::Path) {
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
                    "view" => {
                        s.view = d.view.clone();
                        s.browser.font_size = d.browser.font_size;
                        s.browser.line_spacing = d.browser.line_spacing.clone();
                    }
                    "analysis" => s.analysis = d.analysis.clone(),
                    "controller" => {
                        s.controller = d.controller.clone();
                        s.deck = d.deck.clone();
                        s.mixer = d.mixer.clone();
                    }
                    "recording" => s.recording = d.recording.clone(),
                    "advanced" => {
                        s.advanced = d.advanced.clone();
                        s.update.auto_check = d.update.auto_check;
                        s.update.prereleases = d.update.prereleases;
                    }
                    _ => {}
                }
            });
            if page == "controller" {
                apply_engine_settings(app);
            }
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
        "update.check" => crate::update::check_now(app, true),
        "rekordbox.settings" => {
            let m = crate::import::import_rekordbox_settings(app, None);
            app.status(m);
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
    w.on_flag(handler!(app, |a, k: SharedString, v: bool| if k == "midi.monitor" { midi_act(&a, if v { "midi.on" } else { "midi.off" }) } else { flag(&a, k.as_str(), v) }));
    w.on_act(handler!(app, |a, k: SharedString| if k.starts_with("midi.") || k.starts_with("learn.") || k.starts_with("flx10.") { midi_act(&a, k.as_str()) } else { act(&a, k.as_str()) }));
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
    w.on_text(handler!(app, |a, k: SharedString, v: SharedString| text(&a, k.as_str(), v.as_str())));
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
    fn engine_settings_cover_every_deck_and_channel() {
        let mut s = Settings::default();
        let cmds = engine_settings(&s, 4);
        // 3 per deck, quantize, 3 per channel, crossfader curve.
        assert_eq!(cmds.len(), 4 * 3 + 1 + 4 * 3 + 1);
        assert!(cmds.iter().any(|c| matches!(c, Command::SetQuantize(true))));
        assert!(cmds.iter().any(|c| matches!(c, Command::Deck(3, DeckCommand::SetTempoRange(TempoRange::Ten)))));
        assert!(cmds.iter().any(|c| matches!(c, Command::Channel(0, ChannelCommand::SetEqMode(EqMode::Isolator)))));
        s.deck.tempo_range = "wide".into();
        s.deck.jog_mode = "bend".into();
        s.deck.auto_cue = false;
        s.mixer.eq_mode = "classic".into();
        s.mixer.fader_curve = "sharp".into();
        s.mixer.crossfader_curve = "cut".into();
        s.mixer.auto_gain = false;
        assert_eq!(
            format!("{:?}", deck_defaults(&s)),
            format!("{:?}", vec![DeckCommand::SetTempoRange(TempoRange::Wide), DeckCommand::SetJogMode(JogMode::Bend), DeckCommand::SetAutoCue(false)])
        );
        let m = mixer_settings(&s, 2);
        assert!(m.iter().any(|c| matches!(c, Command::Channel(1, ChannelCommand::SetEqMode(EqMode::Classic)))));
        assert!(m.iter().any(|c| matches!(c, Command::Channel(1, ChannelCommand::SetFaderCurve(FaderCurve::Sharp)))));
        assert!(m.iter().any(|c| matches!(c, Command::Channel(0, ChannelCommand::SetAutoGain(false)))));
        assert!(m.iter().any(|c| matches!(c, Command::Mixer(MixerCommand::SetCrossfaderCurve(CrossfaderCurve::Cut)))));
        assert_eq!(default_deck_zoom(&s), 1.0);
        s.view.zoom_span = 16.0;
        assert_eq!(default_deck_zoom(&s), 2.0);
    }

    #[test]
    fn settings_tables_hold_the_defaults() {
        let d = Settings::default();
        assert!(BUFFER_SIZES.contains(&d.audio.buffer));
        assert!(SAMPLE_RATES.contains(&d.audio.sample_rate));
        assert!(SCALES.contains(&d.view.scale));
        assert!(BPM_RANGES.contains(&(d.analysis.bpm_min, d.analysis.bpm_max)));
        assert!(TEMPO_RANGES.contains(&d.deck.tempo_range.as_str()) && JOG_MODES.contains(&d.deck.jog_mode.as_str()));
        assert!(EQ_MODES.contains(&d.mixer.eq_mode.as_str()) && FADER_CURVES.contains(&d.mixer.fader_curve.as_str()));
        assert!(CROSSFADER_CURVES.contains(&d.mixer.crossfader_curve.as_str()));
        assert!(FONT_SIZES.contains(&d.browser.font_size) && LINE_SPACINGS.contains(&d.browser.line_spacing.as_str()));
        assert!(ZOOM_SPANS.contains(&d.view.zoom_span) && REC_GAINS.contains(&d.recording.gain_db));
    }
}
