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
const META_MODE: &str = "ui.mode";
const META_SIDEBAR: &str = "ui.sidebar_width";
const META_UPPER: &str = "ui.upper_height";

/// Theme, language, kiosk and window size.
pub(crate) fn setup_window(ui: &AppWindow, config: &AppConfig, settings: &Settings) {
    apply_language(&settings.language());
    ui.set_kiosk(config.kiosk);
    apply_theme(&ui.global::<crate::Theme>(), settings, config.lite);
    ui.set_show_phrases(settings.view.phrases);
    if config.kiosk {
        ui.global::<crate::Theme>().set_row_height(44.0);
    }
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

/// Palette, contrast and motion of one window's theme.
pub(crate) fn apply_theme(theme: &crate::Theme<'_>, settings: &Settings, lite: bool) {
    theme.set_colorblind_safe(settings.view.colorblind_safe);
    theme.set_high_contrast(settings.view.high_contrast);
    theme.set_motion(!settings.view.reduced_motion && !lite);
    crate::render::set_colorblind_safe(settings.view.colorblind_safe);
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

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
