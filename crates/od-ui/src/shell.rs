//! Shell: window set-up, mode and section switches, pane sizes (persisted
//! in the library meta), the status band and key bindings.

use crate::app::{App, handler};
use crate::settings::Settings;
use crate::state::AppConfig;
use crate::keymap::{Chord, Context};
use crate::{AppWindow, browser, devices, recorder};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

/// Pane size limits (logical px), shared with the splitters in `app.slint`.
pub const SIDEBAR_MIN: f32 = 180.0;
pub const SIDEBAR_MAX: f32 = 480.0;

const META_NODE: &str = "ui.node";
const META_TRACK: &str = "ui.track";
const META_SECTION: &str = "ui.section";
const META_CDJ: &str = "ui.cdj";
const META_MODE: &str = "ui.mode";
const META_SIDEBAR: &str = "ui.sidebar_width";
const META_UPPER: &str = "ui.upper_height";
const META_LAYOUT: &str = "ui.prepare_layout";

/// Prepare layouts: the player above the browser, 2 players (a compact
/// second player below), dual (two compact players side by side), simple
/// (one compact player) and the full browser.
pub(crate) const PREPARE_LAYOUTS: [&str; 5] = ["player", "two", "dual", "simple", "browser"];

/// Switches the Prepare layout (and to Prepare); unknown names are ignored.
pub(crate) fn set_prepare_layout(app: &App, layout: &str) {
    if !PREPARE_LAYOUTS.contains(&layout) {
        return;
    }
    app.ui.set_prepare_layout(layout.into());
    if app.ui.get_mode() != "prepare" && !app.state.borrow().config.kiosk {
        app.ui.set_mode("prepare".into());
    }
    let _ = app.state.borrow_mut().lib.set_meta(META_LAYOUT, layout);
    crate::player::refresh_minis(app);
}

/// The next Prepare layout (the top-bar switch).
pub(crate) fn next_layout(current: &str) -> &'static str {
    let i = PREPARE_LAYOUTS.iter().position(|l| *l == current).unwrap_or(0);
    PREPARE_LAYOUTS[(i + 1) % PREPARE_LAYOUTS.len()]
}

/// Top-bar panel switches: "mixer", "mpl" (Mix Point Link) or "rec"
/// (recording section / bar). Stored in the settings.
pub(crate) fn toggle_panel(app: &App, which: &str) {
    let on = {
        let p = &app.state.borrow().settings.perform;
        !match which {
            "mixer" => p.mixer_panel,
            "mpl" => p.mpl_panel,
            _ => p.rec_panel,
        }
    };
    app.state.borrow_mut().change_settings(|s| match which {
        "mixer" => s.perform.mixer_panel = on,
        "mpl" => s.perform.mpl_panel = on,
        _ => s.perform.rec_panel = on,
    });
    apply_panels(app);
}

/// Shows the panels as the settings say.
pub(crate) fn apply_panels(app: &App) {
    let p = app.state.borrow().settings.perform.clone();
    app.ui.set_mixer_panel(p.mixer_panel);
    app.ui.set_mpl_panel(p.mpl_panel);
    app.ui.set_rec_panel(p.rec_panel);
}

/// Status band summary of the open list: "12 TRACKS · 1:02:03 · 84.2 MB"
/// and the selection count when more than one row is selected.
pub(crate) fn list_summary(count: usize, seconds: f64, bytes: u64, selected: usize) -> String {
    let mut s = od_i18n::trf("{} TRACKS  ·  {}  ·  {}", &[&count, &crate::format::time(seconds), &crate::format::bytes(bytes)]);
    if selected > 1 {
        s.push_str("  ·  ");
        s.push_str(&od_i18n::trf("{} SELECTED", &[&selected]));
    }
    s
}

/// Theme, language, kiosk and window size.
pub(crate) fn setup_window(ui: &AppWindow, config: &AppConfig, settings: &Settings) {
    apply_language(&settings.language());
    ui.set_kiosk(config.kiosk);
    apply_theme(&ui.global::<crate::Theme>(), settings, config.lite);
    ui.set_show_phrases(settings.view.phrases);
    ui.set_show_phrases_overview(settings.view.phrases_overview);
    apply_browser_style(ui, settings, config.kiosk);
    ui.set_ground_texture(crate::render::ground_texture());
    if config.kiosk {
        ui.set_mode("perform".into());
        // Nothing is loaded yet: start on the browse screen.
        ui.set_browser_open(true);
    }
    match config.window {
        Some((w, h)) => {
            let size = slint::LogicalSize::new(w as f32, h as f32);
            ui.window().set_size(size);
            // The first layout pass may resize the window; apply again once shown.
            let weak = ui.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.window().set_size(size);
                }
            });
        }
        None if config.kiosk => ui.window().set_fullscreen(true),
        None => {}
    }
}

/// Selects the interface language ("de…" German, anything else English).
pub(crate) fn apply_language(lang: &str) {
    if lang.starts_with("de") {
        if let Err(e) = slint::select_bundled_translation("de") {
            tracing::warn!(error = %e, "German translation unavailable");
        }
        od_i18n::set_language(od_i18n::Language::German);
    } else {
        // The source strings are English.
        let _ = slint::select_bundled_translation("en");
        od_i18n::set_language(od_i18n::Language::English);
    }
}

/// Track table text size and row height (Preferences > View > Browser);
/// the kiosk keeps its 44 px touch rows.
pub(crate) fn apply_browser_style(ui: &AppWindow, settings: &Settings, kiosk: bool) {
    let size = settings.browser.font_size as f32;
    ui.global::<crate::TableStyle>().set_text(size);
    let row = if kiosk { 44.0 } else { settings.row_height().max(size + 10.0) };
    ui.global::<crate::Theme>().set_row_height(row);
}

/// Palette, contrast and motion of one window's theme.
pub(crate) fn apply_theme(theme: &crate::Theme<'_>, settings: &Settings, lite: bool) {
    theme.set_colorblind_safe(settings.view.colorblind_safe);
    theme.set_high_contrast(settings.view.high_contrast);
    theme.set_motion(!settings.view.reduced_motion && !lite);
    theme.set_phrase_names(settings.view.phrase_types != "colors");
    crate::appearance::apply(theme, &settings.view.appearance);
    crate::render::set_colorblind_safe(settings.view.colorblind_safe);
    crate::render::set_waveform_style(&settings.view.waveform);
}

/// Parses a stored pane size.
fn pane(v: Option<String>) -> Option<f32> {
    v.and_then(|v| v.parse::<f32>().ok()).filter(|v| v.is_finite())
}

/// Restores the last list, track, section, mode and pane sizes.
pub(crate) fn restore_view(app: &App) {
    let (node, track, section, mode, sidebar, upper) = {
        let st = app.state.borrow();
        let get = |k: &str| st.lib.meta(k).ok().flatten();
        (
            get(META_NODE).and_then(|v| v.parse::<i64>().ok()).map(od_library::NodeId),
            get(META_TRACK).and_then(|v| v.parse::<i64>().ok()).map(od_core::TrackId),
            get(META_SECTION),
            get(META_MODE),
            pane(get(META_SIDEBAR)),
            pane(get(META_UPPER)),
        )
    };
    browser::restore(app, node, track);
    devices::select_default(app);
    if let Some(s) = section {
        app.ui.set_section(s.into());
    }
    if let Some(w) = sidebar {
        app.ui.set_sidebar_width(w.clamp(SIDEBAR_MIN, SIDEBAR_MAX));
    }
    if let Some(h) = upper {
        app.ui.set_upper_height(h.max(0.0));
    }
    if let Some(m) = mode.filter(|_| !app.state.borrow().config.kiosk) {
        app.ui.set_mode(m.into());
    }
    let layout = app.state.borrow().lib.meta(META_LAYOUT).ok().flatten();
    if let Some(l) = layout.filter(|l| PREPARE_LAYOUTS.contains(&l.as_str())) {
        app.ui.set_prepare_layout(l.into());
    }
    let cdj = app.state.borrow().lib.meta(META_CDJ).ok().flatten();
    if cdj.as_deref() == Some("1") {
        crate::cdj::set_enabled(app, true);
    }
}

/// Remembers the view for the next start (best effort).
pub(crate) fn remember_view(app: &App) {
    let mut st = app.state.borrow_mut();
    let node = st.browser.node.map(|n| n.0.to_string()).unwrap_or_else(|| "-1".into());
    let track = st.player.current.map(|t| t.0.to_string()).unwrap_or_default();
    let _ = st.lib.set_meta(META_NODE, &node);
    let _ = st.lib.set_meta(META_TRACK, &track);
    let _ = st.lib.set_meta(META_SECTION, app.ui.get_section().as_str());
    let _ = st.lib.set_meta(META_MODE, app.ui.get_mode().as_str());
    let _ = st.lib.set_meta(META_SIDEBAR, &app.ui.get_sidebar_width().to_string());
    let _ = st.lib.set_meta(META_UPPER, &app.ui.get_upper_height().to_string());
    let cdj = app.ui.global::<crate::Cdj>().get_enabled();
    let _ = st.lib.set_meta(META_CDJ, if cdj { "1" } else { "0" });
}

/// Status band: undo label and analysis progress.
pub(crate) fn refresh_status(app: &App) {
    let st = app.state.borrow();
    let journal = st.lib.journal(1).unwrap_or_default();
    app.ui.set_undo_label(
        journal.first().filter(|j| !j.undone).map(|j| od_i18n::translate_message(&j.label).to_uppercase()).unwrap_or_default().into(),
    );
    match analysis_band(st.analysis) {
        Some((progress, (done, total))) => {
            app.ui.set_analysis_progress(progress);
            app.ui.set_analysis_text(od_i18n::trf("ANALYSING {}/{}", &[&done, &total]).into());
        }
        None => {
            app.ui.set_analysis_progress(0.0);
            app.ui.set_analysis_text(SharedString::new());
        }
    }
}

/// Progress fraction while analysis runs.
fn analysis_band((done, total): (usize, usize)) -> Option<(f32, (usize, usize))> {
    (total > 0 && done < total).then(|| (done as f32 / total as f32, (done, total)))
}

/// Keyboard: every key press goes through the keymap (Preferences >
/// Keyboard); unbound keys are left to the focused element.
fn key(app: &Rc<App>, text: &str, control: bool, meta: bool, alt: bool, shift: bool) -> bool {
    let Some(chord) = Chord::from_event(text, control, meta, alt, shift, app.prefs.borrow().layout) else { return false };
    let mode = if app.ui.get_mode() == "perform" { Context::Perform } else { Context::Prepare };
    let id = app.state.borrow().keymap.lookup(&chord, mode).map(|a| a.id.clone());
    match id {
        Some(id) => {
            tracing::debug!(%chord, id, "key");
            crate::commands::run(app, &id);
            true
        }
        None => false,
    }
}

/// The shortcut a tooltip shows for a keymap action: its bindings in
/// effect, as the platform writes them ("" when unbound or unknown).
pub(crate) fn shortcut_text(keymap: &crate::keymap::Keymap, id: &str) -> String {
    let platform = keymap.platform();
    keymap.bindings(id).iter().map(|c| c.display(platform)).collect::<Vec<_>>().join("  ·  ")
}

/// Tooltips of a window look up the current binding when they open.
pub(crate) fn wire_tooltips(app: &Rc<App>, tips: &crate::TooltipState<'_>) {
    let weak = Rc::downgrade(app);
    tips.on_shortcut_for(move |id| {
        let Some(app) = weak.upgrade() else { return SharedString::new() };
        let st = app.state.borrow();
        shortcut_text(&st.keymap, id.as_str()).into()
    });
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    wire_tooltips(app, &ui.global::<crate::TooltipState>());
    apply_panels(app);
    crate::mpl::wire(app);
    crate::preferences::store_stick_settings(app);
    ui.on_select_section(handler!(app, |a, s: SharedString| {
        a.ui.set_section(s.clone());
        match s.as_str() {
            "devices" => {
                devices::select_default(&a);
                devices::refresh(&a);
            }
            "recordings" => recorder::refresh_list(&a),
            _ => {}
        }
    }));
    ui.on_set_mode(handler!(app, |a, m: SharedString| a.ui.set_mode(m)));
    ui.on_undo(handler!(app, |a| a.undo()));
    ui.on_redo(handler!(app, |a| a.redo()));
    let weak = Rc::downgrade(app);
    ui.on_key_pressed(move |text, control, meta, alt, shift| {
        let Some(app) = weak.upgrade() else { return false };
        key(&app, text.as_str(), control, meta, alt, shift)
    });
    ui.on_key_released(handler!(app, |a, text: SharedString| crate::commands::release(&a, text.as_str())));
    ui.on_open_preferences(handler!(app, |a| crate::preferences::open(&a)));
    ui.on_cycle_layout(handler!(app, |a| {
        if a.ui.get_mode() == "perform" {
            crate::decks::cycle_perform_layout(&a);
            return;
        }
        let next = next_layout(a.ui.get_prepare_layout().as_str());
        set_prepare_layout(&a, next);
    }));
    // Files from Finder / the file manager.
    ui.on_drop_accepts(|data| data.has_file_paths());
    ui.on_files_dropped(handler!(app, |a, data: slint::DataTransfer, target: i32| {
        let paths: Vec<std::path::PathBuf> = data.file_paths().map(|p| p.map(|p| p.to_path_buf()).collect()).unwrap_or_default();
        crate::import::files_dropped(&a, paths, target);
    }));
    crate::import::wire_native_drop(app);
    let weak = Rc::downgrade(app);
    ui.on_menu_action(move |id| {
        if let Some(app) = weak.upgrade() {
            crate::commands::run(&app, id.as_str());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_shortcut_follows_the_keymap() {
        use crate::keymap::{Chord, Keymap, Platform};
        let mut k = Keymap::new(Platform::Mac);
        assert_eq!(shortcut_text(&k, "deck1.play"), "Z");
        assert_eq!(shortcut_text(&k, "prefs.open"), Chord::parse("Mod+,", Platform::Mac).unwrap().display(Platform::Mac));
        let p = Chord::parse("Shift+P", Platform::Mac).unwrap();
        k.set("deck1.play", vec![p.clone()]);
        assert_eq!(shortcut_text(&k, "deck1.play"), p.display(Platform::Mac));
        k.set("deck1.play", Vec::new());
        assert_eq!(shortcut_text(&k, "deck1.play"), "");
        assert_eq!(shortcut_text(&k, "no.such"), "");
    }

    #[test]
    fn layouts_cycle() {
        assert_eq!(next_layout("player"), "two");
        assert_eq!(next_layout("two"), "dual");
        assert_eq!(next_layout("dual"), "simple");
        assert_eq!(next_layout("simple"), "browser");
        assert_eq!(next_layout("browser"), "player");
        assert_eq!(next_layout("unknown"), "two");
    }

    #[test]
    fn list_summary_counts_time_size_and_selection() {
        // Language-neutral checks (other tests may switch the language).
        let s = list_summary(12, 3723.0, 84_200_000, 1);
        assert!(s.starts_with("12 ") && s.contains("1:02:03") && s.ends_with("84 MB"), "{s}");
        let sel = list_summary(3, 60.0, 0, 2);
        assert!(sel.len() > list_summary(3, 60.0, 0, 1).len() && sel.contains(" 2 "), "{sel}");
    }

    #[test]
    fn pane_sizes_parse() {
        assert_eq!(pane(Some("312.5".into())), Some(312.5));
        assert_eq!(pane(Some("NaN".into())), None);
        assert_eq!(pane(Some("wide".into())), None);
        assert_eq!(pane(None), None);
    }

    #[test]
    fn analysis_band_only_while_running() {
        assert_eq!(analysis_band((0, 0)), None);
        assert_eq!(analysis_band((4, 4)), None);
        assert_eq!(analysis_band((1, 4)), Some((0.25, (1, 4))));
    }
}
