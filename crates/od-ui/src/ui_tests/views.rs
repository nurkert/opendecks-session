//! SPEC §16.3 view completeness, driven by `tests/parity/views.toml`.
//!
//! For each size (desktop 1440×900, laptop 1280×800, kiosk 800×480) and each
//! view that lists the size, a fresh app is opened in that view (mode,
//! deck count, panels, browser source, popup, Preferences page) and every
//! required element is checked:
//!
//! - present: an element carries the label (exactly once),
//! - visible: non-zero size, opacity > 0,
//! - unclipped: inside the window (±1 px),
//! - no-overlap: no overlap with another element of the same region,
//! - order: along the region's axis (row / column / grid, ±1 px),
//! - min-target: interactive roles at least `min_target` px on both edges.
//!
//! Views the app cannot show are reported as unavailable. The results go to
//! `target/parity/views-<size>.json`, which `parity check` summarises
//! (labels present, views passing). Report-only by default; with
//! `OD_PARITY_STRICT=1` every view must pass.

use super::harness::*;
use i_slint_backend_testing::ElementHandle;
use od_parity::model::{Region, Size, View, ViewsFile};
use slint::ComponentHandle;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Context-menu items are native `MenuItem`s, which cannot carry an
/// accessible label: they are found by their (English) title in the open
/// menu instead. Only items that exist are listed.
pub(crate) const MENU_ITEMS: &[(&str, &str)] = &[
    ("ctx.row.load", "Load to deck"),
    ("ctx.row.import", "Import to collection"),
    ("ctx.row.analyse", "Analyse"),
    ("ctx.row.analysis-lock", "Lock / unlock analysis"),
    ("ctx.row.add-to-playlist", "Add to playlist"),
    ("ctx.row.add-to-tag-list", "Add to tag list"),
    ("ctx.row.reload-tag", "Reload tags from file"),
    ("ctx.row.export-track", "Export file…"),
    ("ctx.row.reset-play-count", "Reset play count"),
    ("ctx.row.memory-to-hot-cues", "Memory cues to hot cues"),
    ("ctx.row.sort-hot-cues", "Sort hot cues by time"),
    ("ctx.row.remove", "Remove from this list"),
    ("ctx.row.show-info", "Show info"),
    ("ctx.row.show-in-finder", "Show in file manager"),
    ("ctx.header.columns", "PREVIEW"),
    ("menu.view.full-screen", "Full Screen"),
    ("menu.view.layouts", "Prepare Layout"),
    ("menu.view.my-tag", "My Tag"),
    ("menu.view.related", "Related Tracks"),
    ("menu.view.info", "Info Panel"),
    ("menu.view.sub-browser", "Sub Browser"),
];

pub(crate) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

pub(crate) fn views_file() -> ViewsFile {
    od_parity::model::load(&workspace_root().join(od_parity::VIEWS)).expect("views.toml loads")
}

pub(crate) fn strict() -> bool {
    std::env::var("OD_PARITY_STRICT").is_ok_and(|v| v == "1")
}

/// Puts a freshly started app into `view`. `Err` = the app has no such view.
pub(crate) fn open_view(ui: &Ui, view: &View, kiosk: bool) -> Result<(), String> {
    let w = &ui.app.ui;
    let id = view.id.as_str();
    // Clicks an element when the view shows it (the kiosk has no sidebar).
    let click_if = |label: &str| {
        if let Some(e) = ui.find(label) {
            ui.click_el(&e);
        }
    };
    let perform = |decks: i32| {
        // Perform with audio running (the mixer strips appear with the engine).
        crate::engine::ensure_engine(&ui.app);
        w.invoke_set_mode("perform".into());
        w.invoke_set_deck_count(decks);
        if kiosk {
            w.set_browser_open(false);
        }
    };
    let prepare_library = || {
        if kiosk {
            w.set_browser_open(true);
        } else {
            w.invoke_set_mode("prepare".into());
        }
        w.invoke_select_section("library".into());
    };
    match id {
        "perform.2deck-horizontal" | "panel.recording" => perform(2),
        "perform.4deck-horizontal" => perform(4),
        "perform.2deck-vertical" | "perform.4deck-vertical" => {
            perform(if id.starts_with("perform.4") { 4 } else { 2 });
            slint::ComponentHandle::global::<crate::PerformLayout>(w).set_layout("vertical".into());
        }
        "perform.browse" => {
            perform(2);
            slint::ComponentHandle::global::<crate::PerformLayout>(w).set_layout("browse".into());
        }
        "lighting" => {
            if kiosk {
                return Err("Lighting is not part of the kiosk".into());
            }
            w.invoke_set_mode("lighting".into());
        }
        "panel.fx" => {
            perform(2);
            if !w.get_fx_panel() {
                w.invoke_toggle_fx_panel();
            }
            // The view lists all three slots: multi mode on both units.
            for u in 0..2 {
                crate::fx::toggle_multi(&ui.app, u);
            }
        }
        "panel.sampler" => {
            perform(2);
            if !w.get_sampler_panel() {
                w.invoke_toggle_sampler_panel();
            }
        }
        "prepare.1-player" | "prepare.1-player.grid" => {
            if kiosk {
                return Err("Prepare is not part of the kiosk".into());
            }
            prepare_library();
            w.invoke_row_clicked(0, false, false);
            if id.ends_with(".grid") {
                ui.pump(200);
                click_if("player1.tab.grid");
            }
        }
        "prepare.2-players"
        | "prepare.dual-player"
        | "prepare.simple-player"
        | "prepare.full-browser"
        | "prepare.recording-bar" => {
            if kiosk {
                return Err("Prepare is not part of the kiosk".into());
            }
            prepare_library();
            w.invoke_row_clicked(0, false, false);
            let layout = match id {
                "prepare.2-players" => "view.layout.two",
                "prepare.dual-player" => "view.layout.dual",
                "prepare.simple-player" => "view.layout.simple",
                "prepare.full-browser" => "view.layout.browser",
                _ => "view.layout.player",
            };
            w.invoke_menu_action(layout.into());
            if id == "prepare.recording-bar" && !w.get_rec_panel() {
                w.invoke_menu_action("view.panel.rec".into());
            }
        }
        "browser.collection" => {
            prepare_library();
            w.invoke_select_node(-1);
        }
        "browser.playlists" => {
            prepare_library();
            w.invoke_select_node(ui.fx.playlist.0 as i32);
        }
        "browser.related" => {
            prepare_library();
            w.invoke_select_node(-4);
        }
        "browser.explorer" => {
            prepare_library();
            w.invoke_explorer_select(0);
        }
        "browser.devices" => {
            // The Devices source of the rail: the tree shows the sticks, the
            // list keeps the open list.
            prepare_library();
            ui.pump(200);
            click_if("browser.rail.devices");
        }
        "browser.recordings" => {
            // The rail's REC source: the recordings page, the tree at its row.
            prepare_library();
            ui.pump(200);
            click_if("browser.rail.recordings");
            if w.get_section() != "recordings" {
                w.invoke_select_section("recordings".into());
            }
        }
        "browser.info-panel" => {
            prepare_library();
            w.invoke_select_node(-1);
            w.invoke_row_clicked(0, false, false);
            w.set_info_open(true);
            ui.pump(200);
            // The file details of the INFO tab.
            click_if("info.tab.info");
        }
        "ctx.track-row" => {
            prepare_library();
            w.invoke_select_node(-1);
            ui.pump(200);
            let row = ui
                .find("browser.row0")
                .ok_or("no track row to right-click")?;
            ui.right_click_el(&row);
        }
        "ctx.column-header" => {
            prepare_library();
            w.invoke_select_node(-1);
            ui.pump(200);
            let header = ui
                .find("browser.header.title")
                .ok_or("no column header to right-click")?;
            ui.right_click_el(&header);
        }
        "perform.mixer-off" => {
            perform(2);
            if w.get_mixer_panel() {
                w.invoke_menu_action("perform.mixer.panel".into());
            }
        }
        "panel.mix-point-link" => {
            perform(2);
            if !w.get_mpl_panel() {
                w.invoke_menu_action("perform.mpl.panel".into());
            }
        }
        "browser.histories" => {
            // One played session, as the decks log it.
            let track = ui.fx.tracks[0].0;
            let _ = ui.app.state.borrow_mut().lib.record_play(
                track,
                "Session 2026-11-07 21:30",
                "opendecks",
            );
            crate::browser::refresh_tree(&ui.app);
            prepare_library();
            ui.pump(200);
            click_if("browser.rail.histories");
            click_if("browser.tree.history0");
        }
        "browser.tag-list" => {
            // The rail's TAG source: opens the tag list and scrolls the tree to it.
            prepare_library();
            ui.pump(200);
            click_if("browser.rail.tag-list");
            if w.get_selected_node() != -3 {
                w.invoke_select_node(-3);
            }
        }
        "browser.sub-browser" => {
            prepare_library();
            w.invoke_select_node(-1);
            if !w.get_sub_browser_open() {
                w.invoke_toggle_sub_browser();
            }
        }
        "browser.sampler" => {
            prepare_library();
            ui.pump(200);
            click_if("browser.rail.sampler");
        }
        "menu.view" => {
            // The View menu lives in the platform menu bar (shown in the
            // window on macOS; elsewhere only its shortcuts work).
            if !cfg!(target_os = "macos") {
                return Err("the View menu is the platform menu bar (macOS only)".into());
            }
            prepare_library();
            ui.pump(200);
            let view = ui.menu_item("View").ok_or("no View menu in the menu bar")?;
            ui.click_el(&view);
        }
        _ if id.starts_with("prefs.") => {
            if kiosk {
                return Err("Preferences are not part of the kiosk".into());
            }
            w.invoke_open_preferences();
        }
        _ => return Err("the app has no such view".into()),
    }
    ui.pump(500);
    Ok(())
}

/// Opens the Preferences page of a `prefs.<page>` view.
fn prefs_window(ui: &Ui, view: &View) -> Option<crate::PreferencesWindow> {
    let w = ui
        .app
        .prefs
        .borrow()
        .window
        .as_ref()
        .map(|w| w.clone_strong())?;
    let page = view.id.trim_start_matches("prefs.");
    w.set_page(page.into());
    Some(w)
}

#[derive(Debug, Default)]
struct ElementResult {
    label: String,
    present: bool,
    failures: Vec<String>,
}

#[derive(Debug, Default)]
struct ViewResult {
    id: String,
    available: bool,
    reason: String,
    elements: Vec<ElementResult>,
    region_failures: Vec<String>,
}

impl ViewResult {
    fn passed(&self) -> bool {
        self.available
            && self.region_failures.is_empty()
            && self
                .elements
                .iter()
                .all(|e| e.present && e.failures.is_empty())
    }
    fn present(&self) -> usize {
        self.elements.iter().filter(|e| e.present).count()
    }
}

fn check_region(
    region: &Region,
    found: &BTreeMap<String, Vec<ElementHandle>>,
    window: &Rect,
    size: &Size,
    out: &mut ViewResult,
) {
    let mut placed: Vec<(String, Rect)> = Vec::new();
    for label in &region.elements {
        let mut r = ElementResult {
            label: label.clone(),
            ..Default::default()
        };
        if let Some(es) = found.get(label) {
            r.present = true;
            if es.len() > 1 {
                r.failures.push(format!("label used {} times", es.len()));
            }
            let e = &es[0];
            let rect = Rect::of(e);
            if rect.w <= 0.5 || rect.h <= 0.5 || e.computed_opacity() <= 0.01 {
                r.failures.push(format!(
                    "not visible ({:.0}×{:.0}, opacity {:.2})",
                    rect.w,
                    rect.h,
                    e.computed_opacity()
                ));
            } else {
                if !window.contains(&rect, 1.0) {
                    r.failures.push(format!(
                        "clipped by the window at ({:.0},{:.0} {:.0}×{:.0})",
                        rect.x, rect.y, rect.w, rect.h
                    ));
                }
                if interactive(e)
                    && (rect.w + 0.5 < size.min_target as f32
                        || rect.h + 0.5 < size.min_target as f32)
                {
                    r.failures.push(format!(
                        "touch target {:.0}×{:.0} < {} px",
                        rect.w, rect.h, size.min_target
                    ));
                }
                placed.push((label.clone(), rect));
            }
        }
        out.elements.push(r);
    }
    // No overlap between siblings of the region.
    for i in 0..placed.len() {
        for j in i + 1..placed.len() {
            if placed[i].1.overlap(&placed[j].1) > 0.0 {
                out.region_failures.push(format!(
                    "{}: `{}` overlaps `{}`",
                    region.id, placed[i].0, placed[j].0
                ));
            }
        }
    }
    // Order along the region's axis (present elements, in list order).
    const TOL: f32 = 1.0;
    for pair in placed.windows(2) {
        let (a, b) = (&pair[0].1, &pair[1].1);
        let ok = match region.order.as_str() {
            "row" => b.x + TOL >= a.x,
            "column" => b.y + TOL >= a.y,
            "grid" => b.y > a.y + TOL || ((b.y - a.y).abs() <= TOL && b.x + TOL >= a.x),
            _ => true,
        };
        if !ok {
            out.region_failures.push(format!(
                "{}: `{}` is not after `{}` ({} order)",
                region.id, pair[1].0, pair[0].0, region.order
            ));
        }
    }
}

fn check_view(view: &View, size: &Size) -> ViewResult {
    let kiosk = size.id == "kiosk";
    let mut res = ViewResult {
        id: view.id.clone(),
        ..Default::default()
    };
    let ui = Ui::start(Fixture::new(), (size.width, size.height), kiosk);
    if let Err(reason) = open_view(&ui, view, kiosk) {
        res.reason = reason;
        for r in &view.regions {
            for l in r.elements.iter().filter(|l| !view.omitted_at(&size.id, l)) {
                res.elements.push(ElementResult {
                    label: l.clone(),
                    ..Default::default()
                });
            }
        }
        return res;
    }
    res.available = true;
    let (found, window) = if view.id.starts_with("prefs.") {
        let Some(p) = prefs_window(&ui, view) else {
            res.available = false;
            res.reason = "Preferences window did not open".into();
            return res;
        };
        let _ = p.show();
        p.window().set_size(slint::LogicalSize::new(
            size.width as f32,
            size.height as f32,
        ));
        ui.pump(400);
        let s = p.window().size();
        (
            labelled_in(&p),
            Rect {
                x: 0.0,
                y: 0.0,
                w: s.width as f32,
                h: s.height as f32,
            },
        )
    } else {
        let (w, h) = ui.size();
        let mut found = ui.labelled();
        if view.id.starts_with("ctx.") || view.id.starts_with("menu.") {
            for (label, title) in MENU_ITEMS {
                if let Some(e) = ui.menu_item(title) {
                    found.insert(label.to_string(), vec![e]);
                }
            }
        }
        (
            found,
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
        )
    };
    for region in &view.regions {
        // Kiosk: elements the controller carries are not on the touch screen.
        let kept: Vec<String> = region
            .elements
            .iter()
            .filter(|l| !view.omitted_at(&size.id, l))
            .cloned()
            .collect();
        if kept.is_empty() {
            continue;
        }
        let region = Region {
            elements: kept,
            ..region.clone()
        };
        check_region(&region, &found, &window, size, &mut res);
    }
    res
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

fn run_size(size_id: &str) {
    let file = views_file();
    let size = file
        .sizes
        .iter()
        .find(|s| s.id == size_id)
        .expect("size in views.toml")
        .clone();
    let mut results = Vec::new();
    for view in file
        .views
        .iter()
        .filter(|v| v.sizes.is_empty() || v.sizes.iter().any(|s| s == size_id))
    {
        results.push(check_view(view, &size));
    }
    // Report: Markdown on stderr, JSON for `parity check`.
    let passing = results.iter().filter(|r| r.passed()).count();
    let mut md = format!(
        "## View completeness at {} ({}×{})\n\n{passing} of {} views pass.\n\n",
        size.id,
        size.width,
        size.height,
        results.len()
    );
    let mut json = format!(
        "{{\"size\":{},\"width\":{},\"height\":{},\"views\":[",
        json_str(&size.id),
        size.width,
        size.height
    );
    for (i, r) in results.iter().enumerate() {
        let required = r.elements.len();
        let status = if r.passed() {
            "PASS"
        } else if r.available {
            "FAIL"
        } else {
            "UNAVAILABLE"
        };
        let _ = writeln!(
            md,
            "- {status} `{}` — {}/{} elements present{}",
            r.id,
            r.present(),
            required,
            if r.reason.is_empty() {
                String::new()
            } else {
                format!(" ({})", r.reason)
            }
        );
        let missing: Vec<&str> = r
            .elements
            .iter()
            .filter(|e| !e.present)
            .map(|e| e.label.as_str())
            .collect();
        if r.available && !missing.is_empty() {
            let _ = writeln!(md, "  - missing: {}", missing.join(", "));
        }
        let mut fails: Vec<String> = r
            .elements
            .iter()
            .flat_map(|e| e.failures.iter().map(move |f| format!("`{}` {f}", e.label)))
            .collect();
        fails.extend(r.region_failures.iter().cloned());
        for f in &fails {
            let _ = writeln!(md, "  - {f}");
        }
        let present: Vec<String> = r
            .elements
            .iter()
            .filter(|e| e.present)
            .map(|e| json_str(&e.label))
            .collect();
        let missing_json: Vec<String> = missing.iter().map(|m| json_str(m)).collect();
        let fails_json: Vec<String> = fails.iter().map(|f| json_str(f)).collect();
        let _ = write!(
            json,
            "{}{{\"id\":{},\"available\":{},\"passed\":{},\"reason\":{},\"required\":{required},\"present\":[{}],\"missing\":[{}],\"failures\":[{}]}}",
            if i > 0 { "," } else { "" },
            json_str(&r.id),
            r.available,
            r.passed(),
            json_str(&r.reason),
            present.join(","),
            missing_json.join(","),
            fails_json.join(",")
        );
    }
    json.push_str("]}\n");
    eprintln!("{md}");
    let dir = workspace_root().join("target/parity");
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join(format!("views-{size_id}.json")), json).unwrap();
    std::fs::write(dir.join(format!("views-{size_id}.md")), md).unwrap();
    if strict() {
        assert_eq!(
            passing,
            results.len(),
            "views failing at {size_id} (OD_PARITY_STRICT=1); see target/parity/views-{size_id}.md"
        );
    }
}

#[test]
fn view_completeness_desktop() {
    run_size("desktop");
}

#[test]
fn view_completeness_laptop() {
    run_size("laptop");
}

#[test]
fn view_completeness_kiosk() {
    run_size("kiosk");
}
