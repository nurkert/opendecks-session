//! SPEC §16.4 "Steps to common tasks": the flows of
//! `tests/parity/flows.toml`, driven through the real window.
//!
//! Every user action goes through the harness input helpers, which count
//! one step each (`meta.step_rule`: a click, a drag, a key, a hover that
//! opens a submenu, one typed text). Each test asserts that the task's end
//! state is reached as far as the app supports it, records the counted
//! steps in `target/parity/flows/<id>.json` (read by `parity check`) and,
//! with `OD_PARITY_STRICT=1`, fails when the flow is not achievable or needs
//! more steps than its target.

use super::harness::*;
use super::views::{strict, workspace_root};
use od_core::CueKind;
use std::time::Duration;

struct FlowResult {
    id: &'static str,
    steps: u32,
    achieved: bool,
    note: String,
}

fn record(r: FlowResult) {
    let flows: od_parity::model::FlowsFile = od_parity::model::load(&workspace_root().join(od_parity::FLOWS)).unwrap();
    let flow = flows.flows.iter().find(|f| f.id == r.id).expect("flow in flows.toml");
    let within = r.steps <= flow.target_steps;
    let dir = workspace_root().join("target/parity/flows");
    let _ = std::fs::create_dir_all(&dir);
    let json = format!(
        "{{\"id\":{},\"steps\":{},\"target\":{},\"reference\":{},\"achieved\":{},\"within_target\":{},\"note\":{}}}\n",
        serde_json::to_string(r.id).unwrap(),
        r.steps,
        flow.target_steps,
        flow.reference_steps,
        r.achieved,
        within,
        serde_json::to_string(&r.note).unwrap()
    );
    std::fs::write(dir.join(format!("{}.json", r.id)), json).unwrap();
    eprintln!("flow {}: {} steps (target {}, usual {}), achieved: {} {}", r.id, r.steps, flow.target_steps, flow.reference_steps, r.achieved, r.note);
    if strict() {
        assert!(r.achieved, "flow {} not achievable: {}", r.id, r.note);
        assert!(within, "flow {} took {} steps, target {}", r.id, r.steps, flow.target_steps);
    }
}

fn perform(ui: &Ui) {
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(300);
}

/// The deck's title read-out once a track is loaded.
pub(crate) fn deck_title(ui: &Ui, deck: usize) -> String {
    ui.find(&format!("deck{deck}.title")).and_then(|e| e.accessible_value()).map(|v| v.to_string()).unwrap_or_default()
}

/// The deck view models (Perform).
pub(crate) fn deck_views(ui: &Ui) -> Vec<crate::DeckView> {
    use slint::Model;
    ui.app.ui.get_decks().iter().collect()
}

/// Load a file that is not in the collection from a folder on disk into
/// deck 1.
#[test]
fn load_from_explorer() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let before = ui.fx.lib().track_count().unwrap();
    assert!(!deck_title(&ui, 1).contains("Outside"));
    ui.steps.set(0);
    // The Explorer roots are always listed in the sidebar (no source switch
    // needed); the folder sits one level below the root.
    ui.click("browser.tree.home.expand");
    // In Perform the tree pane is short: the user scrolls it to the folder.
    let crate_row = ui.scroll_into_view("browser.tree", |u| u.by_value("browser.tree.folder", "Crate").map(|x| x.1)).expect("folder below the Explorer root");
    ui.click_el(&crate_row);
    assert!(ui.wait_until(10.0, |u| u.by_value("browser.row", "Outside Loop").is_some()), "folder lists its file");
    let (_, file_row) = ui.by_value("browser.row", "Outside Loop").unwrap();
    ui.double_click_el(&file_row);
    let steps = ui.steps.get();
    let loaded = ui.wait_until(30.0, |u| deck_title(u, 1).contains("Outside Loop"));
    // The file was added to the collection on load (SPEC §5.5).
    let imported = ui.fx.lib().track_by_path(&ui.fx.outside_file).unwrap().is_some();
    assert!(loaded, "deck 1 shows the file, got `{}`", deck_title(&ui, 1));
    assert!(imported && ui.fx.lib().track_count().unwrap() == before + 1, "the file is in the collection");
    record(FlowResult { id: "load-from-explorer", steps, achieved: loaded && imported, note: format!("expand the root, {}click the folder, double-click the file", if steps > 3 { "scroll the short Perform tree pane, " } else { "" }) });
}

/// Set hot cue A at the playhead and name it.
#[test]
fn set_and_name_hot_cue() {
    let ui = Ui::open((1440, 900));
    let track = ui.fx.tracks[0].0;
    // Start: Prepare, the track loaded in the player and paused.
    let (_, row) = ui.by_value("browser.row", &ui.fx.tracks[0].1).expect("row");
    ui.click_el(&row);
    assert!(ui.wait_until(10.0, |u| u.has("player1.hot-cue.A")), "player shows the hot cue pads");
    ui.steps.set(0);
    ui.click("player1.hot-cue.A");
    let set = ui.wait_until(20.0, |u| u.fx.lib().cues(track).unwrap().iter().any(|c| c.kind == CueKind::Hot(0)));
    assert!(set, "hot cue A is stored in the library");
    // Naming: double-click the cue in the list, type, Return.
    let row = ui.get("player1.cue-list.row0");
    ui.double_click_el(&row);
    ui.type_text("Drop");
    ui.key(slint::platform::Key::Return);

    let named = ui.wait_until(20.0, |u| u.fx.lib().cues(track).unwrap().iter().any(|c| c.kind == CueKind::Hot(0) && c.label == "Drop"));
    record(FlowResult {
        id: "set-and-name-hot-cue",
        steps: ui.steps.get(),
        achieved: named,
        note: if named { String::new() } else { "the cue name was not stored".into() },
    });
    assert!(named, "hot cue A is named `Drop`");
}

/// Add the selected track to an existing playlist.
#[test]
fn add_to_playlist() {
    let ui = Ui::open((1440, 900));
    let (id, title) = ui.fx.tracks[1].clone();
    let (_, row) = ui.by_value("browser.row", &title).expect("row");
    ui.click_el(&row);
    ui.steps.set(0);
    ui.right_click_el(&row);
    assert!(ui.click_menu_item("Add to playlist"), "row menu has Add to playlist");
    assert!(ui.click_menu_item("Warm-up"), "submenu lists the playlist");
    let steps = ui.steps.get();
    let added = ui.wait_until(5.0, |u| u.fx.lib().entries(u.fx.playlist).unwrap().iter().any(|e| e.track == id));
    assert!(added, "the track is in the playlist");
    record(FlowResult { id: "add-to-playlist", steps, achieved: added, note: "right-click, open the Add to playlist submenu, click the playlist (no inline recent-playlists group)".into() });
}

/// Bring an edited playlist onto a registered USB stick: no step at all.
#[test]
fn sync_stick() {
    let fx = Fixture::new();
    let stick = test_env().join(format!("volumes/STICK-{}", fx.root.file_name().unwrap().to_string_lossy()));
    std::fs::create_dir_all(&stick).unwrap();
    let ui = Ui::start(fx, (1440, 900), false);
    // Start state: the stick is registered (with the playlist) and synced.
    assert!(ui.wait_until(20.0, |u| u.with_prefix("browser.tree.volume").iter().any(|(l, _, _)| l.ends_with(".register"))), "the stick shows up");
    // Registration takes the selected list onto the stick.
    ui.click(&format!("browser.tree.node{}", ui.fx.playlist.0));
    let register = ui.with_prefix("browser.tree.volume").into_iter().find(|(l, _, _)| l.ends_with(".register")).unwrap().2;
    ui.click_el(&register);
    let (track, _) = ui.fx.tracks[0].clone();
    let on_stick = |cue_at: Option<f64>| {
        let Ok(Some(s)) = od_format_native::Stick::open(&stick) else { return false };
        match cue_at {
            None => s.library.track(track).is_ok(),
            Some(t) => s.library.cues(track).unwrap_or_default().iter().any(|c| (c.start - t).abs() < 1e-6),
        }
    };
    let synced = ui.wait_until(40.0, |_| on_stick(None));
    if !synced {
        let _ = std::fs::remove_dir_all(&stick);
        panic!("first sync did not reach the stick");
    }
    // The edit: a track of the playlist gets a new hot cue (through the
    // library, as any editor does).
    {
        let mut lib = ui.fx.lib();
        let mut cues = lib.cues(track).unwrap();
        cues.push(od_core::Cue::hot(2, 3.25));
        lib.set_cues(track, cues).unwrap();
    }
    ui.steps.set(0);
    let reached = ui.wait_until(40.0, |_| on_stick(Some(3.25)));
    // Status is visible: the stick row in the sidebar.
    let status_visible = ui.with_prefix("browser.tree.device").iter().any(|(l, _, _)| !l.contains('.') || l.matches('.').count() == 2);
    let _ = std::fs::remove_dir_all(&stick);
    assert!(reached, "the new hot cue reaches the stick without any input");
    record(FlowResult { id: "sync-stick", steps: ui.steps.get(), achieved: reached && status_visible, note: "automatic sync after the edit; stick row with its sync state in the sidebar".into() });
}

/// Start recording the mix.
#[test]
fn start_recording() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    ui.steps.set(0);
    ui.click("rec.toggle");
    let steps = ui.steps.get();
    let recording = ui.wait_until(10.0, |u| u.app.ui.get_recording());
    assert!(recording, "recording runs");
    std::thread::sleep(Duration::from_millis(300));
    ui.pump(300);
    // Stop it again and check the file.
    ui.click("rec.toggle");
    assert!(ui.wait_until(10.0, |u| !u.app.ui.get_recording()));
    let dir = ui.fx.root.join("recordings");
    let files: Vec<_> = std::fs::read_dir(&dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    assert!(files.iter().any(|p| p.extension().is_some_and(|e| e == "wav")), "a WAV file in {}: {files:?}", dir.display());
    record(FlowResult { id: "start-recording", steps, achieved: recording, note: String::new() });
}

/// Separate a loaded track and mute its vocals.
#[test]
fn separate_stems() {
    let ui = Ui::open((1440, 900));
    perform(&ui);
    let title = ui.fx.tracks[0].1.clone();
    // Start: the track loaded on deck 1 (drag from the list), never separated.
    let (_, row) = ui.by_value("browser.row", &title).unwrap();
    let target = Rect::of(&ui.get("deck1.title")).centre();
    ui.drag_el(&row, target);
    assert!(ui.wait_until(30.0, |u| deck_title(u, 1) == title), "deck 1 loads, got `{}`", deck_title(&ui, 1));
    // Separation is queued on load; the strip says so and the stem
    // controls stay in place, dimmed.
    let queued = ui.wait_until(10.0, |u| deck_views(u).into_iter().any(|d| d.deck == 1 && d.stems_busy) || u.app.decks.borrow().stems_requested());
    assert!(queued, "separation job queued on load");
    assert!(ui.has("deck1.stem.vocal"), "VOCAL stays visible while separating");
    ui.steps.set(0);
    // With a model, VOCAL becomes available and mutes the vocals in one
    // click. Headless runs never download the model.
    let available = ui.wait_until(2.0, |u| deck_views(u).into_iter().any(|d| d.deck == 1 && d.stems_available));
    let mut muted = false;
    if available {
        ui.click("deck1.stem.vocal");
        muted = ui.wait_until(5.0, |u| deck_views(u).into_iter().any(|d| d.deck == 1 && d.mute_vocal));
    }
    record(FlowResult {
        id: "separate-stems",
        steps: if available { ui.steps.get() } else { 1 },
        achieved: muted,
        note: if available { String::new() } else { "separation is queued on load and the strip shows SPLIT…; no model on this machine, so VOCAL stays dimmed".into() },
    });
}
