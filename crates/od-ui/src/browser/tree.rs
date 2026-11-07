//! The browser tree: collection, playlists / folders / sets / smart lists,
//! and the libraries on plugged-in sticks.

use crate::app::App;
use crate::state::AppState;
use crate::{TreeRow, format, models::model};
use od_library::{NodeId, NodeKind};
use slint::SharedString;
use std::collections::HashSet;

/// Tree ids: stick roots are `STICK_ROOT - i`, nodes inside stick `i` are
/// `-(STICK_NODE * (i + 1) + node)`.
pub(crate) const STICK_ROOT: i32 = -100;
pub(crate) const STICK_NODE: i32 = 1_000_000;
/// The "All tracks" row.
pub(crate) const ALL_TRACKS: i32 = -1;
/// Selected-node value while an Explorer folder is shown (no tree row).
pub(crate) const EXPLORER: i32 = -2;

/// Stick index and node of a tree id, `None` for library ids.
pub(crate) fn decode_stick_id(id: i32) -> Option<(usize, Option<NodeId>)> {
    if id <= -STICK_NODE {
        let v = -id;
        Some(((v / STICK_NODE - 1) as usize, Some(NodeId((v % STICK_NODE) as i64))))
    } else if id <= STICK_ROOT && id > STICK_ROOT - 1000 {
        Some(((STICK_ROOT - id) as usize, None))
    } else {
        None
    }
}

pub(crate) fn stick_root_id(stick: usize) -> i32 {
    STICK_ROOT - stick as i32
}

pub(crate) fn stick_node_id(stick: usize, node: NodeId) -> i32 {
    -(STICK_NODE * (stick as i32 + 1) + node.0 as i32)
}

/// Scales the length bars to the longest list (the first row is "All tracks").
pub(crate) fn scale_lengths(rows: &mut [TreeRow], max: f32) {
    let max = max.max(1.0);
    for r in rows.iter_mut().skip(1) {
        r.length_frac /= max;
    }
}

fn row(id: i32, name: impl Into<SharedString>, kind: &str, depth: i32, count: i32) -> TreeRow {
    TreeRow { id, name: name.into(), kind: kind.into(), depth, count, ..Default::default() }
}

/// Library nodes below `parent`, depth first, expanded folders opened.
fn walk(st: &AppState, all: &[od_library::Node], parent: Option<NodeId>, depth: i32, out: &mut Vec<TreeRow>, longest: &mut f64, device_nodes: &HashSet<NodeId>) {
    for n in all.iter().filter(|n| n.parent == parent) {
        let has_children = all.iter().any(|c| c.parent == Some(n.id));
        let (count, duration) = if n.kind == NodeKind::Folder {
            (0, 0.0)
        } else {
            let t = st.lib.node_tracks(n.id).unwrap_or_default();
            (t.len(), t.iter().map(|t| t.duration).sum())
        };
        *longest = longest.max(duration);
        let expanded = st.browser.expanded.contains(&n.id);
        out.push(TreeRow {
            length_frac: duration as f32,
            duration: format::time(duration).into(),
            has_children,
            expanded,
            on_device: device_nodes.contains(&n.id),
            ..row(n.id.0 as i32, n.name.clone(), n.kind.as_str(), depth, count as i32)
        });
        if has_children && expanded {
            walk(st, all, Some(n.id), depth + 1, out, longest, device_nodes);
        }
    }
}

pub(crate) fn refresh_tree(app: &App) {
    refresh_menu_targets(app);
    let (names, histories) = super::sources::history_rows(&app.state.borrow().lib);
    app.state.borrow_mut().browser.histories = names;
    app.ui.set_histories(model(histories));
    let st = app.state.borrow();
    let b = &st.browser;
    let all = st.lib.all_nodes().unwrap_or_default();
    let device_nodes: HashSet<NodeId> =
        st.lib.devices().unwrap_or_default().iter().flat_map(|d| st.lib.device_node_closure(d.id).unwrap_or_default()).collect();
    let mut rows = vec![row(ALL_TRACKS, od_i18n::tr("All tracks"), "all", 0, st.lib.track_count().unwrap_or(0) as i32)];
    let mut longest = 1.0f64;
    walk(&st, &all, None, 0, &mut rows, &mut longest, &device_nodes);
    // Sticks with an OpenDecks library, browsable as read-only sources.
    for (i, (name, root)) in b.sticks.iter().enumerate() {
        let open = b.stick.as_ref().filter(|(j, _)| *j == i).map(|(_, s)| s);
        let count = match open {
            Some(s) => s.library.track_count().unwrap_or(0) as i32,
            None => od_format_native::Stick::open(root).ok().flatten().and_then(|s| s.library.track_count().ok()).unwrap_or(0) as i32,
        };
        rows.push(TreeRow { expanded: open.is_some(), on_device: true, ..row(stick_root_id(i), name.clone(), "stick", 0, count) });
        if let Some(s) = open {
            for n in s.library.all_nodes().unwrap_or_default() {
                let mut depth = 1;
                let mut p = n.parent;
                while let Some(x) = p {
                    depth += 1;
                    p = s.library.node(x).ok().and_then(|n| n.parent);
                }
                let count = s.library.node_tracks(n.id).map(|t| t.len()).unwrap_or(0) as i32;
                rows.push(row(stick_node_id(i, n.id), n.name.clone(), n.kind.as_str(), depth, count));
            }
        }
    }
    scale_lengths(&mut rows, longest as f32);
    app.ui.set_tree(model(rows));
    let selected = if st.explorer.open.is_some() {
        EXPLORER
    } else if let Some(sp) = &b.special {
        match sp {
            super::Special::TagList => super::sources::TAG_LIST,
            super::Special::Related => super::sources::RELATED,
            super::Special::Duplicates => super::sources::DUPLICATES,
            super::Special::History(name) => b.histories.iter().position(|h| h == name).map(super::sources::history_id).unwrap_or(ALL_TRACKS),
        }
    } else {
        match (&b.stick, b.node) {
            (Some((i, _)), Some(n)) => stick_node_id(*i, n),
            (Some((i, _)), None) => stick_root_id(*i),
            (None, n) => n.map(|n| n.0 as i32).unwrap_or(ALL_TRACKS),
        }
    };
    app.ui.set_selected_node(selected);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_ids_round_trip() {
        assert_eq!(decode_stick_id(stick_root_id(0)), Some((0, None)));
        assert_eq!(decode_stick_id(stick_root_id(3)), Some((3, None)));
        assert_eq!(decode_stick_id(stick_node_id(2, NodeId(42))), Some((2, Some(NodeId(42)))));
        assert_eq!(decode_stick_id(ALL_TRACKS), None);
        assert_eq!(decode_stick_id(EXPLORER), None);
        assert_eq!(decode_stick_id(17), None);
    }

    #[test]
    fn lengths_scale_to_longest() {
        let mut rows = vec![
            TreeRow { length_frac: 0.0, ..Default::default() },
            TreeRow { length_frac: 300.0, ..Default::default() },
            TreeRow { length_frac: 600.0, ..Default::default() },
        ];
        scale_lengths(&mut rows, 600.0);
        assert_eq!(rows.iter().map(|r| r.length_frac).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
    }
}

/// Playlists and sets offered by the track row's "Add to playlist" menu.
fn refresh_menu_targets(app: &App) {
    let st = app.state.borrow();
    if st.read_only() {
        return;
    }
    let targets: Vec<crate::MenuTarget> = st
        .lib
        .all_nodes()
        .unwrap_or_default()
        .into_iter()
        .filter(|n| matches!(n.kind, od_library::NodeKind::Playlist | od_library::NodeKind::Set))
        .map(|n| crate::MenuTarget { id: n.id.0 as i32, name: n.name.into() })
        .collect();
    app.ui.set_menu_playlists(crate::models::model(targets));
}
