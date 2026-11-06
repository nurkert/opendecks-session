//! Offline-render tests for Beat FX, Colour FX, Release FX and Pad FX.

mod common;

use common::*;
use od_engine::*;

/// One beat at 120 BPM.
const BEAT: usize = 24_000;

fn rms(sig: &[f32], ch: usize, from: usize, len: usize) -> f32 {
    let s: f32 = (from..from + len).map(|i| sig[i * 2 + ch] * sig[i * 2 + ch]).sum();
    (s / len as f32).sqrt()
}

/// Silent 120 BPM track with a 50 ms 1 kHz burst at `at` frames.
fn burst_track(id: i64, seconds: f64, at: usize) -> std::sync::Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let mut s = vec![0f32; n * 2];
    for k in 0..2400 {
        let v = (std::f64::consts::TAU * 1000.0 * k as f64 / SR as f64).sin() as f32 * 0.5;
        s[(at + k) * 2] = v;
        s[(at + k) * 2 + 1] = v;
    }
    track_from(id, s, 120.0, 0.0)
}

fn fx(rig: &mut Rig, unit: usize, c: BeatFxCommand) {
    rig.send(Command::BeatFx(unit, c));
}

fn render_frames(rig: &mut Rig, frames: usize) -> Vec<f32> {
    rig.render(frames.div_ceil(BLOCK))
}

#[test]
fn echo_repeats_at_the_beat_offset() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(burst_track(1, 10.0, 2 * BEAT)));
    rig.deck(0, DeckCommand::Play);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::Echo));
    fx(&mut rig, 0, BeatFxCommand::SetBeats(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    let out = render_frames(&mut rig, 6 * BEAT);
    let dry = rms(&out, 0, 2 * BEAT, 2400);
    let first = rms(&out, 0, 3 * BEAT, 2400);
    let half = rms(&out, 0, 2 * BEAT + BEAT / 2, 2400);
    let second = rms(&out, 0, 4 * BEAT, 2400);
    assert!(first > 0.3 * dry, "first repeat {first} vs dry {dry}");
    assert!(half < 1e-4, "nothing half a beat later: {half}");
    assert!(second > 0.1 * dry && second < first, "decaying repeats {second}");
    // Onset of the first repeat within 2 frames of one beat later.
    let onset = (3 * BEAT - 100..3 * BEAT + 100)
        .find(|&i| out[i * 2].abs() > 0.01)
        .unwrap();
    assert!((onset as i64 - (3 * BEAT) as i64).abs() <= 2, "onset {onset}");
}

#[test]
fn delay_repeats_once_and_ping_pong_alternates() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(burst_track(1, 10.0, 2 * BEAT)));
    rig.deck(0, DeckCommand::Play);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::Delay));
    fx(&mut rig, 0, BeatFxCommand::SetBeats(0.5));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    let out = render_frames(&mut rig, 4 * BEAT);
    let dry = rms(&out, 0, 2 * BEAT, 2400);
    assert!(rms(&out, 0, 2 * BEAT + BEAT / 2, 2400) > 0.5 * dry);
    assert!(rms(&out, 0, 3 * BEAT, 2400) < 1e-4, "single repeat only");

    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(burst_track(1, 10.0, 2 * BEAT)));
    rig.deck(0, DeckCommand::Play);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::PingPong));
    fx(&mut rig, 0, BeatFxCommand::SetBeats(0.5));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    let out = render_frames(&mut rig, 4 * BEAT);
    let p1 = 2 * BEAT + BEAT / 2;
    let p2 = 3 * BEAT;
    let (l1, r1) = (rms(&out, 0, p1, 2400), rms(&out, 1, p1, 2400));
    let (l2, r2) = (rms(&out, 0, p2, 2400), rms(&out, 1, p2, 2400));
    assert!(l1 > 0.1 && r1 < 1e-4, "first repeat left: {l1} {r1}");
    assert!(r2 > 0.05 && l2 < 1e-4, "second repeat right: {l2} {r2}");
}

/// Continuous sine on deck 0 at 120 BPM.
fn sine_rig(freq: f64) -> Rig {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 30.0, freq, 0.5, 120.0)));
    rig.deck(0, DeckCommand::Play);
    // Two seconds of history (backspin reads into the past).
    rig.render(750);
    rig
}

/// Two tones (211 Hz + 3137 Hz) on deck 0, so both filter sides of an
/// effect are audible and no loop length is a whole number of cycles.
fn rich_rig() -> Rig {
    let n = 30 * SR as usize;
    let s: Vec<f32> = (0..n)
        .flat_map(|i| {
            let t = i as f64 / SR as f64;
            let v = (0.3 * (std::f64::consts::TAU * 211.0 * t).sin()
                + 0.2 * (std::f64::consts::TAU * 3137.0 * t).sin()) as f32;
            [v, v]
        })
        .collect();
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(track_from(1, s, 120.0, 0.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(20);
    rig
}

#[test]
fn vinyl_brake_slows_to_a_stop_in_n_beats() {
    // As Beat FX.
    let mut rig = sine_rig(1000.0);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::VinylBrake));
    fx(&mut rig, 0, BeatFxCommand::SetBeats(2.0));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    let out = render_frames(&mut rig, 3 * BEAT);
    // Speed falls linearly 1 → 0 over 2 beats: mean speed over a window
    // starting at t is about 1 − t/2 beats.
    let f0 = estimate_freq(&out[2 * 2000..2 * 14_000]);
    let f1 = estimate_freq(&out[2 * 24_000..2 * 36_000]);
    assert!((f0 - 1000.0 * 0.83).abs() < 60.0, "early {f0}");
    assert!((f1 - 1000.0 * 0.375).abs() < 60.0, "late {f1}");
    assert!(rms(&out, 0, 2 * BEAT, BEAT / 2) < 1e-4, "stopped after 2 beats");
    fx(&mut rig, 0, BeatFxCommand::SetOn(false));
    let out = rig.render(40);
    assert!(rms(&out, 0, 2000, 3000) > 0.3, "dry returns");

    // As Release FX.
    let mut rig = sine_rig(1000.0);
    let rel = |on| BeatFxCommand::Release {
        kind: ReleaseFxKind::VinylBrake,
        beats: 1.0,
        on,
    };
    fx(&mut rig, 0, rel(true));
    let out = render_frames(&mut rig, 2 * BEAT);
    assert!(rms(&out, 0, 0, 4000) > 0.2);
    assert!(rms(&out, 0, BEAT, BEAT / 2) < 1e-4, "stopped after 1 beat");
    assert_eq!(rig.state().beat_fx[0].release, Some(ReleaseFxKind::VinylBrake));
    fx(&mut rig, 0, rel(false));
    let out = rig.render(40);
    assert!(rms(&out, 0, 2000, 3000) > 0.3, "dry returns");
    assert_eq!(rig.state().beat_fx[0].release, None);
}

#[test]
fn backspin_and_echo_out_release() {
    let mut rig = sine_rig(1000.0);
    fx(
        &mut rig,
        1,
        BeatFxCommand::Release {
            kind: ReleaseFxKind::Backspin,
            beats: 1.0,
            on: true,
        },
    );
    let out = render_frames(&mut rig, 2 * BEAT);
    assert!(rms(&out, 0, 2000, 4000) > 0.1, "spinning back");
    assert!(rms(&out, 0, BEAT + 1000, BEAT / 2) < 1e-4, "stopped after 1 beat");

    // Echo out: input cut, last beat echoed with decaying repeats.
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(burst_track(1, 20.0, BEAT - 3000)));
    rig.deck(0, DeckCommand::Play);
    render_frames(&mut rig, BEAT);
    fx(
        &mut rig,
        0,
        BeatFxCommand::Release {
            kind: ReleaseFxKind::EchoOut,
            beats: 1.0,
            on: true,
        },
    );
    // The burst sits at −3000 … −600 frames before the press (block aligned
    // within 128): its echo arrives one beat after it.
    let out = render_frames(&mut rig, 8 * BEAT);
    let e1 = rms(&out, 0, BEAT - 2700, 1800);
    let e5 = rms(&out, 0, 5 * BEAT - 2700, 1800);
    assert!(e1 > 0.15, "first echo {e1}");
    assert!(e5 < e1 * 0.3 && e5 > 1e-4, "decaying {e5}");
}

#[test]
fn pitch_fx_shifts_an_octave() {
    let mut rig = sine_rig(500.0);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::Pitch));
    fx(&mut rig, 0, BeatFxCommand::SetParam(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    rig.render(40);
    let out = rig.render(200);
    let f = estimate_freq(&out);
    assert!((f - 1000.0).abs() < 20.0, "pitch +12: {f}");
    assert!((rig.state().beat_fx[0].param - 1.0).abs() < 1e-6);
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[test]
fn every_beat_fx_runs_and_returns_to_dry() {
    for kind in BeatFxKind::ALL {
        let mut dry = rich_rig();
        let mut wet = rich_rig();
        fx(&mut wet, 0, BeatFxCommand::SetKind(kind));
        fx(&mut wet, 0, BeatFxCommand::SetBeats(0.5));
        fx(&mut wet, 0, BeatFxCommand::SetLevel(1.0));
        fx(&mut wet, 0, BeatFxCommand::SetOn(true));
        let a = dry.render(300);
        let b = wet.render(300);
        assert!(b.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{kind:?} bounded");
        assert!(max_abs_diff(&a[BLOCK * 100..], &b[BLOCK * 100..]) > 0.01, "{kind:?} audible");
        assert!(wet.state().beat_fx[0].on);
        fx(&mut wet, 0, BeatFxCommand::SetOn(false));
        let mut waited = 0;
        loop {
            dry.render(100);
            wet.render(100);
            waited += 100;
            let st = wet.state().beat_fx[0];
            if !st.ringing {
                break;
            }
            assert!(kind.has_tail(), "{kind:?} only tail kinds ring");
            assert!(waited < 40 * 375, "{kind:?} tail never ends");
        }
        let a = dry.render(50);
        let b = wet.render(50);
        assert_eq!(a, b, "{kind:?} bit-exact dry after switching off");
    }
}

#[test]
fn beat_fx_toggles_are_click_free() {
    let max_step = |s: &[f32]| {
        s.chunks_exact(2)
            .zip(s.chunks_exact(2).skip(1))
            .map(|(a, b)| (a[0] - b[0]).abs())
            .fold(0.0, f32::max)
    };
    for kind in [
        BeatFxKind::Filter,
        BeatFxKind::Trans,
        BeatFxKind::Flanger,
        BeatFxKind::Phaser,
        BeatFxKind::Pitch,
        BeatFxKind::Echo,
        BeatFxKind::Reverb,
        BeatFxKind::VinylBrake,
    ] {
        let mut rig = sine_rig(100.0);
        fx(&mut rig, 0, BeatFxCommand::SetKind(kind));
        fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
        let mut all = Vec::new();
        for i in 0..8 {
            fx(&mut rig, 0, BeatFxCommand::SetOn(i % 2 == 0));
            fx(&mut rig, 0, BeatFxCommand::SetLevel(0.4 + 0.6 * (i % 3) as f32 / 2.0));
            all.extend(rig.render(37));
        }
        // A 100 Hz sine at 0.5 moves at most 0.0066 per frame.
        let step = max_step(&all);
        assert!(step < 0.05, "{kind:?}: step {step}");
    }
}

#[test]
fn beat_fx_follows_the_channel_bpm_and_tap() {
    let mut rig = Rig::new(2);
    // Deck 0 (master) at 120 BPM, deck 1 at 100 BPM, both playing.
    rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 300.0, 0.3, 120.0)));
    rig.deck(1, DeckCommand::Load(sine_track(2, 20.0, 300.0, 0.3, 100.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(5);
    rig.deck(1, DeckCommand::Play);
    fx(&mut rig, 0, BeatFxCommand::SetTarget(FxTarget::Channel(1)));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    fx(&mut rig, 1, BeatFxCommand::SetOn(true));
    rig.render(10);
    let st = rig.state();
    assert_eq!(st.master_deck, Some(0));
    assert!((st.beat_fx[0].bpm - 100.0).abs() < 1e-3, "channel bpm {}", st.beat_fx[0].bpm);
    assert!((st.beat_fx[1].bpm - 120.0).abs() < 1e-3, "master bpm");
    fx(&mut rig, 1, BeatFxCommand::SetTapBpm(Some(90.0)));
    rig.render(2);
    assert!((rig.state().beat_fx[1].bpm - 90.0).abs() < 1e-3, "tapped");
}

#[test]
fn trans_gate_on_the_channel_deck_grid() {
    // Channel FX lock to the channel's own beat, not the master's.
    let n = 20 * SR as usize;
    let s: Vec<f32> = (0..n).flat_map(|_| [0.5f32, 0.5]).collect();
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 300.0, 0.0, 120.0)));
    rig.deck(1, DeckCommand::Load(track_from(2, s, 100.0, 0.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(3);
    rig.deck(1, DeckCommand::Play);
    fx(&mut rig, 0, BeatFxCommand::SetKind(BeatFxKind::Trans));
    fx(&mut rig, 0, BeatFxCommand::SetTarget(FxTarget::Channel(1)));
    fx(&mut rig, 0, BeatFxCommand::SetBeats(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetLevel(1.0));
    fx(&mut rig, 0, BeatFxCommand::SetOn(true));
    let start = rig.state().decks[1].position;
    let out = rig.render(800);
    // Deck 1 beat = 28800 frames: open in the first half of each beat.
    let beat = 28_800.0;
    let mut checked = 0;
    for k in (1000..800 * BLOCK).step_by(997) {
        let b = (start * SR as f64 + k as f64) / beat;
        let ph = b.rem_euclid(1.0);
        if (0.05..0.45).contains(&ph) {
            assert!((out[k * 2] - 0.5).abs() < 0.01, "open at {k}");
            checked += 1;
        } else if (0.55..0.95).contains(&ph) {
            assert!(out[k * 2].abs() < 0.01, "closed at {k}");
            checked += 1;
        }
    }
    assert!(checked > 50);
}

// ----- colour FX -------------------------------------------------------

fn color(rig: &mut Rig, ch: usize, c: ChannelCommand) {
    rig.send(Command::Channel(ch, c));
}

#[test]
fn every_colour_fx_runs_and_returns_to_dry() {
    for kind in [
        ColorFxKind::Filter,
        ColorFxKind::Noise,
        ColorFxKind::Space,
        ColorFxKind::Crush,
        ColorFxKind::Echo,
        ColorFxKind::Pitch,
        ColorFxKind::DubEcho,
        ColorFxKind::Sweep,
    ] {
        for amount in [-0.8f32, 0.8] {
            let mut dry = rich_rig();
            let mut wet = rich_rig();
            color(&mut wet, 0, ChannelCommand::SetColorFx(kind));
            color(&mut wet, 0, ChannelCommand::SetColorFxAmount(amount));
            let a = dry.render(200);
            let b = wet.render(200);
            assert!(b.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{kind:?} bounded");
            assert!(
                max_abs_diff(&a[BLOCK * 50..], &b[BLOCK * 50..]) > 0.01,
                "{kind:?} {amount} audible"
            );
            color(&mut wet, 0, ChannelCommand::SetColorFxAmount(0.0));
            dry.render(30 * 375);
            wet.render(30 * 375);
            let a = dry.render(50);
            let b = wet.render(50);
            assert_eq!(a, b, "{kind:?} {amount} dry again");
        }
    }
}

#[test]
fn dub_echo_tail_rings_after_the_knob_returns() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(burst_track(1, 10.0, 2 * BEAT)));
    rig.deck(0, DeckCommand::Play);
    color(&mut rig, 0, ChannelCommand::SetColorFx(ColorFxKind::DubEcho));
    color(&mut rig, 0, ChannelCommand::SetColorFxAmount(0.8));
    render_frames(&mut rig, 2 * BEAT + 3000);
    color(&mut rig, 0, ChannelCommand::SetColorFxAmount(0.0));
    let out = render_frames(&mut rig, 4 * BEAT);
    // Repeats every half beat continue with the knob centred.
    let base = BEAT / 2 - 3000;
    let e1 = rms(&out, 0, base, 2400);
    let e4 = rms(&out, 0, base + 3 * BEAT / 2, 2400);
    assert!(e1 > 0.02, "first repeat {e1}");
    assert!(e4 > 0.002 && e4 < e1, "tail {e4}");
}

#[test]
fn colour_pitch_and_sweep_gate() {
    let mut rig = sine_rig(500.0);
    color(&mut rig, 0, ChannelCommand::SetColorFx(ColorFxKind::Pitch));
    color(&mut rig, 0, ChannelCommand::SetColorFxAmount(1.0));
    rig.render(40);
    let f = estimate_freq(&rig.render(200));
    assert!((f - 1000.0).abs() < 20.0, "+12: {f}");
    color(&mut rig, 0, ChannelCommand::SetColorFxAmount(-1.0));
    rig.render(40);
    let f = estimate_freq(&rig.render(200));
    assert!((f - 250.0).abs() < 10.0, "−12: {f}");

    // Sweep left: 1/16 gate on the beat grid.
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 2000.0, 0.5, 120.0)));
    color(&mut rig, 0, ChannelCommand::SetColorFx(ColorFxKind::Sweep));
    color(&mut rig, 0, ChannelCommand::SetColorFxAmount(-1.0));
    rig.deck(0, DeckCommand::Play);
    let out = render_frames(&mut rig, 2 * BEAT);
    let sixteenth = BEAT / 4;
    for k in 2..7 {
        let open = rms(&out, 0, k * sixteenth + 500, 2000);
        let closed = rms(&out, 0, k * sixteenth + sixteenth / 2 + 500, 2000);
        assert!(open > 0.25 && closed < 0.01, "16th {k}: {open} {closed}");
    }
}

// ----- Pad FX ----------------------------------------------------------

#[test]
fn pad_fx_momentary_and_release_presets() {
    let mut rig = sine_rig(1000.0);
    let trans = PadFxPreset {
        effect: PadFxEffect::Beat(BeatFxKind::Trans),
        beats: 0.25,
        level: 1.0,
        param: 0.0,
    };
    rig.send(Command::PadFx(0, PadFxCommand::Press(trans)));
    let out = render_frames(&mut rig, BEAT);
    let st = rig.state().channels[0].pad_fx;
    assert!(st.held && st.preset == Some(trans));
    // Gated: average level well below the dry sine.
    let gated = rms(&out, 0, 2000, BEAT - 4000);
    assert!(gated < 0.28 && gated > 0.15, "gated {gated}");
    rig.send(Command::PadFx(0, PadFxCommand::Release));
    rig.render(10);
    let out = render_frames(&mut rig, BEAT / 2);
    assert!((rms(&out, 0, 0, BEAT / 2) - 0.3535).abs() < 0.01, "dry");
    assert!(!rig.state().channels[0].pad_fx.held);

    // A release preset stops the channel, letting go brings it back.
    let brake = PadFxPreset {
        effect: PadFxEffect::Release(ReleaseFxKind::VinylBrake),
        beats: 0.5,
        level: 1.0,
        param: 0.0,
    };
    rig.send(Command::PadFx(0, PadFxCommand::Press(brake)));
    let out = render_frames(&mut rig, BEAT);
    assert!(rms(&out, 0, BEAT / 2 + 500, BEAT / 4) < 1e-4, "braked to silence");
    rig.send(Command::PadFx(0, PadFxCommand::Release));
    rig.render(10);
    let out = render_frames(&mut rig, BEAT / 4);
    assert!((rms(&out, 0, 0, BEAT / 4) - 0.3535).abs() < 0.01, "dry again");
}

// ----- mixer outputs ---------------------------------------------------

#[test]
fn booth_output_and_master_cue() {
    let mut rig = sine_rig(440.0);
    rig.send(Command::Mixer(MixerCommand::SetMasterLevel(0.0)));
    rig.send(Command::Mixer(MixerCommand::SetBoothLevel(0.5)));
    rig.render(20);
    rig.render(1);
    let m = rms(&rig.master, 0, 0, BLOCK);
    let b = rms(&rig.booth, 0, 0, BLOCK);
    assert!(m < 1e-6, "master muted");
    assert!((b - 0.3535 * 0.25).abs() < 0.01, "booth at −12 dB: {b}");

    // Headphones on cue only (mix 0) with no channel cue: silent until master cue.
    rig.send(Command::Mixer(MixerCommand::SetMasterLevel(1.0)));
    rig.send(Command::Mixer(MixerCommand::SetHeadphoneMix(0.0)));
    rig.render(20);
    assert!(rms(&rig.phones, 0, 0, BLOCK) < 1e-6);
    rig.send(Command::Mixer(MixerCommand::SetMasterCue(true)));
    rig.render(20);
    let diff = max_abs_diff(&rig.phones[..BLOCK * 2], &rig.master[..BLOCK * 2]);
    assert!(rms(&rig.master, 0, 0, BLOCK) > 0.1 && diff < 1e-6, "master in phones: {diff}");
    let st = rig.state();
    assert!(st.master_cue && (st.booth_level - 0.5).abs() < 1e-6);
}
