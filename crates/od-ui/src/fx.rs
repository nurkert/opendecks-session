//! FX: the Beat FX panel (two units), Release FX, the Colour FX selection of
//! the mixer's CFX knobs and Pad FX presets (SPEC §10.2).
//!
//! The panel is a projection of `EngineState::beat_fx`; every control sends
//! a `Command::BeatFx`. The Colour FX selection is one global choice (as on
//! the mixer hardware), kept in the settings and applied to every channel.

use crate::app::{App, handler};
use crate::models::Rows;
use crate::settings::COLOR_FX;
use crate::{FxSlotView, FxUnitView, decks, engine};
use od_engine::{
    BEAT_FX_SLOTS, BEAT_FX_UNITS, BeatFxCommand, BeatFxKind, BeatFxState, ChannelCommand, ColorFxKind, Command, EngineState, FxTarget, PadFxEffect, PadFxPreset,
    ReleaseFxKind,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Beat lengths of the beat ‹ › buttons, as on the hardware.
pub(crate) const FX_BEATS: [f32; 9] = [0.125, 0.25, 0.5, 0.75, 1.0, 2.0, 4.0, 8.0, 16.0];

/// Release FX in panel order (BRAKE, BACKSPIN, ECHO OUT).
pub(crate) const RELEASE_FX: [ReleaseFxKind; 3] = [ReleaseFxKind::VinylBrake, ReleaseFxKind::Backspin, ReleaseFxKind::EchoOut];

/// Runtime part of the FX panel: the bound unit rows and tap tempo.
#[derive(Default)]
pub(crate) struct FxRuntime {
    pub units: Rows<FxUnitView>,
    /// Slots 2 and 3 per unit; bound once so rows keep their identity.
    slots: [Rows<FxSlotView>; BEAT_FX_UNITS],
    taps: RefCell<[TapTempo; BEAT_FX_UNITS]>,
}

/// Tap tempo: the average of the last taps; a pause of two seconds starts
/// over.
#[derive(Clone, Debug, Default)]
pub(crate) struct TapTempo {
    taps: Vec<f64>,
}

impl TapTempo {
    /// Registers a tap at `t` seconds; returns the tapped BPM once there are
    /// three taps (40–300 BPM).
    pub fn tap(&mut self, t: f64) -> Option<f32> {
        if self.taps.last().is_some_and(|&last| t - last > 2.0 || t <= last) {
            self.taps.clear();
        }
        self.taps.push(t);
        if self.taps.len() > 8 {
            self.taps.remove(0);
        }
        if self.taps.len() < 3 {
            return None;
        }
        let span = self.taps.last()? - self.taps.first()?;
        let bpm = 60.0 * (self.taps.len() - 1) as f64 / span;
        (40.0..=300.0).contains(&bpm).then_some(bpm as f32)
    }
}

/// "1/8", "3/4", "16" for a beat length.
pub(crate) fn beats_label(b: f32) -> String {
    if (b - 0.75).abs() < 1e-3 {
        "3/4".into()
    } else if b < 1.0 {
        format!("1/{}", (1.0 / b).round())
    } else {
        format!("{}", b.round())
    }
}

/// The next beat length from `current` (`delta` steps through [`FX_BEATS`]).
pub(crate) fn step_beats(current: f32, delta: i32) -> f32 {
    let i = FX_BEATS.iter().enumerate().min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs())).map(|(i, _)| i).unwrap_or(3);
    FX_BEATS[(i as i32 + delta).clamp(0, FX_BEATS.len() as i32 - 1) as usize]
}

/// The effect `delta` places from `kind`, wrapping around the list.
pub(crate) fn step_kind(kind: BeatFxKind, delta: i32) -> BeatFxKind {
    let n = BeatFxKind::ALL.len() as i32;
    BeatFxKind::ALL[(kind.index() as i32 + delta).rem_euclid(n) as usize]
}

/// Beat FX by a controller mapping name (`echo`, `low_cut_echo`, …). Names
/// of effects OpenDecks does not have map to the closest one.
pub(crate) fn kind_from_name(name: &str) -> Option<BeatFxKind> {
    Some(match name {
        "echo" | "low_cut_echo" => BeatFxKind::Echo,
        "delay" | "multi_tap_delay" => BeatFxKind::Delay,
        "ping_pong" => BeatFxKind::PingPong,
        "reverb" | "shimmer" => BeatFxKind::Reverb,
        "flanger" | "enigma_jet" => BeatFxKind::Flanger,
        "phaser" => BeatFxKind::Phaser,
        "roll" | "stretch" => BeatFxKind::Roll,
        "slip_roll" => BeatFxKind::SlipRoll,
        "filter" | "mobius_saw" | "mobius_triangle" => BeatFxKind::Filter,
        "trans" => BeatFxKind::Trans,
        "pitch" => BeatFxKind::Pitch,
        "vinyl_brake" => BeatFxKind::VinylBrake,
        "helix" => BeatFxKind::Helix,
        "spiral" => BeatFxKind::Spiral,
        _ => return None,
    })
}

/// Settings name → engine Colour FX (unknown names: filter).
pub(crate) fn color_fx_kind(name: &str) -> ColorFxKind {
    match name {
        "echo" => ColorFxKind::Echo,
        "noise" => ColorFxKind::Noise,
        "space" => ColorFxKind::Space,
        "crush" => ColorFxKind::Crush,
        "pitch" => ColorFxKind::Pitch,
        "dub_echo" => ColorFxKind::DubEcho,
        "sweep" => ColorFxKind::Sweep,
        _ => ColorFxKind::Filter,
    }
}

/// Index of a Colour FX name in [`COLOR_FX`] (the panel order).
pub(crate) fn color_fx_index(name: &str) -> usize {
    COLOR_FX.iter().position(|n| *n == name).unwrap_or(0)
}

/// Target → panel channel number (0 = master).
pub(crate) fn target_number(t: FxTarget) -> i32 {
    match t {
        FxTarget::Master => 0,
        FxTarget::Channel(c) => c as i32 + 1,
        FxTarget::Sampler => -1,
    }
}

/// Panel channel number → target (−1 = sampler).
pub(crate) fn target_from_number(n: i32) -> FxTarget {
    if n < 0 {
        FxTarget::Sampler
    } else if n == 0 {
        FxTarget::Master
    } else { FxTarget::Channel((n as usize - 1).min(decks::DECKS - 1)) }
}

/// What a unit shows before audio runs (the engine's start values).
pub(crate) fn idle_unit() -> BeatFxState {
    BeatFxState { level: 0.5, beats: 0.75, ..Default::default() }
}

/// Projects slots 2 and 3 of a unit (pure).
pub(crate) fn slot_views(slots: &[BeatFxState]) -> Vec<FxSlotView> {
    slots
        .iter()
        .enumerate()
        .map(|(k, s)| FxSlotView { slot: k as i32 + 2, kind: s.kind.index() as i32, beats: beats_label(s.beats).into(), level: s.level, on: s.on })
        .collect()
}

/// Projects one unit (pure).
pub(crate) fn unit_view(unit: usize, s: &BeatFxState) -> FxUnitView {
    FxUnitView {
        multi: false,
        slots: Default::default(),
        unit: unit as i32 + 1,
        kind: s.kind.index() as i32,
        beats: beats_label(s.beats).into(),
        target: target_number(s.target),
        level: s.level,
        param: s.param,
        on: s.on,
        ringing: s.ringing,
        bpm: if s.bpm > 0.0 { format!("{:.1}", s.bpm) } else { String::new() }.into(),
        tapped: s.tap_bpm.is_some(),
        release: s.release.and_then(|r| RELEASE_FX.iter().position(|k| *k == r)).map(|i| i as i32).unwrap_or(-1),
    }
}

/// Pad FX presets of the Pad FX pad modes (page 1 and 2), pads A–H.
pub(crate) fn pad_fx_preset(page: usize, pad: usize) -> PadFxPreset {
    use BeatFxKind as K;
    let beat = |kind: K, beats: f32, level: f32| PadFxPreset { effect: PadFxEffect::Beat(kind), beats, level, param: kind.default_param() };
    let release = |kind: ReleaseFxKind, beats: f32| PadFxPreset { effect: PadFxEffect::Release(kind), beats, level: 1.0, param: 0.0 };
    let presets = [
        [
            beat(K::Echo, 0.5, 0.6),
            beat(K::Echo, 0.75, 0.6),
            beat(K::Reverb, 4.0, 0.7),
            beat(K::Trans, 0.25, 1.0),
            beat(K::Roll, 0.25, 1.0),
            beat(K::Roll, 0.5, 1.0),
            beat(K::Filter, 4.0, 1.0),
            release(ReleaseFxKind::EchoOut, 1.0),
        ],
        [
            beat(K::PingPong, 0.5, 0.6),
            beat(K::Helix, 1.0, 0.6),
            beat(K::Spiral, 0.5, 0.6),
            beat(K::Flanger, 8.0, 0.8),
            beat(K::SlipRoll, 0.125, 1.0),
            beat(K::Pitch, 1.0, 1.0),
            release(ReleaseFxKind::VinylBrake, 1.0),
            release(ReleaseFxKind::Backspin, 1.0),
        ],
    ];
    presets[page.min(1)][pad.min(7)]
}

/// Short pad caption of a Pad FX preset ("ECHO 1/2", "BRAKE").
pub(crate) fn pad_fx_caption(p: &PadFxPreset) -> String {
    match p.effect {
        PadFxEffect::Beat(k) => format!("{} {}", kind_short(k), beats_label(p.beats)),
        PadFxEffect::Release(ReleaseFxKind::VinylBrake) => "BRAKE".into(),
        PadFxEffect::Release(ReleaseFxKind::Backspin) => "BACKSPIN".into(),
        PadFxEffect::Release(ReleaseFxKind::EchoOut) => "ECHO OUT".into(),
    }
}

fn kind_short(k: BeatFxKind) -> &'static str {
    use BeatFxKind as K;
    match k {
        K::Echo => "ECHO",
        K::Delay => "DELAY",
        K::PingPong => "P.PONG",
        K::Reverb => "REVERB",
        K::Flanger => "FLANGER",
        K::Phaser => "PHASER",
        K::Roll => "ROLL",
        K::SlipRoll => "S.ROLL",
        K::Filter => "FILTER",
        K::Trans => "TRANS",
        K::Pitch => "PITCH",
        K::VinylBrake => "BRAKE",
        K::Helix => "HELIX",
        K::Spiral => "SPIRAL",
    }
}

/// The commands that apply the Colour FX `name` to every channel whose
/// selection differs (empty when all match).
pub(crate) fn color_fx_commands(state: &EngineState, name: &str) -> Vec<Command> {
    let kind = color_fx_kind(name);
    (0..state.decks_count.min(decks::DECKS)).filter(|&c| state.channels[c].color_fx != kind).map(|c| Command::Channel(c, ChannelCommand::SetColorFx(kind))).collect()
}

/// Frame tick: engine state → unit rows; keeps the channels on the selected
/// Colour FX.
pub(crate) fn project(app: &App, state: &EngineState) {
    for u in 0..BEAT_FX_UNITS {
        app.fx.slots[u].sync_eq(slot_views(&state.beat_fx_slots[u]));
    }
    app.fx.units.sync_eq(
        (0..BEAT_FX_UNITS)
            .map(|u| FxUnitView { multi: state.beat_fx_multi[u], slots: app.fx.slots[u].handle(), ..unit_view(u, &state.beat_fx[u]) })
            .collect(),
    );
    let name = app.state.borrow().settings.perform.color_fx.clone();
    for cmd in color_fx_commands(state, &name) {
        engine::send(app, cmd);
    }
}

/// Before audio starts: the units at their start values.
pub(crate) fn show_idle(app: &App) {
    if app.fx.units.len() == 0 {
        app.fx.units.sync_eq((0..BEAT_FX_UNITS).map(|u| unit_view(u, &idle_unit())).collect());
    }
}

/// Opens or closes the Beat FX panel and remembers it.
pub(crate) fn toggle_panel(app: &App) {
    let on = !app.state.borrow().settings.perform.fx_panel;
    app.state.borrow_mut().change_settings(|s| s.perform.fx_panel = on);
    app.ui.set_fx_panel(on);
}

/// Current state of unit `u` (0-based), if audio runs.
fn unit_state(app: &App, u: usize) -> Option<BeatFxState> {
    engine::snapshot(app).map(|s| s.beat_fx[u.min(BEAT_FX_UNITS - 1)])
}

/// Sends a command to unit `u` (0-based), starting audio first.
pub(crate) fn send(app: &App, u: usize, cmd: BeatFxCommand) {
    engine::ensure_engine(app);
    engine::send(app, Command::BeatFx(u.min(BEAT_FX_UNITS - 1), cmd));
}

pub(crate) fn toggle_on(app: &App, u: usize) {
    let on = unit_state(app, u).map(|s| s.on).unwrap_or(false);
    send(app, u, BeatFxCommand::SetOn(!on));
}

/// Slot 2 or 3 of unit `u` (0-based).
fn slot_state(app: &App, u: usize, slot: usize) -> Option<BeatFxState> {
    engine::snapshot(app).and_then(|s| s.beat_fx_slots[u.min(BEAT_FX_UNITS - 1)].get(slot.wrapping_sub(2)).copied())
}

fn send_slot(app: &App, u: usize, slot: usize, cmd: BeatFxCommand) {
    engine::ensure_engine(app);
    if (2..=BEAT_FX_SLOTS).contains(&slot) {
        engine::send(app, Command::BeatFxSlot { unit: u, slot: slot - 1, cmd });
    }
}

/// Multi mode on / off for unit `u` (0-based).
pub(crate) fn toggle_multi(app: &App, u: usize) {
    engine::ensure_engine(app);
    let on = engine::snapshot(app).is_some_and(|s| s.beat_fx_multi[u.min(BEAT_FX_UNITS - 1)]);
    engine::send(app, Command::SetBeatFxMulti(u, !on));
}

pub(crate) fn step_unit_kind(app: &App, u: usize, delta: i32) {
    let kind = unit_state(app, u).map(|s| s.kind).unwrap_or_default();
    send(app, u, BeatFxCommand::SetKind(step_kind(kind, delta)));
}

pub(crate) fn step_unit_beats(app: &App, u: usize, delta: i32) {
    let beats = unit_state(app, u).map(|s| s.beats).unwrap_or(0.75);
    send(app, u, BeatFxCommand::SetBeats(step_beats(beats, delta)));
}

/// TAP: every tap refines the tapped tempo of unit `u`.
pub(crate) fn tap(app: &App, u: usize) {
    let t = TAP_EPOCH.with(|e| e.elapsed().as_secs_f64());
    let bpm = app.fx.taps.borrow_mut()[u.min(BEAT_FX_UNITS - 1)].tap(t);
    if let Some(bpm) = bpm {
        send(app, u, BeatFxCommand::SetTapBpm(Some(bpm)));
    }
}

/// AUTO: back to the tempo of the decks.
pub(crate) fn auto_beat(app: &App, u: usize) {
    app.fx.taps.borrow_mut()[u.min(BEAT_FX_UNITS - 1)] = TapTempo::default();
    send(app, u, BeatFxCommand::SetTapBpm(None));
}

/// Release FX `kind` on unit `u` while `on`, lasting the unit's beat length.
pub(crate) fn release(app: &App, u: usize, kind: ReleaseFxKind, on: bool) {
    let beats = unit_state(app, u).map(|s| s.beats).unwrap_or(1.0).max(0.5);
    send(app, u, BeatFxCommand::Release { kind, beats, on });
}

/// Selects the Colour FX of the CFX knobs (index into [`COLOR_FX`]).
pub(crate) fn select_color_fx(app: &App, index: usize) {
    let name = COLOR_FX[index.min(COLOR_FX.len() - 1)];
    app.state.borrow_mut().change_settings(|s| s.perform.color_fx = name.to_string());
    app.ui.set_color_fx(index as i32);
    engine::ensure_engine(app);
    for c in 0..decks::DECKS {
        engine::send(app, Command::Channel(c, ChannelCommand::SetColorFx(color_fx_kind(name))));
    }
}

pub(crate) fn step_color_fx(app: &App, delta: i32) {
    let cur = color_fx_index(&app.state.borrow().settings.perform.color_fx) as i32;
    select_color_fx(app, (cur + delta).rem_euclid(COLOR_FX.len() as i32) as usize);
}

thread_local! {
    static TAP_EPOCH: Instant = Instant::now();
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.set_fx_panel(app.state.borrow().settings.perform.fx_panel);
    ui.set_fx_units(app.fx.units.handle());
    ui.set_color_fx(color_fx_index(&app.state.borrow().settings.perform.color_fx) as i32);
    show_idle(app);
    ui.on_toggle_fx_panel(handler!(app, |a| toggle_panel(&a)));
    // Units are 1-based in the UI.
    let u = |n: i32| (n as usize).saturating_sub(1);
    ui.on_fx_kind(handler!(app, |a, n: i32, k: i32| send(&a, u(n), BeatFxCommand::SetKind(BeatFxKind::ALL[(k.max(0) as usize).min(BeatFxKind::ALL.len() - 1)]))));
    let slots = slint::ComponentHandle::global::<crate::FxSlots>(&app.ui);
    slots.on_multi(handler!(app, |a, n: i32| toggle_multi(&a, u(n))));
    slots.on_auto(handler!(app, |a, n: i32| auto_beat(&a, u(n))));
    slots.on_kind_step(handler!(app, |a, n: i32, k: i32, d: i32| {
        let kind = slot_state(&a, u(n), k as usize).map(|s| s.kind).unwrap_or_default();
        send_slot(&a, u(n), k as usize, BeatFxCommand::SetKind(step_kind(kind, d)));
    }));
    slots.on_beats_step(handler!(app, |a, n: i32, k: i32, d: i32| {
        let beats = slot_state(&a, u(n), k as usize).map(|s| s.beats).unwrap_or(0.75);
        send_slot(&a, u(n), k as usize, BeatFxCommand::SetBeats(step_beats(beats, d)));
    }));
    slots.on_level(handler!(app, |a, n: i32, k: i32, v: f32| send_slot(&a, u(n), k as usize, BeatFxCommand::SetLevel(v))));
    slots.on_on(handler!(app, |a, n: i32, k: i32| {
        let on = slot_state(&a, u(n), k as usize).is_some_and(|s| s.on);
        send_slot(&a, u(n), k as usize, BeatFxCommand::SetOn(!on));
    }));
    ui.on_fx_kind_step(handler!(app, |a, n: i32, d: i32| step_unit_kind(&a, u(n), d)));
    ui.on_fx_beats_step(handler!(app, |a, n: i32, d: i32| step_unit_beats(&a, u(n), d)));
    ui.on_fx_target(handler!(app, |a, n: i32, t: i32| send(&a, u(n), BeatFxCommand::SetTarget(target_from_number(t)))));
    ui.on_fx_level(handler!(app, |a, n: i32, v: f32| send(&a, u(n), BeatFxCommand::SetLevel(v))));
    ui.on_fx_param(handler!(app, |a, n: i32, v: f32| send(&a, u(n), BeatFxCommand::SetParam(v))));
    ui.on_fx_on(handler!(app, |a, n: i32| toggle_on(&a, u(n))));
    ui.on_fx_tap(handler!(app, |a, n: i32| tap(&a, u(n))));
    ui.on_fx_release(handler!(app, |a, n: i32, k: i32, on: bool| release(&a, u(n), RELEASE_FX[(k.max(0) as usize).min(2)], on)));
    ui.on_color_fx_select(handler!(app, |a, i: i32| select_color_fx(&a, i.max(0) as usize)));
    ui.on_color_fx_step(handler!(app, |a, d: i32| step_color_fx(&a, d)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_lengths_step_and_print() {
        assert_eq!(step_beats(0.75, 1), 1.0);
        assert_eq!(step_beats(0.75, -1), 0.5);
        assert_eq!(step_beats(16.0, 1), 16.0);
        assert_eq!(step_beats(0.125, -1), 0.125);
        // A value between steps snaps to the nearest one first.
        assert_eq!(step_beats(3.0, 1), 4.0);
        assert_eq!(beats_label(0.125), "1/8");
        assert_eq!(beats_label(0.75), "3/4");
        assert_eq!(beats_label(4.0), "4");
    }

    #[test]
    fn kinds_wrap_and_map_controller_names() {
        assert_eq!(step_kind(BeatFxKind::Echo, -1), BeatFxKind::Spiral);
        assert_eq!(step_kind(BeatFxKind::Spiral, 1), BeatFxKind::Echo);
        assert_eq!(step_kind(BeatFxKind::Echo, 3), BeatFxKind::Reverb);
        assert_eq!(kind_from_name("low_cut_echo"), Some(BeatFxKind::Echo));
        assert_eq!(kind_from_name("slip_roll"), Some(BeatFxKind::SlipRoll));
        assert_eq!(kind_from_name("vinyl_brake"), Some(BeatFxKind::VinylBrake));
        assert_eq!(kind_from_name("laser"), None);
    }

    #[test]
    fn tap_tempo_averages_and_restarts() {
        let mut t = TapTempo::default();
        assert_eq!(t.tap(0.0), None);
        assert_eq!(t.tap(0.5), None);
        assert_eq!(t.tap(1.0), Some(120.0));
        let bpm = t.tap(1.48).unwrap();
        assert!((bpm - 121.6).abs() < 0.1, "{bpm}");
        // A pause starts over.
        assert_eq!(t.tap(5.0), None);
    }

    #[test]
    fn unit_projection() {
        let s = BeatFxState {
            kind: BeatFxKind::Reverb,
            target: FxTarget::Channel(1),
            on: true,
            level: 0.7,
            beats: 2.0,
            param: -0.5,
            tap_bpm: Some(128.0),
            bpm: 128.0,
            release: Some(ReleaseFxKind::Backspin),
            ringing: false,
        };
        let v = unit_view(1, &s);
        assert_eq!((v.unit, v.kind, v.target, v.release), (2, 3, 2, 1));
        assert_eq!((v.beats.as_str(), v.bpm.as_str()), ("2", "128.0"));
        assert!(v.on && v.tapped);
        let idle = unit_view(0, &idle_unit());
        assert_eq!((idle.target, idle.release, idle.beats.as_str(), idle.bpm.as_str()), (0, -1, "3/4", ""));
        assert_eq!(target_from_number(target_number(FxTarget::Channel(3))), FxTarget::Channel(3));
        assert_eq!(target_from_number(0), FxTarget::Master);
    }

    #[test]
    fn color_fx_follows_the_selection() {
        let mut st = EngineState { decks_count: 2, ..Default::default() };
        assert!(color_fx_commands(&st, "filter").is_empty());
        let cmds = color_fx_commands(&st, "dub_echo");
        assert_eq!(cmds.len(), 2);
        assert!(matches!(cmds[0], Command::Channel(0, ChannelCommand::SetColorFx(ColorFxKind::DubEcho))));
        st.channels[0].color_fx = ColorFxKind::DubEcho;
        assert_eq!(color_fx_commands(&st, "dub_echo").len(), 1);
        for (i, n) in COLOR_FX.iter().enumerate() {
            assert_eq!(color_fx_index(n), i);
        }
        assert_eq!(color_fx_kind("sweep"), ColorFxKind::Sweep);
        assert_eq!(color_fx_kind("nope"), ColorFxKind::Filter);
    }

    #[test]
    fn pad_fx_presets_cover_two_pages() {
        let p = pad_fx_preset(0, 0);
        assert_eq!(p.effect, PadFxEffect::Beat(BeatFxKind::Echo));
        assert_eq!(pad_fx_caption(&p), "ECHO 1/2");
        assert_eq!(pad_fx_caption(&pad_fx_preset(1, 7)), "BACKSPIN");
        // Out-of-range pads clamp.
        assert_eq!(pad_fx_preset(5, 12), pad_fx_preset(1, 7));
    }
}
