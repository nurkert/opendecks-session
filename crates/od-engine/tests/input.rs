//! Microphone / aux input channel.

mod common;
use common::*;
use od_engine::*;

fn input(rig: &mut Rig, c: InputCommand) {
    rig.send(Command::Input(c));
}

/// A mono capture ring pre-filled with `frames` samples of `value`.
fn mono_ring(frames: usize, value: f32) -> rtrb::Consumer<f32> {
    let (mut tx, rx) = rtrb::RingBuffer::new(frames * 2);
    for _ in 0..frames {
        tx.push(value).unwrap();
    }
    rx
}

#[test]
fn mono_input_reaches_master_and_cue_at_its_level() {
    let mut rig = Rig::new(2);
    input(&mut rig, InputCommand::Attach(mono_ring(BLOCK * 3, 0.5), 1));
    // Attached but off: silent.
    let out = rig.render(1);
    assert!(out.iter().all(|&v| v == 0.0));
    input(&mut rig, InputCommand::SetLevel(1.0));
    input(&mut rig, InputCommand::SetOn(true));
    rig.render(1); // gain ramp (10 ms ≈ 480 frames) over the first blocks
    rig.render(3);
    let st = rig.state().input;
    assert!(st.attached && st.on);
    assert!(st.underruns > 0, "ring ran dry after its samples");

    // A live source: one block arrives per block rendered.
    let mut rig = Rig::new(2);
    let (mut tx, rx) = rtrb::RingBuffer::new(BLOCK * 4);
    input(&mut rig, InputCommand::Attach(rx, 1));
    input(&mut rig, InputCommand::SetLevel(1.0));
    input(&mut rig, InputCommand::SetOn(true));
    input(&mut rig, InputCommand::SetCue(true));
    rig.send(Command::Mixer(MixerCommand::SetHeadphoneMix(0.0)));
    let mut out = Vec::new();
    for _ in 0..6 {
        for _ in 0..BLOCK {
            tx.push(0.5).unwrap();
        }
        out = rig.render(1);
    }
    // Mono duplicated to both sides at unity gain.
    assert!(
        out.iter().all(|&v| (v - 0.5).abs() < 1e-6),
        "{:?}",
        &out[..4]
    );
    assert!(
        rig.phones[..BLOCK * 2]
            .iter()
            .all(|&v| (v - 0.5).abs() < 1e-3)
    );
    assert!(rig.state().input.meter.peak > 0.4);
}

#[test]
fn talkover_ducks_the_music_while_the_mic_is_live() {
    let mut rig = Rig::new(2);
    rig.deck(0, DeckCommand::Load(sine_track(1, 10.0, 440.0, 0.5, 120.0)));
    rig.send(Command::Channel(0, ChannelCommand::SetFader(1.0)));
    rig.deck(0, DeckCommand::Play);
    rig.render(20);
    let music = rms(&rig.render(20));
    assert!(music > 0.1);

    // A mic ring with silence first, then a voice-level signal.
    let (mut tx, rx) = rtrb::RingBuffer::new(48_000 * 2);
    input(&mut rig, InputCommand::Attach(rx, 2));
    input(&mut rig, InputCommand::SetLevel(1.0));
    input(&mut rig, InputCommand::SetOn(true));
    input(&mut rig, InputCommand::SetTalkover(true));
    input(&mut rig, InputCommand::SetTalkoverDepth(-20.0));
    // Silent mic: no ducking.
    for _ in 0..BLOCK * 20 {
        tx.push(0.0).unwrap();
        tx.push(0.0).unwrap();
    }
    let quiet = rms(&rig.render(20));
    assert!(
        (quiet - music).abs() / music < 0.05,
        "no ducking without voice: {quiet} vs {music}"
    );
    assert!(!rig.state().input.ducking);

    // Voice: an alternating ±0.1 signal (cancels in the RMS of the sum
    // only partly, so measure the ducking from the state and level drop).
    for i in 0..BLOCK * 40 {
        let v = if i % 2 == 0 { 0.1 } else { -0.1 };
        tx.push(v).unwrap();
        tx.push(v).unwrap();
    }
    rig.render(20);
    assert!(rig.state().input.ducking);
    let ducked = rms(&rig.render(10));
    assert!(ducked < music * 0.5, "music ducked: {ducked} vs {music}");
}

#[test]
fn backlog_is_skipped_to_keep_latency_low() {
    let mut rig = Rig::new(2);
    // Ten blocks waiting: only the last three are kept.
    let (mut tx, rx) = rtrb::RingBuffer::new(BLOCK * 20);
    for i in 0..BLOCK * 10 {
        tx.push(if i < BLOCK * 7 { 0.9 } else { 0.25 }).unwrap();
    }
    input(&mut rig, InputCommand::Attach(rx, 1));
    input(&mut rig, InputCommand::SetLevel(1.0));
    input(&mut rig, InputCommand::SetOn(true));
    rig.render(1);
    let out = rig.render(1);
    // The stale 0.9 part was dropped (gain ramp over: 0.25 only).
    assert!(out.iter().all(|&v| v <= 0.25 + 1e-6), "{:?}", &out[..4]);
}

#[test]
fn detach_hands_the_ring_back_for_dropping() {
    let mut rig = Rig::new(2);
    input(&mut rig, InputCommand::Attach(mono_ring(BLOCK, 0.1), 1));
    rig.render(1);
    assert!(rig.state().input.attached);
    input(&mut rig, InputCommand::Detach);
    rig.render(1);
    assert!(!rig.state().input.attached);
}
