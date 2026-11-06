//! Perform: audio engine lifecycle, deck loading, the per-frame deck view,
//! controller actions and LED feedback, recording, and the Prepare preview.

use crate::app::App;
use crate::{CueMark, DeckView, render};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_controller::{
    BrowseAction, ControlAction, ControlFeedback, CrossfaderSide, DeckAction, DeckElement, FeedbackTarget,
    MixerAction, MixerElement, PadMode,
};
use od_controller::io::ControllerManager;
use od_core::{Beatgrid, CueKind, TrackId, Waveform};
use od_engine::{
    ChannelCommand, Command, DeckCommand, Engine, EngineConfig, JogMode, MemoryCueJump, MixerCommand, Profile, SamplerCommand, TempoRange,
    TrackData, XfAssign,
};
use od_recorder::Recorder;
use slint::{ComponentHandle, Image, ModelRc, VecModel};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DECKS: usize = 4;
const PREVIEW_DECK: usize = 3;
/// Seconds of audio shown across the scrolling waveform.
const DETAIL_SPAN: f64 = 8.0;
/// Loudness target for auto-gain.
const TARGET_LUFS: f32 = -10.0;

#[derive(Clone)]
struct Loaded {
    id: TrackId,
    title: String,
    artist: String,
    key: String,
    waveform: Waveform,
    grid: Beatgrid,
    overview: Image,
    cues: Vec<CueMark>,
}

struct Prepared {
    deck: usize,
    data: Arc<TrackData>,
    play_from: Option<f64>,
}

pub(crate) struct Perform {
    engine: Option<Engine>,
    host: Option<od_audio_io::AudioHost>,
    recorder: Option<Recorder>,
    controllers: Option<ControllerManager>,
    deck_count: usize,
    loaded: [Option<Loaded>; DECKS],
    /// View data of tracks being prepared, moved to `loaded` on arrival.
    pending: [Option<Loaded>; DECKS],
    prepared_tx: Sender<Prepared>,
    prepared_rx: Receiver<Prepared>,
    previewing: bool,
    shift: [bool; DECKS],
    pad_mode: [PadMode; DECKS],
    last_dominant: Option<usize>,
    message: String,
    timer: slint::Timer,
    last_frame: Instant,
    controller_name: Option<String>,
}

impl Default for Perform {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            engine: None,
            host: None,
            recorder: None,
            controllers: None,
            deck_count: 2,
            loaded: Default::default(),
            pending: Default::default(),
            prepared_tx: tx,
            prepared_rx: rx,
            previewing: false,
            shift: [false; DECKS],
            pad_mode: [PadMode::HotCue; DECKS],
            last_dominant: None,
            message: od_i18n::tr("Audio starts when you load a track").into(),
            timer: slint::Timer::default(),
            last_frame: Instant::now(),
            controller_name: None,
        }
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}.{}", s / 60, s % 60, ((secs.max(0.0).fract()) * 10.0) as u64)
}

/// Directory with controller mapping files.
fn mappings_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OPENDECKS_MAPPINGS") {
        return PathBuf::from(d);
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().skip(1).take(4) {
            let p = anc.join("mappings");
            if p.is_dir() {
                return p;
            }
            for sub in ["Resources/mappings", "share/opendecks/mappings"] {
                let p = anc.join(sub);
                if p.is_dir() {
                    return p;
                }
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../mappings"))
}

/// Starts the controller layer (MIDI) once.
fn ensure_controllers(app: &App) {
    let mut p = app.perform.borrow_mut();
    if p.controllers.is_some() {
        return;
    }
    let mappings: Vec<Arc<od_controller::Mapping>> = od_controller::Mapping::load_dir(&mappings_dir())
        .into_iter()
        .filter_map(|(path, m)| match m {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "mapping failed to load");
                None
            }
        })
        .collect();
    p.controllers = Some(ControllerManager::start(mappings, Default::default()));
}

/// Starts audio on first use. Prefers the controller's audio interface.
fn ensure_engine(app: &App) -> bool {
    if app.perform.borrow().engine.is_some() {
        return true;
    }
    let lite = app.state.borrow().config.lite;
    let device = od_audio_io::list_devices(od_audio_io::Backend::Default)
        .ok()
        .and_then(|devs| devs.into_iter().find(|d| d.name.contains("DDJ") || d.name.contains("FLX")).map(|d| d.name));
    let mut cfg = od_audio_io::AudioConfig {
        device: device.clone(),
        buffer_frames: Some(if lite { 256 } else { 128 }),
        ..Default::default()
    };
    let negotiated = match od_audio_io::AudioHost::negotiate(&cfg) {
        Ok(n) => n,
        Err(e) => {
            app.perform.borrow_mut().message = format!("No audio output: {e}");
            return false;
        }
    };
    cfg.sample_rate = Some(negotiated.sample_rate);
    let (mut engine, processor) = Engine::new(EngineConfig {
        sample_rate: negotiated.sample_rate,
        max_block: 2048,
        decks: DECKS,
        profile: if lite { Profile::Lite } else { Profile::Full },
    });
    match od_audio_io::AudioHost::start(&cfg, processor) {
        Ok(host) => {
            let recorder = engine.take_record_tap().map(|t| Recorder::new(t, negotiated.sample_rate));
            let mut p = app.perform.borrow_mut();
            p.message = format!(
                "{} · {} Hz{}",
                negotiated.device_name,
                negotiated.sample_rate,
                if negotiated.routing.headphones.is_some() { " · headphones on 3/4" } else { "" }
            );
            p.engine = Some(engine);
            p.host = Some(host);
            p.recorder = recorder;
            true
        }
        Err(e) => {
            app.perform.borrow_mut().message = format!("Audio could not start: {e}");
            false
        }
    }
}

/// Shows a status message in the current language.
fn say(app: &App, text: impl Into<String>) {
    app.ui.set_status_text(od_i18n::translate_message(&text.into()).into());
}

fn send(app: &App, cmd: Command) {
    if let Some(e) = app.perform.borrow_mut().engine.as_mut() {
        if let Err(err) = e.send(cmd) {
            tracing::warn!(error = ?err, "engine queue full");
        }
    }
}

fn deck_cmd(app: &App, deck: usize, cmd: DeckCommand) {
    send(app, Command::Deck(deck, cmd));
}

/// Loads `track` into `deck` in the background.
pub(crate) fn load_track(app: &App, deck: usize, track: TrackId, play_from: Option<f64>) {
    if !ensure_engine(app) {
        app.ui.set_engine_message(app.perform.borrow().message.clone().into());
        return;
    }
    let (rate, tx) = {
        let p = app.perform.borrow();
        (p.engine.as_ref().map(|e| e.config().sample_rate).unwrap_or(48_000), p.prepared_tx.clone())
    };
    let st = app.state.borrow();
    let Ok(t) = st.src().track(track) else { return };
    let analysis = st.src().analysis(track).unwrap_or_default();
    let cues = st.src().cues(track).unwrap_or_default();
    let scale = app.ui.window().scale_factor();
    let marks = cues
        .iter()
        .map(|c| CueMark {
            slot: match c.kind {
                CueKind::Hot(s) => s as i32,
                CueKind::Memory => -1,
            },
            letter: c.kind.hot_slot_letter().map(|l| l.to_string()).unwrap_or_default().into(),
            pos: (c.start / t.duration.max(0.001)) as f32,
            end: (c.loop_end.unwrap_or(c.start) / t.duration.max(0.001)) as f32,
            is_loop: c.is_loop(),
            time: Default::default(),
            label: c.label.clone().into(),
        })
        .collect();
    let meta = Loaded {
        id: track,
        title: t.tags.title.clone(),
        artist: t.tags.artist.clone(),
        key: t.key.map(|k| k.format(st.config.key_notation)).unwrap_or_default(),
        overview: render::overview(&analysis.waveform, (900.0 * scale) as u32, (30.0 * scale) as u32),
        waveform: analysis.waveform.clone(),
        grid: analysis.beatgrid.clone(),
        cues: marks,
    };
    let gain = analysis.loudness_lufs.map(|l| (TARGET_LUFS - l).clamp(-12.0, 12.0)).unwrap_or(0.0);
    let path = st.resolve(&t.path);
    let grid = analysis.beatgrid;
    drop(st);
    app.ui.set_engine_message(format!("Loading {} …", meta.title).into());
    app.perform.borrow_mut().pending[deck] = Some(meta);
    std::thread::spawn(move || match TrackData::load(track, &path, rate, grid, &cues, gain) {
        Ok(data) => {
            let _ = tx.send(Prepared { deck, data, play_from });
        }
        Err(e) => tracing::warn!(error = %e, "track load failed"),
    });
}

/// Double-click in the library: load into the first deck that is not
/// playing (Perform) or start the preview (Prepare).
pub(crate) fn load_selected_into_free_deck(app: &App) {
    let Some(track) = app.state.borrow().current else { return };
    if app.ui.get_mode() == "prepare" {
        toggle_preview(app);
        return;
    }
    let deck = {
        let mut p = app.perform.borrow_mut();
        let count = p.deck_count;
        let playing: Vec<bool> = match p.engine.as_mut() {
            Some(e) => e.state().decks.iter().map(|d| d.playing).collect(),
            None => vec![false; DECKS],
        };
        (0..count).find(|&d| !playing[d] && p.loaded[d].is_none()).or_else(|| (0..count).find(|&d| !playing[d])).unwrap_or(0)
    };
    load_track(app, deck, track, None);
}

pub(crate) fn toggle_preview(app: &App) {
    let previewing = app.perform.borrow().previewing;
    if previewing {
        deck_cmd(app, PREVIEW_DECK, DeckCommand::Pause);
        app.perform.borrow_mut().previewing = false;
        app.ui.set_previewing(false);
        return;
    }
    let (track, cursor) = {
        let st = app.state.borrow();
        (st.current, st.cursor)
    };
    let Some(track) = track else { return };
    let already = app.perform.borrow().loaded[PREVIEW_DECK].as_ref().map(|l| l.id) == Some(track);
    if already {
        deck_cmd(app, PREVIEW_DECK, DeckCommand::Seek { seconds: cursor });
        deck_cmd(app, PREVIEW_DECK, DeckCommand::Play);
    } else {
        load_track(app, PREVIEW_DECK, track, Some(cursor));
    }
    app.perform.borrow_mut().previewing = true;
    app.ui.set_previewing(true);
    // The preview deck is audible on the master at full level.
    send(app, Command::Channel(PREVIEW_DECK, ChannelCommand::SetFader(1.0)));
}

pub(crate) fn preview_seek(app: &App) {
    if app.perform.borrow().previewing {
        let cursor = app.state.borrow().cursor;
        deck_cmd(app, PREVIEW_DECK, DeckCommand::Seek { seconds: cursor });
    }
}

fn toggle_recording(app: &App) {
    if !ensure_engine(app) {
        return;
    }
    let recording = app.perform.borrow().recorder.as_ref().is_some_and(|r| r.is_recording());
    if recording {
        send(app, Command::SetRecordTap(false));
        let result = app.perform.borrow_mut().recorder.as_mut().map(|r| r.stop());
        match result {
            Some(Ok(rec)) => {
                let st = app.state.borrow();
                let _ = st.lib.conn().execute(
                    "INSERT INTO recording (path, started, duration, tracklist) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2, ?3)",
                    rusqlite_params(&rec),
                );
                drop(st);
                say(app, format!("Recording saved: {} ({} tracks)", rec.path.display(), rec.tracklist.len()));
            }
            Some(Err(e)) => say(app, format!("Recording failed: {e}")),
            None => {}
        }
        app.perform.borrow_mut().last_dominant = None;
    } else {
        let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Music/OpenDecks Recordings");
        let name = format!("Mix {}", app.state.borrow().lib.now().unwrap_or_default().replace(':', "-").trim_end_matches('Z').replace('T', " "));
        send(app, Command::SetRecordTap(true));
        let r = app.perform.borrow_mut().recorder.as_mut().map(|r| r.start(&dir, &name));
        if let Some(Err(e)) = r {
            say(app, format!("Recording could not start: {e}"));
        }
    }
}

fn rusqlite_params(rec: &od_recorder::Recording) -> (String, f64, String) {
    (rec.path.display().to_string(), rec.duration, serde_json::to_string(&rec.tracklist).unwrap_or_default())
}

fn pad_beats(index: u8) -> f64 {
    [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0][index.min(7) as usize]
}

fn handle_action(app: &App, action: ControlAction) {
    match action {
        ControlAction::Deck { deck, action } => {
            let d = (deck as usize).saturating_sub(1).min(DECKS - 1);
            let state = app.perform.borrow_mut().engine.as_mut().map(|e| e.state().decks[d]);
            let Some(s) = state else {
                if matches!(action, DeckAction::Load) {
                    load_selected(app, d);
                }
                return;
            };
            let cmd = match action {
                DeckAction::PlayPause => Some(DeckCommand::TogglePlay),
                DeckAction::CuePress => Some(DeckCommand::Cue(true)),
                DeckAction::CueRelease => Some(DeckCommand::Cue(false)),
                DeckAction::JumpToStart => Some(DeckCommand::Seek { seconds: 0.0 }),
                DeckAction::Sync => Some(DeckCommand::SetSync(!s.sync)),
                DeckAction::SyncOff => Some(DeckCommand::SetSync(false)),
                DeckAction::SetMaster => {
                    send(app, Command::SetMasterDeck(Some(d)));
                    None
                }
                DeckAction::KeyLock => Some(DeckCommand::SetKeylock(!s.keylock)),
                DeckAction::KeyReset => Some(DeckCommand::SetKeyShift(0.0)),
                DeckAction::Tempo(v) => Some(DeckCommand::SetTempoFader(v)),
                DeckAction::TempoRange => Some(DeckCommand::SetTempoRange(match s.tempo_range {
                    TempoRange::Six => TempoRange::Ten,
                    TempoRange::Ten => TempoRange::Sixteen,
                    TempoRange::Sixteen => TempoRange::Wide,
                    TempoRange::Wide => TempoRange::Six,
                })),
                DeckAction::TempoReset => Some(DeckCommand::SetTempoFader(0.0)),
                DeckAction::Quantize => {
                    let q = app.perform.borrow_mut().engine.as_mut().map(|e| e.state().quantize).unwrap_or(true);
                    send(app, Command::SetQuantize(!q));
                    None
                }
                DeckAction::JogTouch(t) => Some(DeckCommand::JogTouch(t)),
                DeckAction::JogTurn { ticks, mode } => {
                    let jm = match mode {
                        od_controller::JogMode::Scratch => JogMode::Vinyl,
                        _ => JogMode::Bend,
                    };
                    if jm != s.jog_mode {
                        deck_cmd(app, d, DeckCommand::SetJogMode(jm));
                    }
                    let scale = if matches!(mode, od_controller::JogMode::Search) { 8.0 } else { 1.0 };
                    Some(DeckCommand::JogTurn(ticks as f32 / 720.0 * scale))
                }
                DeckAction::HotCue { slot, pressed } => Some(DeckCommand::HotCue { slot: slot as usize, pressed }),
                DeckAction::HotCueDelete { slot } => Some(DeckCommand::DeleteHotCue { slot: slot as usize }),
                DeckAction::MemoryCuePrev => Some(DeckCommand::MemoryCue(MemoryCueJump::Previous)),
                DeckAction::MemoryCueNext => Some(DeckCommand::MemoryCue(MemoryCueJump::Next)),
                DeckAction::LoopIn => Some(DeckCommand::LoopIn),
                DeckAction::LoopOut => Some(DeckCommand::LoopOut),
                DeckAction::AutoLoop { beats } => Some(DeckCommand::AutoLoop { beats: beats as f64 }),
                DeckAction::LoopRoll { beats, pressed } => Some(DeckCommand::LoopRoll { beats: beats as f64, on: pressed }),
                DeckAction::LoopHalve => Some(DeckCommand::LoopHalve),
                DeckAction::LoopDouble => Some(DeckCommand::LoopDouble),
                DeckAction::ReloopExit => Some(if s.loop_state.active { DeckCommand::LoopExit } else { DeckCommand::Reloop }),
                DeckAction::BeatJump { beats } => Some(DeckCommand::BeatJump { beats: beats as f64 }),
                DeckAction::Slip => Some(DeckCommand::SetSlip(!s.slip)),
                DeckAction::Reverse => Some(DeckCommand::SetReverse(!s.reverse)),
                DeckAction::Censor(on) => {
                    deck_cmd(app, d, DeckCommand::SetSlip(on || s.slip));
                    Some(DeckCommand::SetReverse(on))
                }
                DeckAction::VinylMode => Some(DeckCommand::SetJogMode(if s.jog_mode == JogMode::Vinyl { JogMode::Bend } else { JogMode::Vinyl })),
                DeckAction::NeedleSearch(f) => Some(DeckCommand::NeedleSearch { fraction: f as f64 }),
                DeckAction::Shift(on) => {
                    app.perform.borrow_mut().shift[d] = on;
                    None
                }
                DeckAction::PadMode(m) => {
                    app.perform.borrow_mut().pad_mode[d] = m;
                    None
                }
                DeckAction::Pad { index, pressed, .. } => {
                    let mode = app.perform.borrow().pad_mode[d];
                    match mode {
                        PadMode::HotCue => Some(DeckCommand::HotCue { slot: index as usize, pressed }),
                        PadMode::BeatLoop if pressed => Some(DeckCommand::AutoLoop { beats: pad_beats(index) }),
                        PadMode::BeatJump if pressed => {
                            let b = [1.0, 2.0, 4.0, 8.0][(index % 4) as usize];
                            Some(DeckCommand::BeatJump { beats: if index < 4 { -b } else { b } })
                        }
                        PadMode::Sampler if pressed => {
                            send(app, Command::Sampler(SamplerCommand::Trigger { slot: index as usize }));
                            None
                        }
                        _ => None,
                    }
                }
                DeckAction::Load => {
                    load_selected(app, d);
                    None
                }
                _ => None,
            };
            if let Some(c) = cmd {
                deck_cmd(app, d, c);
            }
        }
        ControlAction::Mixer { channel, action } => {
            let c = (channel as usize).saturating_sub(1).min(DECKS - 1);
            let cmd = match action {
                MixerAction::Trim(v) => {
                    let x = v * 2.0 - 1.0;
                    ChannelCommand::SetTrim(if x < 0.0 { x * 24.0 } else { x * 12.0 })
                }
                MixerAction::EqHigh(v) => ChannelCommand::SetEq { band: 2, value: v * 2.0 - 1.0 },
                MixerAction::EqMid(v) => ChannelCommand::SetEq { band: 1, value: v * 2.0 - 1.0 },
                MixerAction::EqLow(v) => ChannelCommand::SetEq { band: 0, value: v * 2.0 - 1.0 },
                MixerAction::ColorFx(v) => ChannelCommand::SetColorFxAmount(v),
                MixerAction::Fader(v) => ChannelCommand::SetFader(v),
                MixerAction::CueToggle => {
                    let on = app.perform.borrow_mut().engine.as_mut().map(|e| e.state().channels[c].cue).unwrap_or(false);
                    ChannelCommand::SetCue(!on)
                }
                MixerAction::CrossfaderAssign(side) => ChannelCommand::SetCrossfaderAssign(match side {
                    CrossfaderSide::A => XfAssign::A,
                    CrossfaderSide::B => XfAssign::B,
                    CrossfaderSide::Thru => XfAssign::Thru,
                }),
            };
            send(app, Command::Channel(c, cmd));
        }
        ControlAction::Crossfader(v) => send(app, Command::Mixer(MixerCommand::SetCrossfader(v * 2.0 - 1.0))),
        ControlAction::MasterLevel(v) => send(app, Command::Mixer(MixerCommand::SetMasterLevel(v))),
        ControlAction::HeadphoneMix(v) => send(app, Command::Mixer(MixerCommand::SetHeadphoneMix(v))),
        ControlAction::HeadphoneLevel(v) => send(app, Command::Mixer(MixerCommand::SetHeadphoneLevel(v))),
        ControlAction::Browse(b) => match b {
            BrowseAction::Rotate { delta, .. } => {
                let (len, cur) = {
                    let st = app.state.borrow();
                    (app.ui.get_tracks().iter_len(), st.current)
                };
                let _ = (len, cur);
                let key = if delta < 0 { slint::platform::Key::UpArrow } else { slint::platform::Key::DownArrow };
                for _ in 0..delta.unsigned_abs().min(20) {
                    app.ui.invoke_key_pressed(slint::SharedString::from(key), false, false);
                }
            }
            BrowseAction::Press { .. } => load_selected_into_free_deck(app),
            BrowseAction::Back => {
                if app.state.borrow().config.kiosk {
                    app.cycle_list(true);
                } else {
                    let next = if app.ui.get_mode() == "prepare" { "perform" } else { "prepare" };
                    app.ui.set_mode(next.into());
                }
            }
            _ => {}
        },
        ControlAction::RecordingToggle => toggle_recording(app),
        ControlAction::Sampler(od_controller::SamplerAction::Pad { slot, pressed: true, .. }) => {
            send(app, Command::Sampler(SamplerCommand::Trigger { slot: slot as usize }))
        }
        _ => {}
    }
}

trait IterLen {
    fn iter_len(&self) -> usize;
}

impl<T: Clone + 'static> IterLen for ModelRc<T> {
    fn iter_len(&self) -> usize {
        crate::app::model_len(self)
    }
}

fn load_selected(app: &App, deck: usize) {
    if let Some(t) = app.state.borrow().current {
        load_track(app, deck, t, None);
    }
}

/// LED feedback from the engine state.
fn feedback(app: &App) {
    let mut p = app.perform.borrow_mut();
    let Some(state) = p.engine.as_mut().map(|e| *e.state()) else { return };
    let Some(ctrl) = p.controllers.as_ref() else { return };
    for d in 0..DECKS {
        let s = &state.decks[d];
        let deck = d as u8 + 1;
        let led = |el: DeckElement, on: bool| ControlFeedback::led(FeedbackTarget::Deck { deck, element: el }, on);
        let blink_cue = !s.playing && s.track.is_some();
        ctrl.send_feedback(None, led(DeckElement::Play, s.playing));
        ctrl.send_feedback(
            None,
            ControlFeedback::led_state(
                FeedbackTarget::Deck { deck, element: DeckElement::Cue },
                if blink_cue { od_controller::LedState::Blink } else { od_controller::LedState::from_bool(s.playing) },
            ),
        );
        ctrl.send_feedback(None, led(DeckElement::Sync, s.sync));
        ctrl.send_feedback(None, led(DeckElement::Master, s.is_master));
        ctrl.send_feedback(None, led(DeckElement::KeyLock, s.keylock));
        ctrl.send_feedback(None, led(DeckElement::Slip, s.slip));
        ctrl.send_feedback(None, led(DeckElement::LoopActive, s.loop_state.active));
        ctrl.send_feedback(None, led(DeckElement::Loaded, s.track.is_some()));
        for slot in 0..8u8 {
            ctrl.send_feedback(
                None,
                led(DeckElement::Pad { mode: PadMode::HotCue, index: slot }, s.hot_cues[slot as usize].is_some()),
            );
        }
        ctrl.send_feedback(
            None,
            ControlFeedback::level(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Vu }, state.channels[d].meter.peak.min(1.0)),
        );
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Cue }, state.channels[d].cue));
    }
    let recording = p.recorder.as_ref().is_some_and(|r| r.is_recording());
    ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Recording, recording));
}

fn frame(app: &App) {
    // Prepared tracks → engine.
    let prepared: Vec<Prepared> = app.perform.borrow().prepared_rx.try_iter().collect();
    for pr in prepared {
        let (deck, play_from) = (pr.deck, pr.play_from);
        let meta = app.perform.borrow_mut().pending[deck].take().filter(|m| m.id == pr.data.id);
        let Some(meta) = meta else { continue };
        let title = meta.title.clone();
        app.perform.borrow_mut().loaded[deck] = Some(meta);
        deck_cmd(app, deck, DeckCommand::Load(pr.data));
        if let Some(t) = play_from {
            deck_cmd(app, deck, DeckCommand::Seek { seconds: t });
            deck_cmd(app, deck, DeckCommand::Play);
        }
        let msg = app.perform.borrow().message.clone();
        app.ui.set_engine_message(msg.into());
        say(app, format!("Loaded {title} on deck {}", deck + 1));
    }
    // Controller events.
    let events: Vec<od_controller::io::ControllerEvent> =
        app.perform.borrow().controllers.as_ref().map(|c| c.events().try_iter().collect()).unwrap_or_default();
    for e in events {
        match e {
            od_controller::io::ControllerEvent::Action { action, .. } => {
                ensure_engine(app);
                handle_action(app, action);
            }
            od_controller::io::ControllerEvent::Connected { device, .. } => {
                say(app, format!("Controller connected: {device}"));
                if device.contains("FLX10") || device.contains("flx10") {
                    app.perform.borrow_mut().deck_count = 4;
                }
                app.perform.borrow_mut().controller_name = Some(device);
            }
            od_controller::io::ControllerEvent::Disconnected { device, .. } => {
                say(app, format!("Controller disconnected: {device}"));
                app.perform.borrow_mut().controller_name = None;
            }
            od_controller::io::ControllerEvent::Error(e) => tracing::warn!(error = %e, "controller"),
        }
    }

    let Some(state) = app.perform.borrow_mut().engine.as_mut().map(|e| {
        e.collect_garbage();
        *e.state()
    }) else {
        return;
    };

    // Prepare preview: the cursor follows the preview deck.
    if app.perform.borrow().previewing {
        let d = &state.decks[PREVIEW_DECK];
        let current = app.state.borrow().current;
        if d.track == current && d.track.is_some() {
            app.state.borrow_mut().cursor = d.position;
            if d.duration > 0.0 {
                app.ui.set_cursor((d.position / d.duration) as f32);
            }
        }
        if d.at_end {
            app.perform.borrow_mut().previewing = false;
            app.ui.set_previewing(false);
        }
    }

    // Tracklist for recordings.
    let recording = app.perform.borrow().recorder.as_ref().is_some_and(|r| r.is_recording());
    if recording {
        let dom = state.audible.dominant;
        if dom != app.perform.borrow().last_dominant {
            app.perform.borrow_mut().last_dominant = dom;
            if let Some(d) = dom {
                let meta = app.perform.borrow().loaded[d].clone();
                if let (Some(m), Some(r)) = (meta, app.perform.borrow_mut().recorder.as_mut()) {
                    r.mark_track(Some(m.id), &m.title, &m.artist);
                    if !app.state.borrow().read_only() {
                        let _ = app.state.borrow_mut().lib.record_play(m.id, "recording", "opendecks");
                    }
                }
            }
        }
        let elapsed = app.perform.borrow().recorder.as_ref().map(|r| r.elapsed()).unwrap_or(0.0);
        app.ui.set_recording_time(fmt_time(elapsed).into());
    }
    app.ui.set_recording(recording);

    if app.ui.get_mode() != "perform" {
        return;
    }
    let scale = app.ui.window().scale_factor();
    let count = app.perform.borrow().deck_count;
    let width = ((app.ui.window().size().width as f32 / scale - 120.0).max(400.0) * scale) as u32;
    let height = ((if count > 2 { 64.0 } else { 120.0 }) * scale) as u32;
    let p = app.perform.borrow();
    let decks: Vec<DeckView> = (0..count)
        .map(|d| {
            let s = &state.decks[d];
            let meta = p.loaded[d].as_ref().filter(|m| Some(m.id) == s.track);
            let (waveform, overview) = match meta {
                Some(m) => (render::detail(&m.waveform, &m.grid, s.position, DETAIL_SPAN * s.tempo.max(0.25), width, height), m.overview.clone()),
                None => (Image::default(), Image::default()),
            };
            DeckView {
                deck: d as i32 + 1,
                loaded: meta.is_some(),
                title: meta.map(|m| m.title.clone()).unwrap_or_default().into(),
                artist: meta.map(|m| m.artist.clone()).unwrap_or_default().into(),
                bpm: if s.bpm > 0.0 { format!("{:.2}", s.bpm) } else { String::new() }.into(),
                key: meta.map(|m| m.key.clone()).unwrap_or_default().into(),
                tempo: format!("{:+.1}%", (s.tempo - 1.0) * 100.0).into(),
                elapsed: fmt_time(s.position).into(),
                remaining: if s.duration > 0.0 { format!("-{}", fmt_time(s.duration - s.position)) } else { String::new() }.into(),
                progress: if s.duration > 0.0 { (s.position / s.duration) as f32 } else { 0.0 },
                playing: s.playing,
                master: s.is_master,
                sync: s.sync,
                keylock: s.keylock,
                looping: s.loop_state.active,
                loop_beats: s.loop_state.beats.map(|b| if b < 1.0 { format!("1/{}", (1.0 / b).round()) } else { format!("{b}") }).unwrap_or_default().into(),
                slip: s.slip,
                level: state.channels[d].meter.peak,
                fader: state.channels[d].fader,
                waveform,
                overview,
                cues: ModelRc::new(VecModel::from(meta.map(|m| m.cues.clone()).unwrap_or_default())),
            }
        })
        .collect();
    drop(p);
    app.ui.set_decks(ModelRc::new(VecModel::from(decks)));
    app.ui.set_master_bpm(format!("{:.1}", state.master_bpm).into());
    app.ui.set_crossfader((state.crossfader + 1.0) / 2.0);
    app.ui.set_engine_ready(true);
    if let Some(h) = app.perform.borrow().host.as_ref() {
        let s = h.stats();
        if s.xruns > 0 || s.overruns > 0 {
            app.ui.set_engine_message(format!("Audio dropouts: {}", s.xruns + s.overruns).into());
        }
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    ensure_controllers(app);
    let ui = &app.ui;
    let w = Rc::downgrade(app);
    ui.on_deck_toggle_play(move |d| {
        if let Some(app) = w.upgrade() {
            deck_cmd(&app, d as usize - 1, DeckCommand::TogglePlay);
        }
    });
    let w = Rc::downgrade(app);
    ui.on_deck_cue(move |d| {
        if let Some(app) = w.upgrade() {
            deck_cmd(&app, d as usize - 1, DeckCommand::Cue(true));
            deck_cmd(&app, d as usize - 1, DeckCommand::Cue(false));
        }
    });
    let w = Rc::downgrade(app);
    ui.on_deck_sync(move |d| {
        if let Some(app) = w.upgrade() {
            let s = app.perform.borrow_mut().engine.as_mut().map(|e| e.state().decks[d as usize - 1].sync).unwrap_or(false);
            deck_cmd(&app, d as usize - 1, DeckCommand::SetSync(!s));
        }
    });
    let w = Rc::downgrade(app);
    ui.on_deck_seek(move |d, f| {
        if let Some(app) = w.upgrade() {
            deck_cmd(&app, d as usize - 1, DeckCommand::NeedleSearch { fraction: f as f64 });
        }
    });
    let w = Rc::downgrade(app);
    ui.on_deck_hot_cue(move |d, s| {
        if let Some(app) = w.upgrade() {
            deck_cmd(&app, d as usize - 1, DeckCommand::HotCue { slot: s as usize, pressed: true });
            deck_cmd(&app, d as usize - 1, DeckCommand::HotCue { slot: s as usize, pressed: false });
        }
    });
    let w = Rc::downgrade(app);
    ui.on_toggle_recording(move || {
        if let Some(app) = w.upgrade() {
            toggle_recording(&app);
        }
    });
    let w = Rc::downgrade(app);
    ui.on_set_deck_count(move |n| {
        if let Some(app) = w.upgrade() {
            app.perform.borrow_mut().deck_count = (n as usize).clamp(2, 4);
        }
    });

    let lite = app.state.borrow().config.lite;
    let period = Duration::from_millis(if lite { 33 } else { 16 });
    let w = Rc::downgrade(app);
    app.perform.borrow().timer.start(slint::TimerMode::Repeated, period, move || {
        let Some(app) = w.upgrade() else { return };
        frame(&app);
        let due = app.perform.borrow().last_frame.elapsed() >= Duration::from_millis(40);
        if due {
            app.perform.borrow_mut().last_frame = Instant::now();
            feedback(&app);
        }
    });
    let msg = app.perform.borrow().message.clone();
    ui.set_engine_message(msg.into());

    // OPENDECKS_AUTOLOAD="1:12,2:13" loads tracks on start (standalone
    // setups and testing).
    if let Ok(spec) = std::env::var("OPENDECKS_AUTOLOAD") {
        for part in spec.split(',') {
            if let Some((d, t)) = part.split_once(':') {
                if let (Ok(d), Ok(t)) = (d.trim().parse::<usize>(), t.trim().parse::<i64>()) {
                    if (1..=DECKS).contains(&d) {
                        load_track(app, d - 1, TrackId(t), None);
                    }
                }
            }
        }
    }
}
