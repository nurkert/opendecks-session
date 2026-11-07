//! Behaviour tests for the Prepare grid editor, Preferences (deck and mixer
//! defaults, browser text, UI scale), the View / File menu entries
//! (layouts, panels, duplicates, XML, archive, consolidate, rekordbox
//! preferences), sets (planned start, backups) and the recording panel.

use super::harness::*;
use i_slint_backend_testing::ElementHandle;
use od_library::NodeKind;
use slint::ComponentHandle;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use std::time::Duration;

fn prefs(ui: &Ui, page: &str) -> crate::PreferencesWindow {
    ui.click("topbar.settings");
    let w = ui.app.prefs.borrow().window.as_ref().map(|w| w.clone_strong()).expect("Preferences open");
    w.set_page(page.into());
    ui.pump(200);
    w
}

fn prefs_el(w: &crate::PreferencesWindow, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(w, label).next().unwrap_or_else(|| panic!("no `{label}` in Preferences"))
}

fn stored(ui: &Ui) -> crate::settings::Settings {
    crate::settings::Settings::from_json(&ui.fx.lib().meta(crate::settings::META_SETTINGS).unwrap().unwrap())
}

/// Presses the default binding of a keymap action.
fn press_action(ui: &Ui, id: &str) {
    let chord = ui.app.state.borrow().keymap.bindings(id)[0].clone();
    let text = if chord.key.chars().count() == 1 { chord.key.to_lowercase() } else { chord.key.clone() };
    let m = chord.mods;
    assert!(ui.app.ui.invoke_key_pressed(text.into(), m.primary, m.secondary, m.alt, m.shift), "{id} handled");
    ui.pump(100);
}

/// Replaces the text of the focused field.
fn retype(ui: &Ui, label: &str, text: &str) {
    let f = ui.get(label);
    ui.click_el(&f);
    ui.key(Key::End);
    for _ in 0..16 {
        ui.key(Key::Backspace);
    }
    ui.type_text(text);
    ui.key(Key::Return);
}

/// Grid editor: a typed BPM and TAP set a constant grid at that tempo.
#[test]
fn grid_editor_typed_bpm_and_tap_tempo() {
    let ui = Ui::open((1440, 900));
    let (id, _) = ui.fx.tracks[0].clone();
    ui.app.ui.invoke_row_clicked(0, false, false);
    ui.pump(200);
    let id = ui.app.state.borrow().player.current.unwrap_or(id);
    ui.click("player1.tab.grid");
    assert!(ui.has("player1.grid.bpm") && ui.has("player1.grid.tap"), "BPM field and TAP in the grid tools");
    // Type 128 and press Return.
    retype(&ui, "player1.grid.bpm", "128");
    let bpm = |u: &Ui| u.fx.lib().beatgrid(id).unwrap().and_then(|g| g.dominant_bpm()).unwrap_or(0.0);
    assert!(ui.wait_until(5.0, |u| (bpm(u) - 128.0).abs() < 0.01), "grid at 128 BPM, got {}", bpm(&ui));
    assert_eq!(ui.fx.lib().track(id).unwrap().bpm.map(|b| (b * 100.0).round() / 100.0), Some(128.0), "track BPM follows");
    assert_eq!(ui.value("player1.grid.bpm"), "128.00", "the field shows the new tempo");
    // A typo leaves the grid alone.
    retype(&ui, "player1.grid.bpm", "fast");
    assert!((bpm(&ui) - 128.0).abs() < 0.01);
    // TAP four times, half a second apart: about 120 BPM.
    for i in 0..4 {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        ui.click("player1.grid.tap");
    }
    assert!(ui.wait_until(5.0, |u| (bpm(u) - 120.0).abs() < 12.0), "tapped about 120 BPM, got {}", bpm(&ui));
    // Exact taps through the same path (no timing jitter).
    for t in [100.0, 100.5, 101.0, 101.5] {
        crate::player::tap_at(&ui.app, t);
    }
    assert!((bpm(&ui) - 120.0).abs() < 0.01, "exact taps give 120 BPM, got {}", bpm(&ui));
    // The keyboard taps too (B in Prepare).
    ui.app.state.borrow_mut().player.taps.clear();
    press_action(&ui, "player.grid.tap");
    assert_eq!(ui.app.state.borrow().player.taps.len(), 1, "B counts a tap");
    // A locked grid is not changed.
    ui.click("player1.grid.lock");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().beatgrid(id).unwrap().is_some_and(|g| g.locked)));
    crate::player::type_bpm(&ui.app, "90");
    assert!((bpm(&ui) - 120.0).abs() < 0.01, "locked grid unchanged");
}

/// Preferences > Controller: deck defaults and mixer settings are stored,
/// reach the running engine at once and again when audio starts.
#[test]
fn deck_and_mixer_defaults_reach_the_engine() {
    use od_engine::{CrossfaderCurve, JogMode, TempoRange};
    let ui = Ui::open((1440, 900));
    assert!(crate::engine::ensure_engine(&ui.app));
    crate::preferences::apply_engine_settings(&ui.app);
    let w = prefs(&ui, "controller");
    for l in ["prefs.controller.tempo-range", "prefs.controller.quantize", "prefs.controller.sync-mode", "prefs.controller.jog-mode", "prefs.controller.auto-cue", "prefs.controller.eq-mode", "prefs.controller.fader-curve", "prefs.controller.crossfader-curve", "prefs.controller.auto-gain"] {
        prefs_el(&w, l);
    }
    w.invoke_choose("deck.tempo-range".into(), 2);
    w.invoke_choose("deck.jog-mode".into(), 1);
    w.invoke_choose("mixer.eq-mode".into(), 1);
    w.invoke_choose("mixer.fader-curve".into(), 2);
    w.invoke_choose("mixer.crossfader-curve".into(), 2);
    prefs_el(&w, "prefs.controller.auto-cue").mock_single_click(PointerEventButton::Left);
    prefs_el(&w, "prefs.controller.quantize").mock_single_click(PointerEventButton::Left);
    prefs_el(&w, "prefs.controller.auto-gain").mock_single_click(PointerEventButton::Left);
    ui.pump(100);
    let s = stored(&ui);
    assert_eq!((s.deck.tempo_range.as_str(), s.deck.jog_mode.as_str(), s.deck.auto_cue, s.deck.quantize), ("sixteen", "bend", false, false));
    assert_eq!((s.mixer.eq_mode.as_str(), s.mixer.fader_curve.as_str(), s.mixer.crossfader_curve.as_str(), s.mixer.auto_gain), ("classic", "sharp", "cut", false));
    let applied = |u: &Ui| {
        crate::engine::snapshot(&u.app).is_some_and(|e| {
            (0..crate::decks::DECKS).all(|d| e.decks[d].tempo_range == TempoRange::Sixteen && e.decks[d].jog_mode == JogMode::Bend && !e.decks[d].auto_cue)
                && !e.quantize
                && (0..crate::decks::DECKS).all(|c| !e.channels[c].auto_gain)
                && e.crossfader_curve == CrossfaderCurve::Cut
        })
    };
    assert!(ui.wait_until(5.0, applied), "the running engine follows the preferences");
    // Next start: the stored values are applied when audio starts.
    let Ui { fx, app, .. } = ui;
    let _ = app.ui.hide();
    if let Some(w) = app.prefs.borrow().window.as_ref() {
        let _ = w.hide();
    }
    drop(app);
    let ui = Ui::start(fx, (1440, 900), false);
    assert!(crate::engine::ensure_engine(&ui.app));
    crate::preferences::apply_engine_settings(&ui.app);
    assert!(ui.wait_until(5.0, applied), "applied at engine start");
}

/// Preferences > View: text size and line spacing of the track table.
#[test]
fn browser_text_size_and_line_spacing() {
    let ui = Ui::open((1440, 900));
    let row0 = |u: &Ui| Rect::of(&u.get("browser.row0")).h;
    assert!((row0(&ui) - 28.0).abs() < 0.5, "default row 28 px, got {}", row0(&ui));
    let w = prefs(&ui, "view");
    prefs_el(&w, "prefs.view.font-size");
    prefs_el(&w, "prefs.view.line-space");
    w.invoke_choose("view.line-spacing".into(), 2);
    w.invoke_choose("view.font-size".into(), 3);
    ui.pump(200);
    assert_eq!(ui.app.ui.global::<crate::TableStyle>().get_text(), 18.0);
    assert!((row0(&ui) - 34.0).abs() < 0.5, "roomy rows, got {}", row0(&ui));
    w.invoke_choose("view.line-spacing".into(), 0);
    w.invoke_choose("view.font-size".into(), 0);
    ui.pump(200);
    assert!((row0(&ui) - 22.0).abs() < 0.5, "compact rows, got {}", row0(&ui));
    let s = stored(&ui);
    assert_eq!((s.browser.font_size, s.browser.line_spacing.as_str()), (12, "compact"));
}

/// UI scale 0.75–2.0: chosen in Preferences > View and stored; at every
/// scale the Prepare view fits a 2560×1600 display without clipping or
/// overlap.
#[test]
fn ui_scale_without_clipping() {
    let ui = Ui::open((1440, 900));
    let w = prefs(&ui, "view");
    prefs_el(&w, "prefs.view.scale");
    w.invoke_choose("view.scale".into(), 5);
    assert_eq!(stored(&ui).view.scale, 2.0);
    assert!(w.get_scale_pending(), "says it applies after a restart");
    let _ = w.hide();
    ui.app.ui.invoke_row_clicked(0, false, false);
    const LABELS: [&str; 14] = [
        "topbar.mode", "topbar.layout", "topbar.settings", "player1.play", "player1.cue", "player1.title", "player1.bpm", "player1.key", "player1.tab.grid",
        "player1.hot-cue.H", "player1.memory.set", "browser.search", "browser.list", "status.summary",
    ];
    for &scale in crate::settings::SCALES.iter() {
        ui.app.ui.window().dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
        let (lw, lh) = (2560.0 / scale, 1600.0 / scale);
        ui.app.ui.window().set_size(slint::LogicalSize::new(lw, lh));
        ui.pump(400);
        assert!((ui.app.ui.window().scale_factor() - scale).abs() < 1e-3, "scale {scale} in effect");
        let window = Rect { x: 0.0, y: 0.0, w: lw, h: lh };
        let mut rects = Vec::new();
        for l in LABELS {
            let r = Rect::of(&ui.get(l));
            assert!(r.w > 0.5 && r.h > 0.5, "{l} visible at {scale}");
            assert!(window.contains(&r, 1.0), "{l} clipped at scale {scale}: {r:?} in {lw}×{lh}");
            rects.push((l, r));
        }
        for (i, (a, ra)) in rects.iter().enumerate() {
            for (b, rb) in rects.iter().skip(i + 1) {
                let nested = ra.contains(rb, 0.5) || rb.contains(ra, 0.5);
                assert!(nested || ra.overlap(rb) == 0.0, "{a} overlaps {b} at scale {scale}");
            }
        }
    }
}

/// View menu and layout keys: Prepare layouts, panels; the layout persists.
#[test]
fn view_menu_layouts_and_panels() {
    let ui = Ui::open((1440, 900));
    assert!(ui.has("player1.play"));
    ui.app.ui.invoke_menu_action("view.layout.browser".into());
    ui.pump(300);
    assert!(!ui.has("player1.play") && ui.has("browser.list"), "full browser hides the player");
    assert!(!ui.has("browser.upper-splitter"));
    press_action(&ui, "view.layout.player");
    ui.pump(200);
    assert!(ui.has("player1.play"), "the layout key brings the player back");
    // The top-bar switch cycles, also from Perform the keys switch to Prepare.
    ui.click("topbar.layout");
    assert_eq!(ui.app.ui.get_prepare_layout(), "browser");
    ui.app.ui.invoke_set_mode("perform".into());
    press_action(&ui, "view.layout.player");
    assert_eq!(ui.app.ui.get_mode(), "prepare");
    // Panels from the View menu and their keys.
    assert!(!ui.app.ui.get_info_open());
    ui.app.ui.invoke_menu_action("view.panel.info".into());
    assert!(ui.app.ui.get_info_open());
    press_action(&ui, "view.panel.related");
    assert!(ui.app.ui.get_related_open());
    press_action(&ui, "view.panel.my-tag");
    assert!(ui.app.ui.get_my_tag_open());
    // The layout is remembered.
    ui.app.ui.invoke_menu_action("view.layout.browser".into());
    let Ui { fx, app, .. } = ui;
    let _ = app.ui.hide();
    drop(app);
    let ui = Ui::start(fx, (1440, 900), false);
    assert_eq!(ui.app.ui.get_prepare_layout(), "browser");
}

/// Status band: the open list's summary (tracks, time, size).
#[test]
fn status_band_list_summary() {
    let ui = Ui::open((1440, 900));
    ui.app.ui.invoke_select_node(-1);
    ui.pump(200);
    let s = ui.value("status.summary");
    assert!(s.starts_with("3 TRACKS") && s.contains("0:24") && s.ends_with("MB"), "summary `{s}`");
    ui.app.ui.invoke_row_clicked(0, false, false);
    ui.app.ui.invoke_row_clicked(1, true, false);
    ui.pump(100);
    assert!(ui.value("status.summary").ends_with("2 SELECTED"), "selection counted: `{}`", ui.value("status.summary"));
    ui.app.ui.invoke_select_node(ui.fx.playlist.0 as i32);
    ui.pump(200);
    assert!(ui.value("status.summary").starts_with("1 TRACKS"));
}

/// Duplicates: the view lists the groups; KEEP ONE OF EACH removes the
/// extra copies from the collection.
#[test]
fn duplicates_view_and_removal() {
    let fx = Fixture::new();
    {
        let mut lib = fx.lib();
        let mut tags = lib.track(fx.tracks[1].0).unwrap().tags;
        tags.title = fx.tracks[0].1.clone();
        lib.set_tags(fx.tracks[1].0, tags).unwrap();
    }
    let ui = Ui::start(fx, (1440, 900), false);
    ui.app.ui.invoke_menu_action("browser.duplicates".into());
    ui.pump(300);
    assert_eq!(ui.app.ui.get_node_kind(), "duplicates");
    assert_eq!(ui.app.ui.get_duplicate_groups(), 1);
    assert_eq!(ui.app.state.borrow().browser.rows.len(), 2, "both copies listed");
    ui.click("browser.duplicates.keep-first");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().track_count().unwrap() == 2), "one copy removed");
    assert!(ui.fx.lib().track(ui.fx.tracks[0].0).is_ok(), "the oldest copy stays");
    assert!(ui.fx.root.join("music/track2.wav").exists(), "the file stays on disk");
    assert_eq!(ui.app.ui.get_duplicate_groups(), 0);
    // The key opens the view too.
    ui.app.ui.invoke_select_node(-1);
    press_action(&ui, "browser.duplicates");
    assert_eq!(ui.app.ui.get_node_kind(), "duplicates");
}

/// File menu: library XML (rekordbox-compatible) export and import.
#[test]
fn library_xml_export_and_import() {
    let ui = Ui::open((1440, 900));
    let xml = ui.fx.root.join("export/library.xml");
    std::fs::create_dir_all(xml.parent().unwrap()).unwrap();
    let m = crate::import::export_xml(&ui.app, &xml);
    assert!(m.contains('3'), "{m}");
    assert!(std::fs::read_to_string(&xml).unwrap().contains("DJ_PLAYLISTS"));
    let other = Ui::open((1440, 900));
    let before = other.fx.lib().track_count().unwrap();
    let m = crate::import::import_xml(&other.app, &xml);
    assert!(m.contains("3 tracks"), "{m}");
    assert_eq!(other.fx.lib().track_count().unwrap(), before + 3);
}

/// File menu: library archive export (with audio) and restore; consolidate.
#[test]
fn library_archive_and_consolidate() {
    let ui = Ui::open((1440, 900));
    let archive = ui.fx.root.join("backup/library.odlib");
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    crate::import::export_archive(&ui.app, &archive, true);
    assert!(ui.wait_until(30.0, |u| u.app.ui.get_status_text().contains("archive saved")), "archive written: {}", ui.app.ui.get_status_text());
    let restored = ui.fx.root.join("restored");
    let before = crate::settings::GlobalConfig::load();
    let m = crate::import::restore_archive(&ui.app, &archive, &restored);
    assert!(m.contains("restored"), "{m}");
    let lib = od_library::Library::open(&restored.join("library.sqlite")).unwrap();
    assert_eq!(lib.track_count().unwrap(), 3);
    assert!(lib.tracks(od_library::SortKey::DateAdded, false).unwrap().iter().all(|t| t.path.starts_with(&restored)), "tracks point at the restored audio");
    assert_eq!(crate::settings::GlobalConfig::load().library, Some(restored.join("library.sqlite")), "opens at the next start");
    before.save().unwrap();
    // Consolidate: every track copied below one folder.
    let dest = ui.fx.root.join("collected");
    crate::import::consolidate(&ui.app, &dest);
    let inside = |u: &Ui| u.fx.lib().tracks(od_library::SortKey::DateAdded, false).unwrap().iter().all(|t| t.path.starts_with(&dest) && t.path.exists());
    assert!(ui.wait_until(30.0, inside), "tracks moved to {}", dest.display());
    assert!(ui.fx.root.join("music/track1.wav").exists(), "originals stay");
}

/// rekordbox preferences and shortcuts are taken over.
#[test]
fn rekordbox_preferences_and_shortcuts_import() {
    let ui = Ui::open((1440, 900));
    let dir = ui.fx.root.join("rekordbox");
    std::fs::create_dir_all(dir.join("KeyMappings")).unwrap();
    std::fs::write(
        dir.join("rekordbox3.settings"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<PROPERTIES>
  <VALUE name="showPhrase" val="0"/>
  <VALUE name="EqualizerType" val="0"/>
  <VALUE name="AutoCueEnableDeck1" val="0"/>
  <VALUE name="trackAnalysisBpmRange" val="3"/>
  <VALUE name="enableKeyAnalysis" val="0"/>
</PROPERTIES>"#,
    )
    .unwrap();
    std::fs::write(dir.join("PlaySettings.xml"), r#"<PROPERTIES><VALUE name="TempoRange0" val="6"/></PROPERTIES>"#).unwrap();
    std::fs::write(
        dir.join("KeyMappings/rekordbox_0000000000042.mappings"),
        r#"<PROPERTIES>
  <VALUE name="keyMappingName" val="My Keys"/>
  <VALUE name="keyMappingXml">
    <KEYMAPPINGS basedOnDefaults="0">
      <MAPPING commandId="7003" description="Search" key="command + F"/>
      <MAPPING commandId="3006" description="Play" key="X"/>
    </KEYMAPPINGS>
  </VALUE>
</PROPERTIES>"#,
    )
    .unwrap();
    let (rb, _) = od_import_rekordbox::settings::read(Some(&dir)).unwrap();
    let values = rb.preference_values();
    assert!(!values.is_empty());
    let m = crate::import::import_rekordbox_settings(&ui.app, Some(&dir));
    assert!(m.starts_with("Took over"), "{m}");
    let s = stored(&ui);
    let want = |k: &str| values.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
    assert!(!s.view.phrases, "phrase display off");
    assert_eq!(Some(s.mixer.eq_mode.clone()), want("mixer.eq_mode"));
    assert_eq!(Some(s.deck.tempo_range.clone()), want("deck1.tempo_range"));
    assert_eq!(Some(s.deck.auto_cue.to_string()), want("deck1.auto_cue"));
    assert!(!s.analysis.detect_key);
    // The shortcuts that do not collide are in the keymap now.
    let bound = rb.keymap_bindings();
    let changed = bound.keys().filter(|id| !ui.app.state.borrow().keymap.is_default(id)).count();
    assert!(changed >= 1 || bound.is_empty(), "shortcuts applied: {bound:?}");
    assert!(ui.app.state.borrow().keymap.all_conflicts().is_empty(), "no key bound twice");
    // A folder without preferences is reported.
    let empty = ui.fx.root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(crate::import::import_rekordbox_settings(&ui.app, Some(&empty)).starts_with("No rekordbox"));
}

/// Sets: planned start and backup tracks of a position.
#[test]
fn set_planned_start_and_backups() {
    let fx = Fixture::new();
    let set = {
        let mut lib = fx.lib();
        let set = lib.create_node(None, NodeKind::Set, "Friday").unwrap();
        lib.add_to_node(set, &[fx.tracks[0].0, fx.tracks[1].0], None).unwrap();
        set
    };
    let ui = Ui::start(fx, (1440, 900), false);
    ui.app.ui.invoke_select_node(set.0 as i32);
    ui.pump(300);
    retype(&ui, "browser.set.planned-start", "23:30");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().node(set).unwrap().set_meta.and_then(|m| m.planned_start).as_deref() == Some("23:30")));
    // Backups: the tag list becomes the backups of position 1.
    let backup = ui.fx.tracks[2].0;
    ui.app.state.borrow_mut().browser.tag_list = vec![backup];
    ui.app.ui.invoke_flow_select(0);
    ui.pump(200);
    ui.click("browser.set.backups-from-tag-list");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(set).unwrap()[0].backups == vec![backup]), "backup stored");
    assert!(ui.value("browser.set.backups").contains(&ui.fx.tracks[2].1), "shown: `{}`", ui.value("browser.set.backups"));
    ui.click("browser.set.backups-clear");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(set).unwrap()[0].backups.is_empty()));
}

/// Recording: file name pattern, level, the panel (meter, elapsed, size,
/// tracklist) and automatic start / stop on silence.
#[test]
fn recording_panel_settings_and_silence_auto() {
    let fx = Fixture::new();
    {
        let lib = fx.lib();
        let mut s = crate::settings::Settings::from_json(&lib.meta(crate::settings::META_SETTINGS).unwrap().unwrap());
        s.recording.silence_start = true;
        s.recording.silence_stop = true;
        s.recording.name_pattern = "Club {n}".into();
        let mut lib = lib;
        lib.set_meta(crate::settings::META_SETTINGS, &s.to_json()).unwrap();
    }
    let ui = Ui::start(fx, (1440, 900), false);
    crate::recorder::set_silence_times(&ui.app, 0.3, 0.6);
    // Recording level from the panel.
    ui.app.ui.invoke_select_section("recordings".into());
    ui.pump(200);
    for l in ["rec.panel.toggle", "rec.panel.level", "rec.panel.meter", "rec.panel.time", "rec.size"] {
        assert!(ui.has(l), "{l} in the recording panel");
    }
    ui.app.ui.global::<crate::Rec>().invoke_step_gain(1);
    assert_eq!(stored(&ui).recording.gain_db, 3.0);
    assert_eq!(ui.value("rec.panel.level"), "+3 dB");
    // Sound starts the recording.
    ui.app.ui.invoke_select_section("library".into());
    ui.app.ui.invoke_set_mode("perform".into());
    ui.pump(200);
    let title = ui.fx.tracks[0].1.clone();
    let row = ui.by_value("browser.row", &title).unwrap().1;
    ui.double_click_el(&row);
    assert!(ui.wait_until(30.0, |u| super::flows::deck_title(u, 1) == title));
    ui.click("deck1.play");
    assert!(ui.wait_until(15.0, |u| u.app.ui.global::<crate::Rec>().get_recording()), "recording started by sound");
    let g = ui.app.ui.global::<crate::Rec>();
    assert!(ui.wait_until(10.0, |u| !u.app.ui.global::<crate::Rec>().get_size().is_empty() && u.app.ui.global::<crate::Rec>().get_level() > 0.0), "size and level shown");
    assert!(g.get_file().starts_with("Club 1"), "file named by the pattern: {}", g.get_file());
    assert!(ui.wait_until(10.0, |u| slint::Model::row_count(&u.app.ui.global::<crate::Rec>().get_tracks()) >= 1), "the tracklist grows");
    // Silence stops it.
    ui.click("deck1.play");
    assert!(ui.wait_until(15.0, |u| !u.app.ui.global::<crate::Rec>().get_recording()), "silence stops the recording");
    let dir = ui.fx.root.join("recordings");
    assert!(dir.join("Club 1.wav").exists(), "file in {}", dir.display());
    let n: i64 = ui.fx.lib().conn().query_row("SELECT count(*) FROM recording", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    assert!(g.get_armed(), "still armed for the next start");
}
