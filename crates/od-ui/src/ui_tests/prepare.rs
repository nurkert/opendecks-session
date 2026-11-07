//! Behaviour tests of Prepare and browser features of
//! `tests/parity/features.toml`: files dropped from the file manager,
//! browsing from a controller, the source rail, the Prepare layouts, the
//! top-bar panel toggles and batch separation.

use super::flows::deck_title;
use super::harness::*;
use i_slint_core::input::BackendDragEvent;
use i_slint_core::items::{AllowedDragActions, DragAction, DropEvent};
use i_slint_core::window::WindowInner;
use slint::{ComponentHandle, LogicalPosition, Model};
use std::path::{Path, PathBuf};

/// Delivers an OS file drag over `at` and drops it there, as a backend does
/// for files dragged in from Finder / the file manager. Returns the action
/// the window took and whether the drop hint showed while hovering.
pub(crate) fn drop_files(ui: &Ui, paths: &[&Path], at: LogicalPosition) -> (DragAction, bool) {
    let mut data = slint::DataTransfer::default();
    data.set_file_paths(paths);
    let mut event = DropEvent::default();
    event.data = data;
    event.position = at;
    event.proposed_action = DragAction::Copy;
    let allowed = AllowedDragActions {
        copy: true,
        move_: false,
        link: false,
    };
    let w = WindowInner::from_pub(ui.app.ui.window());
    w.process_drag_event(BackendDragEvent::Move {
        event: event.clone(),
        allowed,
    });
    ui.pump(50);
    let hint = ui.has("browser.drop-files");
    let action = w
        .process_drag_event(BackendDragEvent::Drop { event, allowed })
        .unwrap_or(DragAction::None);
    ui.pump(50);
    (action, hint)
}

fn new_wav(ui: &Ui, name: &str, pitch: f32) -> PathBuf {
    let p = ui.fx.root.join("downloads").join(name);
    write_wav(&p, 6.0, pitch);
    p
}

/// Files dragged in from the file manager: an audio file, a folder with
/// audio and a text file dropped on the browser while a playlist is open
/// are added to the collection and to that playlist (the text file is
/// skipped); in Perform a file dropped on the left deck loads on deck 1.
#[test]
fn files_dropped_from_the_file_manager() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_select_node(ui.fx.playlist.0 as i32);
    ui.pump(200);
    let single = new_wav(&ui, "Dropped One.wav", 500.0);
    let folder = ui.fx.root.join("downloads/Pack");
    write_wav(&folder.join("Pack Loop.wav"), 6.0, 540.0);
    let note = ui.fx.root.join("downloads/readme.txt");
    std::fs::write(&note, "not audio").unwrap();
    let list = Rect::of(&ui.get("browser.list")).centre();
    let (action, hint) = drop_files(&ui, &[&single, &folder, &note], list);
    assert_eq!(action, DragAction::Copy, "the window takes file drops");
    assert!(hint, "the drop hint shows while files hover the window");
    assert!(
        !ui.has("browser.drop-files"),
        "the hint is gone after the drop"
    );
    let added = ui.wait_until(30.0, |u| {
        let lib = u.fx.lib();
        lib.track_by_path(&single).ok().flatten().is_some()
            && lib
                .track_by_path(&folder.join("Pack Loop.wav"))
                .ok()
                .flatten()
                .is_some()
    });
    assert!(
        added,
        "dropped file and folder content added to the collection"
    );
    let lib = ui.fx.lib();
    assert!(
        lib.track_by_path(&note).ok().flatten().is_none(),
        "the text file is skipped"
    );
    let entries = lib.entries(ui.fx.playlist).unwrap();
    assert_eq!(
        entries.len(),
        3,
        "both dropped tracks appended to the open playlist"
    );
    assert!(
        ui.wait_until(10.0, |u| u.app.ui.get_tracks().row_count() == 3),
        "the list on screen shows them"
    );

    // Perform: onto the left deck.
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
    let deck_file = new_wav(&ui, "Deck Drop.wav", 610.0);
    let deck1 = Rect::of(&ui.get("deck1.overview")).centre();
    let (action, _) = drop_files(&ui, &[&deck_file], deck1);
    assert_eq!(action, DragAction::Copy);
    assert!(
        ui.wait_until(30.0, |u| deck_title(u, 1) == "Deck Drop"),
        "deck 1 loads the dropped file, got `{}`",
        deck_title(&ui, 1)
    );
    assert!(
        ui.fx
            .lib()
            .track_by_path(&deck_file)
            .ok()
            .flatten()
            .is_some(),
        "and it joined the collection"
    );
}

/// Hovers an OS file drag over `at` without dropping; returns the drop
/// target the window claims there (`DragState.target`).
fn hover_files(ui: &Ui, paths: &[&Path], at: LogicalPosition) -> i32 {
    let mut data = slint::DataTransfer::default();
    data.set_file_paths(paths);
    let mut event = DropEvent::default();
    event.data = data;
    event.position = at;
    event.proposed_action = DragAction::Copy;
    let allowed = AllowedDragActions {
        copy: true,
        move_: false,
        link: false,
    };
    WindowInner::from_pub(ui.app.ui.window())
        .process_drag_event(BackendDragEvent::Move { event, allowed });
    ui.pump(50);
    ui.app.ui.global::<crate::DragState>().get_target()
}

/// Files from the file manager use the drop targets of dragged rows: the
/// Prepare player lights up under them and shows the dropped file, a
/// playlist in the tree takes the file, and in Perform with four decks a
/// file dropped on deck 3 loads there.
#[test]
fn files_dropped_on_players_lists_and_decks() {
    let ui = Ui::open((1440, 900));
    ui.pump(200);
    let to_player = new_wav(&ui, "Player Drop.wav", 520.0);
    let player = Rect::of(&ui.get("player1.enlarged")).centre();
    assert_eq!(
        hover_files(&ui, &[&to_player], player),
        -1,
        "the Prepare player claims the hovering files"
    );
    let drag = ui.app.ui.global::<crate::DragState>();
    assert!(
        drag.get_active() && drag.get_files(),
        "the files drag like rows (ghost, highlights)"
    );
    let (action, _) = drop_files(&ui, &[&to_player], player);
    assert_eq!(action, DragAction::Copy);
    assert!(
        ui.wait_until(30.0, |u| u.value("player1.title") == "Player Drop"),
        "the player shows the dropped file, got `{}`",
        ui.value("player1.title")
    );
    assert!(
        !drag.get_active() && !drag.get_files() && drag.get_target() == 0,
        "the drag ends with the drop"
    );
    let entries = ui.fx.lib().entries(ui.fx.playlist).unwrap().len();

    // Onto the playlist in the tree (the collection is open).
    let to_list = new_wav(&ui, "List Drop.wav", 560.0);
    let node = Rect::of(&ui.get(&format!("browser.tree.node{}", ui.fx.playlist.0))).centre();
    assert_eq!(
        hover_files(&ui, &[&to_list], node),
        200_001_000 + ui.fx.playlist.0 as i32,
        "the playlist lights up under the files"
    );
    drop_files(&ui, &[&to_list], node);
    assert!(
        ui.wait_until(30.0, |u| u.fx.lib().entries(u.fx.playlist).unwrap().len()
            == entries + 1),
        "the playlist under the pointer takes the dropped file"
    );
    let lib = ui.fx.lib();
    let id = lib.track_by_path(&to_list).unwrap().unwrap().id;
    assert_eq!(
        lib.entries(ui.fx.playlist).unwrap().last().map(|e| e.track),
        Some(id)
    );

    // Perform, four decks: onto deck 3.
    ui.app.ui.invoke_set_mode("perform".into());
    ui.app.ui.invoke_set_deck_count(4);
    ui.pump(400);
    let to_deck = new_wav(&ui, "Deck Three.wav", 600.0);
    let deck3 = Rect::of(&ui.get("deck3.overview")).centre();
    assert_eq!(
        hover_files(&ui, &[&to_deck], deck3),
        3,
        "deck 3 lights up under the files"
    );
    drop_files(&ui, &[&to_deck], deck3);
    assert!(
        ui.wait_until(30.0, |u| deck_title(u, 3) == "Deck Three"),
        "deck 3 loads the dropped file, got `{}`",
        deck_title(&ui, 3)
    );
}

/// Where dropped files go, by drop target (pure).
#[test]
fn file_drop_targets() {
    use crate::import::{FileDrop, file_drop};
    assert_eq!(file_drop(0), FileDrop::Collection);
    assert_eq!(file_drop(3), FileDrop::Deck(2));
    assert_eq!(file_drop(-1), FileDrop::Player(1));
    assert_eq!(file_drop(-2), FileDrop::Player(2));
    assert_eq!(file_drop(100_004), FileDrop::Row(4));
    assert_eq!(
        file_drop(200_001_000 + 7),
        FileDrop::List(od_library::NodeId(7))
    );
    assert_eq!(
        file_drop(200_001_000 - 3),
        FileDrop::Collection,
        "special tree rows (tag list …) add to the collection"
    );
    // The AppKit pointer (points, bottom-left origin) in window coordinates.
    assert_eq!(
        crate::import::content_point_to_logical(100.0, 700.0, 900.0, 2.0, 2.0),
        (100.0, 200.0)
    );
    assert_eq!(
        crate::import::content_point_to_logical(100.0, 700.0, 900.0, 2.0, 4.0),
        (50.0, 100.0),
        "a larger UI scale shrinks logical units"
    );
}

/// The pure part: which dropped paths are audio.
#[test]
fn dropped_audio_expands_folders_and_skips_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.wav");
    let sub = dir.path().join("sub");
    write_wav(&a, 0.2, 300.0);
    write_wav(&sub.join("b.wav"), 0.2, 320.0);
    std::fs::write(sub.join("c.txt"), "x").unwrap();
    let got = crate::import::dropped_audio(&[
        a.clone(),
        sub.clone(),
        a.clone(),
        dir.path().join("missing.wav"),
    ]);
    assert_eq!(got, vec![a, sub.join("b.wav")]);
}

/// The source rail at the left edge of the tree pane: the sources in the
/// familiar order; each opens its source or scrolls the tree to it.
#[test]
fn source_rail_switches_sources() {
    let ui = Ui::open((1440, 900));
    let order = [
        "collection",
        "playlists",
        "related",
        "sampler",
        "explorer",
        "devices",
        "histories",
        "recordings",
        "tag-list",
    ];
    let rects: Vec<Rect> = order
        .iter()
        .map(|o| Rect::of(&ui.get(&format!("browser.rail.{o}"))))
        .collect();
    assert!(
        rects.windows(2).all(|w| w[0].y < w[1].y),
        "rail order top to bottom"
    );
    let tree = Rect::of(&ui.get("browser.tree"));
    assert!(
        rects
            .iter()
            .all(|r| r.right() <= tree.x + 0.5 && r.w <= 40.0),
        "the rail sits left of the tree, 38 px wide"
    );
    ui.click("browser.rail.tag-list");
    assert_eq!(ui.app.ui.get_node_kind(), "taglist");
    ui.click("browser.rail.related");
    assert_eq!(ui.app.ui.get_node_kind(), "related");
    ui.click("browser.rail.recordings");
    assert_eq!(ui.app.ui.get_section(), "recordings");
    ui.click("browser.rail.collection");
    assert_eq!(
        (
            ui.app.ui.get_section().as_str(),
            ui.app.ui.get_node_kind().as_str()
        ),
        ("library", "all")
    );
    assert_eq!(
        ui.get("browser.rail.collection").accessible_checked(),
        Some(true),
        "the open source is marked"
    );
    // Perform: the tree pane is short; EXPLORER scrolls it to the folders.
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
    ui.click("browser.rail.tag-list");
    ui.pump(200);
    ui.click("browser.rail.explorer");
    ui.pump(200);
    let tree = Rect::of(&ui.get("browser.tree"));
    let home = Rect::of(&ui.get("browser.tree.home"));
    assert!(
        home.y >= tree.y - 0.5 && home.bottom() <= tree.bottom() + 0.5,
        "Explorer roots scrolled into view: {home:?} in {tree:?}"
    );
    // SAMPLER shows the sampler panel.
    ui.app.ui.invoke_set_mode("prepare".into());
    ui.pump(200);
    ui.click("browser.rail.sampler");
    assert_eq!(ui.app.ui.get_mode(), "perform");
    assert!(ui.app.ui.get_sampler_panel(), "sampler panel open");
}

fn stored(ui: &Ui) -> crate::settings::Settings {
    crate::settings::Settings::from_json(
        &ui.fx
            .lib()
            .meta(crate::settings::META_SETTINGS)
            .unwrap()
            .unwrap(),
    )
}

/// Top-bar REC shows / hides the recording bar in Prepare (whose button
/// records); in Perform MIXER hides the mixer column and MIX POINT shows
/// the Mix Point Link panel; the toggles sit in the familiar order and are
/// stored.
#[test]
fn top_bar_mixer_rec_and_mix_point_toggles() {
    let ui = Ui::open((1440, 900));
    assert!(
        ui.has("rec.toggle") && ui.has("rec.meter"),
        "recording bar under the player"
    );
    ui.click("topbar.toggle.rec");
    assert!(!ui.has("rec.toggle"), "REC hides it");
    assert!(!stored(&ui).perform.rec_panel);
    ui.click("topbar.toggle.rec");
    assert!(ui.has("rec.toggle"));
    ui.click("rec.toggle");
    assert!(
        ui.wait_until(10.0, |u| u.app.ui.get_recording()),
        "the bar's button records"
    );
    assert!(
        ui.value("topbar.toggle.rec") != "",
        "the toggle shows the elapsed time while recording"
    );
    ui.click("rec.toggle");
    assert!(ui.wait_until(10.0, |u| !u.app.ui.get_recording()));

    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    ui.pump(400);
    let order = ["fx", "mpl", "sampler", "mixer", "rec"];
    let xs: Vec<f32> = order
        .iter()
        .map(|t| Rect::of(&ui.get(&format!("topbar.toggle.{t}"))).x)
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] < w[1]),
        "toggle order {order:?}: {xs:?}"
    );
    let mixer_w = |u: &Ui| {
        u.find("mixer.ch1.cue")
            .map(|e| Rect::of(&e).w)
            .unwrap_or(0.0)
    };
    assert!(mixer_w(&ui) > 0.0, "mixer shown");
    ui.click("topbar.toggle.mixer");
    assert!(
        ui.wait_until(5.0, |u| mixer_w(u) == 0.0),
        "MIXER hides the mixer"
    );
    assert!(!stored(&ui).perform.mixer_panel);
    ui.click("topbar.toggle.mixer");
    assert!(ui.wait_until(5.0, |u| mixer_w(u) > 0.0));
    assert!(!ui.has("mpl.link"));
    ui.click("topbar.toggle.mpl");
    assert!(
        ui.has("mpl.link") && ui.has("mpl.deck1.mix-out-time") && ui.has("mpl.deck2.mix-in-time"),
        "Mix Point Link panel"
    );
    assert!(stored(&ui).perform.mpl_panel);
}

/// Mix Point Link: a mix-out point on deck 1 and a mix-in point on deck 2;
/// with LINK on, deck 2 starts from its mix-in point as deck 1 passes its
/// mix-out point.
#[test]
fn mix_point_link_starts_the_next_deck() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_set_mode("perform".into());
    crate::engine::ensure_engine(&ui.app);
    let (a, b) = (ui.fx.tracks[0].0, ui.fx.tracks[1].0);
    crate::decks::load_track(&ui.app, 0, a, None);
    crate::decks::load_track(&ui.app, 1, b, None);
    let loaded = |u: &Ui| {
        crate::engine::snapshot(&u.app).is_some_and(|s| {
            s.decks[0].track == Some(a)
                && s.decks[1].track == Some(b)
                && s.decks[0].duration > 0.0
                && s.decks[1].duration > 0.0
        })
    };
    assert!(ui.wait_until(30.0, loaded), "both decks loaded");
    ui.click("topbar.toggle.mpl");
    let pos = |u: &Ui, d: usize| {
        crate::engine::snapshot(&u.app)
            .map(|s| s.decks[d].position)
            .unwrap_or(-1.0)
    };
    crate::engine::deck_cmd(&ui.app, 0, od_engine::DeckCommand::Seek { seconds: 3.0 });
    assert!(ui.wait_until(5.0, |u| (pos(u, 0) - 3.0).abs() < 0.05));
    ui.click("mpl.mix-out");
    assert!(
        ui.wait_until(5.0, |u| u.value("mpl.deck1.mix-out-time") != "—"
            && u.value("mpl.deck1.mix-out-time") != ""),
        "mix-out shown"
    );
    ui.click("mpl.edit");
    assert_eq!(ui.value("mpl.edit"), "2");
    crate::engine::deck_cmd(&ui.app, 1, od_engine::DeckCommand::Seek { seconds: 2.0 });
    assert!(ui.wait_until(5.0, |u| (pos(u, 1) - 2.0).abs() < 0.05));
    ui.click("mpl.mix-in");
    assert_eq!(
        crate::mpl::load(&ui.fx.lib(), b)
            .mix_in
            .map(|t| (t * 10.0).round()),
        Some(20.0),
        "mix-in stored with deck 2's track"
    );
    crate::engine::deck_cmd(&ui.app, 1, od_engine::DeckCommand::Seek { seconds: 0.0 });
    ui.click("mpl.link");
    crate::engine::deck_cmd(&ui.app, 0, od_engine::DeckCommand::Seek { seconds: 2.6 });
    crate::engine::deck_cmd(&ui.app, 0, od_engine::DeckCommand::Play);
    let started = ui.wait_until(20.0, |u| {
        crate::engine::snapshot(&u.app)
            .is_some_and(|s| s.decks[1].playing && s.decks[1].position >= 1.95)
    });
    assert!(
        started,
        "deck 2 came in from its mix-in point: playing {:?} at {}",
        crate::engine::snapshot(&ui.app).map(|s| s.decks[1].playing),
        pos(&ui, 1)
    );
}

/// Browsing from a controller: BACK moves the encoder to the tree, turning
/// it steps through the lists, a press on a folder opens it and on a list
/// moves into the tracks; there the encoder steps rows (⇧ by eight), ⇧-press
/// collects into the tag list, a press loads; RELATED and VIEW switch view.
#[test]
fn controller_browses_tree_and_list() {
    use od_controller::BrowseAction as B;
    let fx = Fixture::new();
    let (folder, inner) = {
        let mut lib = fx.lib();
        let f = lib
            .create_node(None, od_library::NodeKind::Folder, "Crates")
            .unwrap();
        let p = lib
            .create_node(Some(f), od_library::NodeKind::Playlist, "Inner")
            .unwrap();
        (f, p)
    };
    let ui = Ui::start(fx, (1440, 900), false);
    let go = |a: B| {
        crate::browser::navigate::controller(&ui.app, &a);
        ui.pump(100);
    };
    go(B::Back);
    assert_eq!(ui.app.ui.get_browse_focus(), "tree");
    assert_eq!(
        ui.value("browser.tree"),
        "controller",
        "the tree shows the encoder focus"
    );
    // Tree order: All tracks, Warm-up, Crates (folder).
    go(B::Rotate {
        delta: 2,
        shifted: false,
    });
    assert_eq!(ui.app.ui.get_selected_node(), folder.0 as i32);
    assert!(
        ui.app.ui.get_tree().iter().any(|r| r.id == inner.0 as i32),
        "the folder opens"
    );
    go(B::Press { shifted: false });
    assert_eq!(
        ui.app.ui.get_selected_node(),
        inner.0 as i32,
        "press on an open folder steps into it"
    );
    assert_eq!(ui.app.ui.get_browse_focus(), "tree");
    go(B::Back);
    assert_eq!(
        ui.app.ui.get_selected_node(),
        folder.0 as i32,
        "BACK in the tree goes up to the folder"
    );
    go(B::Rotate {
        delta: -5,
        shifted: false,
    });
    assert_eq!(ui.app.ui.get_node_kind(), "all", "clamped at the top");
    go(B::Press { shifted: false });
    assert_eq!(ui.app.ui.get_browse_focus(), "list");
    assert_eq!(
        ui.app
            .state
            .borrow()
            .browser
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![0],
        "first row selected"
    );
    go(B::Rotate {
        delta: 2,
        shifted: false,
    });
    assert_eq!(
        ui.app
            .state
            .borrow()
            .browser
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![2]
    );
    go(B::Rotate {
        delta: -1,
        shifted: true,
    });
    assert_eq!(
        ui.app
            .state
            .borrow()
            .browser
            .selection
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![0],
        "⇧ steps by eight, clamped"
    );
    go(B::Press { shifted: true });
    assert_eq!(
        ui.app.ui.get_tag_list_count(),
        1,
        "⇧-press collects into the tag list"
    );
    go(B::ViewToggle);
    assert_eq!(
        ui.app.ui.get_prepare_layout(),
        "browser",
        "VIEW enlarges the browser"
    );
    go(B::ViewToggle);
    go(B::Related);
    assert_eq!(ui.app.ui.get_node_kind(), "related");
    go(B::Back);
    go(B::Rotate {
        delta: -20,
        shifted: false,
    });
    go(B::Press { shifted: false });
    // Perform: a press loads the selected track into a free deck.
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(200);
    let title = ui
        .app
        .ui
        .get_tracks()
        .row_data(0)
        .unwrap()
        .title
        .to_string();
    go(B::Press { shifted: false });
    assert!(
        ui.wait_until(30.0, |u| deck_title(u, 1) == title),
        "press loads deck 1, got `{}`",
        deck_title(&ui, 1)
    );
    go(B::ViewToggle);
    assert_eq!(
        slint::ComponentHandle::global::<crate::PerformLayout>(&ui.app.ui).get_layout(),
        "browse",
        "Perform: VIEW switches to the browse layout"
    );
}

fn prefs_el(w: &crate::PreferencesWindow, label: &str) -> i_slint_backend_testing::ElementHandle {
    i_slint_backend_testing::ElementHandle::find_by_accessible_label(w, label)
        .next()
        .unwrap_or_else(|| panic!("no `{label}` in Preferences"))
}

/// Preferences ▸ Controller: the MIDI monitor lists incoming messages,
/// MIDI learn turns a moved control into a mapping entry that is written to
/// the own mapping file (which then loads), and the DDJ-FLX10 settings are
/// stored with the transfers they write.
#[test]
fn controller_page_monitor_learn_and_flx10() {
    use i_slint_backend_testing::ElementHandle;
    use od_controller::monitor::{Direction, MidiMonitor};
    use slint::platform::PointerEventButton;
    let ui = Ui::open((1440, 900));
    let m = MidiMonitor::new(256);
    ui.app.prefs.borrow_mut().midi.monitor = Some(m.clone());
    ui.click("topbar.settings");
    let w = ui
        .app
        .prefs
        .borrow()
        .window
        .as_ref()
        .map(|w| w.clone_strong())
        .expect("Preferences open");
    w.set_page("controller".into());
    w.window().set_size(slint::LogicalSize::new(900.0, 2600.0));
    ui.pump(200);
    prefs_el(&w, "prefs.controller.midi-monitor").mock_single_click(PointerEventButton::Left);
    ui.pump(50);
    assert!(w.get_midi_monitor());
    m.record("DDJ-TEST", Direction::In, &[0x90, 0x0B, 0x7F], Vec::new());
    assert!(
        ui.wait_until(5.0, |_| w.get_midi_lines().row_count() >= 1),
        "the monitor shows the message"
    );
    assert!(
        ElementHandle::find_by_accessible_label(&w, "prefs.controller.midi-log")
            .next()
            .is_some()
    );
    // Learn: a jog-like encoder on CC 0x21.
    let deck_play = od_controller::templates::ACTION_NAMES
        .iter()
        .position(|a| *a == "browse.rotate")
        .unwrap();
    w.invoke_choose("learn.action".into(), deck_play as i32);
    prefs_el(&w, "prefs.controller.learn").mock_single_click(PointerEventButton::Left);
    ui.pump(50);
    assert!(w.get_learn_active());
    for v in [0x41u8, 0x42, 0x41, 0x3F] {
        m.record("DDJ-TEST", Direction::In, &[0xB0, 0x21, v], Vec::new());
    }
    assert!(
        ui.wait_until(5.0, |_| w.get_learn_result() != ""),
        "the learned control is described"
    );
    assert!(
        w.get_learn_entry().contains("cc = 0x21") && w.get_learn_entry().contains("browse.rotate"),
        "{}",
        w.get_learn_entry()
    );
    prefs_el(&w, "prefs.controller.learn-save").mock_single_click(PointerEventButton::Left);
    ui.pump(100);
    let file = crate::midi_tools::default_learn_file(&ui.fx.library);
    let mapping = od_controller::Mapping::load(&file).expect("the learned mapping loads");
    assert!(mapping.matches_port("DDJ-TEST"));
    assert_eq!(
        stored(&ui).controller.mapping_file,
        file.display().to_string(),
        "it becomes the own mapping file"
    );
    assert!(!w.get_learn_active());
    // FLX10.
    w.invoke_choose("flx10.fader-start".into(), 0);
    w.invoke_choose("flx10.boost".into(), 3);
    prefs_el(&w, "prefs.controller.flx10-virtual-deck").mock_single_click(PointerEventButton::Left);
    ui.wait_until(10.0, |u| !stored(u).controller.flx10.virtual_deck);
    let f = stored(&ui).controller.flx10;
    assert_eq!(
        (f.fader_start.as_str(), f.boost_db, f.virtual_deck),
        ("off", 9, false)
    );
    assert_eq!(
        w.get_flx10_wire(),
        crate::midi_tools::flx10_wire_text(&f).as_str(),
        "the transfers to write are shown"
    );
    prefs_el(&w, "prefs.controller.flx10-apply").mock_single_click(PointerEventButton::Left);
    ui.wait_until(10.0, |u| u.app.ui.get_status_text().contains("FLX10"));
    assert!(
        ui.app.ui.get_status_text().contains("FLX10"),
        "the write reports back: {}",
        ui.app.ui.get_status_text()
    );
}

/// Separate parts ahead of time: the row menu queues the selected tracks
/// on the background separation worker (stem cache of the decks) and the
/// status line reports the batch. Headless runs have no model, so every
/// job ends as failed — the batch still completes and says why.
#[test]
fn separate_parts_for_selected_tracks() {
    let ui = Ui::open((1440, 900));
    let rows: Vec<_> = ui
        .fx
        .tracks
        .iter()
        .map(|(_, t)| ui.by_value("browser.row", t).unwrap().1)
        .collect();
    ui.click_el(&rows[0]);
    ui.app.ui.invoke_row_clicked(1, true, false);
    ui.pump(50);
    ui.right_click_el(&rows[1]);
    assert!(
        ui.click_menu_item("Separate parts"),
        "row menu offers Separate parts"
    );
    let c = crate::stems_batch::count();
    assert_eq!(c.total, 2, "both selected tracks queued");
    assert!(
        ui.wait_until(30.0, |_| crate::stems_batch::count().finished()),
        "the batch completes: {:?}",
        crate::stems_batch::count()
    );
    let c = crate::stems_batch::count();
    assert_eq!(c.done + c.failed, 2);
    assert!(
        ui.wait_until(5.0, |u| u.app.ui.get_status_text().contains('2')),
        "status reports the batch: {}",
        ui.app.ui.get_status_text()
    );
}

/// Prepare layouts: 2 players (the full player plus a compact second one),
/// dual (two compact players side by side) and simple (one compact player);
/// the second player takes the selected track with LOAD or a dragged row,
/// plays on the preview deck and seeks from its overview; the layout is
/// stored.
#[test]
fn prepare_layouts_two_dual_and_simple() {
    let ui = Ui::open((1440, 900));
    let a = ui.fx.tracks[0].1.clone();
    let b = ui.fx.tracks[1].1.clone();
    ui.click_el(&ui.by_value("browser.row", &a).unwrap().1);
    ui.app.ui.invoke_menu_action("view.layout.two".into());
    ui.pump(200);
    assert!(
        ui.has("player1.enlarged") && ui.has("player2.overview"),
        "full player 1 and compact player 2"
    );
    assert_eq!(
        ui.fx.lib().meta("ui.prepare_layout").unwrap().as_deref(),
        Some("two")
    );
    // LOAD takes the selected track.
    ui.click_el(&ui.by_value("browser.row", &b).unwrap().1);
    ui.click("player2.load");
    assert_eq!(ui.value("player2.title"), b);
    // A row dragged onto player 2.
    let target = Rect::of(&ui.get("player2.overview")).centre();
    ui.drag_el(&ui.by_value("browser.row", &a).unwrap().1, target);
    assert!(
        ui.wait_until(5.0, |u| u.value("player2.title") == a),
        "dragged row loads player 2, got `{}`",
        ui.value("player2.title")
    );
    // Seek and play on the preview deck.
    let ov = Rect::of(&ui.get("player2.overview"));
    ui.click_at(
        LogicalPosition::new(ov.x + ov.w * 0.5, ov.centre().y),
        slint::platform::PointerEventButton::Left,
    );
    assert!(
        ui.value("player2.time-elapsed").starts_with("0:04"),
        "seek to the middle: {}",
        ui.value("player2.time-elapsed")
    );
    ui.click("player2.play");
    let id_a = ui.fx.tracks[0].0;
    let playing = ui.wait_until(20.0, |u| {
        crate::engine::snapshot(&u.app).is_some_and(|s| {
            s.decks[crate::decks::PREVIEW_DECK].track == Some(id_a)
                && s.decks[crate::decks::PREVIEW_DECK].playing
        })
    });
    assert!(playing, "player 2 plays on the preview deck");
    assert_eq!(ui.get("player2.play").accessible_checked(), Some(true));
    ui.click("player2.play");
    // Dual: two compact players.
    ui.app.ui.invoke_menu_action("view.layout.dual".into());
    ui.pump(200);
    let (p1, p2) = (
        Rect::of(&ui.get("player1.overview")),
        Rect::of(&ui.get("player2.overview")),
    );
    assert!(
        p1.right() <= p2.x + 1.0 && (p1.y - p2.y).abs() < 1.0,
        "side by side: {p1:?} {p2:?}"
    );
    assert!(!ui.has("player1.enlarged"));
    assert_eq!(
        ui.value("player1.title"),
        a,
        "player 1 shows the selected track (the row pressed last)"
    );
    // Simple: one compact player.
    ui.app.ui.invoke_menu_action("view.layout.simple".into());
    ui.pump(200);
    assert!(
        ui.has("player1.overview") && !ui.has("player2.overview") && !ui.has("player1.enlarged")
    );
    // The top-bar switch moves on to the full browser and back to the player.
    ui.click("topbar.layout");
    assert_eq!(ui.app.ui.get_prepare_layout(), "browser");
    ui.click("topbar.layout");
    assert_eq!(ui.app.ui.get_prepare_layout(), "player");
}

/// Artwork: a track's cover art shows in the info panel and next to the
/// title in the Prepare player.
#[test]
fn artwork_in_info_panel_and_player() {
    let fx = Fixture::new();
    {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 40, 40, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let lib = fx.lib();
        lib.conn()
            .execute(
                "INSERT INTO artwork (track_id, media_type, data) VALUES (?1, 'image/png', ?2)",
                (fx.tracks[1].0.0, png),
            )
            .unwrap();
    }
    let ui = Ui::start(fx, (1440, 900), false);
    ui.app.ui.set_info_open(true);
    ui.click_el(
        &ui.by_value("browser.row", &ui.fx.tracks[0].1.clone())
            .unwrap()
            .1,
    );
    ui.click("info.tab.artwork");
    assert!(
        !ui.has("info.artwork") && ui.value("player1.artwork") == "none",
        "no art, no image (the player keeps an empty frame)"
    );
    ui.click_el(
        &ui.by_value("browser.row", &ui.fx.tracks[1].1.clone())
            .unwrap()
            .1,
    );
    assert!(
        ui.has("info.artwork") && ui.value("player1.artwork") == "cover",
        "the cover shows"
    );
    let size = ui.app.ui.get_track_info().artwork.size();
    assert_eq!((size.width, size.height), (264, 264), "a square thumbnail");
}

/// File ▸ Import Music / iTunes Library XML: the tracks of the export and
/// its playlist appear in the tree (in their own folder).
#[test]
fn music_library_xml_import() {
    let ui = Ui::open((1440, 900));
    let song = new_wav(&ui, "Library Song.wav", 720.0);
    let loc = od_import_rekordbox::xml::location(&song).replace("file://localhost", "file://");
    let xml = ui.fx.root.join("Library.xml");
    std::fs::write(
        &xml,
        format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Tracks</key><dict><key>7</key><dict>\
             <key>Track ID</key><integer>7</integer><key>Name</key><string>From Music</string><key>Location</key><string>{loc}</string>\
             </dict></dict><key>Playlists</key><array><dict><key>Name</key><string>Sunday</string>\
             <key>Playlist Items</key><array><dict><key>Track ID</key><integer>7</integer></dict></array></dict></array></dict></plist>"
        ),
    )
    .unwrap();
    let m = crate::import::import_itunes(&ui.app, &xml);
    assert!(m.contains("1 tracks") && m.contains("1 playlists"), "{m}");
    let folder = ui
        .fx
        .lib()
        .all_nodes()
        .unwrap()
        .into_iter()
        .find(|n| n.name == od_import_rekordbox::itunes::IMPORT_FOLDER)
        .expect("import folder");
    assert!(
        ui.by_value(
            "browser.tree.node",
            od_import_rekordbox::itunes::IMPORT_FOLDER
        )
        .is_some(),
        "the folder is in the tree"
    );
    ui.app.ui.invoke_toggle_node(folder.id.0 as i32);
    ui.pump(100);
    let (_, row) = ui
        .by_value("browser.tree.node", "Sunday")
        .expect("the playlist is in the opened folder");
    ui.click_el(&row);
    assert!(
        ui.wait_until(5.0, |u| u.by_value("browser.row", "From Music").is_some()),
        "it lists the track"
    );
}

/// Variable-tempo grid: + MARKER in the grid tools puts the typed tempo on
/// the grid from the beat at the cursor on; the beats before keep theirs.
#[test]
fn tempo_marker_in_the_grid_editor() {
    let ui = Ui::open((1440, 900));
    let id = ui.fx.tracks[0].0;
    ui.click_el(
        &ui.by_value("browser.row", &ui.fx.tracks[0].1.clone())
            .unwrap()
            .1,
    );
    crate::player::type_bpm(&ui.app, "120");
    ui.click("player1.tab.grid");
    assert!(ui.has("player1.grid.marker"));
    ui.app.ui.invoke_seek(0.5);
    ui.pump(50);
    let cursor = ui.app.state.borrow().player.cursor;
    let field = ui.get("player1.grid.bpm");
    ui.double_click_el(&field);
    ui.key(slint::platform::Key::End);
    for _ in 0..8 {
        ui.key(slint::platform::Key::Backspace);
    }
    ui.type_text("100");
    ui.click("player1.grid.marker");
    let grid = ui.fx.lib().beatgrid(id).unwrap().unwrap();
    assert_eq!(crate::player::tempo_changes(&grid), 1, "one tempo change");
    assert!(
        grid.beats
            .iter()
            .filter(|b| b.time < cursor - 0.3)
            .all(|b| (b.bpm - 120.0).abs() < 1e-6),
        "before the cursor: 120"
    );
    assert!(
        grid.beats
            .iter()
            .filter(|b| b.time > cursor + 0.3)
            .all(|b| (b.bpm - 100.0).abs() < 1e-6),
        "after it: 100"
    );
}
