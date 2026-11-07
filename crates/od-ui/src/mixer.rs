//! Mixer: channel strips, master, headphones and crossfader.

use crate::app::{App, handler};
use crate::models::Rows;
use crate::{ChannelView, decks, engine};
use od_engine::{
    ChannelCommand, ChannelState, Command, DeckCommand, DeckState, EngineState, MixerCommand, Stem,
};
use slint::ComponentHandle;
use slint::Model as _;
use std::cell::Cell;
use std::rc::Rc;

/// Parts on the EQ knobs in STEM mode, by EQ band (0 low, 1 mid, 2 high).
pub(crate) const PART_BANDS: [Stem; 3] = [Stem::Drums, Stem::Inst, Stem::Vocals];

/// Part level onto an EQ knob (−1..1): centre and above = full level.
pub(crate) fn part_to_knob(level: f32) -> f32 {
    (level - 1.0).clamp(-1.0, 0.0)
}

/// EQ knob onto a part level.
pub(crate) fn knob_to_part(v: f32) -> f32 {
    (v + 1.0).clamp(0.0, 1.0)
}

#[derive(Default)]
pub(crate) struct MixerView {
    /// Bound to `channels`; rows are written only when a value changed.
    pub channels: Rows<ChannelView>,
    /// STEM per channel.
    pub parts: Cell<[bool; decks::DECKS]>,
    /// Smart Fader on, and the low EQ it last set per channel.
    pub smart: Cell<bool>,
    smart_low: Cell<[Option<f32>; decks::DECKS]>,
}

/// Smart Fader bass swap: the low EQ of the A side (`a = true`) or B side
/// for crossfader position `u` (0 = A … 1 = B). The basses cross between
/// 40 % and 60 %; the side being faded out loses its bass.
pub(crate) fn smart_low(u: f32, a: bool) -> f32 {
    let t = ((u - 0.4) / 0.2).clamp(0.0, 1.0);
    if a { -t } else { -(1.0 - t) }
}

/// Trim in dB (−24..+12) onto the knob's −1..1.
pub(crate) fn trim_to_knob(db: f32) -> f32 {
    if db < 0.0 { db / 24.0 } else { db / 12.0 }
}

/// Knob −1..1 onto trim in dB (−24..+12).
pub(crate) fn knob_to_trim(v: f32) -> f32 {
    if v < 0.0 { v * 24.0 } else { v * 12.0 }
}

/// A channel in STEM mode: the EQ knobs show the deck's part levels (pure).
pub(crate) fn parts_view(channel: usize, c: &ChannelState, d: &DeckState) -> ChannelView {
    let part = |band: usize| part_to_knob(d.stem_levels[PART_BANDS[band].index()]);
    ChannelView {
        high: part(2),
        mid: part(1),
        low: part(0),
        parts: true,
        ..channel_view(channel, c)
    }
}

pub(crate) fn channel_view(channel: usize, c: &ChannelState) -> ChannelView {
    ChannelView {
        parts: false,
        channel: channel as i32 + 1,
        trim: trim_to_knob(c.trim_db),
        high: c.eq[2],
        mid: c.eq[1],
        low: c.eq[0],
        cfx: c.color_fx_amount,
        fader: c.fader,
        cue: c.cue,
        level: c.meter.rms.min(1.0),
        peak: c.meter.peak,
    }
}

/// Frame tick: engine state → channel rows and master section.
pub(crate) fn project(app: &App, state: &EngineState) {
    let count = app.decks.borrow().deck_count;
    let parts = app.mixer.parts.get();
    app.mixer.channels.sync_eq(
        (0..count)
            .map(|d| {
                if parts[d] {
                    parts_view(d, &state.channels[d], &state.decks[d])
                } else {
                    channel_view(d, &state.channels[d])
                }
            })
            .collect(),
    );
    smart_fader(app, state);
    let ui = &app.ui;
    ui.set_master_level(state.master_level);
    ui.set_headphone_mix(state.headphone_mix);
    ui.set_headphone_level(state.headphone_level);
    ui.set_master_bpm(
        if state.master_deck.is_some() {
            format!("{:.1}", state.master_bpm)
        } else {
            String::new()
        }
        .into(),
    );
    ui.set_crossfader((state.crossfader + 1.0) / 2.0);
    let xf: Vec<slint::SharedString> = state
        .channels
        .iter()
        .take(decks::DECKS)
        .map(|c| match c.xf_assign {
            od_engine::XfAssign::A => "A",
            od_engine::XfAssign::B => "B",
            od_engine::XfAssign::Thru => "T",
        })
        .map(Into::into)
        .collect();
    let now = ui.get_xf_assign();
    if now.row_count() != xf.len()
        || xf
            .iter()
            .enumerate()
            .any(|(i, x)| now.row_data(i).as_ref() != Some(x))
    {
        ui.set_xf_assign(crate::models::model(xf));
    }
}

/// Smart Fader: moves the low EQs of the A and B channels with the
/// crossfader (only when they change).
fn smart_fader(app: &App, state: &EngineState) {
    if !app.mixer.smart.get() {
        return;
    }
    let u = (state.crossfader + 1.0) / 2.0;
    let mut last = app.mixer.smart_low.get();
    let count = app.decks.borrow().deck_count.min(decks::DECKS);
    for c in 0..count {
        let side = match state.channels[c].xf_assign {
            od_engine::XfAssign::A => true,
            od_engine::XfAssign::B => false,
            od_engine::XfAssign::Thru => continue,
        };
        let low = smart_low(u, side);
        if last[c].is_none_or(|l| (l - low).abs() > 0.005) {
            engine::send(
                app,
                Command::Channel(
                    c,
                    ChannelCommand::SetEq {
                        band: 0,
                        value: low,
                    },
                ),
            );
            last[c] = Some(low);
        }
    }
    app.mixer.smart_low.set(last);
}

/// Smart Fader on / off: on, the decks on the crossfader follow the master
/// tempo; off, their basses return to flat.
pub(crate) fn toggle_smart_fader(app: &App) {
    engine::ensure_engine(app);
    let on = !app.mixer.smart.get();
    app.mixer.smart.set(on);
    app.ui.global::<crate::MixerParts>().set_smart_fader(on);
    let Some(state) = engine::snapshot(app) else {
        return;
    };
    let count = app.decks.borrow().deck_count.min(decks::DECKS);
    for c in 0..count {
        if state.channels[c].xf_assign == od_engine::XfAssign::Thru {
            continue;
        }
        if on {
            if state.master_deck != Some(c) && state.decks[c].track.is_some() {
                engine::deck_cmd(app, c, DeckCommand::SetSync(true));
            }
        } else if app.mixer.smart_low.get()[c].is_some() {
            engine::send(
                app,
                Command::Channel(
                    c,
                    ChannelCommand::SetEq {
                        band: 0,
                        value: 0.0,
                    },
                ),
            );
        }
    }
    app.mixer.smart_low.set([None; decks::DECKS]);
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    // Channel callbacks carry 1-based channel numbers and start audio.
    fn ch(app: &App, c: i32, cmd: ChannelCommand) {
        engine::ensure_engine(app);
        let c = (c as usize).saturating_sub(1).min(decks::DECKS - 1);
        engine::send(app, Command::Channel(c, cmd));
    }
    ui.on_mixer_trim(handler!(app, |a, c: i32, v: f32| ch(
        &a,
        c,
        ChannelCommand::SetTrim(knob_to_trim(v))
    )));
    ui.on_mixer_eq(handler!(app, |a, c: i32, band: i32, v: f32| {
        let i = (c as usize).saturating_sub(1).min(decks::DECKS - 1);
        if a.mixer.parts.get()[i] {
            engine::ensure_engine(&a);
            let stem = PART_BANDS[(band as usize).min(2)];
            engine::deck_cmd(
                &a,
                i,
                DeckCommand::SetStemLevel {
                    stem,
                    level: knob_to_part(v),
                },
            );
        } else {
            ch(
                &a,
                c,
                ChannelCommand::SetEq {
                    band: band as usize,
                    value: v,
                },
            )
        }
    }));
    ui.global::<crate::MixerParts>()
        .on_toggle_smart_fader(handler!(app, |a| toggle_smart_fader(&a)));
    ui.global::<crate::MixerParts>()
        .on_toggle(handler!(app, |a, c: i32| {
            let i = (c as usize).saturating_sub(1).min(decks::DECKS - 1);
            let mut p = a.mixer.parts.get();
            p[i] = !p[i];
            a.mixer.parts.set(p);
        }));
    ui.on_mixer_cfx(handler!(app, |a, c: i32, v: f32| ch(
        &a,
        c,
        ChannelCommand::SetColorFxAmount(v)
    )));
    ui.on_mixer_fader(handler!(app, |a, c: i32, v: f32| ch(
        &a,
        c,
        ChannelCommand::SetFader(v)
    )));
    ui.on_mixer_cue(handler!(app, |a, c: i32| {
        engine::ensure_engine(&a);
        let i = (c as usize).saturating_sub(1).min(decks::DECKS - 1);
        let on = engine::snapshot(&a)
            .map(|s| s.channels[i].cue)
            .unwrap_or(false);
        ch(&a, c, ChannelCommand::SetCue(!on));
    }));
    // Crossfader assign keys: a deck goes to the pressed side, or back to THRU.
    ui.on_mixer_xf_assign(handler!(app, |a, d: i32, side: slint::SharedString| {
        engine::ensure_engine(&a);
        let i = (d as usize).saturating_sub(1).min(decks::DECKS - 1);
        let want = if side.as_str() == "A" {
            od_engine::XfAssign::A
        } else {
            od_engine::XfAssign::B
        };
        let now = engine::snapshot(&a)
            .map(|s| s.channels[i].xf_assign)
            .unwrap_or_default();
        let next = if now == want {
            od_engine::XfAssign::Thru
        } else {
            want
        };
        ch(&a, d, ChannelCommand::SetCrossfaderAssign(next));
    }));
    ui.on_set_crossfader(handler!(app, |a, v: f32| engine::send(
        &a,
        Command::Mixer(MixerCommand::SetCrossfader(v * 2.0 - 1.0))
    )));
    ui.on_set_master_level(handler!(app, |a, v: f32| engine::send(
        &a,
        Command::Mixer(MixerCommand::SetMasterLevel(v))
    )));
    ui.on_set_headphone_mix(handler!(app, |a, v: f32| engine::send(
        &a,
        Command::Mixer(MixerCommand::SetHeadphoneMix(v))
    )));
    ui.on_set_headphone_level(handler!(app, |a, v: f32| engine::send(
        &a,
        Command::Mixer(MixerCommand::SetHeadphoneLevel(v))
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_mapping_round_trips() {
        for db in [-24.0, -12.0, 0.0, 6.0, 12.0] {
            assert!((knob_to_trim(trim_to_knob(db)) - db).abs() < 1e-5);
        }
        assert_eq!(trim_to_knob(-24.0), -1.0);
        assert_eq!(trim_to_knob(12.0), 1.0);
    }

    #[test]
    fn channel_projection() {
        let c = ChannelState {
            fader: 0.7,
            cue: true,
            ..Default::default()
        };
        let v = channel_view(0, &c);
        assert_eq!(v.channel, 1);
        assert_eq!(v.fader, 0.7);
        assert!(v.cue);
        assert_eq!(v, channel_view(0, &c));
    }
}
