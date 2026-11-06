//! Offline-render tests: render blocks and assert on samples and state.

mod common;

use common::*;
use od_engine::*;

fn decode_pos(sample: f32) -> f64 {
    sample as f64 * RAMP_SCALE as f64
}

#[test]
fn position_advances_at_tempo_ratios() {
    for (fader, range, expected) in [
        (0.0f32, TempoRange::Ten, 1.0f64),
        (0.5, TempoRange::Ten, 1.05),
        (-1.0, TempoRange::Sixteen, 0.84),
        (0.25, TempoRange::Wide, 1.25),
        (1.0, TempoRange::Six, 1.06),
    ] {
        let mut rig = Rig::new(2);
        rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
        rig.deck(0, DeckCommand::SetTempoRange(range));
        rig.deck(0, DeckCommand::SetTempoFader(fader));
        rig.deck(0, DeckCommand::Play);
        let blocks = 200;
        let out = rig.render(blocks);
        let frames = (blocks * BLOCK) as f64;
        let st = rig.state();
        let pos = st.decks[0].position * SR as f64;
        assert!(
            (pos - frames * expected).abs() < 1e-6,
            "fader {fader}: pos {pos} vs {}",
            frames * expected
        );
        // Output sample k reads the track at k * speed.
        for k in [0usize, 1000, 12_345, blocks * BLOCK - 1] {
            let got = decode_pos(out[k * 2]);
            assert!((got - k as f64 * expected).abs() < 0.01, "k={k} got {got}");
        }
        assert!((st.decks[0].tempo - expected).abs() < 1e-9);
        assert!((st.decks[0].bpm - 120.0 * expected).abs() < 1e-6);
    }
}

#[test]
fn sync_aligns_bpm_and_phase_within_1ms() {
    let mut rig = Rig::new(2);
    let a = sine_track(1, 30.0, 220.0, 0.3, 128.0);
    // Deck B: 124 BPM, grid starting at 0.123 s.
    let n = 30 * SR as usize;
    let b_samples: Vec<f32> = (0..n).flat_map(|_| [0.1f32, 0.1]).collect();
    let b = track_from(2, b_samples, 124.0, 0.123);
    rig.deck(0, DeckCommand::Load(a));
    rig.deck(1, DeckCommand::Load(b));
    rig.deck(0, DeckCommand::SetTempoFader(0.3)); // master at 128 × 1.03
    rig.deck(0, DeckCommand::Play);
    rig.render(100);
    rig.deck(1, DeckCommand::Seek { seconds: 5.37 });
    rig.deck(1, DeckCommand::SetSync(true));
    rig.deck(1, DeckCommand::Play);
    for _ in 0..20 {
        rig.render(40);
        let st = rig.state();
        let (da, db) = (st.decks[0], st.decks[1]);
        assert_eq!(st.master_deck, Some(0));
        assert!(
            (da.bpm - db.bpm).abs() < 1e-6,
            "bpm {} vs {}",
            da.bpm,
            db.bpm
        );
        let mut dphase = (da.beat - db.beat).rem_euclid(1.0);
        if dphase > 0.5 {
            dphase -= 1.0;
        }
        let ms = dphase * 60.0 / da.bpm * 1000.0;
        assert!(ms.abs() < 1.0, "phase error {ms} ms");
    }
    // Master tempo change is followed.
    rig.deck(0, DeckCommand::SetTempoFader(-0.2));
    rig.render(200);
    let st = rig.state();
    assert!((st.decks[0].bpm - st.decks[1].bpm).abs() < 1e-6);
    assert!((st.decks[0].bpm - 128.0 * 0.98).abs() < 1e-6);
}

#[test]
fn loops_are_sample_accurate() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::AutoLoop { beats: 1.0 }); // [0, 24000)
    rig.deck(0, DeckCommand::Play);
    let blocks = 600; // 76800 frames, 3.2 loop iterations, 24000 is not a block multiple
    let out = rig.render(blocks);
    for k in 0..blocks * BLOCK {
        let got = decode_pos(out[k * 2]);
        let want = (k % 24_000) as f64;
        assert!(
            (got - want).abs() < 1e-3,
            "frame {k}: got {got} want {want}"
        );
    }
    let st = rig.state();
    assert!(st.decks[0].loop_state.active);
    assert_eq!(st.decks[0].loop_state.end, Some(0.5));

    // Fractional loop (1/32 beat = 750 frames) and halve/double.
    rig.deck(0, DeckCommand::AutoLoop { beats: 1.0 / 32.0 });
    rig.deck(0, DeckCommand::LoopDouble);
    let st_out = rig.render(1);
    let _ = st_out;
    let st = rig.state();
    let ls = st.decks[0].loop_state;
    assert_eq!(ls.beats, Some(1.0 / 16.0));
    assert!(((ls.end.unwrap() - ls.start.unwrap()) * SR as f64 - 1500.0).abs() < 1e-6);
    rig.deck(0, DeckCommand::LoopExit);
    rig.render(1);
    assert!(!rig.state().decks[0].loop_state.active);
}

#[test]
fn manual_loop_in_out_and_reloop() {
    let mut rig = Rig::new(2);
    rig.send(Command::SetQuantize(false));
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 1.0 });
    rig.deck(0, DeckCommand::Play);
    rig.deck(0, DeckCommand::LoopIn);
    rig.render(100); // 12800 frames
    rig.deck(0, DeckCommand::LoopOut);
    let out = rig.render(300);
    let (li, lo) = (48_000.0, 48_000.0 + 12_800.0);
    for k in 0..300 * BLOCK {
        let p = decode_pos(out[k * 2]);
        assert!(p >= li - 1e-3 && p < lo, "frame {k} at {p} outside loop");
    }
    rig.deck(0, DeckCommand::LoopExit);
    rig.render(200);
    assert!(rig.state().decks[0].position * SR as f64 > lo);
    rig.deck(0, DeckCommand::Reloop);
    let out = rig.render(1);
    assert!((decode_pos(out[0]) - li).abs() < 1e-3);
}

#[test]
fn hot_cue_quantize_set_and_trigger() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(7, 30.0, 120.0)));
    // 1.3 beats → snaps to beat 1 (0.5 s).
    rig.deck(0, DeckCommand::Seek { seconds: 0.65 });
    rig.deck(0, DeckCommand::SetHotCue { slot: 0 });
    rig.render(1);
    let st = rig.state();
    assert_eq!(st.decks[0].hot_cues[0].unwrap().start, 0.5);
    let ev: Vec<Event> = rig.engine.events().collect();
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::HotCueSet { slot: 0, .. }))
    );

    // Play from 10.1 s; trigger: the jump waits for the next beat (10.5 s).
    rig.deck(0, DeckCommand::Seek { seconds: 10.1 });
    rig.deck(0, DeckCommand::Play);
    rig.render(1);
    rig.deck(
        0,
        DeckCommand::HotCue {
            slot: 0,
            pressed: true,
        },
    );
    rig.deck(
        0,
        DeckCommand::HotCue {
            slot: 0,
            pressed: false,
        },
    );
    let out = rig.render(200);
    let start = 10.1 * SR as f64 + BLOCK as f64;
    let jump_at = (10.5 * SR as f64 - start) as usize; // output frame index of the jump
    assert!((decode_pos(out[(jump_at - 1) * 2]) - (10.5 * SR as f64 - 1.0)).abs() < 1e-3);
    assert!(
        (decode_pos(out[jump_at * 2]) - 0.5 * SR as f64).abs() < 1e-3,
        "lands exactly on the cue"
    );
    assert!(rig.state().decks[0].playing);

    // Pressing an empty slot while playing stores a cue (quantized).
    rig.deck(
        0,
        DeckCommand::HotCue {
            slot: 3,
            pressed: true,
        },
    );
    rig.render(1);
    let c = rig.state().decks[0].hot_cues[3].unwrap();
    assert!(
        (c.start * 2.0 - (c.start * 2.0).round()).abs() < 1e-9,
        "on a beat: {}",
        c.start
    );
    rig.deck(0, DeckCommand::DeleteHotCue { slot: 3 });
    rig.render(1);
    assert!(rig.state().decks[0].hot_cues[3].is_none());
}

#[test]
fn loop_hot_cue_activates_loop() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 20.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 2.0 });
    rig.deck(0, DeckCommand::AutoLoop { beats: 2.0 });
    rig.deck(0, DeckCommand::SetHotCue { slot: 1 });
    rig.deck(0, DeckCommand::LoopExit);
    rig.deck(0, DeckCommand::Seek { seconds: 8.0 });
    rig.render(1);
    let hc = rig.state().decks[0].hot_cues[1].unwrap();
    assert_eq!((hc.start, hc.loop_end), (2.0, Some(3.0)));
    rig.deck(
        0,
        DeckCommand::HotCue {
            slot: 1,
            pressed: true,
        },
    ); // paused: jump + preview
    rig.deck(0, DeckCommand::Play); // latch play
    rig.deck(
        0,
        DeckCommand::HotCue {
            slot: 1,
            pressed: false,
        },
    );
    rig.render(800);
    let st = rig.state().decks[0];
    assert!(st.playing && st.loop_state.active);
    assert!(st.position >= 2.0 && st.position < 3.0);
}

#[test]
fn slip_returns_to_the_right_position() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::SetSlip(true));
    rig.deck(0, DeckCommand::SetTempoFader(0.4)); // 1.04
    rig.deck(0, DeckCommand::Play);
    rig.render(375); // 1 s
    rig.deck(0, DeckCommand::AutoLoop { beats: 0.5 });
    rig.render(750);
    assert!(rig.state().decks[0].slip_position.is_some());
    rig.deck(0, DeckCommand::LoopExit);
    let out = rig.render(10);
    let elapsed = (375 + 750) as f64 * BLOCK as f64 * 1.04;
    assert!(
        (decode_pos(out[0]) - elapsed).abs() < 0.01,
        "got {} want {elapsed}",
        decode_pos(out[0])
    );

    // Loop roll returns even without slip mode; reverse with slip ("censor").
    rig.deck(0, DeckCommand::SetSlip(false));
    let p0 = rig.state().decks[0].position * SR as f64;
    rig.deck(
        0,
        DeckCommand::LoopRoll {
            beats: 0.25,
            on: true,
        },
    );
    rig.render(300);
    rig.deck(
        0,
        DeckCommand::LoopRoll {
            beats: 0.25,
            on: false,
        },
    );
    let out = rig.render(1);
    let want = p0 + 300.0 * BLOCK as f64 * 1.04;
    assert!(
        (decode_pos(out[0]) - want).abs() < 0.01,
        "roll: got {} want {want}",
        decode_pos(out[0])
    );
    assert!(!rig.state().decks[0].loop_state.active);

    rig.deck(0, DeckCommand::SetSlip(true));
    let p0 = rig.state().decks[0].position * SR as f64;
    rig.deck(0, DeckCommand::SetReverse(true));
    let out = rig.render(100);
    assert!(decode_pos(out[200]) < p0, "plays backwards");
    rig.deck(0, DeckCommand::SetReverse(false));
    let out = rig.render(1);
    let want = p0 + 100.0 * BLOCK as f64 * 1.04;
    assert!((decode_pos(out[0]) - want).abs() < 0.01);
}

#[test]
fn beat_jump_and_needle_search() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 4.2 });
    rig.deck(0, DeckCommand::BeatJump { beats: 4.0 });
    rig.render(1);
    assert!((rig.state().decks[0].position - 6.2).abs() < 1e-9);
    rig.deck(0, DeckCommand::BeatJump { beats: -16.0 });
    rig.render(1);
    assert!(
        (rig.state().decks[0].position - 0.0).abs() < 1e-9,
        "clamped at start"
    );
    rig.deck(0, DeckCommand::NeedleSearch { fraction: 0.5 });
    rig.render(1);
    assert!((rig.state().decks[0].position - 15.0).abs() < 1e-9);
}

#[test]
fn cdj_cue_button() {
    let mut rig = Rig::new(2);
    rig.send(Command::SetQuantize(false));
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 3.0 });
    rig.deck(0, DeckCommand::Cue(true)); // paused away from cue: sets cue
    rig.deck(0, DeckCommand::Cue(false));
    rig.render(1);
    let st = rig.state().decks[0];
    assert_eq!(st.cue_point, 3.0);
    assert!(!st.playing);
    // At cue: hold → preview, release → back to cue.
    rig.deck(0, DeckCommand::Cue(true));
    rig.render(100);
    assert!(rig.state().decks[0].playing);
    rig.deck(0, DeckCommand::Cue(false));
    rig.render(1);
    let st = rig.state().decks[0];
    assert!(!st.playing);
    assert_eq!(st.position, 3.0);
    // Hold cue, press play, release: keeps playing.
    rig.deck(0, DeckCommand::Cue(true));
    rig.deck(0, DeckCommand::Play);
    rig.deck(0, DeckCommand::Cue(false));
    rig.render(100);
    assert!(rig.state().decks[0].playing);
    // Playing: cue returns to cue point and pauses.
    rig.deck(0, DeckCommand::Cue(true));
    rig.deck(0, DeckCommand::Cue(false));
    rig.render(1);
    let st = rig.state().decks[0];
    assert!(!st.playing);
    assert_eq!(st.position, 3.0);
}

#[test]
fn scratch_follows_jog() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(ramp_track(1, 30.0, 120.0)));
    rig.deck(0, DeckCommand::Seek { seconds: 5.0 });
    rig.deck(0, DeckCommand::Play);
    rig.deck(0, DeckCommand::JogTouch(true));
    rig.render(10);
    let p0 = rig.state().decks[0].position;
    // Back half a revolution (0.9 s), then let the smoothing settle.
    rig.deck(0, DeckCommand::JogTurn(-0.5));
    rig.render(200);
    let p1 = rig.state().decks[0].position;
    assert!((p1 - (p0 - 0.9)).abs() < 0.01, "p0 {p0} p1 {p1}");
    assert!(rig.state().decks[0].scratching);
    rig.deck(0, DeckCommand::JogTouch(false));
    rig.render(100);
    let st = rig.state().decks[0];
    assert!(st.playing && !st.scratching);
    assert!((st.speed - 1.0).abs() < 1e-9);
}

#[test]
fn eq_kill_attenuates_more_than_40db() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 10.0, 50.0, 0.5, 120.0)));
    rig.deck(0, DeckCommand::Play);
    let reference = rms(&rig.render(400)[..]);
    rig.send(Command::Channel(
        0,
        ChannelCommand::SetEq {
            band: 0,
            value: -1.0,
        },
    ));
    rig.render(100); // let the 10 ms ramp and filter settle
    let killed = rms(&rig.render(400)[..]);
    let db = 20.0 * (killed / reference).log10();
    assert!(db < -40.0, "low kill only {db} dB");

    let mut rig = Rig::new(2);
    rig.deck(
        0,
        DeckCommand::Load(sine_track(1, 10.0, 12_000.0, 0.5, 120.0)),
    );
    rig.deck(0, DeckCommand::Play);
    let reference = rms(&rig.render(400)[..]);
    rig.send(Command::Channel(
        0,
        ChannelCommand::SetEq {
            band: 2,
            value: -1.0,
        },
    ));
    rig.render(100);
    let killed = rms(&rig.render(400)[..]);
    let db = 20.0 * (killed / reference).log10();
    assert!(db < -40.0, "high kill only {db} dB");
}

#[test]
fn crossfader_assign_and_curves() {
    let mut rig = Rig::new(2);
    rig.deck(
        0,
        DeckCommand::Load(sine_track(1, 10.0, 440.0, 0.25, 120.0)),
    );
    rig.deck(
        1,
        DeckCommand::Load(sine_track(2, 10.0, 660.0, 0.25, 120.0)),
    );
    rig.deck(0, DeckCommand::Play);
    rig.deck(1, DeckCommand::Play);
    rig.send(Command::Channel(
        0,
        ChannelCommand::SetCrossfaderAssign(XfAssign::A),
    ));
    rig.send(Command::Channel(
        1,
        ChannelCommand::SetCrossfaderAssign(XfAssign::B),
    ));
    rig.send(Command::Mixer(MixerCommand::SetCrossfader(-1.0)));
    rig.render(50);
    let st = rig.state();
    assert!((st.channels[0].gain_to_master - 1.0).abs() < 1e-6);
    assert!(st.channels[1].gain_to_master.abs() < 1e-6);
    let only_a = rig.render(100);
    assert!((estimate_freq(&only_a) - 440.0).abs() < 2.0);

    rig.send(Command::Mixer(MixerCommand::SetCrossfader(0.0)));
    rig.render(10);
    let st = rig.state();
    let (a, b) = (st.channels[0].gain_to_master, st.channels[1].gain_to_master);
    assert!((a * a + b * b - 1.0).abs() < 1e-4, "constant power centre");

    rig.send(Command::Mixer(MixerCommand::SetCrossfaderCurve(
        CrossfaderCurve::Cut,
    )));
    rig.send(Command::Mixer(MixerCommand::SetCrossfader(0.9)));
    rig.render(10);
    let st = rig.state();
    assert_eq!(
        (st.channels[0].gain_to_master, st.channels[1].gain_to_master),
        (1.0, 1.0)
    );

    // Thru ignores the crossfader.
    rig.send(Command::Channel(
        0,
        ChannelCommand::SetCrossfaderAssign(XfAssign::Thru),
    ));
    rig.send(Command::Mixer(MixerCommand::SetCrossfader(1.0)));
    rig.render(10);
    assert_eq!(rig.state().channels[0].gain_to_master, 1.0);
}

#[test]
fn meters_and_audible_track() {
    let mut rig = Rig::new(2);
    rig.deck(
        0,
        DeckCommand::Load(sine_track(11, 10.0, 1000.0, 0.5, 120.0)),
    );
    rig.deck(
        1,
        DeckCommand::Load(sine_track(12, 10.0, 800.0, 0.5, 120.0)),
    );
    rig.deck(0, DeckCommand::Play);
    rig.deck(1, DeckCommand::Play);
    rig.send(Command::Channel(1, ChannelCommand::SetFader(0.2)));
    rig.render(1000);
    let st = rig.state();
    let m = st.channels[0].meter;
    assert!((m.peak - 0.5).abs() < 0.01, "peak {}", m.peak);
    assert!((m.rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {}", m.rms);
    assert!(!m.clipped);
    assert!(st.master_meter.peak > 0.5);
    assert!(st.audible.audible[0] && st.audible.audible[1]);
    assert_eq!(st.audible.dominant, Some(0));
    rig.send(Command::Channel(0, ChannelCommand::SetFader(0.0)));
    rig.render(1000);
    let st = rig.state();
    assert!(!st.audible.audible[0]);
    assert_eq!(st.audible.dominant, Some(1));
    rig.deck(1, DeckCommand::Pause);
    rig.render(1);
    assert_eq!(rig.state().audible.dominant, None);
}

#[test]
fn headphone_cue_mix() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 10.0, 440.0, 0.3, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.send(Command::Channel(0, ChannelCommand::SetFader(0.0)));
    rig.send(Command::Channel(0, ChannelCommand::SetCue(true)));
    rig.render(100);
    rig.block(4800); // 44 whole cycles of 440 Hz
    let master = rms(&rig.master[..4800 * 2]);
    let phones = rms(&rig.phones[..4800 * 2]);
    assert!(master < 1e-6, "fader down: master silent");
    assert!(
        (phones - 0.3 / 2f32.sqrt()).abs() < 0.01,
        "PFL in headphones: {phones}"
    );
}

#[test]
fn record_tap_matches_master() {
    let mut rig = Rig::new(2);
    let mut tap = rig.engine.take_record_tap().unwrap();
    rig.deck(0, DeckCommand::Load(sine_track(1, 10.0, 440.0, 0.3, 120.0)));
    rig.deck(0, DeckCommand::Play);
    rig.send(Command::SetRecordTap(true));
    let out = rig.render(50);
    let mut rec = Vec::new();
    tap.drain_into(&mut rec);
    assert_eq!(rec.len(), out.len());
    assert_eq!(rec, out);
}

#[test]
fn keylock_keeps_pitch() {
    let mut rig = Rig::new(2);
    rig.deck(
        0,
        DeckCommand::Load(sine_track(1, 20.0, 1000.0, 0.5, 120.0)),
    );
    rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
    rig.deck(0, DeckCommand::SetTempoFader(0.5)); // +8 %
    rig.deck(0, DeckCommand::Play);
    let plain = rig.render(400);
    let f_plain = estimate_freq(&plain[100 * BLOCK * 2..]);
    assert!((f_plain - 1080.0).abs() < 2.0, "without keylock: {f_plain}");
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    rig.deck(0, DeckCommand::SetKeylock(true));
    rig.render(100);
    let locked = rig.render(400);
    let f_locked = estimate_freq(&locked);
    assert!((f_locked - 1000.0).abs() < 5.0, "with keylock: {f_locked}");
    // Position still advances at +8 %.
    let st = rig.state().decks[0];
    let expect = 900.0 * BLOCK as f64 * 1.08 / SR as f64;
    assert!((st.position - expect).abs() < 1e-6);
}

#[test]
fn keylock_lite_profile() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    let mut rig = Rig::with(EngineConfig {
        sample_rate: SR,
        max_block: 256,
        decks: 2,
        profile: Profile::Lite,
    });
    rig.deck(
        0,
        DeckCommand::Load(sine_track(1, 20.0, 1000.0, 0.5, 120.0)),
    );
    rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
    rig.deck(0, DeckCommand::SetTempoFader(-0.5)); // −8 %
    rig.deck(0, DeckCommand::SetKeylock(true));
    rig.deck(0, DeckCommand::Play);
    rig.render(100);
    let f = estimate_freq(&rig.render(400));
    assert!((f - 1000.0).abs() < 5.0, "lite keylock: {f}");
}

#[test]
fn keylock_stays_aligned_with_position() {
    if !keylock::Stretcher::AVAILABLE {
        return;
    }
    // A click every beat; with keylock the click must stay at the logical
    // beat position (no drift between the stretcher and the playhead).
    let n = 40 * SR as usize;
    let mut s = vec![0f32; n * 2];
    for beat in 0..80 {
        let i = beat * SR as usize / 2;
        for k in 0..64 {
            s[(i + k) * 2] = 0.8;
            s[(i + k) * 2 + 1] = 0.8;
        }
    }
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(track_from(1, s, 120.0, 0.0)));
    rig.deck(0, DeckCommand::SetTempoRange(TempoRange::Sixteen));
    rig.deck(0, DeckCommand::SetTempoFader(0.5));
    rig.deck(0, DeckCommand::SetKeylock(true));
    rig.deck(0, DeckCommand::Play);
    let mut worst: f64 = 0.0;
    for _ in 0..60 {
        let before = rig.state().decks[0];
        let out = rig.render(25); // 3200 frames
        let _ = before;
        // Each click onset in this chunk: compare with the beat time.
        let mut i = 0;
        while i < out.len() / 2 {
            if out[i * 2].abs() > 0.3 {
                let st_beat = before.beat + (i as f64 * 1.08 / (SR as f64 / 2.0));
                let err = (st_beat - st_beat.round()) * 500.0 / 1.08; // ms
                worst = worst.max(err.abs());
                i += 2000;
            } else {
                i += 1;
            }
        }
    }
    eprintln!("keylock audio vs playhead: worst offset {worst:.3} ms");
    assert!(worst < 5.0, "keylock audio vs playhead offset {worst} ms");
}

#[test]
fn sampler_and_beat_fx_run() {
    let mut rig = Rig::new(4);
    let sample = SampleData::new(
        od_decode::DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples: vec![0.5; 2000],
        },
        SR,
    );
    rig.send(Command::Sampler(SamplerCommand::Load { slot: 3, sample }));
    rig.send(Command::Sampler(SamplerCommand::Trigger { slot: 3 }));
    let out = rig.render(1);
    assert!((out[0] - 0.5).abs() < 1e-6);
    assert!(rig.state().sampler[3].playing);
    rig.render(10);
    assert!(!rig.state().sampler[3].playing, "one-shot ends");

    rig.deck(0, DeckCommand::Load(sine_track(1, 10.0, 440.0, 0.3, 120.0)));
    rig.deck(0, DeckCommand::Play);
    for kind in [
        BeatFxKind::Echo,
        BeatFxKind::Delay,
        BeatFxKind::Reverb,
        BeatFxKind::Flanger,
        BeatFxKind::Phaser,
        BeatFxKind::Roll,
        BeatFxKind::Filter,
        BeatFxKind::Trans,
    ] {
        rig.send(Command::BeatFx(0, BeatFxCommand::SetKind(kind)));
        rig.send(Command::BeatFx(0, BeatFxCommand::SetOn(true)));
        rig.send(Command::BeatFx(0, BeatFxCommand::SetLevel(0.8)));
        let out = rig.render(100);
        assert!(
            out.iter().all(|v| v.is_finite() && v.abs() < 4.0),
            "{kind:?} output sane"
        );
        rig.send(Command::BeatFx(0, BeatFxCommand::SetOn(false)));
    }
    for kind in [
        ColorFxKind::Filter,
        ColorFxKind::Noise,
        ColorFxKind::Space,
        ColorFxKind::Crush,
        ColorFxKind::Echo,
    ] {
        rig.send(Command::Channel(0, ChannelCommand::SetColorFx(kind)));
        rig.send(Command::Channel(0, ChannelCommand::SetColorFxAmount(-0.7)));
        let out = rig.render(50);
        assert!(out.iter().all(|v| v.is_finite()));
        rig.send(Command::Channel(0, ChannelCommand::SetColorFxAmount(0.7)));
        let out = rig.render(50);
        assert!(out.iter().all(|v| v.is_finite()));
    }
}

#[test]
fn trans_gate_follows_master_beat() {
    let mut rig = Rig::new(2);
    let n = 10 * SR as usize;
    rig.deck(
        0,
        DeckCommand::Load(track_from(1, vec![0.5; n * 2], 120.0, 0.0)),
    );
    rig.deck(0, DeckCommand::Play);
    rig.send(Command::BeatFx(
        0,
        BeatFxCommand::SetKind(BeatFxKind::Trans),
    ));
    rig.send(Command::BeatFx(0, BeatFxCommand::SetBeats(1.0)));
    rig.send(Command::BeatFx(0, BeatFxCommand::SetLevel(1.0)));
    rig.send(Command::BeatFx(0, BeatFxCommand::SetOn(true)));
    let out = rig.render(375 * 2); // 2 s = 4 beats
    // Second half of each beat (12000..24000 within a 24000-frame beat) is gated.
    for beat in 1..4 {
        let on = out[(beat * 24_000 + 6_000) * 2];
        let off = out[(beat * 24_000 + 18_000) * 2];
        assert!(
            on > 0.45 && off.abs() < 0.01,
            "beat {beat}: on {on} off {off}"
        );
    }
}

#[test]
fn track_end_stops_and_reports() {
    let mut rig = Rig::new(2);
    rig.deck(1, DeckCommand::Load(ramp_track(5, 0.1, 120.0)));
    rig.deck(1, DeckCommand::Play);
    rig.render(100);
    let st = rig.state().decks[1];
    assert!(!st.playing && st.at_end);
    assert!(
        rig.engine
            .events()
            .any(|e| e == Event::TrackEnded { deck: 1 })
    );
}

#[test]
fn additional_sender_and_garbage() {
    let mut rig = Rig::new(2);
    let mut s = rig.engine.new_sender().unwrap();
    rig.render(1);
    s.send(Command::Deck(
        0,
        DeckCommand::Load(ramp_track(1, 1.0, 120.0)),
    ))
    .unwrap();
    rig.render(1);
    assert_eq!(rig.state().decks[0].track, Some(od_core::TrackId(1)));
    // Replacing the track hands the old one back for freeing off-thread.
    s.send(Command::Deck(
        0,
        DeckCommand::Load(ramp_track(2, 1.0, 120.0)),
    ))
    .unwrap();
    rig.render(1);
    drop(s);
    rig.render(1);
    rig.engine.collect_garbage();
    assert_eq!(rig.state().garbage_leaked, 0);
}
