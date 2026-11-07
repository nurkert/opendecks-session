//! Stem playback: per-part levels in the resampler, keylock, loop and
//! scratch paths.

mod common;

use common::*;
use od_engine::*;

const FV: f64 = 440.0; // "vocals"
const FD: f64 = 3_100.0; // "drums"
const FO: f64 = 1_250.0; // "other" (inst remainder)

fn db(x: f64, reference: f64) -> f64 {
    20.0 * (x / reference).log10()
}

fn playing_rig(keylock: bool, tempo: f32) -> Rig {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(stem_track(1, 20.0, FV, FD, FO)));
    rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
    rig.deck(0, DeckCommand::SetTempoFader(tempo));
    rig.deck(0, DeckCommand::SetKeylock(keylock));
    rig.deck(0, DeckCommand::Play);
    rig.render(100);
    rig
}

#[test]
fn bypass_is_bit_exact_when_all_levels_are_one() {
    let mut a = Rig::new(2);
    let mut b = Rig::new(2);
    let with = stem_track(1, 5.0, FV, FD, FO);
    let plain = {
        let mix = with.samples.to_vec();
        track_from(2, mix, 120.0, 0.0)
    };
    a.deck(0, DeckCommand::Load(with));
    b.deck(0, DeckCommand::Load(plain));
    a.deck(0, DeckCommand::SetTempoFader(0.3));
    b.deck(0, DeckCommand::SetTempoFader(0.3));
    a.deck(0, DeckCommand::Play);
    b.deck(0, DeckCommand::Play);
    assert_eq!(a.render(200), b.render(200));
    // Mute and unmute again: after the ramp, back to bit-exact bypass.
    a.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
    a.render(50);
    a.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 1.0 });
    b.render(50);
    a.render(20);
    b.render(20);
    assert_eq!(a.render(100), b.render(100));
    let st = a.state().decks[0];
    assert!(st.stems_available);
    assert_eq!(st.stem_levels, [1.0; 4]);
}

#[test]
fn muting_each_part_removes_it_by_more_than_40db() {
    for (stem, f, others) in [
        (Stem::Vocals, FV, [FD, FO]),
        (Stem::Drums, FD, [FV, FO]),
        (Stem::Inst, FO, [FV, FD]),
    ] {
        let mut rig = playing_rig(false, 0.25); // +4 %: fractional resampling
        let reference = rig.render(400);
        rig.deck(0, DeckCommand::SetStemLevel { stem, level: 0.0 });
        rig.render(20); // 20 ms ramp
        let muted = rig.render(400);
        let st = rig.state().decks[0];
        assert_eq!(st.stem_levels[stem.index()], 0.0);
        let fr = f * 1.04;
        let removed = db(tone_level(&muted, fr), tone_level(&reference, fr));
        eprintln!("{stem:?} mute: {removed:.1} dB");
        assert!(removed < -40.0, "{stem:?}: only {removed:.1} dB");
        for o in others {
            let kept = db(tone_level(&muted, o * 1.04), tone_level(&reference, o * 1.04));
            assert!(kept.abs() < 0.5, "{stem:?} mute changed {o} Hz by {kept:.2} dB");
        }
    }
}

#[test]
fn half_level_and_bass_mute_on_silent_bass() {
    let mut rig = playing_rig(false, 0.0);
    let reference = rig.render(400);
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.5 });
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Bass, level: 0.0 });
    rig.render(20);
    let out = rig.render(400);
    let v = db(tone_level(&out, FV), tone_level(&reference, FV));
    assert!((v - -6.02).abs() < 0.1, "half level: {v:.2} dB");
    let d = db(tone_level(&out, FD), tone_level(&reference, FD));
    assert!(d.abs() < 0.1, "bass mute must not touch drums: {d:.2} dB");
}

#[test]
fn level_changes_are_click_free() {
    let mut rig = playing_rig(false, 0.0);
    let before = rig.render(10);
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Inst, level: 0.0 });
    let during = rig.render(20);
    let mut all = before;
    all.extend(during);
    // Second difference of the left channel: a step would show up as a
    // spike of the size of the removed signal (~0.5).
    let l: Vec<f32> = all.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
    let worst = l.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0f32, f32::max);
    assert!(worst < 0.4, "click: second difference {worst}");
    // Ramp takes about 20 ms, not one sample.
    let first = &all[10 * BLOCK * 2..11 * BLOCK * 2];
    assert!(tone_level(first, FV) > 0.05, "muted instantly");
}

#[test]
fn loops_and_reverse_play_the_stem_mix() {
    let mut rig = playing_rig(false, 0.0);
    let reference = rig.render(400);
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
    rig.deck(0, DeckCommand::AutoLoop { beats: 1.0 });
    rig.render(20);
    let looped = rig.render(800); // > 2 loop passes
    assert!(rig.state().decks[0].loop_state.active);
    assert!(db(tone_level(&looped, FV), tone_level(&reference, FV)) < -40.0);
    rig.deck(0, DeckCommand::LoopExit);
    rig.deck(0, DeckCommand::SetReverse(true));
    rig.render(10);
    let rev = rig.render(300);
    assert!(rig.state().decks[0].speed < 0.0);
    assert!(db(tone_level(&rev, FV), tone_level(&reference, FV)) < -40.0);
    assert!(db(tone_level(&rev, FD), tone_level(&reference, FD)).abs() < 0.5);
}

#[test]
fn scratching_plays_the_stem_mix() {
    let mut rig = playing_rig(false, 0.0);
    rig.deck(0, DeckCommand::Seek { seconds: 10.0 });
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Drums, level: 0.0 });
    rig.deck(0, DeckCommand::JogTouch(true));
    // Turn backwards at roughly −1× speed, block by block.
    let mut out = Vec::new();
    for _ in 0..300 {
        rig.deck(0, DeckCommand::JogTurn(-(BLOCK as f32 / SR as f32) / 1.8));
        out.extend_from_slice(rig.block(BLOCK));
    }
    assert!(rig.state().decks[0].scratching);
    let total = rms(&out[100 * BLOCK * 2..]);
    assert!(total > 0.05, "scratch is silent");
    // The drums tone is gone at any scratch speed near 1: compare a band
    // around FD with the vocals band.
    let tail = &out[100 * BLOCK * 2..];
    let drums: f64 = (2_900..3_300).step_by(25).map(|f| tone_level(tail, f as f64)).fold(0.0, f64::max);
    let vocals: f64 = (400..480).step_by(5).map(|f| tone_level(tail, f as f64)).fold(0.0, f64::max);
    assert!(db(drums, vocals) < -35.0, "drums at {:.1} dB vs vocals", db(drums, vocals));
}

#[test]
fn keylock_stretches_the_stem_mix() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    let mut rig = playing_rig(true, 0.5); // +8 %, pitch kept
    rig.render(100);
    let reference = rig.render(400);
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
    rig.render(100); // ramp + stretcher latency
    let muted = rig.render(400);
    let removed = db(tone_level(&muted, FV), tone_level(&reference, FV));
    eprintln!("keylock vocals mute: {removed:.1} dB");
    assert!(removed < -40.0, "keylock vocals mute only {removed:.1} dB");
    let kept = db(tone_level(&muted, FD), tone_level(&reference, FD));
    assert!(kept.abs() < 1.0, "keylock drums changed by {kept:.2} dB");
    // Re-prime (jump) with the mute active.
    rig.deck(0, DeckCommand::BeatJump { beats: 4.0 });
    rig.render(100);
    let jumped = rig.render(400);
    assert!(db(tone_level(&jumped, FV), tone_level(&reference, FV)) < -40.0);
}

#[test]
fn levels_without_stems_are_kept_but_inaudible() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
    rig.deck(0, DeckCommand::Load(sine_track(1, 5.0, FV, 0.5, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(20);
    let out = rig.render(100);
    assert!((tone_level(&out, FV) - 0.5).abs() < 0.01);
    let st = rig.state().decks[0];
    assert!(!st.stems_available);
    assert_eq!(st.stem_levels[Stem::Vocals.index()], 0.0);
}

#[test]
fn swap_track_data_attaches_stems_without_interruption() {
    let with = stem_track(7, 20.0, FV, FD, FO);
    let plain = track_from(7, with.samples.to_vec(), 120.0, 0.0);
    for keylock in [false, true] {
        if keylock && !keylock::Stretcher::AVAILABLE {
            continue;
        }
        let mut rig = Rig::new(2);
        rig.deck(0, DeckCommand::Load(plain.clone()));
        rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
        rig.deck(0, DeckCommand::SetTempoFader(0.25));
        rig.deck(0, DeckCommand::SetKeylock(keylock));
        rig.deck(0, DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
        rig.deck(0, DeckCommand::Play);
        rig.deck(0, DeckCommand::AutoLoop { beats: 4.0 });
        rig.render(200);
        let reference = rig.render(400);
        let before = rig.state().decks[0];
        assert!(!before.stems_available);
        // Another track's data is ignored.
        rig.deck(0, DeckCommand::SwapTrackData(stem_track(8, 20.0, FV, FD, FO)));
        rig.block(BLOCK);
        assert!(!rig.state().decks[0].stems_available);
        let p0 = rig.state().decks[0].position;
        rig.deck(0, DeckCommand::SwapTrackData(with.clone()));
        let around = rig.render(30);
        let st = rig.state().decks[0];
        assert!(st.stems_available && st.playing && st.loop_state.active);
        assert_eq!(st.keylock, keylock);
        let expect = p0 + 30.0 * BLOCK as f64 * 1.04 / SR as f64;
        let (s, e) = (st.loop_state.start.unwrap(), st.loop_state.end.unwrap());
        let wrapped = s + (expect - s).rem_euclid(e - s);
        assert!((st.position - wrapped).abs() < 1e-6, "position jumped: {} vs {wrapped}", st.position);
        // No click at the swap (the vocals fade out over 20 ms).
        let l: Vec<f32> = around.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        let worst = l.windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0f32, f32::max);
        assert!(worst < 0.4, "click at swap: {worst}");
        rig.render(100);
        let muted = rig.render(400);
        let fv = if keylock { FV } else { FV * 1.04 };
        let removed = db(tone_level(&muted, fv), tone_level(&reference, fv));
        assert!(removed < -40.0, "keylock={keylock}: vocals only {removed:.1} dB after swap");
    }
}

#[test]
fn vocal_regions_follow_the_vocal_stem() {
    // 20 s: vocals sing 2–8 s and 12–16 s, with a 0.5 s breath at 5 s.
    let sr = 48_000u32;
    let n = 20 * sr as usize;
    let sings = |t: f64| ((2.0..8.0).contains(&t) && !(5.0..5.5).contains(&t)) || (12.0..16.0).contains(&t);
    let vocals: Vec<f32> = (0..n)
        .flat_map(|i| {
            let t = i as f64 / sr as f64;
            let v = if sings(t) { (std::f64::consts::TAU * 440.0 * t).sin() as f32 * 0.3 } else { 0.0 };
            [v, v]
        })
        .collect();
    let silence = vec![0.0f32; n * 2];
    let st = StemData::new(&StemInput { sample_rate: sr, vocals, drums: silence.clone(), bass: silence }, sr);
    let r = st.vocal_regions(sr);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!((r[0].0 - 2.0).abs() < 0.3 && (r[0].1 - 8.0).abs() < 0.3, "breath bridged: {r:?}");
    assert!((r[1].0 - 12.0).abs() < 0.3 && (r[1].1 - 16.0).abs() < 0.3, "{r:?}");
}
