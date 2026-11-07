//! Sub-browser: a second tree and list under the main list, for dragging
//! tracks between lists. Main-list rows dropped on a sub-browser list (or
//! the list it shows) go through the main drop path (`DragTargets.tree`);
//! rows dragged from the sub-browser are handled here.

use super::DropTarget;
use crate::app::{App, handler};
use crate::models::model;
use crate::{SubTrackRow, TreeRow, decks};
use od_core::TrackId;
use od_library::{NodeId, NodeKind};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::rc::Rc;

/// The list shown and its tracks (in list order).
#[derive(Default)]
struct SubState {
    node: Option<NodeId>,
    tracks: Vec<TrackId>,
}

thread_local! {
    static SUB: RefCell<SubState> = RefCell::new(SubState::default());
}

/// Library lists below `parent`, depth first, every folder open (pure on
/// the node list).
pub(crate) fn list_rows(all: &[od_library::Node], counts: &dyn Fn(NodeId) -> usize) -> Vec<TreeRow> {
    fn walk(all: &[od_library::Node], parent: Option<NodeId>, depth: i32, counts: &dyn Fn(NodeId) -> usize, out: &mut Vec<TreeRow>) {
        for n in all.iter().filter(|n| n.parent == parent) {
            out.push(TreeRow {
                id: n.id.0 as i32,
                name: n.name.clone().into(),
                kind: n.kind.as_str().into(),
                depth,
                count: if n.kind == NodeKind::Folder { 0 } else { counts(n.id) as i32 },
                has_children: all.iter().any(|c| c.parent == Some(n.id)),
                expanded: true,
                ..Default::default()
            });
            walk(all, Some(n.id), depth + 1, counts, out);
        }
    }
    let mut out = Vec::new();
    walk(all, None, 0, counts, &mut out);
    out
}

/// Shows / hides the sub-browser.
pub(crate) fn toggle(app: &App) {
    let open = !app.ui.get_sub_browser_open();
    app.ui.set_sub_browser_open(open);
    if open && app.ui.get_section() != "library" {
        app.ui.invoke_select_section("library".into());
    }
    refresh(app);
}

/// Shows list `id` in the sub-browser.
pub(crate) fn select(app: &App, id: i32) {
    SUB.with(|s| s.borrow_mut().node = (id >= 0).then_some(NodeId(id as i64)));
    refresh(app);
}

/// Lists and tracks of the sub-browser (library data; nothing while hidden).
pub(crate) fn refresh(app: &App) {
    if !app.ui.get_sub_browser_open() {
        return;
    }
    let st = app.state.borrow();
    let all = st.lib.all_nodes().unwrap_or_default();
    let rows = list_rows(&all, &|n| st.lib.node_tracks(n).map(|t| t.len()).unwrap_or(0));
    let sb = app.ui.global::<crate::SubBrowser>();
    // A deleted list is forgotten.
    let node = SUB.with(|s| s.borrow().node).filter(|n| all.iter().any(|x| x.id == *n));
    let kind = node.and_then(|n| all.iter().find(|x| x.id == n).map(|x| x.kind));
    let tracks = match (node, kind) {
        (Some(_), Some(NodeKind::Folder)) | (None, _) => Vec::new(),
        (Some(n), _) => st.lib.node_tracks(n).unwrap_or_default(),
    };
    let view: Vec<SubTrackRow> = tracks
        .iter()
        .map(|t| SubTrackRow {
            title: t.tags.title.clone().into(),
            artist: t.tags.artist.clone().into(),
            bpm: t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default().into(),
            key: st.key_text(t).into(),
        })
        .collect();
    // Models are only replaced when their rows change: a refresh during a
    // drag must not drop the row being dragged.
    if !update_in_place(&sb.get_lists(), &rows) {
        sb.set_lists(model(rows));
    }
    sb.set_selected(node.map(|n| n.0 as i32).unwrap_or(-1));
    sb.set_title(node.and_then(|n| all.iter().find(|x| x.id == n)).map(|x| x.name.clone()).unwrap_or_default().into());
    sb.set_takes_tracks(matches!(kind, Some(NodeKind::Playlist | NodeKind::Set)) && !st.read_only());
    if !update_in_place(&sb.get_tracks(), &view) {
        sb.set_tracks(model(view));
    }
    SUB.with(|s| {
        let mut s = s.borrow_mut();
        s.node = node;
        s.tracks = tracks.iter().map(|t| t.id).collect();
    });
}

/// Updates the rows of `m` to `rows` when it has as many (row items stay,
/// so a drag in progress survives); false when the model must be replaced.
fn update_in_place<T: Clone + PartialEq + 'static>(m: &slint::ModelRc<T>, rows: &[T]) -> bool {
    if m.row_count() != rows.len() || rows.is_empty() {
        return false;
    }
    for (i, r) in rows.iter().enumerate() {
        if m.row_data(i).as_ref() != Some(r) {
            m.set_row_data(i, r.clone());
        }
    }
    true
}

/// Sub-browser row `index` dropped on `target` (`DragState.target`): a list
/// in the main tree or the sub-browser, a place in the open list, a deck or
/// a Prepare player.
pub(crate) fn row_dropped(app: &App, index: usize, target: i32) {
    let (node, track) = SUB.with(|s| {
        let s = s.borrow();
        (s.node, s.tracks.get(index).copied())
    });
    let Some(track) = track else { return };
    match super::drop_target(target) {
        DropTarget::Tree(id) if id >= 0 => {
            let to = NodeId(id as i64);
            if Some(to) == node || app.guard_read_only() {
                return;
            }
            let name = app.state.borrow().lib.node(to).map(|n| n.name).unwrap_or_default();
            let r = app.state.borrow_mut().lib.add_to_node(to, &[track], None);
            if app.report(r).is_some() {
                app.after_edit(&od_i18n::trf("Added {} track(s) to {}", &[&1, &name]));
            }
        }
        DropTarget::Tree(_) => {}
        DropTarget::Row(at) => {
            let open = app.state.borrow().browser.node;
            if !super::reorderable(&app.state.borrow()) {
                app.status("Sort the list by # to drop tracks into it");
                return;
            }
            let Some(open) = open else { return };
            let r = app.state.borrow_mut().lib.add_to_node(open, &[track], Some(at));
            if app.report(r).is_some() {
                app.after_edit("Added to the list — ⌘Z to undo");
            }
        }
        DropTarget::Deck if (1..=decks::DECKS as i32).contains(&target) => decks::load_track(app, target as usize - 1, track, None),
        DropTarget::Deck if target == -2 => crate::player::load_second(app, track),
        DropTarget::Deck if target == -1 => {
            {
                let mut st = app.state.borrow_mut();
                st.player.current = Some(track);
                st.player.cursor = 0.0;
            }
            crate::player::refresh_detail(app);
        }
        DropTarget::Deck => {}
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    let sb = app.ui.global::<crate::SubBrowser>();
    sb.on_select(handler!(app, |a, id: i32| select(&a, id)));
    sb.on_row_dropped(handler!(app, |a, i: i32, t: i32| row_dropped(&a, i.max(0) as usize, t)));
    app.ui.on_toggle_sub_browser(handler!(app, |a| toggle(&a)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_open_every_folder() {
        let mut lib = od_library::Library::open_in_memory().unwrap();
        let f = lib.create_node(None, NodeKind::Folder, "Gigs").unwrap();
        let a = lib.create_node(Some(f), NodeKind::Playlist, "Friday").unwrap();
        let b = lib.create_node(None, NodeKind::Set, "Closing").unwrap();
        let all = lib.all_nodes().unwrap();
        let rows = list_rows(&all, &|n| if n == a { 3 } else { 0 });
        let names: Vec<(String, i32)> = rows.iter().map(|r| (r.name.to_string(), r.depth)).collect();
        assert_eq!(names, vec![("Gigs".into(), 0), ("Friday".into(), 1), ("Closing".into(), 0)]);
        assert_eq!(rows[1].count, 3);
        assert_eq!(rows[2].id, b.0 as i32);
        assert_eq!(rows[0].kind, "folder");
    }
}
