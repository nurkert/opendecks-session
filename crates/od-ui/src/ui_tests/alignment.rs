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
            .find_map(|k| {
                nodes[k]
                    .e
                    .accessible_label()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| {
                nodes[from]
                    .e
                    .type_name()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            })
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
            let (size, room) = if horizontal {
                (c.r.h, l.r.h)
            } else {
                (c.r.w, l.r.w)
            };
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
    let size = file
        .sizes
        .iter()
        .find(|s| s.id == size_id)
        .expect("size in views.toml")
        .clone();
    let kiosk = size_id == "kiosk";
    let mut failures = Vec::new();
    for view in file
        .views
        .iter()
        // Context menus and the View menu are native; Preferences is its own window.
        .filter(|v| {
            !v.id.starts_with("ctx.") && !v.id.starts_with("menu.") && !v.id.starts_with("prefs.")
        })
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
    assert!(
        failures.is_empty(),
        "controls off their row / column centre:\n{}",
        failures.join("\n")
    );
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

/// Live numbers do not twitch: the bundled fonts set digits with tabular
/// figures (fonts/tabular_figures.py), so readouts of the same pattern have
/// the same width and every unchanged digit keeps its place, whatever the
/// alignment. Measured on two weights (the clock at 600, the status summary
/// in caps) with the countdown patterns of the deck header and platter.
#[test]
fn live_numbers_keep_their_digits_in_place() {
    let ui = Ui::open((1440, 900));
    let width = |label: &str| Rect::of(&ui.get(label)).w;
    let samples = ["-1:08.4", "-1:08.8", "-1:11.1", "-0:00.0", "-7:59.9", "-2:34.5", "-6:66.6", "-9:99.9"];
    let mut problems = Vec::new();
    for (label, set) in [
        ("topbar.clock", &(|u: &Ui, t: &str| u.app.ui.set_clock(t.into())) as &dyn Fn(&Ui, &str)),
        ("status.summary", &|u: &Ui, t: &str| u.app.ui.set_list_summary(t.into())),
    ] {
        let mut widths = Vec::new();
        for s in samples {
            set(&ui, s);
            widths.push((s, width(label)));
        }
        let first = widths[0].1;
        for (s, w) in &widths {
            if (w - first).abs() > 0.01 {
                problems.push(format!("`{label}`: `{s}` is {w:.2} px wide, `{}` {first:.2} px", widths[0].0));
            }
        }
        // Each digit alone: one width for all ten.
        let mut digit = Vec::new();
        for d in 0..10 {
            set(&ui, &format!("{d}{d}{d}"));
            digit.push(width(label));
        }
        if digit.iter().any(|w| (w - digit[0]).abs() > 0.01) {
            problems.push(format!("`{label}`: digit widths differ: {digit:?}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
/// Popups stay inside the window: the placement helper keeps them 4 px in
/// and flips them to the other side of the anchor, and every PopupWindow
/// list (Beat FX type, Colour FX, pad mode, work mode) opened next to a
/// window edge lies fully inside the window.
#[test]
fn popups_stay_inside_the_window() {
    use i_slint_backend_testing::ElementRoot;
    use slint::ComponentHandle;
    let ui = Ui::open((1000, 700));
    let p = ui.app.ui.global::<crate::PopupPlace>();
    let (ww, wh) = (p.get_window_width(), p.get_window_height());
    assert!(ww > 900.0 && wh > 600.0, "window size known: {ww}×{wh}");
    // Wants to start 30 px left of the window: moved in.
    assert_eq!(30.0 + p.invoke_x(30.0, -60.0, 200.0), 4.0);
    // Would run past the right edge: ends 4 px inside.
    assert_eq!(ww - 50.0 + p.invoke_x(ww - 50.0, 0.0, 200.0) + 200.0, ww - 4.0);
    // Below does not fit at the bottom: opens above.
    assert_eq!(p.invoke_y(wh - 20.0, 20.0, -110.0, 100.0), -110.0);
    assert_eq!(p.invoke_y(100.0, 20.0, -110.0, 100.0), 20.0);

    // The lists themselves, opened from controls at the window edges.
    let mut problems = Vec::new();
    // The FX panel makes the decks compact (pad mode then cycles on a key),
    // so the pad mode lists are opened without it.
    let fx = ["fx.unit1.slot1.type", "fx.unit2.slot1.type", "fx.cfx.type", "topbar.mode"];
    let decks = ["deck1.pad-mode", "deck2.pad-mode", "fx.cfx.type", "topbar.mode"];
    for (size, with_fx, openers) in [((1000, 700), true, &fx), ((1440, 900), true, &fx), ((1440, 900), false, &decks)] {
        let ui = Ui::open(size);
        crate::engine::ensure_engine(&ui.app);
        ui.app.ui.invoke_set_mode("perform".into());
        if with_fx {
            ui.app.ui.invoke_toggle_fx_panel();
        }
        ui.pump(300);
        let (ww, wh) = ui.size();
        let win = Rect { x: 0.0, y: 0.0, w: ww, h: wh };
        for opener in openers.iter().copied() {
            let Some(e) = ui.find(opener) else {
                problems.push(format!("{size:?}: no `{opener}`"));
                continue;
            };
            // The value in the middle of a stepper opens its list.
            e.mock_single_click(slint::platform::PointerEventButton::Left);
            ui.pump(100);
            let mut lists = Vec::new();
            ui.app.ui.root_element().visit_descendants(|el| {
                let t = el.type_name().unwrap_or_default();
                if t == "ChoiceList" || (opener == "topbar.mode" && el.accessible_label().is_some_and(|l| l == "LIGHTING")) {
                    lists.push(Rect::of(&el));
                }
                std::ops::ControlFlow::<()>::Continue(())
            });
            if lists.is_empty() {
                problems.push(format!("{size:?}: `{opener}` opened no list"));
            }
            // Popup contents report popup-local positions: the list must
            // start at the popup's corner and the popup be as big as the list
            // (an unsized popup centred its list on the corner, half of it
            // outside to the left and top). Slint and PopupPlace then keep
            // the sized popup inside the window.
            for r in lists {
                let local = r.x.abs() < 0.5 && r.y.abs() < 0.5;
                if !(local || win.contains(&r, 0.5)) || r.w < 1.0 || r.h < 1.0 {
                    problems.push(format!("{size:?}: list of `{opener}` at {:.0},{:.0} {:.0}×{:.0} (not at its popup's corner)", r.x, r.y, r.w, r.h));
                }
            }
            ui.key(slint::platform::Key::Escape);
            ui.pump(100);
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
