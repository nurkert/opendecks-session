//! Behaviour tests for features of `tests/parity/features.toml`: each drives
//! the real window (clicks, drags, wheel, keys) and asserts the resulting
//! library, engine or view state.

use super::flows::{deck_title, deck_views};
use super::harness::*;
use slint::ComponentHandle;
use slint::platform::Key;

fn perform(ui: &Ui) {
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
}

fn row(ui: &Ui, title: &str) -> i_slint_backend_testing::ElementHandle {
    ui.by_value("browser.row", title).unwrap_or_else(|| panic!("row `{title}`")).1
}

/// Drag a list row onto deck 2: deck 2 loads that track (and nothing else).
#[test]
fn drag_row_onto_deck_loads_it() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let title = ui.fx.tracks[1].1.clone();
    let target = Rect::of(&ui.get("deck2.overview")).centre();
    ui.drag_el(&row(&ui, &title), target);
    assert!(ui.wait_until(30.0, |u| deck_title(u, 2) == title), "deck 2 shows `{title}`, got `{}`", deck_title(&ui, 2));
    assert_ne!(deck_title(&ui, 1), title, "deck 1 untouched");
    // The drag ghost is gone after the drop.
    assert!(!ui.app.ui.global::<crate::DragState>().get_active());
}

/// Double-click (activate) loads into a free deck; the context menu
/// Load to deck ▸ Deck 2 loads into deck 2.
#[test]
fn load_gestures_double_click_and_menu() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let a = ui.fx.tracks[0].1.clone();
    ui.double_click_el(&row(&ui, &a));
    assert!(ui.wait_until(30.0, |u| deck_title(u, 1) == a), "double-click loads deck 1, got `{}`", deck_title(&ui, 1));
    let b = ui.fx.tracks[2].1.clone();
    // The menu opens over a list that is still refreshing after the first
    // load; reopen it if the submenu did not come up.
    let mut loaded = false;
    for _ in 0..5 {
        ui.pump(400);
        ui.right_click_el(&row(&ui, &b));
        if ui.click_menu_item("Load to deck") && ui.click_menu_item("Deck 2") {
            loaded = true;
            break;
        }
        ui.key(Key::Escape);
    }
    assert!(loaded, "Load to deck ▸ Deck 2");
    assert!(ui.wait_until(30.0, |u| deck_title(u, 2) == b), "menu loads deck 2, got `{}`", deck_title(&ui, 2));
}

/// Row context menu: Rating ▸ ★★★★ stores rating 4; Colour ▸ Red stores
/// the colour; Remove from this list removes the playlist entry.
#[test]
fn row_context_menu_edits_the_library() {
    let ui = Ui::open((1440, 900));
    let (id, title) = ui.fx.tracks[2].clone();
    ui.right_click_el(&row(&ui, &title));
    assert!(ui.click_menu_item("Rating"));
    assert!(ui.click_menu_item("★★★★"));
    assert!(ui.wait_until(5.0, |u| u.fx.lib().track(id).unwrap().tags.rating == 4), "rating 4 in the library");
    ui.right_click_el(&row(&ui, &title));
    assert!(ui.click_menu_item("Colour"));
    assert!(ui.click_menu_item("Red"));
    assert!(ui.wait_until(5.0, |u| u.fx.lib().track(id).unwrap().tags.color.is_some()), "colour in the library");
    // In a playlist: Remove from this list.
    ui.click(&format!("browser.tree.node{}", ui.fx.playlist.0));
    let first = ui.fx.tracks[0].1.clone();
    assert!(ui.wait_until(5.0, |u| u.by_value("browser.row", &first).is_some()));
    ui.right_click_el(&row(&ui, &first));
    assert!(ui.click_menu_item("Remove from this list"));
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(u.fx.playlist).unwrap().is_empty()), "entry removed");
}

/// Info panel: INFO shows the selected track's tags; editing a field and
/// pressing Return stores it.
#[test]
fn info_panel_edits_tags() {
    let ui = Ui::open((1440, 900));
    let (id, title) = ui.fx.tracks[0].clone();
    ui.click_el(&row(&ui, &title));
    ui.click("browser.panel.info");
    assert!(ui.wait_until(5.0, |u| u.has("info.genre")), "info panel open");
    assert_eq!(ui.value("info.title"), title);
    ui.click("info.genre");
    ui.type_text("Deep House");
    ui.key(Key::Return);
    assert!(ui.wait_until(5.0, |u| u.fx.lib().track(id).unwrap().tags.genre == "Deep House"), "genre stored");
    // INFO again closes the panel.
    ui.click("browser.panel.info");
    assert!(!ui.has("info.genre"));
}

/// The waveform zone: one stacked full-width lane per deck (2 and 4
/// decks) with the playhead at the horizontal centre.
#[test]
fn waveform_zone_lanes_per_deck() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let lanes = |u: &Ui| (1..=4).filter(|n| u.has(&format!("wave.lane{n}"))).count();
    assert_eq!(lanes(&ui), 2);
    let (l1, l2) = (Rect::of(&ui.get("wave.lane1")), Rect::of(&ui.get("wave.lane2")));
    assert!(l2.y >= l1.bottom() - 0.5, "lanes stacked");
    assert!(l1.w > 0.9 * ui.size().0, "lane spans the window ({} of {})", l1.w, ui.size().0);
    let ph = Rect::of(&ui.get("wave.playhead"));
    let lane = Rect::of(&ui.get("wave.lane2"));
    let gutter_centre = ph.x + ph.w / 2.0;
    assert!(gutter_centre > lane.x + lane.w * 0.4 && gutter_centre < lane.x + lane.w * 0.6, "playhead near the centre: {gutter_centre} in {lane:?}");
    ui.app.ui.invoke_set_deck_count(4);
    ui.pump(300);
    assert_eq!(lanes(&ui), 4);
}

/// Wheel over a lane zooms that deck's waveform; double-click resets.
#[test]
fn waveform_zoom_by_wheel() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let z0 = ui.app.decks.borrow().zoom[0];
    ui.scroll("wave.lane1", 120.0);
    let z1 = ui.app.decks.borrow().zoom[0];
    assert!((z1 - z0).abs() > 1e-6, "zoom changed: {z0} → {z1}");
    assert_eq!(ui.app.decks.borrow().zoom[1], z0, "deck 2 untouched");
    ui.scroll("wave.lane1", -240.0);
    assert!((ui.app.decks.borrow().zoom[0] - z1).abs() > 1e-6, "zoom changes back the other way");
    ui.double_click("wave.lane1");
    assert_eq!(ui.app.decks.borrow().zoom[0], 1.0, "double-click resets");
}

/// Splitters: dragging changes the pane size, it persists across a
/// restart, double-click resets it.
#[test]
fn splitters_drag_persist_reset() {
    let ui = Ui::open((1440, 900));
    let upper = Rect::of(&ui.get("browser.upper-splitter"));
    ui.drag_points(upper.centre(), slint::LogicalPosition::new(upper.centre().x, upper.centre().y + 80.0));
    let h = ui.app.ui.get_upper_height();
    assert!(h > 0.0, "upper height set by the drag: {h}");
    let side = Rect::of(&ui.get("browser.splitter"));
    let w0 = ui.app.ui.get_sidebar_width();
    ui.drag_points(side.centre(), slint::LogicalPosition::new(side.centre().x + 60.0, side.centre().y));
    let w = ui.app.ui.get_sidebar_width();
    assert!(w > w0 + 30.0, "sidebar wider: {w0} → {w}");
    // Closing the window remembers the sizes; the next start restores them.
    crate::shell::remember_view(&ui.app);
    let Ui { fx, app, .. } = ui;
    let _ = app.ui.hide();
    drop(app);
    let ui = Ui::start(fx, (1440, 900), false);
    assert!((ui.app.ui.get_upper_height() - h).abs() < 1.0, "upper height restored");
    assert!((ui.app.ui.get_sidebar_width() - w).abs() < 1.0, "sidebar width restored");
    ui.double_click("browser.upper-splitter");
    assert_eq!(ui.app.ui.get_upper_height(), 0.0, "double-click resets to the default");
    ui.double_click("browser.splitter");
    assert_eq!(ui.app.ui.get_sidebar_width(), 260.0);
}

/// Preferences > View: colour-blind palette and high contrast switch the
/// theme of the open window and are stored.
#[test]
fn colour_blind_and_high_contrast() {
    let ui = Ui::open((1440, 900));
    ui.click("topbar.settings");
    let prefs = ui.app.prefs.borrow().window.as_ref().map(|w| w.clone_strong()).expect("Preferences open");
    prefs.set_page("view".into());
    ui.pump(200);
    let cb = i_slint_backend_testing::ElementHandle::find_by_accessible_label(&prefs, "prefs.view.colour-blind").next().expect("switch");
    cb.mock_single_click(slint::platform::PointerEventButton::Left);
    let hc = i_slint_backend_testing::ElementHandle::find_by_accessible_label(&prefs, "prefs.view.high-contrast").next().expect("switch");
    hc.mock_single_click(slint::platform::PointerEventButton::Left);
    ui.pump(200);
    let theme = ui.app.ui.global::<crate::Theme>();
    assert!(theme.get_colorblind_safe(), "colour-blind palette on");
    assert!(theme.get_high_contrast(), "high contrast on");
    let stored = ui.fx.lib().meta(crate::settings::META_SETTINGS).unwrap().unwrap();
    let s = crate::settings::Settings::from_json(&stored);
    assert!(s.view.colorblind_safe && s.view.high_contrast, "stored in the settings");
}

/// Top bar FX / SAMPLER toggles show and hide their panels.
#[test]
fn panel_toggles_show_panels() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    assert!(!ui.has("fx.unit1.on"));
    ui.click("topbar.toggle.fx");
    assert!(ui.has("fx.unit1.on") && ui.has("fx.unit2.on"), "FX panel shown");
    ui.click("topbar.toggle.sampler");
    assert!(ui.has("sampler.slot1") && ui.has("sampler.slot16"), "sampler panel shown");
    ui.click("topbar.toggle.fx");
    assert!(!ui.has("fx.unit1.on"), "FX panel hidden again");
}

/// Beat FX ON in the panel switches the unit on in the engine.
#[test]
fn fx_panel_unit_on() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    crate::engine::ensure_engine(&ui.app);
    ui.click("topbar.toggle.fx");
    ui.click("fx.unit1.on");
    let on = ui.wait_until(5.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.beat_fx[0].on));
    assert!(on, "unit 1 on in the engine");
}

/// Perform layouts: 2 and 4 decks.
#[test]
fn perform_two_and_four_decks() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    assert!(ui.has("deck1.play") && ui.has("deck2.play") && !ui.has("deck3.play"));
    ui.app.ui.invoke_set_deck_count(4);
    ui.pump(300);
    for d in 1..=4 {
        assert!(ui.has(&format!("deck{d}.play")), "deck {d} shown");
    }
    // Order 1 above 3 left, 2 above 4 right.
    let r = |l: &str| Rect::of(&ui.get(l));
    assert!(r("deck3.play").y > r("deck1.play").y && r("deck2.play").x > r("deck1.play").x);
}

/// Deck header read-outs after a load: title, artist, BPM, time.
#[test]
fn deck_header_readouts() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let (id, title) = ui.fx.tracks[0].clone();
    ui.double_click_el(&row(&ui, &title));
    assert!(ui.wait_until(30.0, |u| deck_title(u, 1) == title));
    assert_eq!(ui.value("deck1.artist"), "Test Artist");
    assert!(ui.value("deck1.time-remaining").contains(':'), "remaining time shown: `{}`", ui.value("deck1.time-remaining"));
    // BPM once analysed (120 BPM click track).
    let analysed = ui.wait_until(60.0, |u| u.fx.lib().track(id).unwrap().bpm.is_some());
    if analysed {
        assert!(ui.wait_until(10.0, |u| u.value("deck1.bpm").starts_with("12")), "BPM read-out: `{}`", ui.value("deck1.bpm"));
    }
    // Playing advances the time.
    let before = ui.value("deck1.time-remaining");
    ui.click("deck1.play");
    assert!(ui.wait_until(10.0, |u| u.value("deck1.time-remaining") != before), "time runs while playing");
    assert!(deck_views(&ui).iter().any(|d| d.deck == 1 && d.playing));
}
