//! Appearance: dark, light or "follow system" (SPEC §12.3).
//!
//! The palette itself lives in `ui/tokens.slint` (`Theme.light` selects the
//! light values). "System" follows the OS through Slint's colour scheme
//! (`Palette.color-scheme`, reported by the windowing backend and updated
//! live). Where Slint cannot tell (`ColorScheme.unknown`, e.g. some Linux
//! desktops), a background probe asks the OS every few seconds:
//! `defaults read -g AppleInterfaceStyle` on macOS, the XDG settings portal
//! (`org.freedesktop.appearance color-scheme`) or
//! `gsettings get org.gnome.desktop.interface color-scheme` on Linux.
//!
//! Rust code that draws its own pixels (the waveform renderer) reads
//! [`is_light`].

use crate::app::{App, handler};
use crate::settings::APPEARANCES;
use slint::ComponentHandle;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

/// The effective appearance of the main window (true = light).
static LIGHT: AtomicBool = AtomicBool::new(false);
/// Whether the OS probe should run (appearance "system" and Slint does not
/// know the scheme).
static PROBE: AtomicBool = AtomicBool::new(false);
/// Last probe result: 0 unknown, 1 dark, 2 light.
static OS_SCHEME: AtomicU8 = AtomicU8::new(0);

const PROBE_INTERVAL: Duration = Duration::from_secs(4);

/// True while the interface is drawn in light mode (paper ground, ink
/// legends). Renderers that paint their own pixels pick their palette from
/// this.
#[allow(dead_code)] // for render.rs (light-mode waveform palette)
pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// The next mode of the top-bar button: dark → light → system → dark.
pub(crate) fn next(current: &str) -> &'static str {
    let i = APPEARANCES.iter().position(|a| *a == current).unwrap_or(0);
    APPEARANCES[(i + 1) % APPEARANCES.len()]
}

/// Parses `defaults read -g AppleInterfaceStyle`: "Dark" in dark mode; the
/// key is missing (empty output, non-zero exit) in light mode.
fn parse_macos(stdout: &str, success: bool) -> bool {
    !(success && stdout.trim().eq_ignore_ascii_case("dark"))
}

/// Parses the portal's answer, e.g. `(<<uint32 1>>,)`: 1 dark, 2 light, 0
/// no preference.
fn parse_portal(stdout: &str) -> Option<bool> {
    let digits: String = stdout
        .chars()
        .rev()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    match digits.chars().rev().collect::<String>().as_str() {
        "1" => Some(false),
        "2" => Some(true),
        _ => None,
    }
}

/// Parses `gsettings get org.gnome.desktop.interface color-scheme`
/// ('prefer-dark', 'prefer-light', 'default').
fn parse_gsettings(stdout: &str) -> Option<bool> {
    let v = stdout.trim().trim_matches('\'');
    match v {
        "prefer-dark" => Some(false),
        "prefer-light" => Some(true),
        _ => None,
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<(String, bool)> {
    let out = std::process::Command::new(cmd)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    Some((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

/// Asks the OS whether it is in light mode; `None` when it cannot tell.
pub(crate) fn probe_os_light() -> Option<bool> {
    if cfg!(target_os = "macos") {
        let (out, ok) = run("defaults", &["read", "-g", "AppleInterfaceStyle"])?;
        return Some(parse_macos(&out, ok));
    }
    let portal = run(
        "gdbus",
        &[
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    )
    .filter(|(_, ok)| *ok)
    .and_then(|(out, _)| parse_portal(&out));
    portal.or_else(|| {
        run(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "color-scheme"],
        )
        .and_then(|(out, _)| parse_gsettings(&out))
    })
}

/// Applies the appearance mode to a window's theme.
pub(crate) fn apply(theme: &crate::Theme<'_>, appearance: &str) {
    theme.set_appearance(appearance.into());
    theme.set_os_light(OS_SCHEME.load(Ordering::Relaxed) == 2);
}

/// Called when the main window's effective appearance changes.
fn changed(app: &App, light: bool) {
    if LIGHT.swap(light, Ordering::Relaxed) != light {
        tracing::debug!(light, "appearance");
        // Re-render the pixel images (waveforms) in the new palette.
        app.refresh_all();
    }
}

/// Periodic check (from the app tick): starts or stops the OS probe and
/// hands its result to the windows.
pub(crate) fn tick(app: &App) {
    let theme = app.ui.global::<crate::Theme>();
    let want = theme.get_appearance() == "system" && !theme.get_system_known();
    PROBE.store(want, Ordering::Relaxed);
    if !want {
        return;
    }
    let os_light = OS_SCHEME.load(Ordering::Relaxed) == 2;
    if theme.get_os_light() != os_light {
        theme.set_os_light(os_light);
        crate::preferences::with_theme(app, |t| t.set_os_light(os_light));
    }
}

/// Sets the mode, stores it and refreshes the Preferences window.
pub(crate) fn set(app: &App, appearance: &str) {
    app.state
        .borrow_mut()
        .change_settings(|s| s.view.appearance = appearance.to_string());
    let a = app.state.borrow().settings.view.appearance.clone();
    apply(&app.ui.global::<crate::Theme>(), &a);
    crate::preferences::with_theme(app, |t| apply(t, &a));
    crate::preferences::refresh(app);
}

pub(crate) fn wire(app: &Rc<App>) {
    // The probe thread: idle unless asked for.
    let _ = std::thread::Builder::new()
        .name("appearance-probe".into())
        .spawn(|| {
            loop {
                if PROBE.load(Ordering::Relaxed) {
                    let v = match probe_os_light() {
                        Some(true) => 2,
                        Some(false) => 1,
                        None => 0,
                    };
                    OS_SCHEME.store(v, Ordering::Relaxed);
                }
                std::thread::sleep(PROBE_INTERVAL);
            }
        });
    app.ui
        .on_appearance_changed(handler!(app, |a, light: bool| changed(&a, light)));
    app.ui.on_cycle_appearance(handler!(app, |a| {
        let cur = a.state.borrow().settings.view.appearance.clone();
        set(&a, next(&cur));
    }));
    LIGHT.store(
        app.ui.global::<crate::Theme>().get_light(),
        Ordering::Relaxed,
    );
    tick(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_dark_light_system() {
        assert_eq!(next("dark"), "light");
        assert_eq!(next("light"), "system");
        assert_eq!(next("system"), "dark");
        assert_eq!(next("bogus"), "light");
    }

    #[test]
    fn parses_os_answers() {
        assert!(!parse_macos("Dark\n", true));
        assert!(parse_macos("", false));
        assert_eq!(parse_portal("(<<uint32 1>>,)\n"), Some(false));
        assert_eq!(parse_portal("(<<uint32 2>>,)\n"), Some(true));
        assert_eq!(parse_portal("(<<uint32 0>>,)\n"), None);
        assert_eq!(parse_gsettings("'prefer-dark'\n"), Some(false));
        assert_eq!(parse_gsettings("'prefer-light'\n"), Some(true));
        assert_eq!(parse_gsettings("'default'\n"), None);
    }
}
