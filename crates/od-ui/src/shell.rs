//! Shell: window set-up, mode and section switches, pane sizes (persisted
//! in the library meta), the status band and key bindings.

use crate::app::{App, handler};
use crate::state::AppConfig;
use crate::{AppWindow, browser, devices, player, recorder};
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
pub(crate) fn setup_window(ui: &AppWindow, config: &AppConfig) {
    if config.language.starts_with("de") {
        if let Err(e) = slint::select_bundled_translation("de") {
            tracing::warn!(error = %e, "German translation unavailable");
        }
        od_i18n::set_language(od_i18n::Language::German);
    }
    ui.set_kiosk(config.kiosk);
    {
        let theme = ui.global::<crate::Theme>();
        theme.set_colorblind_safe(config.colorblind_safe);
        theme.set_high_contrast(config.high_contrast);
        theme.set_motion(config.motion && !config.lite);
        crate::render::set_colorblind_safe(config.colorblind_safe);
    }
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

/// Keyboard: undo / redo, delete, row navigation, preview, hot cues.
fn key(app: &App, text: &str, cmd: bool, shift: bool) -> bool {
    use slint::platform::Key;
    let is = |k: Key| text == SharedString::from(k).as_str();
    if cmd && (text == "z" || text == "Z") {
        if shift { app.redo() } else { app.undo() }
        return true;
    }
    if is(Key::Delete) || is(Key::Backspace) {
        browser::remove_selected(app);
        return true;
    }
    if is(Key::UpArrow) || is(Key::DownArrow) {
        browser::step_selection(app, is(Key::UpArrow));
        return true;
    }
    if text == " " {
        player::toggle_preview(app);
        return true;
    }
    if !cmd && ("1"..="8").contains(&text) && text.len() == 1 {
        let slot = text.parse::<i32>().unwrap_or(1) - 1;
        app.ui.invoke_hot_cue(slot);
        return true;
    }
    false
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
    ui.on_key_pressed(move |text, cmd, shift| {
        let Some(app) = weak.upgrade() else { return false };
        key(&app, text.as_str(), cmd, shift)
    });
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
