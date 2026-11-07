//! Behaviour tests of the browser features of `tests/parity/features.toml`:
//! column choice / order / secondary sort, the quick filter, My Tag, the tag
//! list, related tracks, histories, drag and drop into the tree and row /
//! node reordering. Each drives the real window and asserts the library,
//! the settings or the list on screen.

use super::harness::*;
use od_library::NodeKind;
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model};

/// Titles of the list on screen, in order.
fn titles(ui: &Ui) -> Vec<String> {
    ui.app.ui.get_tracks().iter().map(|r| r.title.to_string()).collect()
}

fn row(ui: &Ui, title: &str) -> i_slint_backend_testing::ElementHandle {
    ui.by_value("browser.row", title).unwrap_or_else(|| panic!("row `{title}`")).1
}

fn stored(ui: &Ui) -> crate::settings::Settings {
    crate::settings::Settings::from_json(&ui.fx.lib().meta(crate::settings::META_SETTINGS).unwrap().unwrap())
}

/// Holds ⇧ while running `f` (modifiers come from key events).
fn with_shift(ui: &Ui, f: impl FnOnce()) {
    let shift: slint::SharedString = Key::Shift.into();
    ui.app.ui.window().dispatch_event(WindowEvent::KeyPressed { text: shift.clone() });
    f();
    ui.app.ui.window().dispatch_event(WindowEvent::KeyReleased { text: shift });
}

/// Header right-click opens the column menu; ticking ALBUM shows the
/// column and stores the choice; Default columns resets it.
#[test]
fn column_menu_chooses_columns() {
    let ui = Ui::open((1440, 900));
    assert!(!ui.has("browser.header.album"));
    ui.right_click_el(&ui.get("browser.header.title"));
    assert!(ui.click_menu_item("ALBUM"), "column menu lists ALBUM");
    assert!(ui.wait_until(5.0, |u| u.has("browser.header.album")), "album column shown");
    assert!(stored(&ui).browser.columns.contains(&"album".to_string()), "choice stored in the settings");
    // The columns button opens the same menu: hide GENRE.
    ui.click("browser.header.columns");
    assert!(ui.click_menu_item("GENRE"));
    assert!(ui.wait_until(5.0, |u| !u.has("browser.header.genre")), "genre column hidden");
    ui.click("browser.header.columns");
    assert!(ui.click_menu_item("Default columns"));
    assert!(ui.wait_until(5.0, |u| u.has("browser.header.genre") && !u.has("browser.header.album")));
    assert!(stored(&ui).browser.columns.is_empty(), "defaults stored as the empty list");
}

/// Dragging a header cell onto another moves the column there (stored);
/// ⇧-click adds a secondary sort (stored) and a plain click clears it.
#[test]
fn column_drag_and_secondary_sort() {
    let ui = Ui::open((1440, 900));
    let key = Rect::of(&ui.get("browser.header.key"));
    let artist = Rect::of(&ui.get("browser.header.artist"));
    assert!(key.x > artist.x);
    ui.drag_points(key.centre(), slint::LogicalPosition::new(artist.x + 4.0, artist.centre().y));
    assert!(ui.wait_until(5.0, |u| Rect::of(&u.get("browser.header.key")).x < Rect::of(&u.get("browser.header.artist")).x), "key moved before artist");
    let cols = stored(&ui).browser.columns;
    let pos = |c: &str| cols.iter().position(|x| x == c).unwrap();
    assert!(pos("key") < pos("artist"), "order stored: {cols:?}");
    // Primary: title (click); secondary: rating (⇧-click).
    ui.click("browser.header.title");
    with_shift(&ui, || ui.click("browser.header.rating"));
    let then = stored(&ui).browser.sort_then;
    assert_eq!(then.len(), 1, "secondary sort stored: {then:?}");
    assert_eq!(then[0].column, "rating");
    let ranks: Vec<(String, i32)> = ui.app.ui.get_columns().iter().map(|c| (c.id.to_string(), c.rank)).collect();
    assert!(ranks.contains(&("title".into(), 1)) && ranks.contains(&("rating".into(), 2)), "{ranks:?}");
    ui.click("browser.header.artist");
    assert!(stored(&ui).browser.sort_then.is_empty(), "a plain click sorts by one column again");
}

/// Filter bar: minimum rating and a BPM range narrow the list; CLEAR
/// brings every track back.
#[test]
fn filter_bar_narrows_the_list() {
    let fx = Fixture::new();
    {
        let mut lib = fx.lib();
        let mut tags = lib.track(fx.tracks[1].0).unwrap().tags;
        tags.rating = 4;
        lib.set_tags(fx.tracks[1].0, tags).unwrap();
    }
    let ui = Ui::start(fx, (1440, 900), false);
    assert_eq!(titles(&ui).len(), 3);
    ui.click("browser.header.filter");
    assert!(ui.has("browser.filter.rating3"), "filter bar open");
    ui.click("browser.filter.rating3");
    assert!(ui.wait_until(5.0, |u| titles(u) == [u.fx.tracks[1].1.clone()]), "only the 4-star track: {:?}", titles(&ui));
    ui.click("browser.filter.clear");
    assert_eq!(titles(&ui).len(), 3);
    ui.click("browser.filter.bpm-min");
    ui.type_text("300");
    assert!(ui.wait_until(5.0, |u| titles(u).is_empty()), "no track at 300 BPM or more");
    ui.click("browser.filter.clear");
    assert!(ui.wait_until(5.0, |u| titles(u).len() == 3));
    // The filter button closes the bar again.
    ui.click("browser.header.filter");
    assert!(!ui.has("browser.filter.rating3"));
}

/// My Tag panel: typing "Mood: Peak" tags the selected track (undoable,
/// shown in the MY TAG column and offered by the filter); clicking the value
/// untags it.
#[test]
fn my_tag_panel_tags_tracks() {
    let ui = Ui::open((1440, 900));
    let (id, title) = ui.fx.tracks[0].clone();
    ui.click_el(&row(&ui, &title));
    ui.click("browser.panel.my-tag");
    assert!(ui.wait_until(5.0, |u| u.has("mytag.add")), "My Tag panel open");
    ui.click("mytag.add");
    ui.type_text("Mood: Peak");
    ui.key(Key::Return);
    let peak = od_library::MyTag::new("Mood", "Peak");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().my_tags(id).unwrap() == [peak.clone()]), "tag stored");
    let shown = ui.app.ui.get_tracks().iter().find(|r| r.title == title.as_str()).map(|r| r.my_tag.to_string());
    assert_eq!(shown.as_deref(), Some("Peak"), "MY TAG cell value");
    assert!(ui.app.ui.get_my_tag_options().iter().any(|o| o == "Mood: Peak"), "offered by the filter");
    assert!(ui.app.ui.get_my_tag_chips().iter().any(|c| c.value == "Peak" && c.on));
    ui.click("mytag.value0");
    assert!(ui.wait_until(5.0, |u| u.fx.lib().my_tags(id).unwrap().is_empty()), "untagged");
}

/// Tag list: + collects the selected track, the list shows it, Delete
/// removes it again, SAVE AS PLAYLIST makes a playlist of it.
#[test]
fn tag_list_collects_and_saves() {
    let ui = Ui::open((1440, 900));
    let (a, ta) = ui.fx.tracks[0].clone();
    let (b, tb) = ui.fx.tracks[2].clone();
    ui.click_el(&row(&ui, &ta));
    let add = ui.scroll_into_view("browser.tree", |u| u.find("browser.tree.tag-list.add")).expect("tag list + in the tree");
    ui.click_el(&add);
    ui.right_click_el(&row(&ui, &tb));
    assert!(ui.click_menu_item("Add to tag list"));
    assert_eq!(ui.app.ui.get_tag_list_count(), 2);
    let list = ui.scroll_into_view("browser.tree", |u| u.find("browser.tree.tag-list")).expect("tag list row in the tree");
    ui.click_el(&list);
    assert!(ui.wait_until(5.0, |u| titles(u) == [ta.clone(), tb.clone()]), "tag list in order of adding: {:?}", titles(&ui));
    assert_eq!(ui.app.ui.get_node_kind(), "taglist");
    ui.click_el(&row(&ui, &tb));
    ui.key(Key::Delete);
    assert!(ui.wait_until(5.0, |u| titles(u) == [ta.clone()]), "removed from the tag list");
    ui.click("browser.tag-list.save");
    let lib = ui.fx.lib();
    let saved = lib.all_nodes().unwrap().into_iter().find(|n| n.kind == NodeKind::Playlist && n.name == "Tag list").expect("playlist created");
    assert_eq!(lib.entries(saved.id).unwrap().iter().map(|e| e.track).collect::<Vec<_>>(), [a]);
    let _ = b;
    // The tag list persists in the library.
    assert_eq!(crate::browser::load_tag_list(&lib), [a]);
}

/// Related tracks: the list follows the selected track (no deck loaded)
/// and shows only tracks with a compatible key; the Related panel lists
/// the same suggestions.
#[test]
fn related_tracks_follow_the_reference() {
    let fx = Fixture::new();
    {
        let mut lib = fx.lib();
        for ((id, _), key) in fx.tracks.iter().zip(["8A", "9A", "3B"]) {
            lib.set_key(*id, od_core::MusicalKey::parse(key)).unwrap();
            lib.conn().execute("UPDATE track SET bpm = 120.0 WHERE id = ?1", [id.0]).unwrap();
        }
    }
    let ui = Ui::start(fx, (1440, 900), false);
    let (a, b) = (ui.fx.tracks[0].1.clone(), ui.fx.tracks[1].1.clone());
    ui.click_el(&row(&ui, &a));
    ui.click("browser.panel.related");
    assert!(ui.wait_until(5.0, |u| u.app.ui.get_suggestions().iter().any(|s| s.title == b.as_str())), "panel suggests {b}");
    assert!(ui.app.ui.get_suggestions().iter().all(|s| s.title != ui.fx.tracks[2].1.as_str()), "3B does not fit after 8A");
    ui.click("related.open-list");
    assert_eq!(ui.app.ui.get_node_kind(), "related");
    assert!(ui.wait_until(5.0, |u| titles(u) == [b.clone()]), "Related list: {:?}", titles(&ui));
    assert!(ui.has("browser.related.master") && ui.has("browser.related.deck4"), "deck source selector");
    // The tree row opens the same view.
    ui.app.ui.invoke_select_node(-1);
    ui.pump(100);
    assert_eq!(ui.app.ui.get_node_kind(), "all");
    ui.click("browser.rail.related");
    assert_eq!(ui.app.ui.get_node_kind(), "related");
}

/// Histories: every session is a node; opening it lists the plays in order.
#[test]
fn histories_list_play_sessions() {
    let fx = Fixture::new();
    {
        let mut lib = fx.lib();
        lib.record_play(fx.tracks[2].0, "Session 2026-11-01 22:00", "opendecks").unwrap();
        lib.record_play(fx.tracks[0].0, "Session 2026-11-01 22:00", "opendecks").unwrap();
    }
    let ui = Ui::start(fx, (1440, 900), false);
    assert_eq!(ui.value("browser.tree.history0"), "Session 2026-11-01 22:00");
    ui.click("browser.tree.history0");
    let want = [ui.fx.tracks[2].1.clone(), ui.fx.tracks[0].1.clone()];
    assert!(ui.wait_until(5.0, |u| titles(u) == want), "plays in order: {:?}", titles(&ui));
    assert_eq!(ui.app.ui.get_node_kind(), "history");
}

/// Dragging a row onto a playlist in the tree adds it; inside the playlist
/// dragging a row onto another reorders the entries (one undo step).
#[test]
fn drag_rows_to_tree_and_reorder() {
    let ui = Ui::open((1440, 900));
    let (pl, (b, tb)) = (ui.fx.playlist, ui.fx.tracks[1].clone());
    let a = ui.fx.tracks[0].0;
    let target = Rect::of(&ui.get(&format!("browser.tree.node{}", pl.0))).centre();
    ui.drag_el(&row(&ui, &tb), target);
    assert!(ui.wait_until(5.0, |u| u.fx.lib().entries(pl).unwrap().len() == 2), "dropped track added");
    assert!(!ui.app.ui.global::<crate::DragState>().get_active());
    ui.click(&format!("browser.tree.node{}", pl.0));
    assert!(ui.wait_until(5.0, |u| titles(u).len() == 2));
    assert!(ui.app.ui.get_reorderable(), "playlist in its own order can be reordered");
    let first = Rect::of(&row(&ui, &ui.fx.tracks[0].1));
    ui.drag_el(&row(&ui, &tb), slint::LogicalPosition::new(first.centre().x, first.y + 3.0));
    let order = |u: &Ui| u.fx.lib().entries(pl).unwrap().iter().map(|e| e.track).collect::<Vec<_>>();
    assert!(ui.wait_until(5.0, |u| order(u) == [b, a]), "reordered: {:?}", order(&ui));
    ui.app.undo();
    assert_eq!(order(&ui), [a, b], "undo restores the order");
}

trait FolderFixture {
    fn lib_mut_create_folder(&self) -> od_library::NodeId;
}

impl FolderFixture for Fixture {
    fn lib_mut_create_folder(&self) -> od_library::NodeId {
        let mut lib = self.lib();
        lib.create_node(None, NodeKind::Folder, "Crates").unwrap()
    }
}

/// Dragging a tree node onto a folder moves it into the folder.
#[test]
fn drag_tree_node_into_folder() {
    let fx = Fixture::new();
    let folder = fx.lib_mut_create_folder();
    let ui = Ui::start(fx, (1440, 900), false);
    let pl = ui.fx.playlist;
    let to = Rect::of(&ui.get(&format!("browser.tree.node{}", folder.0))).centre();
    ui.drag_el(&ui.get(&format!("browser.tree.node{}", pl.0)), to);
    assert!(ui.wait_until(5.0, |u| u.fx.lib().node(pl).unwrap().parent == Some(folder)), "playlist moved into the folder");
}
