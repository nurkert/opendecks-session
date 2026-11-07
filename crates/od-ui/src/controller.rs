//! Controllers (MIDI): mapped actions → engine commands, and LED feedback.

use crate::app::App;
use crate::{browser, decks, engine, fx, recorder, sampler};
use od_controller::io::{ControllerEvent, ControllerManager};
use od_controller::{
    BeatFxAction, ColorFxAction, ControlAction, ControlFeedback, CrossfaderSide, DeckAction, DeckElement, FeedbackTarget, LedState, MixerAction,
    MixerElement, PadMode, SamplerAction, StemPart,
};
use od_engine::{
    BEAT_FX_UNITS, BeatFxCommand, ChannelCommand, Command, DeckCommand, JogMode, MemoryCueJump, MixerCommand, ReleaseFxKind, SamplerCommand, Stem, TempoRange,
    XfAssign,
};
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

/// The controller layer's MIDI monitor (Preferences ▸ Controller: monitor
/// and learn), once the layer runs.
pub(crate) fn monitor(app: &App) -> Option<od_controller::monitor::MidiMonitor> {
    app.controller.borrow().manager.as_ref().map(|m| m.monitor().clone())
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
        ControlAction::Browse(b) => browser::navigate::controller(app, &b),
        ControlAction::RecordingToggle => recorder::toggle_from_controller(app),
        ControlAction::BoothLevel(v) => engine::send(app, Command::Mixer(MixerCommand::SetBoothLevel(v))),
        ControlAction::MasterCue => {
            let on = engine::snapshot(app).map(|s| s.master_cue).unwrap_or(false);
            engine::send(app, Command::Mixer(MixerCommand::SetMasterCue(!on)));
        }
        ControlAction::BeatFx { unit, action } => beat_fx_action(app, (unit as usize).saturating_sub(1).min(BEAT_FX_UNITS - 1), action),
        ControlAction::ColorFx(a) => match a {
            ColorFxAction::Select(name) => match color_fx_by_name(&name) {
                Some(i) => fx::select_color_fx(app, i),
                None => tracing::warn!(name, "unknown colour FX in mapping"),
            },
            // One button cycles through the effects (FLX4).
            ColorFxAction::Next | ColorFxAction::Toggle => fx::step_color_fx(app, 1),
        },
        ControlAction::Sampler(a) => sampler_action(app, a),
        ControlAction::Ignored => {}
    }
}

/// Colour FX index (settings order) of a mapping name.
pub(crate) fn color_fx_by_name(name: &str) -> Option<usize> {
    crate::settings::COLOR_FX.iter().position(|n| *n == name)
}

/// A Beat FX unit's controls (FX section of the controller).
fn beat_fx_action(app: &App, u: usize, action: BeatFxAction) {
    match action {
        BeatFxAction::Toggle => fx::toggle_on(app, u),
        // The momentary release button: echo out, as on the hardware.
        BeatFxAction::Release(on) => fx::release(app, u, ReleaseFxKind::EchoOut, on),
        BeatFxAction::Level(v) => fx::send(app, u, BeatFxCommand::SetLevel(v)),
        BeatFxAction::BeatDown => fx::step_unit_beats(app, u, -1),
        BeatFxAction::BeatUp => fx::step_unit_beats(app, u, 1),
        BeatFxAction::AutoBeat => fx::auto_beat(app, u),
        BeatFxAction::Tap => fx::tap(app, u),
        BeatFxAction::SelectNext => fx::step_unit_kind(app, u, 1),
        BeatFxAction::SelectPrev => fx::step_unit_kind(app, u, -1),
        BeatFxAction::Select(name) => match fx::kind_from_name(&name) {
            Some(k) => fx::send(app, u, BeatFxCommand::SetKind(k)),
            None => tracing::warn!(name, "unknown Beat FX in mapping"),
        },
        BeatFxAction::Assign(ch) => fx::send(app, u, BeatFxCommand::SetTarget(fx::target_from_number(ch as i32))),
    }
}

/// Sampler section: pads of the shown bank, banks, volume, cue.
fn sampler_action(app: &App, a: SamplerAction) {
    let bank = app.sampler.bank.get();
    match a {
        SamplerAction::Pad { slot, pressed, .. } => sampler::trigger(app, sampler::slot_of(bank, slot as usize), pressed),
        SamplerAction::Stop { slot } => engine::send(app, Command::Sampler(SamplerCommand::Stop { slot: sampler::slot_of(bank, slot as usize) })),
        SamplerAction::BankNext => sampler::step_bank_by(app, 1),
        SamplerAction::BankPrev => sampler::step_bank_by(app, -1),
        SamplerAction::Volume(v) => sampler::set_volume(app, v),
        SamplerAction::CueToggle => sampler::toggle_cue(app),
    }
}

fn stem_of(p: StemPart) -> Stem {
    match p {
        StemPart::Vocals => Stem::Vocals,
        StemPart::Drums => Stem::Drums,
        StemPart::Bass => Stem::Bass,
        StemPart::Inst => Stem::Inst,
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
        DeckAction::KeySync => Some(DeckCommand::SetKeySync(!s.key_sync)),
        DeckAction::TapBpm => {
            decks::tap_bpm(app, d);
            None
        }
        DeckAction::MemoryCueSet => {
            decks::memory_cue(app, d, true);
            None
        }
        DeckAction::MemoryCueDelete => {
            decks::memory_cue(app, d, false);
            None
        }
        DeckAction::StemLevel { stem, level } => Some(DeckCommand::SetStemLevel { stem: stem_of(stem), level }),
        DeckAction::StemMuteToggle { stem } => {
            let st = stem_of(stem);
            Some(DeckCommand::SetStemLevel { stem: st, level: if s.stem_levels[st.index()] < 0.5 { 1.0 } else { 0.0 } })
        }
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
        DeckAction::Censor(on) => Some(DeckCommand::Censor(on)),
        DeckAction::VinylMode => Some(DeckCommand::SetJogMode(if s.jog_mode == JogMode::Vinyl { JogMode::Bend } else { JogMode::Vinyl })),
        DeckAction::NeedleSearch(f) => Some(DeckCommand::NeedleSearch { fraction: f as f64 }),
        DeckAction::Shift(on) => {
            app.decks.borrow_mut().shift[d] = on;
            None
        }
        DeckAction::PadMode(m) => {
            decks::set_pad_mode(app, d, m);
            None
        }
        // Every pad mode: hot cues, Pad FX, keyboard, key shift, slicer, …
        DeckAction::Pad { index, pressed, .. } => {
            decks::pad(app, d, index as usize, pressed);
            None
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
    if app.controller.borrow().manager.is_none() {
        return;
    }
    let color_fx = app.state.borrow().settings.perform.color_fx.clone();
    let modes = app.decks.borrow().pad_mode;
    let pad_rows: Vec<Vec<crate::PadView>> = (0..decks::DECKS).map(|d| decks::pad_rows(app, d, &state)).collect();
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
        ctrl.send_feedback(None, led(DeckElement::KeySync, s.key_sync));
        ctrl.send_feedback(None, led(DeckElement::Quantize, state.quantize));
        ctrl.send_feedback(None, led(DeckElement::Censor, s.censor));
        ctrl.send_feedback(None, led(DeckElement::Reverse, s.reverse));
        ctrl.send_feedback(None, led(DeckElement::Vinyl, s.jog_mode == JogMode::Vinyl));
        for slot in 0..8u8 {
            ctrl.send_feedback(None, led(DeckElement::Pad { mode: PadMode::HotCue, index: slot }, s.hot_cues[slot as usize].is_some()));
        }
        // The pad mode buttons, and the pads of the current mode.
        let mode = modes[d];
        for m in PadMode::ALL {
            ctrl.send_feedback(None, led(DeckElement::PadMode(m), m == mode));
        }
        if mode != PadMode::HotCue {
            for p in &pad_rows[d] {
                let target = FeedbackTarget::Deck { deck, element: DeckElement::Pad { mode, index: p.index as u8 } };
                ctrl.send_feedback(None, ControlFeedback::led_state(target, pad_led(p.set, p.lit)));
            }
        }
        ctrl.send_feedback(None, ControlFeedback::level(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Vu }, state.channels[d].meter.peak.min(1.0)));
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Mixer { channel: deck, element: MixerElement::Cue }, state.channels[d].cue));
    }
    for u in 0..BEAT_FX_UNITS {
        let unit = u as u8 + 1;
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::BeatFxOn { unit }, state.beat_fx[u].on));
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::BeatFxRelease { unit }, state.beat_fx[u].release.is_some()));
    }
    for name in crate::settings::COLOR_FX {
        ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::ColorFx(name.to_string()), name == color_fx));
    }
    ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::SamplerCue, state.sampler_cue));
    ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::MasterCue, state.master_cue));
    ctrl.send_feedback(None, ControlFeedback::led(FeedbackTarget::Recording, recording));
}

/// Pad LED of a non-hot-cue mode: on while engaged, dim when it has
/// something to play.
pub(crate) fn pad_led(set: bool, lit: bool) -> LedState {
    if lit {
        LedState::On
    } else if set {
        LedState::Dim
    } else {
        LedState::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pads;

    #[test]
    fn pads() {
        assert_eq!(pads::pad_beats(0), 0.25);
        assert_eq!(pads::pad_beats(9), 32.0);
        assert_eq!(pads::pad_jump(0), -1.0);
        assert_eq!(pads::pad_jump(7), 8.0);
        assert_eq!(next_tempo_range(TempoRange::Wide), TempoRange::Six);
    }

    #[test]
    fn mapping_names_and_leds() {
        // The colour FX names the FLX10 mapping sends.
        for n in ["space", "dub_echo", "sweep", "noise", "crush", "filter"] {
            assert!(color_fx_by_name(n).is_some(), "{n}");
        }
        assert_eq!(color_fx_by_name("laser"), None);
        // Every Beat FX name of the FLX10 mapping resolves to an effect.
        for n in [
            "low_cut_echo", "echo", "multi_tap_delay", "spiral", "reverb", "trans", "enigma_jet", "flanger", "phaser", "stretch", "slip_roll", "roll", "mobius_saw",
            "mobius_triangle",
        ] {
            assert!(fx::kind_from_name(n).is_some(), "{n}");
        }
        assert_eq!(pad_led(true, true), LedState::On);
        assert_eq!(pad_led(true, false), LedState::Dim);
        assert_eq!(pad_led(false, false), LedState::Off);
    }

    #[test]
    fn controller_trim_spans_the_knob() {
        assert_eq!(trim_db(0.0), -24.0);
        assert_eq!(trim_db(0.5), 0.0);
        assert_eq!(trim_db(1.0), 12.0);
    }
}
