//! Controllers (MIDI): mapped actions → engine commands, and LED feedback.

use crate::app::App;
use crate::{browser, decks, engine, recorder};
use od_controller::io::{ControllerEvent, ControllerManager};
use od_controller::{
    BrowseAction, ControlAction, ControlFeedback, CrossfaderSide, DeckAction, DeckElement, FeedbackTarget, MixerAction, MixerElement, PadMode,
};
use od_engine::{ChannelCommand, Command, DeckCommand, JogMode, MemoryCueJump, MixerCommand, SamplerCommand, TempoRange, XfAssign};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Controllers {
    manager: Option<ControllerManager>,
    /// Name of the connected controller.
    pub name: Option<String>,
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

/// Names of the built-in mapping files (Preferences > Controller).
pub(crate) fn mapping_names() -> Vec<String> {
    let mut v: Vec<String> = od_controller::Mapping::load_dir(&mappings_dir())
        .into_iter()
        .map(|(path, m)| {
            let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            match m {
                Ok(m) => format!("{} — {file}", m.file.device.name),
                Err(e) => format!("{file} — {e}"),
            }
        })
        .collect();
    v.sort();
    v
}

/// Starts the controller layer (MIDI) once.
pub(crate) fn start(app: &Rc<App>) {
    let mut c = app.controller.borrow_mut();
    if c.manager.is_some() {
        return;
    }
    let own = app.state.borrow().settings.controller.mapping_file.clone();
    let mut files = od_controller::Mapping::load_dir(&mappings_dir());
    if !own.is_empty() {
        // The user's file first, so it wins for its device.
        let p = PathBuf::from(&own);
        files.insert(0, (p.clone(), od_controller::Mapping::load(&p)));
    }
    let mappings: Vec<Arc<od_controller::Mapping>> = files
        .into_iter()
        .filter_map(|(path, m)| match m {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "mapping failed to load");
                None
            }
        })
        .collect();
    c.manager = Some(ControllerManager::start(mappings, Default::default()));
}

/// Frame tick: controller events.
pub(crate) fn poll(app: &App) {
    let events: Vec<ControllerEvent> = app.controller.borrow().manager.as_ref().map(|c| c.events().try_iter().collect()).unwrap_or_default();
    for e in events {
        match e {
            ControllerEvent::Action { action, .. } => {
                engine::ensure_engine(app);
                handle_action(app, action);
            }
            ControllerEvent::Connected { device, .. } => {
                app.status(format!("Controller connected: {device}"));
                if device.contains("FLX10") || device.contains("flx10") {
                    app.decks.borrow_mut().deck_count = 4;
                }
                app.controller.borrow_mut().name = Some(device);
            }
            ControllerEvent::Disconnected { device, .. } => {
                app.status(format!("Controller disconnected: {device}"));
                app.controller.borrow_mut().name = None;
            }
            ControllerEvent::Error(e) => tracing::warn!(error = %e, "controller"),
        }
    }
}

/// Beat lengths of the beat-loop pads.
pub(crate) fn pad_beats(index: u8) -> f64 {
    [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0][index.min(7) as usize]
}

/// Beat jump of a pad: top row backwards, bottom row forwards.
pub(crate) fn pad_jump(index: u8) -> f64 {
    let b = [1.0, 2.0, 4.0, 8.0][(index % 4) as usize];
    if index < 4 { -b } else { b }
}

pub(crate) fn next_tempo_range(r: TempoRange) -> TempoRange {
    match r {
        TempoRange::Six => TempoRange::Ten,
        TempoRange::Ten => TempoRange::Sixteen,
        TempoRange::Sixteen => TempoRange::Wide,
        TempoRange::Wide => TempoRange::Six,
    }
}

/// Trim knob (0..1) → dB (−24..+12).
fn trim_db(v: f32) -> f32 {
    crate::mixer::knob_to_trim(v * 2.0 - 1.0)
}

fn handle_action(app: &App, action: ControlAction) {
    match action {
        ControlAction::Deck { deck, action } => deck_action(app, (deck as usize).saturating_sub(1).min(decks::DECKS - 1), action),
        ControlAction::Mixer { channel, action } => {
            let c = (channel as usize).saturating_sub(1).min(decks::DECKS - 1);
            let cmd = match action {
                MixerAction::Trim(v) => ChannelCommand::SetTrim(trim_db(v)),
                MixerAction::EqHigh(v) => ChannelCommand::SetEq { band: 2, value: v * 2.0 - 1.0 },
                MixerAction::EqMid(v) => ChannelCommand::SetEq { band: 1, value: v * 2.0 - 1.0 },
                MixerAction::EqLow(v) => ChannelCommand::SetEq { band: 0, value: v * 2.0 - 1.0 },
                MixerAction::ColorFx(v) => ChannelCommand::SetColorFxAmount(v),
                MixerAction::Fader(v) => ChannelCommand::SetFader(v),
                MixerAction::CueToggle => {
                    let on = engine::snapshot(app).map(|s| s.channels[c].cue).unwrap_or(false);
                    ChannelCommand::SetCue(!on)
                }
                MixerAction::CrossfaderAssign(side) => ChannelCommand::SetCrossfaderAssign(match side {
                    CrossfaderSide::A => XfAssign::A,
                    CrossfaderSide::B => XfAssign::B,
                    CrossfaderSide::Thru => XfAssign::Thru,
                }),
            };
            engine::send(app, Command::Channel(c, cmd));
        }
        ControlAction::Crossfader(v) => engine::send(app, Command::Mixer(MixerCommand::SetCrossfader(v * 2.0 - 1.0))),
        ControlAction::MasterLevel(v) => engine::send(app, Command::Mixer(MixerCommand::SetMasterLevel(v))),
        ControlAction::HeadphoneMix(v) => engine::send(app, Command::Mixer(MixerCommand::SetHeadphoneMix(v))),
        ControlAction::HeadphoneLevel(v) => engine::send(app, Command::Mixer(MixerCommand::SetHeadphoneLevel(v))),
        ControlAction::Browse(b) => match b {
            BrowseAction::Rotate { delta, .. } => {
                app.ui.set_browser_open(true);
                for _ in 0..delta.unsigned_abs().min(20) {
                    browser::step_selection(app, delta < 0);
                }
            }
            BrowseAction::Press { .. } => {
                if !crate::explorer::load_selection(app, None) {
                    decks::load_selected_into_free_deck(app);
                }
                if app.state.borrow().config.kiosk {
                    app.ui.set_browser_open(false);
                }
            }
            BrowseAction::Back => {
                if app.state.borrow().config.kiosk {
                    browser::cycle_list(app, true);
                } else {
                    let next = if app.ui.get_mode() == "prepare" { "perform" } else { "prepare" };
                    app.ui.set_mode(next.into());
                }
            }
            _ => {}
        },
        ControlAction::RecordingToggle => recorder::toggle_from_controller(app),
        ControlAction::Sampler(od_controller::SamplerAction::Pad { slot, pressed: true, .. }) => {
            engine::send(app, Command::Sampler(SamplerCommand::Trigger { slot: slot as usize }))
        }
        _ => {}
    }
}

fn deck_action(app: &App, d: usize, action: DeckAction) {
    let Some(s) = engine::snapshot(app).map(|st| st.decks[d]) else {
        if matches!(action, DeckAction::Load) {
            decks::load_selected(app, d);
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
            engine::send(app, Command::SetMasterDeck(Some(d)));
            None
        }
        DeckAction::KeyLock => Some(DeckCommand::SetKeylock(!s.keylock)),
        DeckAction::KeyReset => Some(DeckCommand::SetKeyShift(0.0)),
        DeckAction::Tempo(v) => Some(DeckCommand::SetTempoFader(v)),
        DeckAction::TempoRange => Some(DeckCommand::SetTempoRange(next_tempo_range(s.tempo_range))),
        DeckAction::TempoReset => Some(DeckCommand::SetTempoFader(0.0)),
        DeckAction::Quantize => {
            let q = engine::snapshot(app).map(|s| s.quantize).unwrap_or(true);
            engine::send(app, Command::SetQuantize(!q));
            None
        }
        DeckAction::JogTouch(t) => Some(DeckCommand::JogTouch(t)),
        DeckAction::JogTurn { ticks, mode } => {
            let jm = match mode {
                od_controller::JogMode::Scratch => JogMode::Vinyl,
                _ => JogMode::Bend,
            };
            if jm != s.jog_mode {
                engine::deck_cmd(app, d, DeckCommand::SetJogMode(jm));
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
            engine::deck_cmd(app, d, DeckCommand::SetSlip(on || s.slip));
            Some(DeckCommand::SetReverse(on))
        }
        DeckAction::VinylMode => Some(DeckCommand::SetJogMode(if s.jog_mode == JogMode::Vinyl { JogMode::Bend } else { JogMode::Vinyl })),
        DeckAction::NeedleSearch(f) => Some(DeckCommand::NeedleSearch { fraction: f as f64 }),
        DeckAction::Shift(on) => {
            app.decks.borrow_mut().shift[d] = on;
            None
        }
        DeckAction::PadMode(m) => {
            app.decks.borrow_mut().pad_mode[d] = m;
            None
        }
        DeckAction::Pad { index, pressed, .. } => {
            let mode = app.decks.borrow().pad_mode[d];
            match mode {
                PadMode::HotCue => Some(DeckCommand::HotCue { slot: index as usize, pressed }),
                PadMode::BeatLoop if pressed => Some(DeckCommand::AutoLoop { beats: pad_beats(index) }),
                PadMode::BeatJump if pressed => Some(DeckCommand::BeatJump { beats: pad_jump(index) }),
                PadMode::Sampler if pressed => {
                    engine::send(app, Command::Sampler(SamplerCommand::Trigger { slot: index as usize }));
                    None
                }
                _ => None,
            }
        }
        DeckAction::Load => {
            decks::load_selected(app, d);
            None
        }
        _ => None,
    };
    if let Some(c) = cmd {
        engine::deck_cmd(app, d, c);
    }
}

/// LED feedback from the engine state (every 40 ms).
pub(crate) fn feedback(app: &App) {
    let Some(state) = engine::snapshot(app) else { return };
    let recording = app.recorder.borrow().is_recording();
    let c = app.controller.borrow();
    let Some(ctrl) = c.manager.as_ref() else { return };
    for d in 0..decks::DECKS {
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
            ctrl.send_feedback(None, led(DeckElement::Pad { mode: PadMode::HotCue, index: slot }, s.hot_cues[slot as usize].is_some()));
        }
        ctrl.send_feedback(None, ControlFeedback::level(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Vu }, state.channels[d].meter.peak.min(1.0)));
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Cue }, state.channels[d].cue));
    }
    ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Recording, recording));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads() {
        assert_eq!(pad_beats(0), 0.25);
        assert_eq!(pad_beats(9), 32.0);
        assert_eq!(pad_jump(0), -1.0);
        assert_eq!(pad_jump(7), 8.0);
        assert_eq!(next_tempo_range(TempoRange::Wide), TempoRange::Six);
    }

    #[test]
    fn controller_trim_spans_the_knob() {
        assert_eq!(trim_db(0.0), -24.0);
        assert_eq!(trim_db(0.5), 0.0);
        assert_eq!(trim_db(1.0), 12.0);
    }
}
