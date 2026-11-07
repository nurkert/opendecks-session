//! Offline-render tests for the sampler: modes, quantised triggering, tempo
//! sync and the step sequencer.

mod common;

use std::sync::Arc;

use common::*;
use od_engine::*;

fn sample(frames: usize, f: impl Fn(usize) -> f32) -> Arc<SampleData> {
    SampleData::new(
        od_decode::DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples: (0..frames).flat_map(|i| [f(i), f(i)]).collect(),
        },
        SR,
    )
}

fn dc(frames: usize, v: f32) -> Arc<SampleData> {
    sample(frames, |_| v)
}

fn smp(rig: &mut Rig, c: SamplerCommand) {
    rig.send(Command::Sampler(c));
}

/// Master deck 0 playing silence on a 120 BPM grid from frame 0.
fn clock_rig() -> Rig {
    let mut rig = Rig::new(2);
    let n = 60 * SR as usize;
    rig.deck(0, DeckCommand::Load(track_from(1, vec![0.0; n * 2], 120.0, 0.0)));
    rig.deck(0, DeckCommand::Play);
    rig
}

/// Frames where the left channel turns on (|x| crosses 1e-6 from below).
fn onsets(out: &[f32]) -> Vec<usize> {
    let mut v = Vec::new();
    let mut prev = 0.0f32;
    for (i, f) in out.chunks_exact(2).enumerate() {
        if prev.abs() <= 1e-6 && f[0].abs() > 1e-6 {
            v.push(i);
        }
        prev = f[0];
    }
    v
}

#[test]
fn quantised_trigger_starts_on_the_grid() {
    let mut rig = clock_rig();
    smp(&mut rig, SamplerCommand::Load { slot: 0, sample: dc(48_000, 0.5) });
    smp(&mut rig, SamplerCommand::SetQuantize(Some(0.25)));
    rig.render(235); // 30 080 frames
    smp(&mut rig, SamplerCommand::Trigger { slot: 0 });
    rig.render(1);
    assert!(rig.state().sampler[0].pending);
    let out = rig.render(100);
    // Next 1/4-beat boundary: 36 000 = 30 080 + 128 + 5 792.
    assert_eq!(onsets(&out), vec![5792]);
    assert_eq!(out[5792 * 2], 0.5);
    let st = rig.state();
    assert!(st.sampler[0].playing && !st.sampler[0].pending);
    assert_eq!(st.sequencer.quantize, Some(0.25));

    // Without quantize the trigger starts with the next block.
    smp(&mut rig, SamplerCommand::SetQuantize(None));
    smp(&mut rig, SamplerCommand::Stop { slot: 0 });
    rig.render(100);
    smp(&mut rig, SamplerCommand::Trigger { slot: 0 });
    let out = rig.render(1);
    assert_eq!(out[0], 0.5);
}

#[test]
fn one_shot_loop_and_gate_modes() {
    let mut rig = clock_rig();
    smp(&mut rig, SamplerCommand::Load { slot: 3, sample: dc(1000, 0.5) });
    smp(&mut rig, SamplerCommand::Trigger { slot: 3 });
    let out = rig.render(20);
    assert_eq!(out[999 * 2], 0.5);
    assert_eq!(out[1000 * 2], 0.0, "one-shot ends");
    assert!(!rig.state().sampler[3].playing);

    smp(&mut rig, SamplerCommand::SetMode { slot: 3, mode: SampleMode::Loop });
    smp(&mut rig, SamplerCommand::Trigger { slot: 3 });
    smp(&mut rig, SamplerCommand::Release { slot: 3 });
    let out = rig.render(20);
    assert!(out.iter().all(|&v| v == 0.5), "loop keeps playing, release ignored");

    smp(&mut rig, SamplerCommand::Stop { slot: 3 });
    rig.render(2);
    smp(&mut rig, SamplerCommand::SetMode { slot: 3, mode: SampleMode::Gate });
    smp(&mut rig, SamplerCommand::Trigger { slot: 3 });
    let out = rig.render(20);
    assert!(out.iter().all(|&v| v == 0.5), "gate loops while held");
    smp(&mut rig, SamplerCommand::Release { slot: 3 });
    let out = rig.render(2);
    // Faded out over 128 frames, monotonically.
    assert!(out[0] <= 0.5 && out[64 * 2] < 0.3);
    assert!(out.chunks_exact(2).zip(out.chunks_exact(2).skip(1)).all(|(a, b)| b[0] <= a[0]));
    assert!(out[BLOCK * 2..].iter().all(|&v| v == 0.0));
    assert!(!rig.state().sampler[3].playing);

    // Gain is ramped to the new value.
    smp(&mut rig, SamplerCommand::SetMode { slot: 3, mode: SampleMode::Loop });
    smp(&mut rig, SamplerCommand::SetGain { slot: 3, gain: 0.5 });
    smp(&mut rig, SamplerCommand::Trigger { slot: 3 });
    let out = rig.render(20);
    assert!(out[0] > 0.25 && (out[19 * BLOCK * 2] - 0.25).abs() < 1e-6);
}

#[test]
fn retrigger_is_click_free() {
    let mut rig = clock_rig();
    let s = sample(48_000, |i| (std::f64::consts::TAU * 200.0 * i as f64 / SR as f64).sin() as f32 * 0.5);
    smp(&mut rig, SamplerCommand::Load { slot: 0, sample: s });
    let mut all = Vec::new();
    for _ in 0..10 {
        smp(&mut rig, SamplerCommand::Trigger { slot: 0 });
        all.extend(rig.render(13));
    }
    let step = all
        .chunks_exact(2)
        .zip(all.chunks_exact(2).skip(1))
        .map(|(a, b)| (a[0] - b[0]).abs())
        .fold(0.0f32, f32::max);
    // The sine alone moves up to 0.013 per frame.
    assert!(step < 0.03, "step {step}");
}

#[test]
fn sync_follows_the_master_tempo() {
    let mut rig = clock_rig();
    let s = sample(96_000, |i| (std::f64::consts::TAU * 1000.0 * i as f64 / SR as f64).sin() as f32 * 0.5);
    smp(&mut rig, SamplerCommand::Load { slot: 5, sample: s });
    smp(&mut rig, SamplerCommand::SetMode { slot: 5, mode: SampleMode::Loop });
    smp(&mut rig, SamplerCommand::SetBpm { slot: 5, bpm: Some(100.0) });
    smp(&mut rig, SamplerCommand::Trigger { slot: 5 });
    let f = estimate_freq(&rig.render(200));
    assert!((f - 1000.0).abs() < 1.0, "unsynced {f}");
    smp(&mut rig, SamplerCommand::SetSync { slot: 5, on: true });
    let f = estimate_freq(&rig.render(200));
    assert!((f - 1200.0).abs() < 1.0, "synced to 120 BPM: {f}");
    let st = rig.state().sampler[5];
    assert!(st.sync && st.bpm == Some(100.0));
}

#[test]
fn sequencer_plays_steps_on_the_master_beat() {
    let mut rig = clock_rig();
    smp(&mut rig, SamplerCommand::Load { slot: 0, sample: dc(100, 0.5) });
    smp(&mut rig, SamplerCommand::SeqStep { slot: 0, step: 0, on: true });
    smp(&mut rig, SamplerCommand::SeqStep { slot: 0, step: 8, on: true });
    smp(&mut rig, SamplerCommand::SeqPlay(true));
    let out = rig.render(1000); // 128 000 frames = 5.33 beats
    // Steps are 16ths: step 0 at beat 0, step 8 at beat 2, step 0 at beat 4.
    assert_eq!(onsets(&out), vec![0, 48_000, 96_000]);
    let st = rig.state();
    assert!(st.sequencer.playing);
    assert_eq!(st.sampler[0].pattern, 0b1_0000_0001);
    assert_eq!(st.sequencer.step, ((128_000.0 / 6000.0) as u8) % 16);
    smp(&mut rig, SamplerCommand::SeqPlay(false));
    let out = rig.render(400);
    assert!(onsets(&out).is_empty(), "stopped");
}

#[test]
fn sequencer_pattern_recording() {
    let mut rig = clock_rig();
    smp(&mut rig, SamplerCommand::Load { slot: 1, sample: dc(100, 0.5) });
    smp(&mut rig, SamplerCommand::SeqPlay(true));
    smp(&mut rig, SamplerCommand::SeqRecord(true));
    rig.render(191); // beat 1.019: nearest 16th is step 4 (behind)
    smp(&mut rig, SamplerCommand::Trigger { slot: 1 });
    let out = rig.render(1);
    assert_eq!(out[0], 0.5, "played at once");
    assert_eq!(rig.state().sampler[1].pattern, 1 << 4);
    rig.render(33); // block start 28 800 = beat 1.2: nearest is step 5 (ahead)
    smp(&mut rig, SamplerCommand::Trigger { slot: 1 });
    let out = rig.render(20);
    assert_eq!(rig.state().sampler[1].pattern, (1 << 4) | (1 << 5));
    // Not doubled: only the sequencer plays it, exactly at 30 000.
    assert_eq!(onsets(&out), vec![30_000 - 28_800]);
}

#[test]
fn sampler_cue_to_headphones() {
    let mut rig = clock_rig();
    smp(&mut rig, SamplerCommand::Load { slot: 0, sample: dc(48_000, 0.25) });
    smp(&mut rig, SamplerCommand::Trigger { slot: 0 });
    rig.send(Command::Mixer(MixerCommand::SetHeadphoneMix(0.0)));
    rig.render(5);
    assert!(rig.phones[..BLOCK * 2].iter().all(|&v| v == 0.0));
    rig.send(Command::Mixer(MixerCommand::SetSamplerCue(true)));
    rig.render(5);
    assert!(rig.phones[..BLOCK * 2].iter().all(|&v| (v - 0.25).abs() < 1e-6));
    assert!(rig.state().sampler_cue);
}

/// A Beat FX unit inserted on the sampler only changes the sampler: a
/// short sample echoes after it has ended.
#[test]
fn beat_fx_on_the_sampler() {
    let run = |target: FxTarget| {
        let mut rig = clock_rig();
        smp(&mut rig, SamplerCommand::Load { slot: 0, sample: dc(2_400, 0.5) });
        for c in [
            BeatFxCommand::SetKind(BeatFxKind::Echo),
            BeatFxCommand::SetBeats(0.5),
            BeatFxCommand::SetLevel(1.0),
            BeatFxCommand::SetTarget(target),
            BeatFxCommand::SetOn(true),
        ] {
            rig.send(Command::BeatFx(0, c));
        }
        smp(&mut rig, SamplerCommand::Trigger { slot: 0 });
        let out = rig.render(200); // 25 600 frames ≈ 1 beat at 120 BPM
        // Half a beat (12 000 frames) after the sample: the echo.
        let tail: f32 = (12_000..14_000).map(|i| out[i * 2].abs()).sum();
        (tail, rig.state().beat_fx[0].target)
    };
    let (echo, target) = run(FxTarget::Sampler);
    let (dry, _) = run(FxTarget::Channel(1));
    assert_eq!(target, FxTarget::Sampler);
    assert!(echo > 10.0, "sampler echo: {echo}");
    assert!(dry < 1e-3, "no echo when the unit sits on another channel: {dry}");
}
