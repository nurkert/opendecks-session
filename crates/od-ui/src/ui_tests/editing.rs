//! Behaviour tests of the Prepare editing features of
//! `tests/parity/features.toml`: phrase display options, phrase editing
//! (PHRASE tab), edit mode (EDIT tab: a region rendered into a new file)
//! and the sub-browser.

use super::harness::*;
use i_slint_backend_testing::ElementHandle;
use od_core::{Beatgrid, DataSource, Phrase, PhraseKind, TrackAnalysis, TrackId};
use slint::{ComponentHandle, Model};

/// Adds a `secs` long 120 BPM track to the fixture library, analysed: a
/// constant grid from 0 s and the given phrases (bars, kinds).
fn analysed_track(fx: &Fixture, title: &str, secs: f32, phrases: &[(u32, u32, PhraseKind)]) -> TrackId {
    let path = fx.root.join(format!("music/{title}.wav"));
    write_wav(&path, secs, 330.0);
    let mut lib = fx.lib();
    let id = crate::workers::import_file(&mut lib, &path).unwrap();
    let mut tags = lib.track(id).unwrap().tags;
    tags.title = title.into();
    tags.artist = "Test Artist".into();
    lib.set_tags(id, tags).unwrap();
    let a = TrackAnalysis {
        analysis_version: od_analysis::ANALYSIS_VERSION,
        beatgrid: Beatgrid::constant(120.0, 0.0, 1, secs as f64),
        phrases: phrases.iter().map(|&(s, e, kind)| Phrase { start_beat: s * 4, end_beat: e * 4, kind }).collect(),
        ..Default::default()
    };
    lib.store_analysis(id, &a, DataSource::Analysis).unwrap();
    id
}

/// Shows `id` in the Prepare player.
fn show(ui: &Ui, id: TrackId) {
    {
        let mut st = ui.app.state.borrow_mut();
        st.player.current = Some(id);
        st.player.cursor = 0.0;
    }
    crate::player::refresh_detail(&ui.app);
    ui.pump(100);
}

/// Moves the player cursor to `secs` with a click on the overview position.
fn seek(ui: &Ui, id: TrackId, secs: f64) {
    let d = ui.fx.lib().track(id).unwrap().duration;
    ui.app.ui.invoke_seek((secs / d) as f32);
    ui.pump(50);
}

fn phrases(ui: &Ui, id: TrackId) -> Vec<(u32, u32, PhraseKind)> {
    ui.fx.lib().analysis(id).unwrap().phrases.iter().map(|p| (p.start_beat / 4, p.end_beat / 4, p.kind)).collect()
}

fn prefs(ui: &Ui) -> crate::PreferencesWindow {
    ui.click("topbar.settings");
    let w = ui.app.prefs.borrow().window.as_ref().map(|w| w.clone_strong()).expect("Preferences open");
    w.set_page("view".into());
    w.window().set_size(slint::LogicalSize::new(900.0, 1600.0));
    ui.pump(200);
    w
}

fn prefs_el(w: &crate::PreferencesWindow, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(w, label).next().unwrap_or_else(|| panic!("no `{label}` in Preferences"))
}

fn stored(ui: &Ui) -> crate::settings::Settings {
    crate::settings::Settings::from_json(&ui.fx.lib().meta(crate::settings::META_SETTINGS).unwrap().unwrap())
}

/// Preferences > View: phrases on the enlarged waveform and on the
/// overview switch separately; phrase types show as names or colours only.
#[test]
fn phrase_display_options_in_preferences() {
    use PhraseKind::*;
    let fx = Fixture::new();
    let id = analysed_track(&fx, "Phrased", 32.0, &[(0, 8, Intro), (8, 16, Chorus)]);
    let ui = Ui::start(fx, (1440, 900), false);
    show(&ui, id);
    let labels = |u: &Ui| u.app.ui.get_track_phrases().iter().map(|m| m.label.to_string()).collect::<Vec<_>>();
    assert_eq!(labels(&ui), ["INTRO", "CHORUS"], "names on the phrase bar by default");
    let w = prefs(&ui);
    let overview = prefs_el(&w, "prefs.view.phrase-overview");
    prefs_el(&w, "prefs.view.phrase-enlarged");
    prefs_el(&w, "prefs.view.phrase-types");
    overview.mock_single_click(slint::platform::PointerEventButton::Left);
    ui.pump(200);
    assert!(!stored(&ui).view.phrases_overview && stored(&ui).view.phrases, "only the overview phrase bar is off");
    assert!(!ui.app.ui.get_show_phrases_overview() && ui.app.ui.get_show_phrases(), "the window follows");
    w.invoke_flag("view.phrases".into(), false);
    w.invoke_flag("view.phrases-overview".into(), true);
    ui.pump(100);
    assert!(ui.app.ui.get_show_phrases_overview() && !ui.app.ui.get_show_phrases(), "and the enlarged waveform separately");
    w.invoke_choose("view.phrase-types".into(), 1);
    ui.pump(100);
    assert_eq!(stored(&ui).view.phrase_types, "colors");
    assert!(!ui.app.ui.global::<crate::Theme>().get_phrase_names(), "phrase bars drop their names");
    assert_eq!(labels(&ui), ["", ""], "colour blocks only in the Prepare player");
    assert_eq!(ui.app.ui.get_track_phrases().row_count(), 2, "the blocks stay");
}

/// PHRASE tab: cut the phrase at the cursor, change its kind, move a
/// boundary, undo, clear — stored in the library, normalized.
#[test]
fn phrase_editing_in_the_prepare_player() {
    use PhraseKind::*;
    let fx = Fixture::new();
    let id = analysed_track(&fx, "Phrase Edit", 40.0, &[(0, 8, Intro), (8, 20, Chorus)]);
    let ui = Ui::start(fx, (1440, 900), false);
    show(&ui, id);
    ui.click("player1.tab.phrase");
    for l in ["player1.phrase.kind0", "player1.phrase.kind6", "player1.phrase.cut", "player1.phrase.boundary", "player1.phrase.clear"] {
        assert!(ui.has(l), "{l}");
    }
    let current = |u: &Ui| u.app.ui.global::<crate::PhraseEdit>().get_current().to_string();
    // Cut at bar 4 (8 s): the part after the cursor gets its own kind.
    seek(&ui, id, 8.0);
    assert_eq!(current(&ui), "INTRO");
    ui.click("player1.phrase.cut");
    assert_eq!(phrases(&ui, id), [(0, 4, Intro), (4, 8, Verse), (8, 20, Chorus)]);
    assert_eq!(current(&ui), "VERSE", "the tab shows the phrase at the cursor");
    // That part becomes a chorus: it joins the chorus after it.
    ui.click("player1.phrase.kind2");
    assert_eq!(phrases(&ui, id), [(0, 4, Intro), (4, 20, Chorus)]);
    // The nearest boundary (bar 4) moves to the cursor at bar 6.
    seek(&ui, id, 12.0);
    ui.click("player1.phrase.boundary");
    assert_eq!(phrases(&ui, id), [(0, 6, Intro), (6, 20, Chorus)]);
    assert_eq!(ui.app.ui.get_track_phrases().row_count(), 2, "the phrase bar follows");
    // A cut that would leave a part under 4 bars changes nothing.
    seek(&ui, id, 14.0);
    ui.click("player1.phrase.cut");
    assert_eq!(phrases(&ui, id), [(0, 6, Intro), (6, 20, Chorus)]);
    assert!(ui.value("status.message").contains("4 bars"), "and says why: {}", ui.value("status.message"));
    // One undo step per edit.
    ui.app.undo();
    assert_eq!(phrases(&ui, id), [(0, 4, Intro), (4, 20, Chorus)]);
    ui.click("player1.phrase.clear");
    assert!(phrases(&ui, id).is_empty(), "cleared");
    assert_eq!(ui.app.ui.get_track_phrases().row_count(), 0);
    // Without phrases a kind fills the whole grid.
    ui.click("player1.phrase.kind4");
    assert_eq!(phrases(&ui, id), [(0, 20, Up)]);
}

/// EDIT tab: IN / OUT at the cursor and TRIM render a new WAV with only the
/// region; it joins the collection as "<title> (Edit)" and opens in the
/// player; the original file and track stay as they were.
#[test]
fn edit_mode_renders_a_new_track() {
    let fx = Fixture::new();
    let id = analysed_track(&fx, "Edit Source", 16.0, &[]);
    let ui = Ui::start(fx, (1440, 900), false);
    let original = ui.fx.lib().track(id).unwrap();
    let bytes = std::fs::read(&original.path).unwrap();
    show(&ui, id);
    ui.click("player1.tab.edit");
    assert!(ui.has("player1.edit.trim") && ui.has("player1.edit.cut") && ui.has("player1.edit.repeat"));
    seek(&ui, id, 4.0);
    ui.click("player1.edit.in");
    seek(&ui, id, 10.0);
    ui.click("player1.edit.out");
    assert!(ui.app.ui.global::<crate::TrackEdit>().get_ready(), "region set");
    assert!(ui.value("player1.edit.region").contains("0:04.000"), "region readout, got `{}`", ui.value("player1.edit.region"));
    assert!(ui.has("player1.edit.span"), "the region shows on the overview");
    ui.click("player1.edit.trim");
    let edited = |u: &Ui| u.fx.lib().tracks(od_library::SortKey::DateAdded, true).unwrap().into_iter().find(|t| t.tags.title == "Edit Source (Edit)");
    assert!(ui.wait_until(60.0, |u| edited(u).is_some()), "the edit joins the collection");
    let t = edited(&ui).unwrap();
    assert!((t.duration - 6.0).abs() < 0.02, "only the region: {} s", t.duration);
    assert_eq!(t.path.file_name().unwrap().to_string_lossy(), "Edit Source (Edit).wav");
    assert_eq!(t.tags.artist, "Test Artist", "tags carried over");
    assert!(ui.wait_until(10.0, |u| u.value("player1.title") == "Edit Source (Edit)"), "the player opens the edit");
    assert_eq!(std::fs::read(&original.path).unwrap(), bytes, "the original file is untouched");
    assert_eq!(ui.fx.lib().track(id).unwrap().duration, original.duration, "the original track too");
    // A region dragged on the overview (fractions), then cleared.
    ui.app.ui.global::<crate::TrackEdit>().invoke_select(0.5, 0.25);
    ui.pump(50);
    let te = ui.app.ui.global::<crate::TrackEdit>();
    assert!(te.get_ready() && te.get_start() < te.get_end(), "either drag direction");
    ui.click("player1.edit.clear");
    assert!(!te.get_ready() && te.get_start() < 0.0);
}

/// Cut and repeat render the expected lengths (the library side of the
/// EDIT tab, without the window).
#[test]
fn edit_mode_cut_and_repeat_lengths() {
    let fx = Fixture::new();
    let id = analysed_track(&fx, "Lengths", 12.0, &[]);
    let before = fx.lib().track(id).unwrap();
    let cut = crate::track_edit::run_edit(&fx.library, id, crate::track_edit::EditOp::Cut, 2.0, 5.0).unwrap();
    let rep = crate::track_edit::run_edit(&fx.library, id, crate::track_edit::EditOp::Repeat, 2.0, 5.0).unwrap();
    let lib = fx.lib();
    let (c, r) = (lib.track(cut).unwrap(), lib.track(rep).unwrap());
    assert!((c.duration - 9.0).abs() < 0.02, "cut: {}", c.duration);
    assert!((r.duration - 15.0).abs() < 0.02, "repeat: {}", r.duration);
    assert!(r.path.to_string_lossy().ends_with("Lengths (Edit 2).wav"), "a second edit gets its own file: {}", r.path.display());
    assert_eq!(lib.track(id).unwrap(), before, "the original track is unchanged");
}

/// The sub-browser (SUB in the list header, View > Sub Browser): a second
/// tree and list; main-list rows dropped on one of its lists are added
/// there, its rows dropped on a list in the main tree are added there.
#[test]
fn sub_browser_drags_between_lists() {
    let fx = Fixture::new();
    let peak = fx.lib().create_node(None, od_library::NodeKind::Playlist, "Peak").unwrap();
    let ui = Ui::start(fx, (1440, 900), false);
    let pl = ui.fx.playlist;
    assert!(!ui.has("browser.sub"));
    ui.click("browser.panel.sub-browser");
    assert!(ui.has("browser.sub") && ui.has("browser.sub.tree"), "the sub-browser opens under the list");
    ui.click(&format!("browser.sub.node{}", pl.0));
    assert_eq!(ui.value("browser.sub.title"), "Warm-up");
    assert!(ui.has("browser.sub.row0") && !ui.has("browser.sub.row1"), "its one track");
    // A main-list row onto the sub-browser's list.
    let (b, tb) = ui.fx.tracks[1].clone();
    let main_row = ui.by_value("browser.row", &tb).expect("row").1;
    let target = Rect::of(&ui.get(&format!("browser.sub.node{}", pl.0))).centre();
    ui.drag_el(&main_row, target);
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(pl).unwrap().len() == 2), "added to the sub-browser's list");
    assert!(ui.wait_until(5.0, |u| u.has("browser.sub.row1")), "the sub list shows it");
    // A sub-browser row onto a list in the main tree.
    let to = Rect::of(&ui.get(&format!("browser.tree.node{}", peak.0))).centre();
    ui.drag_el(&ui.get("browser.sub.row1"), to);
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(peak).unwrap().iter().map(|e| e.track).collect::<Vec<_>>() == [b]), "added to Peak");
    assert!(!ui.app.ui.global::<crate::DragState>().get_active());
    // Onto its own list: nothing happens.
    let own = Rect::of(&ui.get(&format!("browser.sub.node{}", pl.0))).centre();
    ui.drag_el(&ui.get("browser.sub.row0"), own);
    assert_eq!(ui.fx.lib().entries(pl).unwrap().len(), 2);
    // View > Sub Browser closes it again; its shortcut opens it.
    ui.app.ui.invoke_menu_action("view.panel.sub-browser".into());
    ui.pump(100);
    assert!(!ui.has("browser.sub") && !ui.app.ui.get_sub_browser_open());
    let chord = ui.app.state.borrow().keymap.bindings("view.panel.sub-browser")[0].clone();
    let m = chord.mods;
    assert!(ui.app.ui.invoke_key_pressed(chord.key.to_lowercase().into(), m.primary, m.secondary, m.alt, m.shift));
    ui.pump(100);
    assert!(ui.has("browser.sub"), "the shortcut toggles it");
}
