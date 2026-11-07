//! Offline-render tests for key shift / key sync, keyboard and slicer pad
//! modes, hot cue banks, censor, beat jump size, auto cue and loops.

mod common;

use common::*;
use od_core::{Beatgrid, Cue, Mode, MusicalKey, TrackId};
use od_decode::DecodedAudio;
use od_engine::*;

fn decode_pos(sample: f32) -> f64 {
    sample as f64 * RAMP_SCALE as f64
}

fn semis(f: f64, st: f64) -> f64 {
    f * 2f64.powf(st / 12.0)
}

#[test]
fn key_shift_octave_doubles_the_pitch() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    for keylock in [true, false] {
        let mut rig = Rig::new(2);
        rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 500.0, 0.5, 120.0)));
        rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
        rig.deck(0, DeckCommand::SetTempoFader(0.5)); // +8 %
        rig.deck(0, DeckCommand::SetKeylock(keylock));
        rig.deck(0, DeckCommand::SetKeyShift(12.0));
        rig.deck(0, DeckCommand::Play);
        rig.render(150);
        let out = rig.render(300);
        let want = if keylock { 1000.0 } else { 1080.0 };
        let f = peak_freq(&out, want * 0.8, want * 1.2);
        assert!((f - want).abs() / want < 0.005, "keylock {keylock}: {f} vs {want}");
        let st = rig.state().decks[0];
        let expect_pos = 450.0 * BLOCK as f64 * 1.08 / SR as f64;
        assert!((st.position - expect_pos).abs() < 1e-6, "position follows tempo");
        let pitch = if keylock { 12.0 } else { 12.0 + 12.0 * 1.08f64.log2() };
        assert!((st.pitch_semitones as f64 - pitch).abs() < 1e-3);
    }
}

#[test]
fn key_sync_matches_the_master_key() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    let key = |tonic, mode| Some(MusicalKey::new(tonic, mode));
    // (follower key, follower keylock, fader, expected shift)
    let cases = [
        (key(2, Mode::Major), true, 0.0f32, -2.0f64), // D → C
        (key(9, Mode::Minor), true, 0.0, 0.0),        // A minor = relative of C
        (key(7, Mode::Major), true, 0.0, 5.0),        // G → C: +5 (smallest)
        // Without keylock the +6 % tempo raises the pitch ~1.01 st first.
        (key(2, Mode::Major), false, 0.6, -2.0 - 12.0 * 1.06f64.log2()),
    ];
    for (k, keylock, fader, shift) in cases {
        let mut rig = Rig::new(2);
        rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 500.0, 0.5, 120.0)));
        rig.deck(0, DeckCommand::SetTrackKey(key(0, Mode::Major)));
        rig.deck(1, DeckCommand::Load(sine_track(2, 20.0, 500.0, 0.5, 120.0)));
        rig.deck(1, DeckCommand::SetTrackKey(k));
        rig.deck(1, DeckCommand::SetKeylock(keylock));
        rig.deck(1, DeckCommand::SetTempoFader(fader));
        rig.send(Command::SetMasterDeck(Some(0)));
        rig.deck(0, DeckCommand::Play);
        rig.deck(1, DeckCommand::Play);
        rig.deck(1, DeckCommand::SetKeySync(true));
        rig.send(Command::Channel(0, ChannelCommand::SetFader(0.0)));
        rig.render(150);
        let st = rig.state().decks[1];
        assert!(st.key_sync);
        assert!((st.key_shift as f64 - shift).abs() < 1e-3, "{k:?}: shift {}", st.key_shift);
        // The audible pitch now sits on a key compatible with the master.
        let out = rig.render(300);
        let tempo_st = if keylock { 0.0 } else { 12.0 * 1.06f64.log2() };
        let want = semis(500.0, shift + tempo_st);
        let f = peak_freq(&out, want * 0.85, want * 1.15);
        assert!((f - want).abs() / want < 0.005, "{k:?}: {f} vs {want}");
        // Setting a shift by hand ends key sync.
        rig.deck(1, DeckCommand::SetKeyShift(0.0));
        rig.render(1);
        assert!(!rig.state().decks[1].key_sync);
    }
}

#[test]
fn keyboard_pads_play_the_hot_cue_at_semitone_offsets() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 20.0, 500.0, 0.5, 120.0)));
    rig.deck(0, DeckCommand::SetHotCueAt { slot: 0, start: 2.0, loop_end: None });
    rig.deck(0, DeckCommand::Play);
    rig.render(100);
    rig.deck(0, DeckCommand::Keyboard { slot: 0, semitones: 7.0, pressed: true });
    rig.render(1);
    let st = rig.state().decks[0];
    assert!((st.position - (2.0 + BLOCK as f64 / SR as f64)).abs() < 1e-6, "jumped at once: {}", st.position);
    assert_eq!(st.key_shift, 7.0);
    rig.deck(0, DeckCommand::Keyboard { slot: 0, semitones: 7.0, pressed: false });
    rig.render(150);
    let out = rig.render(300);
    let want = semis(500.0, 7.0);
    let f = peak_freq(&out, want * 0.9, want * 1.1);
    assert!((f - want).abs() / want < 0.005, "fifth up: {f}");
    assert!(rig.state().decks[0].playing, "playing on after release");
}

#[test]
fn hot_cue_banks() {
    let mut rig = Rig::new(2);
    let n = 10 * SR as usize;
    let cues = [Cue::hot(1, 1.0), Cue::hot(9, 2.0), Cue::hot(31, 3.0)];
    let t = TrackData::new(
        TrackId(1),
        DecodedAudio { sample_rate: SR, channels: 2, samples: vec![0.1; n * 2] },
        SR,
        Beatgrid::constant(120.0, 0.0, 1, 10.0),
        &cues,
        0.0,
    );
    rig.deck(0, DeckCommand::Load(t));
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!(st.hot_cue_bank, 0);
    assert_eq!(st.hot_cues[1].map(|c| c.start), Some(1.0));
    assert_eq!(st.hot_cue_banks[1][1].map(|c| c.start), Some(2.0));
    assert_eq!(st.hot_cue_banks[3][7].map(|c| c.start), Some(3.0));

    rig.deck(0, DeckCommand::SetHotCueBank(1));
    rig.deck(0, DeckCommand::SetHotCueAt { slot: 0, start: 4.0, loop_end: None });
    rig.deck(0, DeckCommand::HotCue { slot: 1, pressed: true });
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!(st.hot_cue_bank, 1);
    assert_eq!(st.hot_cues[0].map(|c| c.start), Some(4.0));
    assert!(st.hot_cue_banks[0][0].is_none(), "bank 0 untouched");
    assert!((st.position - 2.0 - BLOCK as f64 / SR as f64).abs() < 1e-6, "bank 1 slot B = 2.0 s");
    let set: Vec<_> = rig
        .engine
        .events()
        .filter_map(|e| match e {
            Event::HotCueSet { bank, slot, cue, .. } => Some((bank, slot, cue.start)),
            _ => None,
        })
        .collect();
    assert_eq!(set, vec![(1, 0, 4.0)]);
    rig.deck(0, DeckCommand::SetHotCueBank(0));
    rig.render(1);
    assert!(rig.state().decks[0].hot_cues[0].is_none());
}

#[test]
fn active_censor_ends_after_its_beats() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(375); // 48 000 frames
    // Two beats at 120 BPM = 1 s = 48 000 frames.
    rig.deck(0, DeckCommand::CensorFor { beats: 2.0 });
    rig.render(370); // 47 360 frames: still censoring
    assert!(rig.state().decks[0].censor);
    rig.render(10);
    let st = rig.state().decks[0];
    assert!(!st.censor && !st.reverse, "ended after two beats");
    // Resumed where playback would be (slip).
    assert!((st.position * 48_000.0 - (48_000.0 + 380.0 * 128.0)).abs() < 200.0, "position {}", st.position * 48_000.0);
}

#[test]
fn censor_reverses_and_resumes_where_playback_would_be() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(375); // 48 000 frames
    rig.deck(0, DeckCommand::Censor(true));
    let out = rig.render(100);
    // Plays backwards from 48 000.
    for k in [10usize, 5000, 12_000] {
        let p = decode_pos(out[k * 2]);
        assert!((p - (48_000.0 - k as f64)).abs() < 0.01, "reverse at {k}: {p}");
    }
    let st = rig.state().decks[0];
    assert!(st.censor && st.reverse && !st.slip);
    rig.deck(0, DeckCommand::Censor(false));
    let out = rig.render(1);
    let resumed = decode_pos(out[0]);
    assert!((resumed - (48_000.0 + 12_800.0)).abs() < 0.01, "resumed at {resumed}");
    let st = rig.state().decks[0];
    assert!(!st.censor && !st.reverse);
}

#[test]
fn search_runs_the_playhead_fast_while_held() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 10.0 });
    // Paused deck: FF moves at 4x real time and is audible.
    rig.deck(0, DeckCommand::Search(4.0));
    let out = rig.render(375); // one second
    assert!((rig.state().decks[0].position - 14.0).abs() < 0.01, "{}", rig.state().decks[0].position);
    assert!(out.iter().any(|s| *s != 0.0));
    // Release: stays where search left it, still paused.
    rig.deck(0, DeckCommand::Search(0.0));
    rig.render(375);
    let st = rig.state().decks[0];
    assert!(!st.playing && (st.position - 14.0).abs() < 0.01);
    // REW while playing, then playback continues at 1x from there.
    rig.deck(0, DeckCommand::Play);
    rig.deck(0, DeckCommand::Search(-4.0));
    rig.render(375);
    assert!((rig.state().decks[0].position - 10.0).abs() < 0.02, "{}", rig.state().decks[0].position);
    rig.deck(0, DeckCommand::Search(0.0));
    rig.render(375);
    assert!((rig.state().decks[0].position - 11.0).abs() < 0.02, "{}", rig.state().decks[0].position);
}

#[test]
fn beat_jump_size_setting() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 10.0 });
    rig.deck(0, DeckCommand::SetBeatJumpSize(8.0));
    rig.deck(0, DeckCommand::BeatJumpStep { forward: true });
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!(st.beat_jump_size, 8.0);
    assert!((st.position - 14.0).abs() < 1e-9);
    rig.deck(0, DeckCommand::SetBeatJumpSize(0.5));
    rig.deck(0, DeckCommand::BeatJumpStep { forward: false });
    rig.render(1);
    assert!((rig.state().decks[0].position - 13.75).abs() < 1e-9);
}

fn silence_then_tone(id: i64, cues: &[Cue]) -> std::sync::Arc<TrackData> {
    let n = 10 * SR as usize;
    let mut s = vec![0f32; n * 2];
    for i in SR as usize..n {
        let v = (std::f64::consts::TAU * 440.0 * i as f64 / SR as f64).sin() as f32 * 0.5;
        s[i * 2] = v;
        s[i * 2 + 1] = v;
    }
    TrackData::new(
        TrackId(id),
        DecodedAudio { sample_rate: SR, channels: 2, samples: s },
        SR,
        Beatgrid::constant(120.0, 0.0, 1, 10.0),
        cues,
        0.0,
    )
}

#[test]
fn auto_cue_on_load() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::SetAutoCue(true));
    rig.deck(0, DeckCommand::Load(silence_then_tone(1, &[])));
    rig.render(1);
    let st = rig.state().decks[0];
    assert!(st.auto_cue);
    // First sample above −54 dBFS of a 440 Hz sine starting at 1 s.
    assert!((st.position - 1.0).abs() < 0.0002, "first sound {}", st.position);
    assert_eq!(st.cue_point, st.position);

    rig.deck(0, DeckCommand::Load(silence_then_tone(2, &[Cue::memory(2.5)])));
    rig.render(1);
    assert_eq!(rig.state().decks[0].position, 2.5, "first memory cue");

    rig.deck(0, DeckCommand::SetAutoCue(false));
    rig.deck(0, DeckCommand::Load(silence_then_tone(3, &[Cue::memory(2.5)])));
    rig.render(1);
    assert_eq!(rig.state().decks[0].position, 0.0, "auto cue off");
}

#[test]
fn active_loop_engages_sample_accurately() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::SetActiveLoop(Some((1.0, 1.5))));
    rig.deck(0, DeckCommand::Play);
    let blocks = 1000; // 128 000 frames
    let out = rig.render(blocks);
    for k in 0..blocks * BLOCK {
        let want = if k < 48_000 { k } else { 48_000 + (k - 48_000) % 24_000 };
        let got = decode_pos(out[k * 2]);
        assert!((got - want as f64).abs() < 1e-3, "frame {k}: {got} vs {want}");
    }
    let st = rig.state().decks[0];
    assert!(st.loop_state.active);
    assert_eq!(st.active_loop, Some((1.0, 1.5)));
    // Re-loading disarms it.
    rig.deck(0, DeckCommand::Load(ramp_track(2, 20.0, 120.0)));
    rig.render(1);
    assert_eq!(rig.state().decks[0].active_loop, None);
}

#[test]
fn saved_loops_are_recalled() {
    let mut rig = Rig::new(2);
    let mut loop1 = Cue::memory(3.0);
    loop1.loop_end = Some(3.5);
    let mut loop2 = Cue::memory(5.0);
    loop2.loop_end = Some(5.25);
    let n = 10 * SR as usize;
    let t = TrackData::new(
        TrackId(1),
        DecodedAudio { sample_rate: SR, channels: 2, samples: vec![0.1; n * 2] },
        SR,
        Beatgrid::constant(120.0, 0.0, 1, 10.0),
        &[Cue::memory(1.0), loop1, loop2],
        0.0,
    );
    rig.deck(0, DeckCommand::Load(t));
    rig.deck(0, DeckCommand::SavedLoop(MemoryCueJump::Index(1)));
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!((st.loop_state.start, st.loop_state.end), (Some(5.0), Some(5.25)));
    assert!(st.loop_state.active && st.position == 5.0);
    rig.deck(0, DeckCommand::SavedLoop(MemoryCueJump::Previous));
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!((st.loop_state.start, st.loop_state.end), (Some(3.0), Some(3.5)));
    assert_eq!(st.cue_point, 3.0);
    // Auto beat loops go up to 512 beats.
    rig.deck(0, DeckCommand::AutoLoop { beats: 64.0 });
    rig.render(1);
    assert_eq!(rig.state().decks[0].loop_state.beats, Some(64.0));
}

#[test]
fn slicer_retriggers_slices_on_the_grid() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(1744); // 223 232 frames = beat 9.301
    let at = 1744 * BLOCK;
    rig.deck(0, DeckCommand::Slicer(SlicerCommand::Enable { beats: 8.0, loop_mode: false }));
    rig.render(1);
    let st = rig.state().decks[0].slicer;
    assert!(st.active && st.start == 4.0 && st.end == 8.0, "{st:?}");
    assert_eq!(st.current, 1);

    rig.deck(0, DeckCommand::Slicer(SlicerCommand::Pad { slice: 2, pressed: true }));
    let held_blocks = 400; // 51 200 frames: more than two slice lengths
    let out = rig.render(held_blocks);
    // Same phase within slice 2 (beats 10..11 = frames 240 000..264 000).
    let phase = (at + BLOCK) as f64 - 8.0 * 24_000.0;
    let first = decode_pos(out[0]);
    assert!((first - (240_000.0 + phase % 24_000.0)).abs() < 0.01, "first {first}");
    let mut wraps = 0;
    for k in 0..held_blocks * BLOCK {
        let p = decode_pos(out[k * 2]);
        assert!((240_000.0..264_000.0).contains(&p), "inside the slice at {k}: {p}");
        if k > 0 {
            let prev = decode_pos(out[k * 2 - 2]);
            if p < prev {
                // Retrigger exactly at the slice boundary.
                assert!((prev - 263_999.0).abs() < 0.01 && (p - 240_000.0).abs() < 0.01);
                wraps += 1;
            }
        }
    }
    assert_eq!(wraps, 2);
    assert_eq!(rig.state().decks[0].slicer.held, Some(2));

    rig.deck(0, DeckCommand::Slicer(SlicerCommand::Pad { slice: 2, pressed: false }));
    let out = rig.render(1);
    let resumed = decode_pos(out[0]);
    let expect = (at + BLOCK + held_blocks * BLOCK) as f64;
    assert!((resumed - expect).abs() < 0.01, "slip resume {resumed} vs {expect}");
    let st = rig.state().decks[0];
    assert!(!st.loop_state.active && st.slicer.held.is_none());

    // The domain moves on with playback.
    rig.render(1000);
    let st = rig.state().decks[0].slicer;
    assert!(st.start == 8.0 && st.end == 12.0, "next domain {st:?}");

    // Loop mode loops the domain; disabling ends it.
    rig.deck(0, DeckCommand::Slicer(SlicerCommand::Enable { beats: 4.0, loop_mode: true }));
    rig.render(1);
    let st = rig.state().decks[0];
    assert!(st.loop_state.active && st.slicer.loop_mode);
    assert_eq!((st.loop_state.start, st.loop_state.end), (Some(st.slicer.start), Some(st.slicer.end)));
    rig.deck(0, DeckCommand::Slicer(SlicerCommand::Disable));
    rig.render(1);
    let st = rig.state().decks[0];
    assert!(!st.loop_state.active && !st.slicer.active);
}
