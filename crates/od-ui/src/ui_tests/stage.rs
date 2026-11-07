//! LIGHTING view and CDJ practice mode, driven through the real window.

use super::flows::deck_views;
use super::harness::*;
use slint::ComponentHandle;

/// Patch a fixture, choose a mode, black out and set an output from the
/// LIGHTING view; the rig file on disk follows.
#[test]
fn lighting_view_patches_and_controls_the_show() {
    let ui = Ui::open((1440, 900));
    ui.click("topbar.mode.lighting");
    ui.pump(200);
    assert_eq!(ui.app.ui.get_mode(), "lighting");
    assert!(ui.has("lighting.stage"));

    ui.click("lighting.rig.name");
    ui.type_text("Front");
    ui.click("lighting.rig.add");
    ui.pump(100);
    assert!(ui.has("lighting.rig.row0"), "the patched fixture is listed");
    assert!(ui.has("lighting.stage.fixture0"), "and drawn on the stage");
    {
        let rt = ui.app.lighting.borrow();
        assert_eq!(rt.config.patch.fixtures.len(), 1);
        assert_eq!(rt.config.patch.fixtures[0].name, "Front");
    }

    ui.click("lighting.mode.easy");
    ui.click("lighting.blackout");
    ui.click("lighting.output.sacn");
    ui.click("lighting.output.apply");
    ui.pump(100);
    let rig = od_lighting::RigConfig::load(&crate::lighting::rig_path(&ui.fx.library)).unwrap();
    assert_eq!(rig.patch.fixtures.len(), 1);
    assert_eq!(rig.settings.mode, od_lighting::Mode::Easy);
    assert!(rig.settings.blackout);
    assert_eq!(rig.outputs, vec![(0, od_lighting::OutputKind::Sacn { universe: 1 })]);
    assert!(rig.enabled);

    ui.click("lighting.rig.remove0");
    ui.pump(100);
    assert!(!ui.has("lighting.rig.row0"));
}

/// CDJ mode swaps the deck panels for player screens; CUE and PLAY act with
/// the player's rules and the checklist follows.
#[test]
fn cdj_mode_player_screens() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(300);
    ui.click("topbar.toggle.cdj");
    ui.pump(300);
    assert!(ui.app.ui.global::<crate::Cdj>().get_enabled());
    assert!(ui.has("deck1.cdj") && ui.has("deck2.cdj"));
    assert!(ui.has("app.cdj.steps"));

    let id = ui.fx.tracks[0].0;
    crate::decks::load_track(&ui.app, 0, id, None);
    assert!(ui.wait_until(30.0, |u| deck_views(u).first().is_some_and(|d| d.loaded)), "deck 1 loads");
    ui.click("deck1.cdj.play");
    assert!(ui.wait_until(10.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.decks[0].playing)), "PLAY starts");
    // Step 3 (start a deck) is seen while it plays.
    assert!(ui.wait_until(10.0, |u| u.app.cdj.borrow().checklist.done[2]));
    // CUE while playing returns to the cue point and pauses.
    let cue = ui.get("deck1.cdj.cue");
    let p = Rect::of(&cue).centre();
    ui.press_at(p, slint::platform::PointerEventButton::Left);
    ui.release_at(p, slint::platform::PointerEventButton::Left);
    assert!(ui.wait_until(10.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| !s.decks[0].playing)), "CUE pauses");

    ui.click("topbar.toggle.cdj");
    ui.pump(200);
    assert!(!ui.has("deck1.cdj"));
}

/// The LIGHT toggle shows the lighting strip inside Perform; its controls
/// act on the same show as the LIGHTING view.
#[test]
fn lighting_strip_in_perform() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
    assert!(!ui.has("lighting.strip"));
    ui.click("topbar.toggle.light");
    ui.pump(200);
    assert!(ui.has("lighting.strip") && ui.has("lighting.strip.stage"));
    ui.click("lighting.strip.mode.pro");
    ui.click("lighting.strip.blackout");
    ui.pump(100);
    let rt = ui.app.lighting.borrow();
    assert_eq!(rt.config.settings.mode, od_lighting::Mode::Pro);
    assert!(rt.config.settings.blackout);
}

/// MIC controls in the global bar: talkover toggles; without an audio
/// device the MIC button explains why it stays off.
#[test]
fn mic_controls_in_the_global_bar() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
    let g = ui.app.ui.global::<crate::Mic>();
    ui.click("global.mic.talkover");
    assert!(g.get_talkover());
    ui.click("global.mic.on");
    assert!(!g.get_on(), "no capture device in the headless test");
    assert!(!g.get_error().is_empty());
}

/// Pro mode programmer: the group FX key picks an attribute effect, the XY
/// square sets pan / tilt; FOG is held like FLASH.
#[test]
fn lighting_pro_effect_xy_and_fog() {
    let ui = Ui::open((1440, 900));
    ui.click("topbar.mode.lighting");
    ui.pump(100);
    let mover = od_lighting::FixtureDef::BUILTIN.iter().position(|n| *n == "moving-head").unwrap();
    crate::lighting::add_fixture(&mut ui.app.lighting.borrow_mut().config.patch, mover, "Spot", "Movers").unwrap();
    ui.click("lighting.mode.pro");
    ui.pump(200);
    ui.click("lighting.pro.group0.effect");
    ui.pump(100);
    assert_eq!(ui.app.lighting.borrow().config.settings.groups["Movers"].effect, crate::lighting::effect_preset(1));
    let xy = Rect::of(&ui.get("lighting.pro.group0.xy"));
    ui.click_at(slint::LogicalPosition::new(xy.x + xy.w * 0.75, xy.y + xy.h * 0.25), slint::platform::PointerEventButton::Left);
    ui.pump(100);
    {
        let rt = ui.app.lighting.borrow();
        let g = &rt.config.settings.groups["Movers"];
        assert!((g.pan.unwrap() - 0.75).abs() < 0.05 && (g.tilt.unwrap() - 0.75).abs() < 0.05, "{:?} {:?}", g.pan, g.tilt);
    }
    let fog = Rect::of(&ui.get("lighting.fog")).centre();
    ui.press_at(fog, slint::platform::PointerEventButton::Left);
    ui.pump(50);
    assert!(slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_fog());
    ui.release_at(fog, slint::platform::PointerEventButton::Left);
    ui.pump(50);
    assert!(!slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_fog());
}

/// Lighting pad mode: A–F pick the easy look, G strobes while held.
#[test]
fn lighting_pad_mode_drives_the_show() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(200);
    ui.app.ui.invoke_deck_pad_mode_set(1, 10);
    ui.pump(100);
    assert_eq!(ui.app.decks.borrow().pad_mode[0], od_controller::PadMode::Lighting);
    crate::decks::pad(&ui.app, 0, 2, true);
    crate::decks::pad(&ui.app, 0, 2, false);
    {
        let rt = ui.app.lighting.borrow();
        assert_eq!(rt.config.settings.mode, od_lighting::Mode::Easy);
        assert_eq!(rt.config.settings.easy_program, od_lighting::Program::Chase);
    }
    crate::decks::pad(&ui.app, 0, 6, true);
    assert!(slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_strobe());
    crate::decks::pad(&ui.app, 0, 6, false);
    assert!(!slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_strobe());
}

/// CDJ browse screen: BROWSE → All tracks → a track loads into that deck;
/// Playlists lists the tree.
#[test]
fn cdj_browse_screen_loads_a_track() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(200);
    slint::ComponentHandle::global::<crate::Cdj>(&ui.app.ui).invoke_set_enabled(true);
    ui.pump(200);
    ui.click("deck2.cdj.browse");
    assert!(ui.wait_until(5.0, |u| u.has("deck2.cdj.browse-row1")), "sources shown");
    ui.click("deck2.cdj.browse-row1"); // Playlists
    assert!(ui.wait_until(5.0, |u| u.by_value("deck2.cdj.browse-row", "Warm-up").is_some()), "playlist tree");
    ui.click("deck2.cdj.browse-back");
    ui.click("deck2.cdj.browse-row0"); // All tracks
    let title = ui.fx.tracks[2].1.clone();
    let (_, row) = ui.by_value("deck2.cdj.browse-row", &format!("Test Artist — {title}")).expect("track row");
    ui.click_el(&row);
    assert!(ui.wait_until(30.0, |u| super::flows::deck_views(u).get(1).is_some_and(|d| d.title == title)), "deck 2 loads {title}");
    assert!(!ui.has("deck2.cdj.browse-list"), "browse closes after loading");
}

/// WINDOW opens the LIGHTING view in its own window; both windows show the
/// same rig and either one drives the show.
#[test]
fn lighting_detaches_into_its_own_window() {
    let ui = Ui::open((1440, 900));
    ui.click("topbar.mode.lighting");
    ui.pump(100);
    ui.click("lighting.detach");
    ui.pump(100);
    let w = ui.app.lighting_window.borrow().as_ref().map(|w| w.clone_strong()).expect("lighting window");
    let g = slint::ComponentHandle::global::<crate::Lighting>(&w);
    // A blackout from the main window shows in the detached one…
    ui.click("lighting.blackout");
    ui.pump(50);
    assert!(g.get_blackout());
    // …and the detached window's controls act on the same show.
    g.invoke_set_mode("pro".into());
    assert_eq!(ui.app.lighting.borrow().config.settings.mode, od_lighting::Mode::Pro);
    assert_eq!(slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_mode(), "pro");
    assert!(i_slint_backend_testing::ElementHandle::find_by_accessible_label(&w, "lighting.stage").next().is_some());
    assert!(i_slint_backend_testing::ElementHandle::find_by_accessible_label(&w, "lighting.detach").next().is_none(), "no WINDOW key in the window itself");
}

/// Mapped controller inputs drive the light show.
#[test]
fn lighting_reacts_to_mapped_controls() {
    use od_controller::action::LightingAction as L;
    let ui = Ui::open((1440, 900));
    crate::lighting::controller(&ui.app, L::Mode("pro".into()));
    crate::lighting::controller(&ui.app, L::Master(0.25));
    crate::lighting::controller(&ui.app, L::BlackoutToggle);
    {
        let rt = ui.app.lighting.borrow();
        assert_eq!(rt.config.settings.mode, od_lighting::Mode::Pro);
        assert_eq!((rt.config.settings.master, rt.config.settings.blackout), (0.25, true));
    }
    crate::lighting::controller(&ui.app, L::Look(3));
    assert_eq!(ui.app.lighting.borrow().config.settings.easy_program, od_lighting::Program::Sweep);
    crate::lighting::controller(&ui.app, L::Fog(true));
    assert!(slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui).get_fog());
}

/// Track cues: with a track playing, CUE AT PLAYHEAD stores the current
/// cue at the bar; the list shows it for that track.
#[test]
fn lighting_track_cue_timeline() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("lighting".into());
    crate::engine::ensure_engine(&ui.app);
    let g = slint::ComponentHandle::global::<crate::Lighting>(&ui.app.ui);
    g.invoke_set_mode("pro".into());
    g.invoke_cue_store("Drop".into());
    let (id, title) = ui.fx.tracks[0].clone();
    crate::decks::load_track(&ui.app, 0, id, None);
    // Deck rows are projected in Perform only; check the engine instead.
    assert!(ui.wait_until(30.0, |u| crate::engine::snapshot(&u.app).is_some_and(|s| s.decks[0].track == Some(id))));
    ui.app.ui.invoke_deck_toggle_play(1);
    assert!(ui.wait_until(10.0, |u| slint::ComponentHandle::global::<crate::Lighting>(&u.app.ui).get_timeline_title() == title.as_str()), "focused track");
    ui.pump(200);
    ui.click("lighting.timeline.add");
    ui.pump(100);
    let cues = ui.app.lighting.borrow().config.settings.track_cues.get(&id.0).cloned().unwrap_or_default();
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].beat % 4, 0, "on a bar");
    assert!(ui.has("lighting.timeline0"));
    ui.click("lighting.timeline0.delete");
    ui.pump(100);
    assert!(ui.app.lighting.borrow().config.settings.track_cues.get(&id.0).is_none_or(|v| v.is_empty()));
}
