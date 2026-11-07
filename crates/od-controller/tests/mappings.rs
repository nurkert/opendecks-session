//! Mapping tests driven by synthetic MIDI byte streams (SPEC §14).

use od_controller::action::*;
use od_controller::feedback::*;
use od_controller::mapping::Mapping;
use od_controller::monitor::{Direction, MidiMonitor};
use od_controller::session::ControllerSession;
use od_controller::MidiAddr;
use od_core::CueColor;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn load(name: &str) -> Arc<Mapping> {
    let p = od_controller::bundled_mappings_dir().join("pioneer").join(name);
    Arc::new(Mapping::load(&p).unwrap_or_else(|e| panic!("{name}: {e}")))
}

fn flx4() -> ControllerSession {
    ControllerSession::new(load("ddj-flx4.toml"))
}

fn flx10() -> ControllerSession {
    ControllerSession::new(load("ddj-flx10.toml"))
}

/// Feeds a stream of messages and collects all produced actions.
fn feed(s: &mut ControllerSession, msgs: &[[u8; 3]]) -> Vec<ControlAction> {
    msgs.iter().flat_map(|m| s.handle_midi(m).actions).collect()
}

/// Polls until the initial "all off" burst is out (it can exceed one poll's budget).
fn drain(s: &mut ControllerSession, t: Instant) {
    while !s.poll_output(t).iter().all(|m| m[0] == 0xF0) {}
}

/// Output messages without keepalive SysEx.
fn leds(v: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    v.into_iter().filter(|m| m[0] != 0xF0).collect()
}

fn deck(deck: u8, action: DeckAction) -> ControlAction {
    ControlAction::Deck { deck, action }
}

// ------------------------------------------------------------------ parsing

#[test]
fn both_mapping_files_parse() {
    let a = load("ddj-flx4.toml");
    assert_eq!(a.device().decks, 2);
    assert!(a.matches_port("DDJ-FLX4 MIDI 1"));
    assert!(!a.matches_port("DDJ-FLX10"));
    let b = load("ddj-flx10.toml");
    assert_eq!(b.device().decks, 4);
    assert!(b.matches_usb(0x2B73, 0x0041));
    assert!(b.matches_port("ddj-flx10"));
    assert!(!b.device().sources.is_empty() && !a.device().sources.is_empty());
    assert!(!b.unverified().is_empty());
}

#[test]
fn load_dir_finds_bundled_mappings() {
    let all = Mapping::load_dir(&od_controller::bundled_mappings_dir());
    assert!(all.len() >= 2);
    for (p, r) in all {
        r.unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    }
}

/// No MIDI address may trigger two bindings (would double-fire actions).
#[test]
fn no_address_is_bound_twice() {
    for name in ["ddj-flx4.toml", "ddj-flx10.toml"] {
        let m = load(name);
        let mut seen: HashMap<MidiAddr, &str> = HashMap::new();
        for b in &m.inputs {
            if b.shift.is_some() {
                continue;
            }
            for a in std::iter::once(b.addr).chain(b.lsb) {
                if let Some(prev) = seen.insert(a, &b.name) {
                    panic!("{name}: {a} bound by `{prev}` and `{}`", b.name);
                }
            }
        }
    }
}

#[test]
fn every_deck_has_core_controls() {
    for (name, decks) in [("ddj-flx4.toml", 2u8), ("ddj-flx10.toml", 4)] {
        let mut s = ControllerSession::new(load(name));
        for d in 1..=decks {
            let ch = d - 1;
            let a = feed(&mut s, &[[0x90 | ch, 0x0B, 0x7F], [0x90 | ch, 0x0C, 0x7F], [0x90 | ch, 0x58, 0x7F]]);
            assert_eq!(
                a,
                vec![deck(d, DeckAction::PlayPause), deck(d, DeckAction::CuePress), deck(d, DeckAction::Sync)],
                "{name} deck {d}"
            );
        }
    }
}

// ------------------------------------------------------------------ inputs

#[test]
fn buttons_fire_on_press_only() {
    let mut s = flx4();
    assert_eq!(feed(&mut s, &[[0x91, 0x0B, 0x7F]]), vec![deck(2, DeckAction::PlayPause)]);
    assert_eq!(feed(&mut s, &[[0x91, 0x0B, 0x00]]), vec![]);
    assert_eq!(feed(&mut s, &[[0x81, 0x0B, 0x40]]), vec![]);
    // Cue is momentary.
    assert_eq!(
        feed(&mut s, &[[0x90, 0x0C, 0x7F], [0x90, 0x0C, 0x00]]),
        vec![deck(1, DeckAction::CuePress), deck(1, DeckAction::CueRelease)]
    );
}

#[test]
fn hardware_shift_layer() {
    let mut s = flx4();
    // SHIFT held: the unit sends different note numbers.
    let a = feed(
        &mut s,
        &[
            [0x90, 0x3F, 0x7F],
            [0x90, 0x48, 0x7F], // shift + cue
            [0x98, 0x03, 0x7F], // shift + hot cue pad 4 on deck 1
            [0x96, 0x42, 0x7F], // shift + browse press
            [0x90, 0x3F, 0x00],
        ],
    );
    assert_eq!(
        a,
        vec![
            deck(1, DeckAction::Shift(true)),
            deck(1, DeckAction::JumpToStart),
            deck(1, DeckAction::HotCueDelete { slot: 3 }),
            ControlAction::Browse(BrowseAction::Back),
            deck(1, DeckAction::Shift(false)),
        ]
    );
}

#[test]
fn software_shift_layer() {
    let toml = r#"
        schema = 1
        [device]
        id = "generic"
        name = "Generic"
        vendor = "Test"
        model = "T1"
        decks = 2
        [device.match]
        port_names = ["Generic"]
        [channels]
        deck = [0, 1]
        [[input]]
        note = 0x10
        channel = "deck"
        action = "deck.shift"
        [[input]]
        note = 0x01
        channel = "deck"
        shift = false
        action = "deck.play_pause"
        [[input]]
        note = 0x01
        channel = "deck"
        shift = true
        action = "deck.reverse"
        [[input]]
        cc = 0x20
        channel = 0
        shift = true
        action = "browse.rotate"
        args = { shifted = true }
        encoding = "twos_complement"
        [[input]]
        cc = 0x20
        channel = 0
        shift = false
        action = "browse.rotate"
        encoding = "twos_complement"
    "#;
    let mut s = ControllerSession::new(Arc::new(Mapping::from_toml_str(toml).unwrap()));
    let a = feed(
        &mut s,
        &[
            [0x90, 0x01, 0x7F],
            [0x90, 0x10, 0x7F], // shift on deck 1
            [0x90, 0x01, 0x7F],
            [0x91, 0x01, 0x7F], // deck 2 is not shifted
            [0xB0, 0x20, 0x7F], // browse -1 while some shift is held
            [0x90, 0x10, 0x00],
            [0xB0, 0x20, 0x02],
        ],
    );
    assert_eq!(
        a,
        vec![
            deck(1, DeckAction::PlayPause),
            deck(1, DeckAction::Shift(true)),
            deck(1, DeckAction::Reverse),
            deck(2, DeckAction::PlayPause),
            ControlAction::Browse(BrowseAction::Rotate { delta: -1, shifted: true }),
            deck(1, DeckAction::Shift(false)),
            ControlAction::Browse(BrowseAction::Rotate { delta: 2, shifted: false }),
        ]
    );
}

#[test]
fn fourteen_bit_fader_needs_msb_and_lsb() {
    let mut s = flx4();
    assert_eq!(feed(&mut s, &[[0xB1, 0x13, 0x40]]), vec![]); // MSB alone: nothing yet
    let a = feed(&mut s, &[[0xB1, 0x33, 0x00]]);
    let ControlAction::Mixer { channel: 2, action: MixerAction::Fader(v) } = a[0] else { panic!("{a:?}") };
    assert!((v - 8192.0 / 16383.0).abs() < 1e-6);
    // Full scale.
    let a = feed(&mut s, &[[0xB1, 0x13, 0x7F], [0xB1, 0x33, 0x7F]]);
    assert_eq!(a, vec![ControlAction::Mixer { channel: 2, action: MixerAction::Fader(1.0) }]);
    // Crossfader on the global channel.
    let a = feed(&mut s, &[[0xB6, 0x1F, 0x00], [0xB6, 0x3F, 0x00]]);
    assert_eq!(a, vec![ControlAction::Crossfader(0.0)]);
}

#[test]
fn tempo_is_inverted_bipolar_with_centre_detent() {
    let mut s = flx4();
    // Exact centre (0x2000) is 0.00006 off in 14-bit; the device module snaps it.
    let a = feed(&mut s, &[[0xB0, 0x00, 0x40], [0xB0, 0x20, 0x00]]);
    assert_eq!(a, vec![deck(1, DeckAction::Tempo(0.0))]);
    let a = feed(&mut s, &[[0xB0, 0x00, 0x00], [0xB0, 0x20, 0x00]]);
    assert_eq!(a, vec![deck(1, DeckAction::Tempo(1.0))]);
    let a = feed(&mut s, &[[0xB0, 0x00, 0x7F], [0xB0, 0x20, 0x7F]]);
    assert_eq!(a, vec![deck(1, DeckAction::Tempo(-1.0))]);
}

#[test]
fn jog_relative_values() {
    let mut s = flx4();
    let a = feed(
        &mut s,
        &[
            [0x90, 0x36, 0x7F], // touch
            [0xB0, 0x22, 0x43], // +3 scratch
            [0xB0, 0x22, 0x3D], // -3 scratch
            [0x90, 0x36, 0x00], // release
            [0xB1, 0x21, 0x3E], // side ring -2 on deck 2
            [0xB1, 0x23, 0x41], // top +1 untouched
            [0xB0, 0x29, 0x42], // shift: search +2 (× module gain 8)
            [0xB0, 0x22, 0x40], // zero movement: nothing
        ],
    );
    assert_eq!(
        a,
        vec![
            deck(1, DeckAction::JogTouch(true)),
            deck(1, DeckAction::JogTurn { ticks: 3, mode: JogMode::Scratch }),
            deck(1, DeckAction::JogTurn { ticks: -3, mode: JogMode::Scratch }),
            deck(1, DeckAction::JogTouch(false)),
            deck(2, DeckAction::JogTurn { ticks: -2, mode: JogMode::Bend }),
            deck(2, DeckAction::JogTurn { ticks: 1, mode: JogMode::Bend }),
            deck(1, DeckAction::JogTurn { ticks: 16, mode: JogMode::Search }),
        ]
    );
}

#[test]
fn pads_per_mode() {
    let mut s = flx4();
    let a = feed(
        &mut s,
        &[
            [0x99, 0x02, 0x7F], // deck 2 hot cue C
            [0x99, 0x02, 0x00],
            [0x97, 0x25, 0x7F], // deck 1 beat jump pad 6 = +4
            [0x97, 0x31, 0x64], // sampler slot 1, velocity 100
            [0x9A, 0x31, 0x7F], // shift: stop sampler slot 1
            [0x97, 0x64, 0x7F], // beat loop pad 5 = 4 beats
            [0x90, 0x20, 0x7F], // pad mode beat jump
        ],
    );
    assert_eq!(
        a,
        vec![
            deck(2, DeckAction::HotCue { slot: 2, pressed: true }),
            deck(2, DeckAction::HotCue { slot: 2, pressed: false }),
            deck(1, DeckAction::BeatJump { beats: 4.0 }),
            ControlAction::Sampler(SamplerAction::Pad { slot: 1, pressed: true, velocity: 100 }),
            ControlAction::Sampler(SamplerAction::Stop { slot: 1 }),
            deck(1, DeckAction::AutoLoop { beats: 4.0 }),
            deck(1, DeckAction::PadMode(PadMode::BeatJump)),
        ]
    );
}

#[test]
fn browser_and_load() {
    let mut s = flx4();
    let a = feed(&mut s, &[[0xB6, 0x40, 0x01], [0xB6, 0x40, 0x7E], [0x96, 0x47, 0x7F], [0x96, 0x41, 0x7F]]);
    assert_eq!(
        a,
        vec![
            ControlAction::Browse(BrowseAction::Rotate { delta: 1, shifted: false }),
            ControlAction::Browse(BrowseAction::Rotate { delta: -2, shifted: false }),
            deck(2, DeckAction::Load),
            ControlAction::Browse(BrowseAction::Press { shifted: false }),
        ]
    );
}

#[test]
fn flx10_deck_3_and_4_layers() {
    let mut s = flx10();
    assert_eq!(s.active_deck(1), Some(1));
    assert_eq!(s.active_deck(2), Some(2));
    let a = feed(
        &mut s,
        &[
            [0x92, 0x72, 0x7F], // DECK 3 selected on the left
            [0x92, 0x0B, 0x7F], // play deck 3
            [0x93, 0x72, 0x7F], // DECK 4 on the right
            [0x9D, 0x02, 0x7F], // deck 4 hot cue C
            [0x9E, 0x05, 0x7F], // deck 4 shift pad: delete F
            [0xB2, 0x07, 0x10], // channel 3 EQ high
            [0xB2, 0x27, 0x00],
            [0x96, 0x48, 0x7F], // load deck 3
            [0x90, 0x72, 0x7F], // back to deck 1
        ],
    );
    assert_eq!(
        a,
        vec![
            deck(3, DeckAction::DeckSelect),
            deck(3, DeckAction::PlayPause),
            deck(4, DeckAction::DeckSelect),
            deck(4, DeckAction::HotCue { slot: 2, pressed: true }),
            deck(4, DeckAction::HotCueDelete { slot: 5 }),
            ControlAction::Mixer { channel: 3, action: MixerAction::EqHigh(2048.0 / 16383.0) },
            deck(3, DeckAction::Load),
            deck(1, DeckAction::DeckSelect),
        ]
    );
    assert_eq!(s.active_deck(1), Some(1));
    assert_eq!(s.active_deck(2), Some(4));
}

#[test]
fn flx10_colour_fx_and_beat_fx() {
    let mut s = flx10();
    let a = feed(&mut s, &[[0x96, 0x03, 0x7F], [0x94, 0x24, 0x7F], [0x94, 0x14, 0x7F], [0x94, 0x11, 0x7F]]);
    assert_eq!(
        a,
        vec![
            ControlAction::ColorFx(ColorFxAction::Select("noise".into())),
            ControlAction::BeatFx { unit: 1, action: BeatFxAction::Select("reverb".into()) },
            ControlAction::BeatFx { unit: 1, action: BeatFxAction::Assign(0) },
            ControlAction::BeatFx { unit: 1, action: BeatFxAction::Assign(2) },
        ]
    );
}

#[test]
fn ignored_controls_produce_ignored() {
    let mut s = flx10();
    let p = s.handle_midi(&[0x90, 0x59, 0x7F]); // mix point select
    assert_eq!(p.actions, vec![ControlAction::Ignored]);
    let p = s.handle_midi(&[0x90, 0x7E, 0x7F]); // nothing bound
    assert!(p.actions.is_empty() && p.hits.is_empty());
}

// ------------------------------------------------------------------ soft takeover

#[test]
fn soft_takeover_on_knob() {
    let mut s = flx4();
    s.feedback(&ControlFeedback::Parameter { target: ParamTarget::EqHigh { channel: 1 }, value: 0.5 });
    let p = s.handle_midi(&[0xB0, 0x07, 0x0C]);
    assert!(p.actions.is_empty());
    let p = s.handle_midi(&[0xB0, 0x27, 0x00]); // 0.09: far away → held
    assert!(p.actions.is_empty());
    assert_eq!(p.hits[0].suppressed, Some("soft takeover"));
    let a = feed(&mut s, &[[0xB0, 0x07, 0x30], [0xB0, 0x27, 0x00]]); // 0.375: still below
    assert!(a.is_empty());
    let a = feed(&mut s, &[[0xB0, 0x07, 0x42], [0xB0, 0x27, 0x00]]); // crossed 0.5
    assert_eq!(a.len(), 1);
    assert!(s.is_caught(ParamTarget::EqHigh { channel: 1 }));
    // Crossfader has takeover disabled in the mapping.
    s.feedback(&ControlFeedback::Parameter { target: ParamTarget::Crossfader, value: 0.5 });
    assert_eq!(feed(&mut s, &[[0xB6, 0x1F, 0x00], [0xB6, 0x3F, 0x00]]).len(), 1);
}

#[test]
fn flx10_tempo_pickup_after_layer_switch() {
    let mut s = flx10();
    // Deck 1 tempo fader at +50 % (raw 0x1000 → inverted).
    let a = feed(&mut s, &[[0xB0, 0x00, 0x10], [0xB0, 0x20, 0x00]]);
    assert_eq!(a.len(), 1);
    // Engine says deck 3 runs at 0 %; switching the left side to deck 3 makes
    // the same physical fader address deck 3.
    s.feedback(&ControlFeedback::Parameter { target: ParamTarget::Tempo { deck: 3 }, value: 0.0 });
    feed(&mut s, &[[0x92, 0x72, 0x7F]]);
    let a = feed(&mut s, &[[0xB2, 0x00, 0x11], [0xB2, 0x20, 0x00]]);
    assert!(a.is_empty(), "far from engine value must not jump: {a:?}");
    let a = feed(&mut s, &[[0xB2, 0x00, 0x30], [0xB2, 0x20, 0x00]]);
    assert!(a.is_empty());
    // Moving to the engine value (centre) picks it up.
    let a = feed(&mut s, &[[0xB2, 0x00, 0x40], [0xB2, 0x20, 0x00]]);
    assert_eq!(a, vec![deck(3, DeckAction::Tempo(0.0))]);
    let a = feed(&mut s, &[[0xB2, 0x00, 0x50], [0xB2, 0x20, 0x00]]);
    assert_eq!(a.len(), 1);
}

// ------------------------------------------------------------------ software layers

const SOFT_LAYERS: &str = r#"
    schema = 1
    [device]
    id = "two-side"
    name = "Two side"
    vendor = "Test"
    model = "T2"
    decks = 4
    [device.match]
    port_names = ["Two side"]
    [channels]
    side = [0, 1]
    [layers]
    mode = "software"
    sides = [[1, 3], [2, 4]]
    [[input]]
    note = 0x01
    channel = "side"
    action = "deck.play_pause"
    [[input]]
    note = 0x30
    channel = 0
    action = "deck.select"
    args = { deck = 3 }
    [[input]]
    note = 0x31
    channel = 0
    action = "deck.select"
    args = { deck = 1 }
    [[input]]
    cc = 0x05
    channel = "side"
    action = "mixer.trim"
    [[output]]
    target = "deck.play"
    note = 0x01
    channel = "side"
"#;

#[test]
fn software_deck_layers_redirect_inputs_and_outputs() {
    let m = Arc::new(Mapping::from_toml_str(SOFT_LAYERS).unwrap());
    let mut s = ControllerSession::new(m);
    let t0 = Instant::now();
    s.connect_messages();
    s.poll_output(t0);
    assert_eq!(feed(&mut s, &[[0x90, 0x01, 0x7F]]), vec![deck(1, DeckAction::PlayPause)]);

    // Deck 3 plays while hidden: nothing is sent.
    s.feedback(&ControlFeedback::deck_led(3, DeckElement::Play, true));
    assert!(s.poll_output(t0 + Duration::from_millis(10)).is_empty());

    feed(&mut s, &[[0x90, 0x30, 0x7F]]);
    assert_eq!(s.active_deck(1), Some(3));
    assert_eq!(feed(&mut s, &[[0x90, 0x01, 0x7F]]), vec![deck(3, DeckAction::PlayPause)]);
    // Switching shows deck 3's state.
    assert_eq!(s.poll_output(t0 + Duration::from_millis(20)), vec![vec![0x90, 0x01, 0x7F]]);
    // And back to deck 1 (off).
    feed(&mut s, &[[0x90, 0x31, 0x7F]]);
    assert_eq!(s.poll_output(t0 + Duration::from_millis(30)), vec![vec![0x90, 0x01, 0x00]]);
    // Right side untouched.
    assert_eq!(feed(&mut s, &[[0x91, 0x01, 0x7F]]), vec![deck(2, DeckAction::PlayPause)]);
}

#[test]
fn software_layer_switch_requires_pickup() {
    let m = Arc::new(Mapping::from_toml_str(SOFT_LAYERS).unwrap());
    let mut s = ControllerSession::new(m);
    assert_eq!(feed(&mut s, &[[0xB0, 0x05, 0x7F]]).len(), 1); // trim ch1 = 1.0
    s.feedback(&ControlFeedback::Parameter { target: ParamTarget::Trim { channel: 3 }, value: 0.2 });
    feed(&mut s, &[[0x90, 0x30, 0x7F]]);
    assert!(feed(&mut s, &[[0xB0, 0x05, 0x7E]]).is_empty());
    assert_eq!(
        feed(&mut s, &[[0xB0, 0x05, 0x1A]]),
        vec![ControlAction::Mixer { channel: 3, action: MixerAction::Trim(26.0 / 127.0) }]
    );
}

// ------------------------------------------------------------------ feedback

#[test]
fn feedback_is_diffed() {
    let mut s = flx4();
    let t0 = Instant::now();
    s.output_mut().set_epoch(t0);
    s.connect_messages();
    let init = s.poll_output(t0);
    // Keepalive + every output address cleared exactly once.
    let addrs: HashSet<_> = s.mapping().outputs.iter().map(|o| o.addr).collect();
    assert_eq!(init.len(), addrs.len() + 1);
    assert!(init.iter().all(|m| m[0] == 0xF0 || m[2] == 0));
    let t = |ms| t0 + Duration::from_millis(ms);
    assert!(s.poll_output(t(1)).is_empty());

    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Play, true));
    s.feedback(&ControlFeedback::deck_led(2, DeckElement::Cue, true));
    let mut out = s.poll_output(t(2));
    out.sort();
    assert_eq!(out, vec![vec![0x90, 0x0B, 0x7F], vec![0x91, 0x0C, 0x7F]]);
    // Same state again: nothing.
    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Play, true));
    assert!(s.poll_output(t(3)).is_empty());
    // On → off → on between two polls: nothing either.
    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Play, false));
    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Play, true));
    assert!(s.poll_output(t(4)).is_empty());
    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Play, false));
    assert_eq!(s.poll_output(t(5)), vec![vec![0x90, 0x0B, 0x00]]);
    // Loop active drives three LEDs (two on the deck channel, one indicator).
    s.feedback(&ControlFeedback::deck_led(2, DeckElement::LoopActive, true));
    let mut out = s.poll_output(t(6));
    out.sort();
    assert_eq!(out, vec![vec![0x91, 0x4D, 0x7F], vec![0x91, 0x50, 0x7F], vec![0x9F, 0x05, 0x7F]]);
    // ResendAll repeats current state.
    s.feedback(&ControlFeedback::ResendAll);
    assert_eq!(s.poll_output(t(7)).len(), addrs.len());
}

#[test]
fn meters_are_rate_limited() {
    let mut s = flx4();
    let t0 = Instant::now();
    s.connect_messages();
    s.poll_output(t0);
    let t = |ms| t0 + Duration::from_millis(ms);
    s.feedback(&ControlFeedback::channel_vu(1, 0.5));
    assert_eq!(s.poll_output(t(100)), vec![vec![0xB0, 0x02, 64]]);
    s.feedback(&ControlFeedback::channel_vu(1, 0.8));
    assert!(s.poll_output(t(110)).is_empty(), "within 33 ms of last meter message");
    s.feedback(&ControlFeedback::channel_vu(1, 1.0));
    assert!(s.poll_output(t(120)).is_empty());
    // After the interval only the latest value goes out.
    assert_eq!(s.poll_output(t(134)), vec![vec![0xB0, 0x02, 127]]);
    // LEDs are not rate limited.
    s.feedback(&ControlFeedback::deck_led(1, DeckElement::Sync, true));
    assert_eq!(s.poll_output(t(135)), vec![vec![0x90, 0x58, 0x7F]]);
}

#[test]
fn software_blink() {
    let mut s = flx4();
    let t0 = Instant::now();
    s.output_mut().set_epoch(t0);
    s.connect_messages();
    s.poll_output(t0);
    let t = |ms| t0 + Duration::from_millis(ms);
    s.feedback(&ControlFeedback::led_state(
        FeedbackTarget::Deck { deck: 1, element: DeckElement::Sync },
        LedState::Blink,
    ));
    assert_eq!(leds(s.poll_output(t(10))), vec![vec![0x90, 0x58, 0x7F]]);
    assert!(leds(s.poll_output(t(100))).is_empty());
    assert_eq!(leds(s.poll_output(t(260))), vec![vec![0x90, 0x58, 0x00]]);
    assert_eq!(leds(s.poll_output(t(510))), vec![vec![0x90, 0x58, 0x7F]]);
}

#[test]
fn flx10_pad_colours_use_palette() {
    let mut s = flx10();
    let t0 = Instant::now();
    s.connect_messages();
    let first = s.poll_output(t0);
    assert_eq!(first.len(), s.mapping().options().max_messages_per_poll, "budget applies");
    drain(&mut s, t0);
    s.feedback(&ControlFeedback::pad_color(3, PadMode::HotCue, 2, Some(CueColor::rgb(10, 240, 20))));
    s.feedback(&ControlFeedback::pad_color(4, PadMode::HotCue, 0, Some(CueColor::rgb(250, 0, 5))));
    let mut out = s.poll_output(t0 + Duration::from_millis(5));
    out.sort();
    assert_eq!(out, vec![vec![0x9B, 0x02, 0x14], vec![0x9D, 0x00, 0x01]]);
    s.feedback(&ControlFeedback::pad_color(3, PadMode::HotCue, 2, None));
    assert_eq!(s.poll_output(t0 + Duration::from_millis(6)), vec![vec![0x9B, 0x02, 0x00]]);
}

#[test]
fn keepalive_is_periodic() {
    let mut s = flx4();
    let t0 = Instant::now();
    let is_ka = |v: &Vec<Vec<u8>>| v.iter().any(|m| m[0] == 0xF0);
    assert!(is_ka(&s.poll_output(t0)));
    assert!(!is_ka(&s.poll_output(t0 + Duration::from_millis(100))));
    assert!(is_ka(&s.poll_output(t0 + Duration::from_millis(250))));
}

// ------------------------------------------------------------------ monitor

#[test]
fn monitor_shows_decoded_hits() {
    let mut s = flx10();
    let mon = MidiMonitor::new(2);
    let rx = mon.subscribe();
    for m in [[0x92u8, 0x0B, 0x7F], [0x90, 0x7E, 0x7F], [0xB0, 0x22, 0x41]] {
        let p = s.handle_midi(&m);
        mon.record("DDJ-FLX10", Direction::In, &m, p.hits);
    }
    let evs: Vec<_> = rx.try_iter().collect();
    assert_eq!(evs.len(), 3);
    let text = evs[0].to_string();
    assert!(text.contains("PlayPause") && text.contains("Play/Pause (deck 3)"), "{text}");
    assert!(evs[1].to_string().contains("(unmapped)"));
    assert!(evs[2].to_string().contains("[unverified]"));
    assert_eq!(mon.snapshot().len(), 2); // bounded log
}

#[test]
fn flx10_part_isolator_drives_stem_levels() {
    let mut s = flx10();
    // Deck 1 (channel 0): HI part knob fully left = vocals muted, centre = full.
    let a = feed(&mut s, &[[0xB0, 0x08, 0x00], [0xB0, 0x28, 0x00]]);
    assert!(a.iter().any(|x| matches!(x, ControlAction::Deck { deck: 1, action: DeckAction::StemLevel { stem: StemPart::Vocals, level } } if *level == 0.0)), "{a:?}");
    let a = feed(&mut s, &[[0xB1, 0x10, 0x40], [0xB1, 0x30, 0x00]]);
    assert!(a.iter().any(|x| matches!(x, ControlAction::Deck { deck: 2, action: DeckAction::StemLevel { stem: StemPart::Drums, level } } if *level >= 0.99)), "{a:?}");
}
