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
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| {
        (s.decks[0].stem_levels[od_engine::Stem::Vocals.index()] - 0.4).abs() < 1e-6 && s.decks[1].stem_levels[od_engine::Stem::Bass.index()] == 0.0
    })));
    assert_eq!(crate::decks::stem_of_ui("inst"), Some(od_engine::Stem::Inst));
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
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_multi[0])));
    assert!(ui.wait_until(5.0, |u| u.has("fx.unit1.slot2.on") && u.has("fx.unit1.slot3.type")));
    ui.click("fx.unit1.slot2.on");

    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_slots[0][0].on)));
    ui.click("fx.unit1.slot3.beats-up");
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx_slots[0][1].beats > 0.75)));
}

/// Stems and Memory Cue are in the pad mode list.
#[test]
fn new_pad_modes_are_selectable() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    ui.app.ui.invoke_deck_pad_mode_set(1, 8);
    ui.pump(100);
    assert_eq!(ui.app.decks.borrow().pad_mode[0], od_controller::PadMode::Stems);
    ui.app.ui.invoke_deck_pad_mode_set(1, 9);
    ui.pump(100);
    assert_eq!(ui.app.decks.borrow().pad_mode[0], od_controller::PadMode::MemoryCue);
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
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.decks[0].stem_levels[od_engine::Stem::Vocals.index()] == 0.0)));
    assert!(ui.wait_until(5.0, |u| u.app.mixer.channels.get(0).is_some_and(|c| c.parts && c.high == -1.0)));
    // The channel EQ itself is untouched.
    assert_eq!(crate::engine::snapshot(&ui.app).unwrap().channels[0].eq[2], 0.0);
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
    assert!(ui.wait_until(30.0, |u| {
        super::flows::deck_views(u).first().is_some_and(|d| d.pads.row_data(0).is_some_and(|p| p.tone == want))
    }), "pad A in red");
}

/// Search ◀◀ / ▶▶ (CDJ screens): the playhead runs fast while held.
#[test]
fn search_buttons_run_while_held() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    slint::ComponentHandle::global::<crate::Cdj>(&ui.app.ui).invoke_set_enabled(true);
    let id = ui.fx.tracks[0].0;
    crate::decks::load_track(&ui.app, 0, id, None);
    assert!(ui.wait_until(30.0, |u| super::flows::deck_views(u).first().is_some_and(|d| d.loaded)));
    ui.pump(200);
    let p = Rect::of(&ui.get("deck1.cdj.search-fwd")).centre();
    ui.press_at(p, slint::platform::PointerEventButton::Left);
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.decks[0].position > 1.0)), "searching forward");
    ui.release_at(p, slint::platform::PointerEventButton::Left);
    ui.pump(200);
    let at = crate::engine::snapshot(&ui.app).unwrap().decks[0].position;
    ui.pump(300);
    assert!((crate::engine::snapshot(&ui.app).unwrap().decks[0].position - at).abs() < 0.05, "stops when released");
}

/// Jog readout: the bar counter shows bars.beats and runs while playing;
/// the phrase line names the phrase with its countdown.
#[test]
fn platter_readout_counts_bars() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let (id, _) = ui.fx.tracks[0].clone();
    assert!(ui.wait_until(60.0, |u| u.fx.lib().track(id).unwrap().bpm.is_some()), "fixture analysed");
    crate::decks::load_track(&ui.app, 0, id, None);
    assert!(ui.wait_until(30.0, |u| super::flows::deck_views(u).first().is_some_and(|d| d.loaded)));
    ui.app.ui.invoke_deck_toggle_play(1);
    let ok = |v: &str| {
        let v = v.trim_start_matches('-');
        let mut it = v.split('.');
        matches!((it.next().map(|b| b.parse::<u32>().is_ok()), it.next().and_then(|b| b.parse::<u8>().ok()), it.next()), (Some(true), Some(1..=4), None))
    };
    assert!(ui.wait_until(10.0, |u| super::flows::deck_views(u).first().is_some_and(|d| ok(&d.beat_pos))), "bars.beats");
    let first = super::flows::deck_views(&ui)[0].beat_pos.clone();
    assert!(ui.wait_until(10.0, |u| super::flows::deck_views(u)[0].beat_pos != first), "counter runs");
    let phrases = [od_core::Phrase { start_beat: 0, end_beat: 64, kind: od_core::PhraseKind::Chorus }];
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
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx[1].target == od_engine::FxTarget::Sampler)));
    // Taps measure wall-clock time; set the tapped tempo directly.
    crate::engine::send(&ui.app, od_engine::Command::BeatFx(1, od_engine::BeatFxCommand::SetTapBpm(Some(124.0))));
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx[1].tap_bpm.is_some())), "tapped tempo");
    ui.click("fx.unit2.auto");
    assert!(ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx[1].tap_bpm.is_none())), "AUTO");
    assert!(ui.has("fx.unit2.beat-fx"));
}
