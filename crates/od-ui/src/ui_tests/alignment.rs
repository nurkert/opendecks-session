//! Cross-axis centring of controls in box layouts.
//!
//! A child with a fixed height in a `HorizontalLayout` (fixed width in a
//! `VerticalLayout`) that is smaller than the layout does not stretch: Slint
//! puts it at the start of the cross axis, so it hangs at the top of its row
//! (or sticks to the left of its column) instead of sitting on the row's
//! centre line. Rows centre such children with an inner layout
//! (`VerticalLayout { alignment: center; … }`).
//!
//! The check walks the real element tree of every view in
//! `tests/parity/views.toml` (plus CDJ mode) at desktop, laptop and kiosk
//! size: for each direct child of a box layout, the room above and below it
//! (left and right of it) must match within 2 px. Children that fill the
//! layout but for its padding (≥ 90 % of the cross size, e.g. a list beside
//! a scrollbar gap) are not checked: their offset is the padding.

use super::harness::*;
use super::views::{open_view, views_file};
use i_slint_backend_testing::{ElementHandle, ElementRoot, LayoutKind};
use std::ops::ControlFlow;

const TOL: f32 = 2.0;

/// Children of box layouts that sit at the start of the cross axis.
pub(crate) fn snapped_children(root: &ElementHandle) -> Vec<String> {
    struct Node {
        e: ElementHandle,
        r: Rect,
        // Index of the last element of this element's subtree (pre-order).
        end: usize,
    }
    let mut all: Vec<ElementHandle> = Vec::new();
    root.visit_descendants(|e| {
        all.push(e);
        ControlFlow::<()>::Continue(())
    });
    let nodes: Vec<Node> = all
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let mut n = 0usize;
            e.visit_descendants(|_| {
                n += 1;
                ControlFlow::<()>::Continue(())
            });
            let r = Rect::of(&e);
            Node { e, r, end: i + n }
        })
        .collect();
    let name = |from: usize, to: usize| {
        (from..to.min(nodes.len()))
            .find_map(|k| nodes[k].e.accessible_label().filter(|s| !s.is_empty()).map(|s| s.to_string()))
            .unwrap_or_else(|| nodes[from].e.type_name().map(|s| s.to_string()).unwrap_or_default())
    };
    let mut out = Vec::new();
    for (i, l) in nodes.iter().enumerate() {
        let horizontal = match l.e.layout_kind() {
            Some(LayoutKind::HorizontalLayout) => true,
            Some(LayoutKind::VerticalLayout) => false,
            _ => continue,
        };
        if l.r.w < 1.0 || l.r.h < 1.0 || l.e.computed_opacity() <= 0.01 {
            continue;
        }
        // Direct children: step over each child's subtree.
        let mut j = i + 1;
        while j <= l.end && j < nodes.len() {
            let c = &nodes[j];
            let next = c.end.max(j) + 1;
            let (size, room) = if horizontal { (c.r.h, l.r.h) } else { (c.r.w, l.r.w) };
            if c.r.w >= 1.0 && c.r.h >= 1.0 && c.e.computed_opacity() > 0.01 && size < 0.9 * room {
                let (before, after) = if horizontal {
                    (c.r.y - l.r.y, l.r.bottom() - c.r.bottom())
                } else {
                    (c.r.x - l.r.x, l.r.right() - c.r.right())
                };
                if after - before > TOL {
                    out.push(format!(
                        "`{}` sits at the {} of its {} ({:.0} px before, {:.0} px after; layout at {:.0},{:.0} {:.0}×{:.0})",
                        name(j, next),
                        if horizontal { "top" } else { "left" },
                        if horizontal { "row" } else { "column" },
                        before,
                        after,
                        l.r.x,
                        l.r.y,
                        l.r.w,
                        l.r.h
                    ));
                }
            }
            j = next;
        }
    }
    out
}

fn check_size(size_id: &str) {
    let file = views_file();
    let size = file.sizes.iter().find(|s| s.id == size_id).expect("size in views.toml").clone();
    let kiosk = size_id == "kiosk";
    let mut failures = Vec::new();
    for view in file
        .views
        .iter()
        // Context menus and the View menu are native; Preferences is its own window.
        .filter(|v| !v.id.starts_with("ctx.") && !v.id.starts_with("menu.") && !v.id.starts_with("prefs."))
        .filter(|v| v.sizes.is_empty() || v.sizes.iter().any(|s| s == size_id))
    {
        let ui = Ui::start(Fixture::new(), (size.width, size.height), kiosk);
        if open_view(&ui, view, kiosk).is_err() {
            continue;
        }
        for f in snapped_children(&ui.app.ui.root_element()) {
            failures.push(format!("{} {}: {f}", size_id, view.id));
        }
    }
    if !kiosk {
        // CDJ practice mode (not a views.toml view).
        let ui = Ui::start(Fixture::new(), (size.width, size.height), false);
        crate::engine::ensure_engine(&ui.app);
        ui.app.ui.invoke_set_mode("perform".into());
        ui.pump(300);
        ui.click("topbar.toggle.cdj");
        ui.pump(400);
        assert!(ui.has("deck1.cdj"));
        for f in snapped_children(&ui.app.ui.root_element()) {
            failures.push(format!("{size_id} cdj: {f}"));
        }
    }
    assert!(failures.is_empty(), "controls off their row / column centre:\n{}", failures.join("\n"));
}

#[test]
fn controls_centred_desktop() {
    check_size("desktop");
}

#[test]
fn controls_centred_laptop() {
    check_size("laptop");
}

#[test]
fn controls_centred_kiosk() {
    check_size("kiosk");
}
