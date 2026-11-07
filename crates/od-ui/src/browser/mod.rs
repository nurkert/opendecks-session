//! Browser: the tree (collection, lists, sticks), the track list, search,
//! sort, selection, list editing, smart rules and set notes.

mod columns;
mod filter;
pub(crate) mod navigate;
mod sources;
pub(crate) mod sub;
mod tracks;
mod tree;

pub(crate) use columns::sanitize as sanitize_columns;
#[cfg(test)]
pub(crate) use sources::load_tag_list;
pub(crate) use tracks::{
    SortFields, art_px, art_sender, default_descending, key_distance, list_images, poll_artworks, refresh_tracks,
    same_row, sort_key_known, sort_rows,
};
pub(crate) use tree::{refresh_tree, stick_root_id};

/// The column headers (the Explorer shows its own reduced set).
pub(crate) fn refresh_columns(app: &App) {
    columns::refresh(app);
}

use crate::app::{App, handler};
use crate::models::Rows;
use crate::workers::spawn_import;
use crate::{TrackRow, decks, player, smart_text};
use od_core::TrackId;
use od_library::{MyTag, NodeId, NodeKind, TrackRecord};
use slint::{ComponentHandle, Model, SharedString};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) struct BrowserState {
    pub node: Option<NodeId>,
    pub sort: String,
    pub descending: bool,
    pub search: String,
    pub rows: Vec<TrackRecord>,
    /// Selected row indices (of the list on screen).
    pub selection: BTreeSet<usize>,
    pub anchor: Option<usize>,
    pub expanded: HashSet<NodeId>,
    /// Tree filter: only lists whose name contains it (lower case) and the
    /// folders above them.
    pub tree_filter: String,
    /// Selected position in a set's flow.
    pub flow_selected: Option<usize>,
    /// Mounted sticks carrying an OpenDecks library: (name, root).
    pub sticks: Vec<(String, PathBuf)>,
    /// The stick being browsed (read-only source), with its index in `sticks`.
    pub stick: Option<(usize, od_format_native::Stick)>,
    /// Mini overview waveforms for the PREVIEW column, by (stick?, track).
    pub previews: HashMap<(bool, TrackId), slint::Image>,
    /// Cover thumbnails for the ARTWORK column (`None`: the track has none).
    pub artworks: HashMap<(bool, TrackId), Option<slint::Image>>,
    /// A source outside the tree of lists (tag list, related, a history).
    pub special: Option<Special>,
    /// Quick filter and its reference track (master deck).
    pub filter: filter::Filter,
    pub filter_open: bool,
    pub reference: Option<filter::Reference>,
    /// The tag list (holding area), stored in the library meta.
    pub tag_list: Vec<TrackId>,
    /// My Tag values of the source's tracks (column, filter, panel).
    pub my_tags: HashMap<TrackId, Vec<MyTag>>,
    /// Related list: "master" or a deck number, and the track it follows.
    pub related_source: String,
    pub related_track: Option<TrackId>,
    /// History sessions in sidebar order (of the shown year).
    pub histories: Vec<String>,
    /// Year the Histories tree shows (None = the latest).
    pub history_year: Option<String>,
    /// The controller's browse encoder works in the tree (else the list).
    pub focus_tree: bool,
}

/// Sources that are not library nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Special {
    TagList,
    Related,
    History(String),
    /// Duplicate tracks of the collection, group after group.
    Duplicates,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            node: None,
            sort: "date_added".into(),
            descending: true,
            search: String::new(),
            rows: Vec::new(),
            selection: BTreeSet::new(),
            anchor: None,
            expanded: HashSet::new(),
            flow_selected: None,
            sticks: Vec::new(),
            stick: None,
            previews: HashMap::new(),
            artworks: HashMap::new(),
            special: None,
            filter: filter::Filter::default(),
            filter_open: false,
            reference: None,
            tag_list: Vec::new(),
            my_tags: HashMap::new(),
            related_source: "master".into(),
            related_track: None,
            histories: Vec::new(),
            history_year: None,
            tree_filter: String::new(),
            focus_tree: false,
        }
    }
}

impl BrowserState {
    /// Lists with an order of their own sort by position, the rest newest first.
    pub fn reset_sort(&mut self, in_list: bool) {
        self.sort = if in_list {
            "position".into()
        } else {
            "date_added".into()
        };
        self.descending = !in_list;
    }
}

/// Persistent view models of the browser.
#[derive(Default)]
pub(crate) struct BrowserViews {
    pub tracks: Rows<TrackRow>,
    pub art: tracks::ArtLoader,
}

/// Restores the last list and track at start-up.
pub(crate) fn restore(app: &App, node: Option<NodeId>, track: Option<TrackId>) {
    let mut st = app.state.borrow_mut();
    st.browser.tag_list = sources::load_tag_list(&st.lib);
    if let Some(n) = node.filter(|n| st.lib.node(*n).is_ok()) {
        st.browser.node = Some(n);
        let in_list = st.lib.node(n).is_ok_and(|n| n.kind.has_entries());
        st.browser.reset_sort(in_list);
        let mut p = st.lib.node(n).ok().and_then(|n| n.parent);
        while let Some(x) = p {
            st.browser.expanded.insert(x);
            p = st.lib.node(x).ok().and_then(|n| n.parent);
        }
    }
    st.load_rows();
    if let Some(t) = track {
        if let Some(i) = st.browser.rows.iter().position(|r| r.id == t) {
            st.browser.selection = [i].into_iter().collect();
            st.browser.anchor = Some(i);
            st.player.current = Some(t);
            if st.node_kind() == Some(NodeKind::Set) {
                st.browser.flow_selected = Some(i);
            }
        }
    }
}

/// Selects row `i` (plain, ⌘-toggle or ⇧-range) and shows its track.
pub(crate) fn select_row(app: &App, i: usize, toggle: bool, range: bool) {
    {
        let mut st = app.state.borrow_mut();
        let b = &mut st.browser;
        if range {
            let a = b.anchor.unwrap_or(i);
            b.selection = (a.min(i)..=a.max(i)).collect();
        } else if toggle {
            if !b.selection.remove(&i) {
                b.selection.insert(i);
            }
            b.anchor = Some(i);
        } else {
            b.selection = [i].into_iter().collect();
            b.anchor = Some(i);
        }
        let new = if st.explorer.open.is_some() {
            st.explorer.track_at(i)
        } else {
            st.browser.rows.get(i).map(|t| t.id)
        };
        if new != st.player.current {
            st.player.current = new;
            st.player.cursor = 0.0;
        }
        if st.node_kind() == Some(NodeKind::Set) && st.explorer.open.is_none() {
            st.browser.flow_selected = Some(i);
        }
    }
    refresh_tracks(app);
    player::refresh_detail(app);
}

/// Selects every row of the list.
pub(crate) fn select_all(app: &App) {
    {
        let mut st = app.state.borrow_mut();
        let n = st.row_count();
        st.browser.selection = (0..n).collect();
    }
    refresh_tracks(app);
}

/// Arrow keys: move the selection one row.
pub(crate) fn step_selection(app: &App, up: bool) {
    let (len, cur) = {
        let st = app.state.borrow();
        (
            st.row_count(),
            st.browser.selection.iter().next_back().copied(),
        )
    };
    if len == 0 {
        return;
    }
    let next = match cur {
        None => 0,
        Some(c) if up => c.saturating_sub(1),
        Some(c) => (c + 1).min(len - 1),
    };
    select_row(app, next, false, false);
    app.ui.set_focus_row(next as i32);
}

/// Selects the next (or previous) list in the tree; used by the
/// controller's back button in kiosk mode.
pub(crate) fn cycle_list(app: &App, forward: bool) {
    let ids: Vec<i32> = app.ui.get_tree().iter().map(|r| r.id).collect();
    if ids.is_empty() {
        return;
    }
    let cur = app.ui.get_selected_node();
    let i = ids.iter().position(|&x| x == cur).unwrap_or(0);
    let next = if forward {
        (i + 1) % ids.len()
    } else {
        (i + ids.len() - 1) % ids.len()
    };
    app.ui.invoke_select_node(ids[next]);
}

/// Removes the selected rows from the open playlist or set.
pub(crate) fn remove_selected(app: &App) {
    {
        let st = app.state.borrow();
        if st.read_only() || st.explorer.open.is_some() {
            return;
        }
    }
    let (node, positions) = {
        let st = app.state.borrow();
        (
            st.browser.node,
            st.browser.selection.iter().copied().collect::<Vec<_>>(),
        )
    };
    if app.state.borrow().browser.special == Some(Special::TagList) {
        sources::remove_from_tag_list(app, &positions);
        return;
    }
    let Some(n) = node else { return };
    if positions.is_empty()
        || !app
            .state
            .borrow()
            .lib
            .node(n)
            .is_ok_and(|n| n.kind.has_entries())
    {
        return;
    }
    // Rows may be re-sorted; map rows back to entry positions.
    let entries = app.state.borrow().lib.entries(n).unwrap_or_default();
    let ids: Vec<TrackId> = app.state.borrow().selected_ids();
    let mut entry_positions: Vec<usize> = Vec::new();
    for id in &ids {
        if let Some(i) =
            (0..entries.len()).find(|i| entries[*i].track == *id && !entry_positions.contains(i))
        {
            entry_positions.push(i);
        }
    }
    let r = app
        .state
        .borrow_mut()
        .lib
        .remove_from_node(n, &entry_positions);
    if app.report(r).is_some() {
        app.state.borrow_mut().browser.selection.clear();
        app.after_edit(&format!(
            "Removed {} track(s) — ⌘Z to undo",
            entry_positions.len()
        ));
    }
}

/// Opens the duplicate tracks of the collection in the list.
pub(crate) fn open_duplicates(app: &App) {
    sources::open(app, sources::DUPLICATES);
}

/// Planned start time of the open set ("23:30"; empty clears it).
pub(crate) fn set_planned_start(app: &App, text: &str) {
    if app.guard_read_only() {
        return;
    }
    let Some(n) = app.state.borrow().browser.node else {
        return;
    };
    let Ok(node) = app.state.borrow().lib.node(n) else {
        return;
    };
    if node.kind != NodeKind::Set {
        return;
    }
    let mut meta = node.set_meta.unwrap_or_default();
    let t = text.trim();
    meta.planned_start = (!t.is_empty()).then(|| t.to_string());
    let r = app.state.borrow_mut().lib.set_set_meta(n, meta);
    if app.report(r).is_some() {
        app.after_edit("Planned start saved");
    }
}

/// Backup tracks of the selected set position: "from-tag-list" takes the
/// tracks of the tag list, "clear" removes them.
pub(crate) fn set_backups(app: &App, action: &str) {
    if app.guard_read_only() {
        return;
    }
    let (node, pos, tag_list) = {
        let st = app.state.borrow();
        (
            st.browser.node,
            st.browser.flow_selected,
            st.browser.tag_list.clone(),
        )
    };
    let (Some(n), Some(p)) = (node, pos) else {
        app.status("Select a position of the set first");
        return;
    };
    let backups = match action {
        "from-tag-list" => {
            if tag_list.is_empty() {
                app.status("The tag list is empty — collect the backup tracks there first");
                return;
            }
            tag_list
        }
        _ => Vec::new(),
    };
    let count = backups.len();
    let r = app.state.borrow_mut().lib.set_entry_backups(n, p, backups);
    if app.report(r).is_some() {
        app.after_edit(&if count == 0 {
            "Backup tracks removed".to_string()
        } else {
            format!("{count} backup track(s) set")
        });
    }
}

/// Opens a tree entry: a library list, "All tracks", a stick or a list on it.
fn select_node(app: &App, id: i32) {
    if sources::decode(id, &app.state.borrow().browser.histories).is_some() {
        sources::open(app, id);
        return;
    }
    {
        let mut st = app.state.borrow_mut();
        st.explorer.close();
        st.browser.special = None;
        match tree::decode_stick_id(id) {
            Some((i, node)) => {
                let reuse = st.browser.stick.as_ref().is_some_and(|(j, _)| *j == i);
                if !reuse {
                    let root = st.browser.sticks.get(i).map(|s| s.1.clone());
                    st.browser.stick = root
                        .and_then(|r| od_format_native::Stick::open(&r).ok().flatten())
                        .map(|s| (i, s));
                }
                st.browser.node = node;
                st.player.current = None;
            }
            None => {
                if st.browser.stick.take().is_some() {
                    st.player.current = None;
                }
                st.browser.node = if id < 0 {
                    None
                } else {
                    Some(NodeId(id as i64))
                };
            }
        }
        st.browser.selection.clear();
        st.browser.flow_selected = None;
        st.browser.search.clear();
        let in_list = st
            .browser
            .node
            .is_some_and(|n| st.src().node(n).is_ok_and(|n| n.kind.has_entries()));
        st.browser.reset_sort(in_list);
        if let (Some(n), false) = (st.browser.node, st.read_only()) {
            if st.lib.node(n).is_ok_and(|n| n.kind == NodeKind::Folder) {
                st.browser.expanded.insert(n);
            }
        }
    }
    app.ui.set_search_text(SharedString::new());
    app.ui.set_section("library".into());
    app.state.borrow_mut().load_rows();
    refresh_tree(app);
    refresh_tracks(app);
    crate::explorer::refresh_tree(app);
}

fn create_node(app: &App, kind: &str) {
    app.state.borrow_mut().browser.special = None;
    if app.state.borrow().read_only() || app.state.borrow().explorer.open.is_some() {
        let mut st = app.state.borrow_mut();
        st.explorer.close();
        st.browser.stick = None;
        st.browser.node = None;
        st.browser.selection.clear();
    }
    let (kind, name) = match kind {
        "folder" => (NodeKind::Folder, "New folder"),
        "set" => (NodeKind::Set, "New set"),
        "smart" => (NodeKind::Smart, "New smart list"),
        _ => (NodeKind::Playlist, "New playlist"),
    };
    let parent = {
        let st = app.state.borrow();
        st.browser
            .node
            .and_then(|n| st.lib.node(n).ok())
            .filter(|n| n.kind == NodeKind::Folder)
            .map(|n| n.id)
    };
    let selected = app.state.borrow().selected_ids();
    let r = app.state.borrow_mut().lib.create_node(parent, kind, name);
    if let Some(id) = app.report(r) {
        if kind.has_entries() && !selected.is_empty() && app.state.borrow().browser.node.is_none() {
            let _ = app.state.borrow_mut().lib.add_to_node(id, &selected, None);
        }
        {
            let mut st = app.state.borrow_mut();
            if let Some(p) = parent {
                st.browser.expanded.insert(p);
            }
            st.browser.node = Some(id);
            st.browser.selection.clear();
            st.browser.sort = if kind.has_entries() {
                "position".into()
            } else {
                "date_added".into()
            };
        }
        app.after_edit("Created — click the title to rename it");
        crate::explorer::refresh_tree(app);
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.on_select_node(handler!(app, |a, id: i32| select_node(&a, id)));
    ui.on_toggle_node(handler!(app, |a, id: i32| {
        {
            let mut st = a.state.borrow_mut();
            let n = NodeId(id as i64);
            if !st.browser.expanded.remove(&n) {
                st.browser.expanded.insert(n);
            }
        }
        refresh_tree(&a);
    }));
    ui.on_add_to_node(handler!(app, |a, id: i32| {
        if a.guard_read_only() {
            return;
        }
        let ids = a.state.borrow().selected_ids();
        let name = a
            .state
            .borrow()
            .lib
            .node(NodeId(id as i64))
            .map(|n| n.name)
            .unwrap_or_default();
        let r = a
            .state
            .borrow_mut()
            .lib
            .add_to_node(NodeId(id as i64), &ids, None);
        if a.report(r).is_some() {
            remember_recent_list(&a, NodeId(id as i64));
            a.after_edit(&format!("Added {} track(s) to {name}", ids.len()));
        }
    }));
    ui.on_create_node(handler!(app, |a, kind: SharedString| create_node(
        &a,
        kind.as_str()
    )));
    ui.on_rename_node(handler!(app, |a, name: SharedString| {
        if a.guard_read_only() {
            return;
        }
        let node = a.state.borrow().browser.node;
        if let Some(n) = node.filter(|_| a.state.borrow().explorer.open.is_none()) {
            let r = a.state.borrow_mut().lib.rename_node(n, name.trim());
            if a.report(r).is_some() {
                a.after_edit("Renamed");
            }
        }
    }));
    ui.on_delete_node(handler!(app, |a| {
        if a.guard_read_only() {
            return;
        }
        let node = a.state.borrow().browser.node;
        if let Some(n) = node.filter(|_| a.state.borrow().explorer.open.is_none()) {
            let r = a.state.borrow_mut().lib.delete_node(n);
            if a.report(r).is_some() {
                a.state.borrow_mut().browser.node = None;
                a.after_edit("Deleted — ⌘Z brings it back");
            }
        }
    }));
    ui.on_remove_selected(handler!(app, |a| remove_selected(&a)));
    ui.on_search(handler!(app, |a, q: SharedString| {
        a.state.borrow_mut().browser.search = q.to_string();
        a.state.borrow_mut().load_rows();
        refresh_tracks(&a);
    }));
    ui.on_set_smart_rule(handler!(app, |a, text: SharedString| {
        if a.guard_read_only() {
            return;
        }
        match smart_text::parse(&text) {
            Ok(rule) => {
                a.ui.set_smart_rule_error(SharedString::new());
                let node = a.state.borrow().browser.node;
                if let Some(n) = node {
                    let r = a.state.borrow_mut().lib.set_smart_rule(n, rule);
                    if a.report(r).is_some() {
                        a.after_edit("Rules updated");
                    }
                }
            }
            Err(e) => a.ui.set_smart_rule_error(e.into()),
        }
    }));
    ui.on_sort(handler!(app, |a, col: SharedString| {
        {
            let mut st = a.state.borrow_mut();
            let b = &mut st.browser;
            if b.sort == col.as_str() {
                b.descending = !b.descending;
            } else {
                b.sort = col.to_string();
                b.descending = default_descending(col.as_str());
            }
            if b.sort == "position" {
                b.descending = false;
            }
        }
        if !a.state.borrow().settings.browser.sort_then.is_empty() {
            a.state
                .borrow_mut()
                .change_settings(|s| s.browser.sort_then.clear());
        }
        a.state.borrow_mut().load_rows();
        refresh_tracks(&a);
    }));
    wire_columns(app);
    sources::wire(app);
    sub::wire(app);
    ui.on_row_clicked(handler!(app, |a, i: i32, toggle: bool, range: bool| {
        select_row(&a, i as usize, toggle, range)
    }));
    ui.on_toggle_full_screen(handler!(app, |a| {
        let w = a.ui.window();
        w.set_fullscreen(!w.is_fullscreen());
    }));
    ui.on_tree_filter(handler!(app, |a, t: SharedString| {
        a.state.borrow_mut().browser.tree_filter = t.trim().to_lowercase();
        refresh_tree(&a);
    }));
    ui.on_history_year_step(handler!(app, |a, d: i32| {
        let years = sources::history_years(&a.state.borrow().lib);
        let now = sources::history_year(&years, a.state.borrow().browser.history_year.as_deref());
        let i = now
            .and_then(|y| years.iter().position(|x| *x == y))
            .unwrap_or(0) as i32;
        let next = years
            .get((i + d.signum()).clamp(0, years.len().max(1) as i32 - 1) as usize)
            .cloned();
        a.state.borrow_mut().browser.history_year = next;
        refresh_tree(&a);
    }));
    ui.on_cycle_density(handler!(app, |a| crate::preferences::cycle_line_spacing(
        &a
    )));
    ui.on_row_activated(handler!(app, |a, i: i32| {
        select_row(&a, i as usize, false, false);
        // A fixed load target (the list title row) in Perform.
        let target = a.ui.get_load_target();
        if target > 0 && a.ui.get_mode() == "perform" {
            decks::load_selected(&a, (target as usize - 1).min(decks::DECKS - 1));
        } else if a.state.borrow().explorer.open.is_some() {
            crate::explorer::activate(&a, i as usize, None);
        } else {
            decks::load_selected_into_free_deck(&a);
        }
    }));
    ui.on_info_edit(handler!(
        app,
        |a, field: SharedString, value: SharedString| {
            let (field, value) = (field.to_string(), value.trim().to_string());
            edit_tags(&a, |t| set_tag_field(t, &field, &value), "Tags saved");
        }
    ));
    ui.on_row_rated(handler!(app, |a, i: i32, n: i32| {
        select_row(&a, i as usize, false, false);
        edit_tags(&a, |t| t.rating = menu_rating(n as i64), "Rating set");
    }));
    ui.on_row_menu(handler!(app, |a, i: i32, action: SharedString| row_menu(
        &a,
        i as usize,
        action.as_str()
    )));
    // Drag and drop: target 1..=4 = that deck, -1 = the Prepare player.
    ui.on_row_dropped(handler!(app, |a, i: i32, target: i32| {
        match drop_target(target) {
            DropTarget::Row(to) => return reorder_rows(&a, i as usize, to),
            DropTarget::Tree(id) => return drop_on_tree(&a, i as usize, id),
            DropTarget::Deck => {}
        }
        // Onto the second Prepare player.
        if target == -2 {
            let track = {
                let st = a.state.borrow();
                if st.explorer.open.is_some() {
                    st.explorer.track_at(i as usize)
                } else {
                    st.browser.rows.get(i as usize).map(|t| t.id)
                }
            };
            match track {
                Some(t) => crate::player::load_second(&a, t),
                None => a.status("Add the file to the collection first"),
            }
            return;
        }
        select_row(&a, i as usize, false, false);
        if target < 0 {
            if a.state.borrow().explorer.open.is_some() {
                crate::explorer::activate(&a, i as usize, None);
            } else {
                crate::player::toggle_preview(&a);
            }
            return;
        }
        let deck = (target as usize).saturating_sub(1).min(decks::DECKS - 1);
        if a.state.borrow().explorer.open.is_some() {
            crate::explorer::activate(&a, i as usize, Some(deck));
        } else {
            let track = a.state.borrow().player.current;
            if let Some(track) = track {
                decks::load_track(&a, deck, track, None);
            }
        }
    }));
    ui.on_import_folder(handler!(app, |a| {
        let Some(dirs) = rfd::FileDialog::new()
            .set_title("Add music folder")
            .pick_folders()
        else {
            return;
        };
        let path = a.state.borrow().config.library_path.clone();
        spawn_import(path, dirs, a.worker_tx.clone(), a.analysis.wake.clone());
        a.status("Adding music…");
    }));
    ui.on_import_rekordbox(handler!(app, |a| crate::import::run_rekordbox_import(&a)));
    ui.on_flow_select(handler!(app, |a, i: i32| select_row(
        &a, i as usize, false, false
    )));
    ui.on_set_transition_note(handler!(app, |a, text: SharedString| {
        if a.guard_read_only() {
            return;
        }
        let (node, pos) = {
            let st = a.state.borrow();
            (st.browser.node, st.browser.flow_selected)
        };
        if let (Some(n), Some(p)) = (node, pos) {
            let r = a.state.borrow_mut().lib.set_entry_note(n, p, text.trim());
            if a.report(r).is_some() {
                a.after_edit("Transition note saved");
            }
        }
    }));
    ui.on_duplicates_action(handler!(app, |a, action: SharedString| {
        sources::duplicates_action(&a, action.as_str())
    }));
    ui.on_save_planned_start(handler!(app, |a, text: SharedString| set_planned_start(
        &a,
        text.as_str()
    )));
    ui.on_backups_action(handler!(app, |a, action: SharedString| set_backups(
        &a,
        action.as_str()
    )));
    ui.on_find_moved_files(handler!(app, |a| {
        let Some(dirs) = rfd::FileDialog::new()
            .set_title("Where are the moved files?")
            .pick_folders()
        else {
            return;
        };
        let found = a
            .state
            .borrow()
            .lib
            .find_relocations(&dirs, od_decode::content_hash);
        match found {
            Ok(rel) if rel.is_empty() => a.status("No moved files found in that folder"),
            Ok(rel) => {
                let n = rel.len();
                let r = a.state.borrow_mut().lib.apply_relocations(&rel);
                if a.report(r).is_some() {
                    a.after_edit(&format!("Found {n} moved file(s)"));
                }
            }
            Err(e) => a.status(format!("{e}")),
        }
    }));
}

/// Where a dragged row was dropped.
#[derive(Debug, PartialEq)]
pub(crate) enum DropTarget {
    /// A deck (1..=4) or the Prepare player (-1): handled by the caller.
    Deck,
    /// Before row `n` of the open list.
    Row(usize),
    /// A tree row (library node id or a special source id).
    Tree(i32),
}

/// `DragState.target` → target (ranges of `DragTargets` in drag.slint).
pub(crate) fn drop_target(t: i32) -> DropTarget {
    const ROW: i32 = 100_000;
    const TREE: i32 = 200_001_000;
    if t >= TREE - 1000 {
        DropTarget::Tree(t - TREE)
    } else if t >= ROW {
        DropTarget::Row((t - ROW) as usize)
    } else {
        DropTarget::Deck
    }
}

/// Whether the open list can be reordered by dragging rows: a playlist or
/// set of the library, in its own order, not searched or filtered.
pub(crate) fn reorderable(st: &crate::state::AppState) -> bool {
    st.explorer.open.is_none()
        && !st.read_only()
        && st.browser.special.is_none()
        && st.browser.sort == "position"
        && !st.browser.descending
        && st.browser.search.trim().is_empty()
        && !st.browser.filter.active()
        && st
            .node_kind()
            .is_some_and(|k| matches!(k, NodeKind::Playlist | NodeKind::Set))
}

/// Dragged row `from` (with the selection it belongs to) dropped before row `to`.
fn reorder_rows(app: &App, from: usize, to: usize) {
    if !reorderable(&app.state.borrow()) {
        app.status("Sort the list by # to reorder it");
        return;
    }
    let (node, mut positions) = {
        let st = app.state.borrow();
        let sel: Vec<usize> = st.browser.selection.iter().copied().collect();
        (
            st.browser.node,
            if sel.contains(&from) { sel } else { vec![from] },
        )
    };
    let Some(n) = node else { return };
    positions.sort_unstable();
    if positions.len() == 1 && (to == positions[0] || to == positions[0] + 1) {
        return;
    }
    let count = positions.len();
    let r = app.state.borrow_mut().lib.move_entries(n, &positions, to);
    if app.report(r).is_some() {
        // Keep the moved rows selected at their new place.
        let start = to - positions.iter().filter(|&&p| p < to).count();
        {
            let mut st = app.state.borrow_mut();
            st.browser.selection = (start..start + count).collect();
            st.browser.anchor = Some(start);
        }
        app.after_edit("Reordered — ⌘Z to undo");
    }
}

/// Rows dropped on a tree row: added to that playlist / set or the tag list.
fn drop_on_tree(app: &App, row: usize, id: i32) {
    if !app.state.borrow().browser.selection.contains(&row) {
        select_row(app, row, false, false);
    }
    if id == sources::TAG_LIST {
        sources::add_selected_to_tag_list(app);
        return;
    }
    if id < 0 || app.guard_read_only() {
        return;
    }
    let node = NodeId(id as i64);
    let ids = app.state.borrow().selected_ids();
    let name = app
        .state
        .borrow()
        .lib
        .node(node)
        .map(|n| n.name)
        .unwrap_or_default();
    let r = app.state.borrow_mut().lib.add_to_node(node, &ids, None);
    if app.report(r).is_some() {
        remember_recent_list(app, node);
        app.after_edit(&format!("Added {} track(s) to {name}", ids.len()));
    }
}

/// Where a tree node dropped on `target` goes: into a folder (at its end),
/// else just before the target among the target's siblings.
pub(crate) fn node_destination(
    target: &od_library::Node,
    siblings: &[od_library::Node],
    moving: NodeId,
) -> (Option<NodeId>, usize) {
    if target.kind == NodeKind::Folder {
        return (Some(target.id), usize::MAX);
    }
    let index = siblings
        .iter()
        .filter(|n| n.id != moving)
        .position(|n| n.id == target.id)
        .unwrap_or(usize::MAX);
    (target.parent, index)
}

fn node_dropped(app: &App, source: i32, target: i32) {
    let DropTarget::Tree(id) = drop_target(target) else {
        return;
    };
    if source < 0 || id < 0 || source == id || app.state.borrow().read_only() {
        return;
    }
    let (moving, target) = (NodeId(source as i64), NodeId(id as i64));
    let dest = {
        let st = app.state.borrow();
        let Ok(t) = st.lib.node(target) else { return };
        let siblings = st.lib.children(t.parent).unwrap_or_default();
        node_destination(&t, &siblings, moving)
    };
    let r = app.state.borrow_mut().lib.move_node(moving, dest.0, dest.1);
    if app.report(r).is_some() {
        if let Some(p) = dest.0 {
            app.state.borrow_mut().browser.expanded.insert(p);
        }
        app.after_edit("Moved — ⌘Z to undo");
    }
}

/// Column choice, order, secondary sort, the quick filter and tree drags.
fn wire_columns(app: &Rc<App>) {
    let ui = &app.ui;
    let columns_changed = |a: &App, f: &dyn Fn(&[String]) -> Vec<String>| {
        let next = f(&a.state.borrow().settings.browser.columns);
        a.state
            .borrow_mut()
            .change_settings(|s| s.browser.columns = next.clone());
        refresh_tracks(a);
    };
    ui.on_toggle_column(handler!(app, |a, id: SharedString| columns_changed(
        &a,
        &|c| columns::toggle(c, &id)
    )));
    ui.on_move_column(handler!(app, |a, id: SharedString, to: i32| {
        columns_changed(&a, &|c| columns::move_to(c, &id, to.max(0) as usize))
    }));
    ui.on_reset_columns(handler!(app, |a| {
        a.state.borrow_mut().change_settings(|s| s.browser.widths.clear());
        columns_changed(&a, &|_| Vec::new())
    }));
    // Drag on a header cell's right edge: the new width (0 = back to the
    // default, double-click).
    ui.on_resize_column(handler!(app, |a, id: SharedString, w: f32| {
        a.state.borrow_mut().change_settings(|s| {
            if w > 0.0 {
                s.browser.widths.insert(id.to_string(), w.clamp(24.0, 900.0).round());
            } else {
                s.browser.widths.remove(id.as_str());
            }
        });
        refresh_tracks(&a);
    }));
    ui.on_sort_then(handler!(app, |a, col: SharedString| {
        let primary = a.state.borrow().browser.sort.clone();
        let next = columns::shift_sort(
            &a.state.borrow().settings.browser.sort_then,
            &primary,
            &col,
            default_descending(&col),
        );
        a.state
            .borrow_mut()
            .change_settings(|s| s.browser.sort_then = next.clone());
        a.state.borrow_mut().load_rows();
        refresh_tracks(&a);
    }));
    ui.on_toggle_filter(handler!(app, |a| {
        let open = !a.state.borrow().browser.filter_open;
        a.state.borrow_mut().browser.filter_open = open;
        a.ui.set_filter_open(open);
        refresh_filter(&a);
    }));
    ui.on_filter_edit(handler!(
        app,
        |a, field: SharedString, value: SharedString| {
            a.state.borrow_mut().browser.filter.edit(&field, &value);
            refresh_filter(&a);
        }
    ));
    ui.on_node_dropped(handler!(app, |a, source: i32, target: i32| node_dropped(
        &a, source, target
    )));
}

/// Re-resolves the filter reference and reloads the list.
pub(crate) fn refresh_filter(app: &App) {
    let reference = if app.state.borrow().browser.filter.uses_reference() {
        sources::resolve_reference(app)
    } else {
        None
    };
    app.state.borrow_mut().browser.reference = reference;
    app.state.borrow_mut().load_rows();
    refresh_tracks(app);
}

/// Applies `f` to the tags of the selected tracks (one undo step per track).
fn edit_tags(app: &App, f: impl Fn(&mut od_core::TrackTags), done: &str) {
    if app.guard_read_only() {
        return;
    }
    let ids = app.state.borrow().selected_ids();
    for id in &ids {
        let tags = app.state.borrow().lib.track(*id).map(|t| t.tags);
        let Ok(mut tags) = tags else { continue };
        f(&mut tags);
        let r = app.state.borrow_mut().lib.set_tags(*id, tags);
        if app.report(r).is_none() {
            return;
        }
    }
    app.after_edit(done);
}

/// Track row context menu.
fn row_menu(app: &App, i: usize, action: &str) {
    // Right-click acts on the clicked row, keeping a selection it belongs to.
    if !app.state.borrow().browser.selection.contains(&i) {
        select_row(app, i, false, false);
    }
    let (what, arg) = action.split_once(':').unwrap_or((action, ""));
    let n: i64 = arg.parse().unwrap_or(0);
    match what {
        "deck" => {
            let deck = (n.clamp(1, decks::DECKS as i64) - 1) as usize;
            if app.state.borrow().explorer.open.is_some() {
                crate::explorer::activate(app, i, Some(deck));
            } else {
                let track = app.state.borrow().player.current;
                if let Some(track) = track {
                    decks::load_track(app, deck, track, None);
                }
            }
        }
        "playlist" => {
            if app.guard_read_only() {
                return;
            }
            let ids = app.state.borrow().selected_ids();
            let node = od_library::NodeId(n);
            let name = app
                .state
                .borrow()
                .lib
                .node(node)
                .map(|n| n.name)
                .unwrap_or_default();
            let r = app.state.borrow_mut().lib.add_to_node(node, &ids, None);
            if app.report(r).is_some() {
                remember_recent_list(app, node);
                app.after_edit(&format!("Added {} track(s) to {name}", ids.len()));
            }
        }
        "color" => {
            let color = menu_color(n);
            edit_tags(app, |t| t.color = color, "Colour set");
        }
        "rating" => edit_tags(app, |t| t.rating = menu_rating(n), "Rating set"),
        "analyse" => {
            if app.guard_read_only() {
                return;
            }
            let ids = app.state.borrow().selected_ids();
            for id in &ids {
                let _ = app.state.borrow().lib.conn().execute(
                    "UPDATE track SET analysis_version = 0 WHERE id = ?1",
                    [id.0],
                );
                app.analysis
                    .control
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .requested
                    .push(*id);
            }
            let _ = app.analysis.wake.send(());
            app.status("Queued for analysis");
        }
        "reveal" => {
            let path = {
                let st = app.state.borrow();
                st.player
                    .current
                    .and_then(|id| st.src().track(id).ok())
                    .map(|t| st.resolve(&t.path))
            };
            if let Some(p) = path {
                crate::preferences::reveal(&p);
            }
        }
        "remove" => remove_selected(app),
        "import" => crate::explorer::import_selection(app),
        "analysis-lock" => {
            if app.guard_read_only() {
                return;
            }
            let ids = app.state.borrow().selected_ids();
            let grids: Vec<_> = ids
                .iter()
                .filter_map(|id| {
                    app.state
                        .borrow()
                        .lib
                        .beatgrid(*id)
                        .ok()
                        .flatten()
                        .map(|g| (*id, g))
                })
                .collect();
            if grids.is_empty() {
                app.status("Analyse the track first: there is no beatgrid to lock");
                return;
            }
            // One state for the whole selection: lock unless all are locked.
            let lock = !grids.iter().all(|(_, g)| g.locked);
            for (id, mut g) in grids {
                g.locked = lock;
                let r = app.state.borrow_mut().lib.set_beatgrid(id, g);
                if app.report(r).is_none() {
                    return;
                }
            }
            app.after_edit(if lock {
                "Analysis locked"
            } else {
                "Analysis unlocked"
            });
        }
        "reload-tags" => {
            let paths: Vec<(TrackId, std::path::PathBuf)> = {
                let st = app.state.borrow();
                st.selected_ids()
                    .into_iter()
                    .filter_map(|id| st.lib.track(id).ok().map(|t| (id, st.resolve(&t.path))))
                    .collect()
            };
            if app.guard_read_only() {
                return;
            }
            for (id, path) in paths {
                let Ok(info) = od_decode::probe(&path) else {
                    continue;
                };
                let r = app
                    .state
                    .borrow()
                    .lib
                    .track(id)
                    .map(|t| reload_tags(t.tags, info.tags));
                let Ok(tags) = r else { continue };
                let r = app.state.borrow_mut().lib.set_tags(id, tags);
                if app.report(r).is_none() {
                    return;
                }
            }
            app.after_edit("Tags reloaded from the files");
        }
        "export" => {
            let paths: Vec<std::path::PathBuf> = {
                let st = app.state.borrow();
                st.selected_ids()
                    .into_iter()
                    .filter_map(|id| st.src().track(id).ok().map(|t| st.resolve(&t.path)))
                    .collect()
            };
            if paths.is_empty() {
                return;
            }
            let Some(dir) = rfd::FileDialog::new()
                .set_title(od_i18n::tr("Export the files to a folder"))
                .pick_folder()
            else {
                return;
            };
            let copied = paths
                .iter()
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|f| std::fs::copy(p, dir.join(f)).is_ok())
                })
                .count();
            app.status(format!("Exported {copied} file(s) to {}", dir.display()));
        }
        "reset-plays" => {
            if app.guard_read_only() {
                return;
            }
            let ids = app.state.borrow().selected_ids();
            for id in &ids {
                let _ = app.state.borrow().lib.conn().execute(
                    "UPDATE track SET play_count = 0, last_played = NULL WHERE id = ?1",
                    [id.0],
                );
            }
            app.state.borrow_mut().load_rows();
            refresh_tracks(app);
            app.status("Play count reset");
        }
        "memory-to-hot" => edit_selected_cues(app, "Memory cues made hot cues", memory_to_hot),
        "sort-hot" => edit_selected_cues(app, "Hot cues sorted by time", sort_hot_cues),
        "info" => {
            app.ui.set_info_open(true);
            refresh_info(app);
        }
        "taglist" => sources::add_selected_to_tag_list(app),
        "stems" => crate::stems_batch::separate_selected(app),
        "related" => {
            let track = app.state.borrow().player.current;
            sources::open(app, sources::RELATED);
            let mut st = app.state.borrow_mut();
            st.browser.related_track = track;
            st.browser.related_source = "track".into();
            drop(st);
            app.refresh_library();
        }
        _ => {}
    }
}

/// File tags read again: the text fields come from the file, rating and
/// colour (library data) stay.
pub(crate) fn reload_tags(old: od_core::TrackTags, file: od_core::TrackTags) -> od_core::TrackTags {
    od_core::TrackTags {
        rating: old.rating,
        color: old.color,
        ..file
    }
}

/// Memory cues become hot cues on the free pads, earliest first (cues
/// beyond the free pads stay memory cues).
pub(crate) fn memory_to_hot(cues: &mut [od_core::Cue]) -> bool {
    use od_core::CueKind;
    let mut free: Vec<u8> = (0..od_core::cue::HOT_CUE_SLOTS)
        .filter(|s| !cues.iter().any(|c| c.kind == CueKind::Hot(*s)))
        .collect();
    free.reverse();
    let mut order: Vec<usize> = (0..cues.len())
        .filter(|&i| cues[i].kind == CueKind::Memory)
        .collect();
    order.sort_by(|&a, &b| cues[a].start.total_cmp(&cues[b].start));
    let mut changed = false;
    for i in order {
        let Some(slot) = free.pop() else { break };
        cues[i].kind = CueKind::Hot(slot);
        changed = true;
    }
    changed
}

/// Hot cues re-lettered by time: the earliest becomes A.
pub(crate) fn sort_hot_cues(cues: &mut [od_core::Cue]) -> bool {
    use od_core::CueKind;
    let idx: Vec<usize> = (0..cues.len())
        .filter(|&i| matches!(cues[i].kind, CueKind::Hot(_)))
        .collect();
    let before: Vec<CueKind> = idx.iter().map(|&i| cues[i].kind).collect();
    let mut slots: Vec<u8> = before
        .iter()
        .filter_map(|k| {
            if let CueKind::Hot(s) = k {
                Some(*s)
            } else {
                None
            }
        })
        .collect();
    slots.sort_unstable();
    let mut by_time = idx.clone();
    by_time.sort_by(|&a, &b| cues[a].start.total_cmp(&cues[b].start));
    for (&i, slot) in by_time.iter().zip(slots) {
        cues[i].kind = CueKind::Hot(slot);
    }
    idx.iter().zip(before).any(|(&i, k)| cues[i].kind != k)
}

/// Applies `f` to the cue lists of the selected tracks (one undo step per
/// changed track).
fn edit_selected_cues(app: &App, done: &str, f: fn(&mut [od_core::Cue]) -> bool) {
    if app.guard_read_only() {
        return;
    }
    let ids = app.state.borrow().selected_ids();
    let mut any = false;
    for id in ids {
        let mut cues = app.state.borrow().lib.cues(id).unwrap_or_default();
        if !f(&mut cues) {
            continue;
        }
        let r = app.state.borrow_mut().lib.set_cues(id, cues);
        if app.report(r).is_none() {
            return;
        }
        any = true;
    }
    if any {
        app.after_edit(done);
    } else {
        app.status("Nothing to change");
    }
}

/// Lists recently added to: the row menu offers them inline (most recent
/// first, three at most; kept in the library meta).
pub(crate) const RECENT_LISTS_META: &str = "browser.recent-lists";

pub(crate) fn remember_recent_list(app: &App, node: NodeId) {
    let mut st = app.state.borrow_mut();
    let mut ids: Vec<i64> = st
        .lib
        .meta(RECENT_LISTS_META)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    ids.retain(|i| *i != node.0);
    ids.insert(0, node.0);
    ids.truncate(3);
    let _ = st.lib.set_meta(
        RECENT_LISTS_META,
        &serde_json::to_string(&ids).unwrap_or_default(),
    );
}

/// Track colour for menu entry `n` (1..=8 the eight colour tags, else none).
pub(crate) fn menu_color(n: i64) -> Option<od_core::CueColor> {
    od_format_pioneer::TRACK_COLORS
        .get((n as usize).wrapping_sub(1))
        .map(|(_, c)| *c)
}

/// Star rating for a click / menu value, 0..=5.
pub(crate) fn menu_rating(n: i64) -> u8 {
    n.clamp(0, 5) as u8
}

/// Sets one text tag field from the info panel ("" clears it).
pub(crate) fn set_tag_field(t: &mut od_core::TrackTags, field: &str, value: &str) {
    let v = value.to_string();
    match field {
        "title" => t.title = v,
        "artist" => t.artist = v,
        "album" => t.album = v,
        "genre" => t.genre = v,
        "label" => t.label = v,
        "remixer" => t.remixer = v,
        "composer" => t.composer = v,
        "comment" => t.comment = v,
        "year" => t.year = value.parse().ok(),
        _ => {}
    }
}

/// The cover art of `track` as a square thumbnail of `size` px.
pub(crate) fn artwork_image(
    lib: &od_library::Library,
    track: TrackId,
    size: u32,
) -> Option<slint::Image> {
    let (data, _) = lib.artwork(track).ok().flatten()?;
    crate::render::artwork_thumbnail(&data, size)
}

/// Fills the info panel from the current track.
pub(crate) fn refresh_info(app: &App) {
    let st = app.state.borrow();
    let info = st
        .player
        .current
        .and_then(|id| st.src().track(id).ok())
        .map(|t| crate::TrackInfo {
            has_track: true,
            read_only: st.read_only(),
            title: t.tags.title.clone().into(),
            artist: t.tags.artist.clone().into(),
            album: t.tags.album.clone().into(),
            genre: t.tags.genre.clone().into(),
            label: t.tags.label.clone().into(),
            remixer: t.tags.remixer.clone().into(),
            composer: t.tags.composer.clone().into(),
            year: t
                .tags
                .year
                .map(|y| y.to_string())
                .unwrap_or_default()
                .into(),
            comment: t.tags.comment.clone().into(),
            bpm: t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default().into(),
            key: st.key_text(&t).into(),
            time: crate::format::time(t.duration).into(),
            path: st.resolve(&t.path).display().to_string().into(),
            added: t.date_added.clone().into(),
            plays: t.play_count.to_string().into(),
            file_type: t
                .format
                .map(|f| format!("{f:?}").to_uppercase())
                .unwrap_or_default()
                .into(),
            size: crate::format::bytes(t.file_size).into(),
            sample_rate: if t.sample_rate > 0 {
                format!("{:.1} kHz", t.sample_rate as f64 / 1000.0)
            } else {
                String::new()
            }
            .into(),
            bitrate: t
                .bit_rate
                .map(|b| format!("{b} kbps"))
                .unwrap_or_default()
                .into(),
            has_artwork: false,
            artwork: Default::default(),
        });
    let info = info.map(|mut i| {
        if let Some(img) = st
            .player
            .current
            .and_then(|id| artwork_image(st.src(), id, 264))
        {
            i.has_artwork = true;
            i.artwork = img;
        }
        i
    });
    app.ui.set_track_info(info.unwrap_or_default());
}

#[cfg(test)]
mod tag_tests {
    use super::*;

    #[test]
    fn colour_and_rating_menu_values() {
        assert_eq!(menu_color(1), Some(od_format_pioneer::TRACK_COLORS[0].1));
        assert_eq!(menu_color(8), Some(od_format_pioneer::TRACK_COLORS[7].1));
        assert_eq!(menu_color(0), None);
        assert_eq!(menu_color(9), None);
        assert_eq!((menu_rating(4), menu_rating(9), menu_rating(-1)), (4, 5, 0));
    }

    #[test]
    fn info_fields_edit_the_right_tag() {
        let mut t = od_core::TrackTags::default();
        set_tag_field(&mut t, "comment", "warm-up opener");
        set_tag_field(&mut t, "year", "2019");
        set_tag_field(&mut t, "genre", "Drum & Bass");
        assert_eq!(
            (t.comment.as_str(), t.year, t.genre.as_str()),
            ("warm-up opener", Some(2019), "Drum & Bass")
        );
        set_tag_field(&mut t, "year", "");
        assert_eq!(t.year, None);
    }
}

#[cfg(test)]
mod tests {
    use od_core::{Cue, CueKind};

    /// Row menu cue tools: memory cues fill the free pads earliest first,
    /// sorting re-letters the hot cues by time; reloading tags keeps the
    /// library's rating and colour.
    #[test]
    fn row_menu_cue_tools_and_tag_reload() {
        let mut cues = vec![
            Cue::hot(0, 30.0),
            Cue::memory(20.0),
            Cue::memory(5.0),
            Cue::hot(3, 10.0),
        ];
        assert!(super::memory_to_hot(&mut cues));
        assert_eq!(
            cues[2].kind,
            CueKind::Hot(1),
            "earliest memory cue takes the first free pad"
        );
        assert_eq!(cues[1].kind, CueKind::Hot(2));
        assert!(!super::memory_to_hot(&mut cues), "no memory cues left");
        assert!(super::sort_hot_cues(&mut cues));
        let by_time: Vec<(f64, CueKind)> = {
            let mut v: Vec<_> = cues.iter().map(|c| (c.start, c.kind)).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            v
        };
        assert_eq!(
            by_time.iter().map(|(_, k)| *k).collect::<Vec<_>>(),
            [
                CueKind::Hot(0),
                CueKind::Hot(1),
                CueKind::Hot(2),
                CueKind::Hot(3)
            ],
            "A is the earliest"
        );
        assert!(!super::sort_hot_cues(&mut cues), "already sorted");
        let old = od_core::TrackTags {
            title: "Old".into(),
            rating: 4,
            color: super::menu_color(2),
            ..Default::default()
        };
        let file = od_core::TrackTags {
            title: "From file".into(),
            artist: "Artist".into(),
            ..Default::default()
        };
        let t = super::reload_tags(old, file);
        assert_eq!(
            (t.title.as_str(), t.artist.as_str(), t.rating),
            ("From file", "Artist", 4)
        );
        assert_eq!(t.color, super::menu_color(2));
    }
}
