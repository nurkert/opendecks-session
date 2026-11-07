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
