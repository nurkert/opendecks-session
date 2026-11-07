//! Perform deck features driven through the real window.

use super::harness::*;

fn perform(ui: &Ui) {
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(300);
}

/// Part levels: the stem keys' level reaches the engine; names map to parts.
#[test]
fn stem_levels_reach_the_engine() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    assert!(ui.has("deck1.stem.vocal") && ui.has("deck2.stem.bass"));
    ui.app.ui.invoke_deck_stem_level(1, "vocal".into(), 0.4);
    ui.app.ui.invoke_deck_stem_level(2, "bass".into(), 0.0);
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| {
            (s.decks[0].stem_levels[od_engine::Stem::Vocals.index()] - 0.4).abs() < 1e-6
                && s.decks[1].stem_levels[od_engine::Stem::Bass.index()] == 0.0
        }))
    );
    assert_eq!(
        crate::decks::stem_of_ui("inst"),
        Some(od_engine::Stem::Inst)
    );
    assert_eq!(crate::decks::stem_of_ui("piano"), None);
}

/// The ×1 / ×3 key turns a Beat FX unit into three chained slots; slot 2
/// gets its own effect and ON.
#[test]
fn fx_multi_mode_slots() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    if !ui.app.ui.get_fx_panel() {
        ui.app.ui.invoke_toggle_fx_panel();
    }
    ui.pump(200);
    ui.click("fx.unit1.menu");
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_multi[0])
    }));
    assert!(ui.wait_until(5.0, |u| u.has("fx.unit1.slot2.on")
        && u.has("fx.unit1.slot3.type")));
    ui.click("fx.unit1.slot2.on");

    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_slots[0][0].on)
    }));
    ui.click("fx.unit1.slot3.beats-up");
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_slots[0][1].beats > 0.75)
    }));
}

/// Stems and Memory Cue are in the pad mode list.
#[test]
fn new_pad_modes_are_selectable() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    ui.app.ui.invoke_deck_pad_mode_set(1, 8);
    ui.pump(100);
    assert_eq!(
        ui.app.decks.borrow().pad_mode[0],
        od_controller::PadMode::Stems
    );
    ui.app.ui.invoke_deck_pad_mode_set(1, 9);
    ui.pump(100);
    assert_eq!(
        ui.app.decks.borrow().pad_mode[0],
        od_controller::PadMode::MemoryCue
    );
}

/// STEM on a mixer channel: the EQ knobs become part levels (HI vocals,
/// MID inst, LOW drums) and show them.
#[test]
fn mixer_stem_button_turns_eq_into_parts() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    assert!(ui.wait_until(5.0, |u| u.has("mixer.ch1.stem")));
    ui.click("mixer.ch1.stem");
    assert!(ui.app.mixer.parts.get()[0]);
    ui.app.ui.invoke_mixer_eq(1, 2, -1.0); // HI fully left: vocals off
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.decks[0].stem_levels
                [od_engine::Stem::Vocals.index()]
                == 0.0))
    );
    assert!(ui.wait_until(5.0, |u| {
        u.app
            .mixer
            .channels
            .get(0)
            .is_some_and(|c| c.parts && c.high == -1.0)
    }));
    // The channel EQ itself is untouched.
    assert_eq!(
        crate::engine::snapshot(&ui.app).unwrap().channels[0].eq[2],
        0.0
    );
    ui.click("mixer.ch1.stem");
    assert!(!ui.app.mixer.parts.get()[0]);
    assert_eq!(crate::mixer::knob_to_part(0.3), 1.0);
    assert_eq!(crate::mixer::part_to_knob(0.5), -0.5);
}

/// Hot cue pads in Perform show the cue's stored colour.
#[test]
fn hot_cue_pads_show_stored_colours() {
    use slint::Model;
    let ui = Ui::open((1440, 900));
    let id = ui.fx.tracks[1].0;
    let red = crate::player::CUE_PALETTE[0];
    let mut cue = od_core::Cue::hot(0, 1.0);
    cue.color = Some(red);
    ui.fx.lib().set_cues(id, vec![cue]).unwrap();
    perform(&ui);
    crate::decks::load_track(&ui.app, 0, id, None);
    let want = slint::Color::from_rgb_u8(red.r, red.g, red.b);
    assert!(
        ui.wait_until(30.0, |u| {
            super::flows::deck_views(u)
                .first()
                .is_some_and(|d| d.pads.row_data(0).is_some_and(|p| p.tone == want))
        }),
        "pad A in red"
    );
}

/// Search ◀◀ / ▶▶ (CDJ screens): the playhead runs fast while held.
#[test]
fn search_buttons_run_while_held() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    slint::ComponentHandle::global::<crate::Cdj>(&ui.app.ui).invoke_set_enabled(true);
    let id = ui.fx.tracks[0].0;
    crate::decks::load_track(&ui.app, 0, id, None);
    assert!(ui.wait_until(30.0, |u| {
        super::flows::deck_views(u)
            .first()
            .is_some_and(|d| d.loaded)
    }));
    ui.pump(200);
    let p = Rect::of(&ui.get("deck1.cdj.search-fwd")).centre();
    ui.press_at(p, slint::platform::PointerEventButton::Left);
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.decks[0].position > 1.0)),
        "searching forward"
    );
    ui.release_at(p, slint::platform::PointerEventButton::Left);
    ui.pump(200);
    let at = crate::engine::snapshot(&ui.app).unwrap().decks[0].position;
    ui.pump(300);
    assert!(
        (crate::engine::snapshot(&ui.app).unwrap().decks[0].position - at).abs() < 0.05,
        "stops when released"
    );
}

/// Jog readout: the bar counter shows bars.beats and runs while playing;
/// the phrase line names the phrase with its countdown.
#[test]
fn platter_readout_counts_bars() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let (id, _) = ui.fx.tracks[0].clone();
    assert!(
        ui.wait_until(60.0, |u| u.fx.lib().track(id).unwrap().bpm.is_some()),
        "fixture analysed"
    );
    crate::decks::load_track(&ui.app, 0, id, None);
    assert!(ui.wait_until(30.0, |u| {
        super::flows::deck_views(u)
            .first()
            .is_some_and(|d| d.loaded)
    }));
    ui.app.ui.invoke_deck_toggle_play(1);
    let ok = |v: &str| {
        let v = v.trim_start_matches('-');
        let mut it = v.split('.');
        matches!(
            (
                it.next().map(|b| b.parse::<u32>().is_ok()),
                it.next().and_then(|b| b.parse::<u8>().ok()),
                it.next()
            ),
            (Some(true), Some(1..=4), None)
        )
    };
    assert!(
        ui.wait_until(10.0, |u| super::flows::deck_views(u)
            .first()
            .is_some_and(|d| ok(&d.beat_pos))),
        "bars.beats"
    );
    let first = super::flows::deck_views(&ui)[0].beat_pos.clone();
    assert!(
        ui.wait_until(10.0, |u| super::flows::deck_views(u)[0].beat_pos != first),
        "counter runs"
    );
    let phrases = [od_core::Phrase {
        start_beat: 0,
        end_beat: 64,
        kind: od_core::PhraseKind::Chorus,
    }];
    assert_eq!(crate::decks::phrase_now(&phrases, 32.0), "CHORUS -7.4");
    assert_eq!(crate::decks::phrase_now(&phrases, 70.0), "");
}

/// FX assign SMP puts the unit on the sampler; TAP then AUTO returns to the
/// deck tempo.
#[test]
fn fx_sampler_assign_and_auto() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    if !ui.app.ui.get_fx_panel() {
        ui.app.ui.invoke_toggle_fx_panel();
    }
    ui.pump(200);
    ui.click("fx.unit2.assign.sampler");
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.beat_fx[1].target == od_engine::FxTarget::Sampler)
    }));
    // Taps measure wall-clock time; set the tapped tempo directly.
    crate::engine::send(
        &ui.app,
        od_engine::Command::BeatFx(1, od_engine::BeatFxCommand::SetTapBpm(Some(124.0))),
    );
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.beat_fx[1].tap_bpm.is_some())),
        "tapped tempo"
    );
    ui.click("fx.unit2.auto");
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.beat_fx[1].tap_bpm.is_none())),
        "AUTO"
    );
    assert!(ui.has("fx.unit2.beat-fx"));
}

/// Sequence Call: an empty pad stores the sampler patterns, a stored pad
/// recalls them and starts the sequencer; the shifted pad clears it.
#[test]
fn sequence_call_stores_and_recalls() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    ui.app.ui.invoke_deck_pad_mode_set(1, 12);
    ui.pump(100);
    assert_eq!(
        ui.app.decks.borrow().pad_mode[0],
        od_controller::PadMode::SeqCall
    );
    let send = |c| crate::engine::send(&ui.app, od_engine::Command::Sampler(c));
    send(od_engine::SamplerCommand::SeqPattern {
        slot: 0,
        mask: 0b1000_1000_1000_1000,
    });
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.sampler[0].pattern != 0)
    }));
    crate::decks::pad(&ui.app, 0, 0, true); // store
    assert!(ui.app.sampler.sequences.borrow()[0].is_some());
    send(od_engine::SamplerCommand::SeqClear);
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.sampler[0].pattern == 0)
    }));
    crate::decks::pad(&ui.app, 0, 0, true); // recall
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s
            .sampler[0]
            .pattern
            == 0b1000_1000_1000_1000
            && s.sequencer.playing))
    );
    crate::decks::pad_alt(&ui.app, 0, 0);
    assert!(ui.app.sampler.sequences.borrow()[0].is_none());
}

/// Browse layout: decks keep header, overview and parts; jog, pads,
/// waveform lanes, mixer and global bar give their room to the browser.
#[test]
fn browse_layout_reduces_the_decks() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    assert!(ui.has("deck1.play") && ui.has("wave.lane1"));
    crate::decks::toggle_browse_layout(&ui.app);
    ui.pump(200);
    assert!(ui.has("deck1.title") && ui.has("deck1.overview") && ui.has("deck1.stem.vocal"));
    assert!(!ui.has("deck1.play") && !ui.has("wave.lane1") && !ui.has("global.xfader"));
    crate::decks::toggle_browse_layout(&ui.app);
    ui.pump(200);
    assert!(ui.has("deck1.play") && ui.has("global.xfader"));
}

/// Vertical layout: the lanes stand upright above the mixer, with zoom keys.
#[test]
fn vertical_layout_turns_the_lanes() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    crate::decks::toggle_vertical_layout(&ui.app);
    ui.pump(300);
    assert!(ui.has("wave.vertical") && ui.has("wave.lane1") && ui.has("wave.zoom-in"));
    let lane = Rect::of(&ui.get("wave.lane1"));
    assert!(lane.h > lane.w, "upright lane: {}×{}", lane.w, lane.h);
    let mixer = Rect::of(&ui.get("mixer.ch1.trim"));
    assert!(lane.bottom() <= mixer.y + 1.0, "lanes above the mixer");
    let before = ui.app.decks.borrow().zoom[0];
    ui.click("wave.zoom-in");
    assert!(ui.app.decks.borrow().zoom[0] < before, "zoom in");
    crate::decks::cycle_perform_layout(&ui.app);
    assert_eq!(
        slint::ComponentHandle::global::<crate::PerformLayout>(&ui.app.ui).get_layout(),
        "browse"
    );
    crate::decks::cycle_perform_layout(&ui.app);
    assert_eq!(
        slint::ComponentHandle::global::<crate::PerformLayout>(&ui.app.ui).get_layout(),
        "standard"
    );
}

/// REC off in the top bar hides the recording section of the global bar.
#[test]
fn rec_toggle_hides_the_recording_section() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    ui.app.ui.set_rec_panel(true);
    ui.pump(100);
    assert!(ui.has("rec.toggle") && ui.has("rec.time"));
    ui.app.ui.set_rec_panel(false);
    ui.pump(100);
    assert!(!ui.has("rec.toggle") && !ui.has("rec.time"));
}

/// Smart Fader: with A and B assigned, the crossfader swaps the basses;
/// off, the basses return to flat. Smart CFX is one of the colour FX.
#[test]
fn smart_fader_swaps_the_basses() {
    use od_engine::{ChannelCommand, Command, MixerCommand, XfAssign};
    assert_eq!(
        (
            crate::mixer::smart_low(0.0, true),
            crate::mixer::smart_low(0.0, false)
        ),
        (0.0, -1.0)
    );
    assert_eq!(
        (
            crate::mixer::smart_low(1.0, true),
            crate::mixer::smart_low(1.0, false)
        ),
        (-1.0, 0.0)
    );
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let send = |c| crate::engine::send(&ui.app, c);
    send(Command::Channel(
        0,
        ChannelCommand::SetCrossfaderAssign(XfAssign::A),
    ));
    send(Command::Channel(
        1,
        ChannelCommand::SetCrossfaderAssign(XfAssign::B),
    ));
    ui.pump(100);
    ui.click("global.smart-fader");
    assert!(ui.app.mixer.smart.get());
    send(Command::Mixer(MixerCommand::SetCrossfader(1.0))); // full B
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s
            .channels[0]
            .eq[0]
            == -1.0
            && s.channels[1].eq[0] == 0.0)),
        "A loses its bass"
    );
    ui.click("global.smart-fader");
    assert!(
        ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.channels[0].eq[0] == 0.0)),
        "flat again"
    );
    assert_eq!(
        crate::fx::color_fx_kind("smart"),
        od_engine::ColorFxKind::Smart
    );
}

/// Stem FX: with a unit on channel 1, the PART keys put it on one part.
#[test]
fn stem_fx_part_keys() {
    use od_engine::{FxTarget, Stem};
    assert_eq!(
        crate::fx::part_target(FxTarget::Channel(1), 0),
        FxTarget::Part(1, Stem::Vocals)
    );
    assert_eq!(
        crate::fx::part_target(FxTarget::Part(1, Stem::Vocals), -1),
        FxTarget::Channel(1)
    );
    assert_eq!(
        crate::fx::part_target(FxTarget::Master, 2),
        FxTarget::Master
    );
    let ui = Ui::open((1440, 900));
    perform(&ui);
    if !ui.app.ui.get_fx_panel() {
        ui.app.ui.invoke_toggle_fx_panel();
    }
    ui.pump(200);
    ui.click("fx.unit1.assign.ch1");
    assert!(ui.wait_until(5.0, |u| u.has("fx.unit1.part.drums")));
    ui.click("fx.unit1.part.drums");
    assert!(
        ui.wait_until(5.0, |u| {
            crate::engine::snapshot(&u.app)
                .is_some_and(|s| s.beat_fx[0].target == FxTarget::Part(0, Stem::Drums))
        })
    );
    ui.click("fx.unit1.part.all");
    assert!(ui.wait_until(5.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx[0].target == FxTarget::Channel(0))
    }));
}

/// Mixer knobs sit centred in their channel strip (the CUE key spans the
/// strip), and the PHONES knobs centred on the global bar row (the
/// crossfader is centred on it), at desktop and laptop size.
#[test]
fn mixer_and_phones_knobs_are_centred() {
    for size in [(1440, 900), (1280, 800)] {
        let ui = Ui::open(size);
        perform(&ui);
        assert!(ui.wait_until(5.0, |u| u.has("mixer.ch1.cue")
            && u.has("global.phones.mix")));
        for ch in 1..=2 {
            let strip = Rect::of(&ui.get(&format!("mixer.ch{ch}.cue")));
            for knob in ["trim", "eq-high", "eq-mid", "eq-low"] {
                let k = Rect::of(&ui.get(&format!("mixer.ch{ch}.{knob}")));
                assert!(
                    (k.centre().x - strip.centre().x).abs() <= 1.0,
                    "{size:?} ch{ch} {knob}: knob x {} vs strip x {}",
                    k.centre().x,
                    strip.centre().x
                );
            }
        }
        let row = Rect::of(&ui.get("global.xfader"));
        for knob in ["global.phones.mix", "global.phones.level"] {
            let k = Rect::of(&ui.get(knob));
            assert!(
                (k.centre().y - row.centre().y).abs() <= 1.0,
                "{size:?} {knob}: knob y {} vs row y {}",
                k.centre().y,
                row.centre().y
            );
        }
    }
}

/// Loading a track shows it on the deck at once (title, LOADING sweep) and
/// takes the old track off immediately; it becomes playable when decoded.
#[test]
fn loading_shows_at_once() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let (first, second) = (ui.fx.tracks[0].clone(), ui.fx.tracks[1].clone());
    crate::decks::load_track(&ui.app, 0, first.0, None);
    assert!(ui.wait_until(30.0, |u| u.app.decks.borrow().loaded_id(0) == Some(first.0)), "first track loaded");
    crate::decks::load_track(&ui.app, 0, second.0, None);
    // One frame later: the new title shows, loading, before decoding ends.
    ui.pump(16);
    let deck = super::flows::deck_views(&ui)[0].clone();
    assert_eq!(deck.title.as_str(), second.1, "the new track shows at once");
    assert!(deck.loading || ui.app.decks.borrow().loaded_id(0) == Some(second.0), "LOADING until playable");
    assert_ne!(ui.app.decks.borrow().loaded_id(0), Some(first.0), "the old track left at once");
    assert!(
        ui.wait_until(30.0, |u| !super::flows::deck_views(u)[0].loading && u.app.decks.borrow().loaded_id(0) == Some(second.0)),
        "playable after decoding"
    );
}
