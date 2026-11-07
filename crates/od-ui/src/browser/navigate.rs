//! Browsing from a controller (SPEC §9): the browse encoder moves through
//! the tree or the track list, a press opens the list (or expands a folder)
//! and then loads the track, BACK returns to the tree, VIEW enlarges the
//! browser and RELATED opens the related tracks. The part with focus is
//! marked in the window (`browse-focus`).

use crate::app::App;
use crate::decks;
use od_controller::BrowseAction;
use slint::{Model, SharedString};

/// The tree rows the encoder steps through (library lists and sticks; the
/// Explorer is left to the mouse).
fn tree_ids(app: &App) -> Vec<i32> {
    app.ui.get_tree().iter().map(|r| r.id).collect()
}

/// The tree row `delta` steps from `current` (clamped at both ends; pure).
pub(crate) fn step_in(ids: &[i32], current: i32, delta: i32) -> Option<i32> {
    if ids.is_empty() {
        return None;
    }
    let i = ids.iter().position(|&x| x == current).unwrap_or(0) as i64;
    let j = (i + delta as i64).clamp(0, ids.len() as i64 - 1) as usize;
    Some(ids[j])
}

fn set_focus(app: &App, tree: bool) {
    app.state.borrow_mut().browser.focus_tree = tree;
    app.ui.set_browse_focus(SharedString::from(if tree { "tree" } else { "list" }));
}

/// Handles one browse action of a controller.
pub(crate) fn controller(app: &App, action: &BrowseAction) {
    let kiosk = app.state.borrow().config.kiosk;
    let in_tree = app.state.borrow().browser.focus_tree;
    match action {
        BrowseAction::Rotate { delta, shifted } => {
            if kiosk {
                app.ui.set_browser_open(true);
            }
            let delta = if *shifted { delta * 8 } else { *delta };
            if in_tree {
                let ids = tree_ids(app);
                if let Some(next) = step_in(&ids, app.ui.get_selected_node(), delta) {
                    app.ui.invoke_select_node(next);
                    set_focus(app, true);
                    let name = app.ui.get_tree().iter().find(|r| r.id == next).map(|r| r.name.to_string()).unwrap_or_default();
                    app.status(name);
                }
            } else {
                for _ in 0..delta.unsigned_abs().min(64) {
                    super::step_selection(app, delta < 0);
                }
            }
        }
        BrowseAction::Press { shifted } => {
            if in_tree {
                let current = app.ui.get_selected_node();
                let rows: Vec<crate::TreeRow> = app.ui.get_tree().iter().collect();
                let at = rows.iter().position(|r| r.id == current);
                match at.map(|i| (i, &rows[i])) {
                    // A folder opens; an open one hands over to its first list.
                    Some((_, r)) if r.kind == "folder" && r.has_children && !r.expanded => app.ui.invoke_toggle_node(r.id),
                    Some((i, r)) if r.kind == "folder" && r.has_children => {
                        if let Some(child) = rows.get(i + 1).filter(|c| c.depth > r.depth) {
                            app.ui.invoke_select_node(child.id);
                        }
                    }
                    Some((_, r)) if r.kind == "folder" => {}
                    _ => {
                        set_focus(app, false);
                        if app.state.borrow().browser.selection.is_empty() && app.state.borrow().row_count() > 0 {
                            super::select_row(app, 0, false, false);
                            app.ui.set_focus_row(0);
                        }
                    }
                }
            } else if *shifted {
                super::sources::add_selected_to_tag_list(app);
            } else {
                if !crate::explorer::load_selection(app, None) {
                    decks::load_selected_into_free_deck(app);
                }
                if kiosk {
                    app.ui.set_browser_open(false);
                }
            }
        }
        BrowseAction::Back => {
            if in_tree {
                // Up to the folder holding the open list.
                let parent = {
                    let st = app.state.borrow();
                    st.browser.node.filter(|_| !st.read_only()).and_then(|n| st.lib.node(n).ok()).and_then(|n| n.parent)
                };
                if let Some(p) = parent {
                    app.ui.invoke_select_node(p.0 as i32);
                }
            }
            set_focus(app, true);
        }
        BrowseAction::ViewToggle => {
            if kiosk {
                app.ui.set_browser_open(!app.ui.get_browser_open());
            } else if app.ui.get_mode() == "prepare" {
                let next = crate::shell::next_layout(app.ui.get_prepare_layout().as_str());
                crate::shell::set_prepare_layout(app, next);
            } else {
                // Perform: a larger browser, then back.
                let big = app.ui.get_upper_height() > 0.0 && app.ui.get_upper_height() <= ENLARGED_UPPER + 0.5;
                app.ui.set_upper_height(if big { 0.0 } else { ENLARGED_UPPER });
            }
        }
        BrowseAction::Related => {
            app.ui.invoke_select_node(super::sources::RELATED);
            set_focus(app, false);
        }
    }
}

/// Height of the deck area while the browser is enlarged (Perform, VIEW).
pub(crate) const ENLARGED_UPPER: f32 = 220.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_steps_clamp_at_the_ends() {
        let ids = [-1, 5, 7, 9];
        assert_eq!(step_in(&ids, -1, 2), Some(7));
        assert_eq!(step_in(&ids, 7, -5), Some(-1));
        assert_eq!(step_in(&ids, 9, 3), Some(9));
        assert_eq!(step_in(&ids, 42, 1), Some(5), "an unknown row counts as the first");
        assert_eq!(step_in(&[], 1, 1), None);
    }
}
