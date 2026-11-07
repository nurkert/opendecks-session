//! The zoomed waveform lanes in Perform: loop band, bar counter.

use super::flows::deck_views;
use super::harness::*;

fn perform_with_track(ui: &Ui) {
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(300);
    crate::decks::load_track(&ui.app, 0, ui.fx.tracks[0].0, None);
    assert!(ui.wait_until(30.0, |u| deck_views(u).first().is_some_and(|d| d.loaded)), "deck 1 loads");
}

/// A running loop shows as an amber band over the lane, around the playhead.
#[test]
fn loop_shows_on_the_waveform() {
    let ui = Ui::open((1440, 900));
    perform_with_track(&ui);
    assert!(!ui.has("wave.loop1"), "no loop yet");
    crate::engine::deck_cmd(&ui.app, 0, od_engine::DeckCommand::AutoLoop { beats: 4.0 });
    assert!(ui.wait_until(5.0, |u| u.has("wave.loop1")), "loop band shown");
    let band = Rect::of(&ui.get("wave.loop1"));
    let lane = Rect::of(&ui.get("wave.lane1"));
    let head = Rect::of(&ui.get("wave.playhead"));
    assert!(lane.contains(&band, 1.0), "inside the lane");
    assert!(band.w > 10.0 && band.x <= head.x + 1.0 && band.right() >= head.x, "around the playhead: {band:?}");
}

/// The bar counter is large, says what it counts, and a click picks the
/// next mode (stored like the preference).
#[test]
fn bar_counter_cycles_its_mode() {
    let ui = Ui::open((1440, 900));
    perform_with_track(&ui);
    assert!(ui.has("wave.counter1"));
    assert!(Rect::of(&ui.get("wave.counter1")).h >= 30.0, "readable from a distance");
    let mode = |u: &Ui| u.app.state.borrow().settings.view.bar_counter.clone();
    let before = mode(&ui);
    ui.click("wave.counter1");
    assert!(ui.wait_until(2.0, |u| mode(u) != before), "click picks the next mode");
    assert!(ui.wait_until(2.0, |u| deck_views(u)[0].counter_mode.as_str() == mode(u)));
}
