//! Updates (SPEC §14.1): the check at start and once a day, the notice bar
//! under the top bar, install with progress and the restart.
//!
//! The work happens in [`od_update`] on a background thread; this module
//! schedules it, never while a deck plays in Perform, and projects its state
//! into the bar (`ui/update.slint`) and Preferences > Advanced.

use crate::app::{App, handler};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_update::{Policy, Prepared, Update, Updater};
use slint::SharedString;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// First check after start.
const FIRST_CHECK: Duration = Duration::from_secs(10);
/// Then once a day while the app runs.
const INTERVAL: Duration = Duration::from_secs(24 * 3600);
/// A check due while a deck plays in Perform waits this long.
const DEFER: Duration = Duration::from_secs(60);

/// What the update machinery is doing.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Checking,
    /// A newer version; the bar offers to install it.
    Available(Update),
    /// Downloading / verifying / staging: bytes done, total.
    Installing(Update, u64, Option<u64>),
    /// Staged; finishes when the app restarts.
    Ready(Update, od_update::Finish),
    /// The DMG or release page was opened, or the install failed.
    Message(String, MessageKind, Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageKind {
    Info,
    Error,
}

enum Msg {
    Checked { manual: bool, result: Result<Option<Update>, String> },
    Progress(u64, Option<u64>),
    Prepared(Result<Prepared, String>),
}

pub(crate) struct UpdateRuntime {
    pub phase: Phase,
    next_check: Instant,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

impl Default for UpdateRuntime {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self { phase: Phase::Idle, next_check: Instant::now() + FIRST_CHECK, tx, rx, cancel: Arc::new(AtomicBool::new(false)) }
    }
}

/// Whether a scheduled check runs now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    Run,
    /// Due, but a deck plays in Perform: try again later.
    Defer,
    /// Not due, switched off, or busy with an update.
    Wait,
}

/// The scheduling rule (pure).
pub(crate) fn gate(auto: bool, kiosk: bool, due: bool, idle: bool, performing: bool) -> Gate {
    if !auto || kiosk || !due || !idle {
        Gate::Wait
    } else if performing {
        Gate::Defer
    } else {
        Gate::Run
    }
}

/// Restarting by itself is fine only when nothing is going on.
pub(crate) fn may_restart_now(mode_perform: bool, playing: bool, recording: bool) -> bool {
    !mode_perform && !playing && !recording
}

/// "Last checked" for Preferences: relative, no time zone needed.
pub(crate) fn ago(now: i64, then: i64) -> String {
    if then <= 0 {
        return od_i18n::tr("never").to_string();
    }
    let s = (now - then).max(0);
    match s {
        0..60 => od_i18n::tr("just now").to_string(),
        60..3600 => od_i18n::trf("{} min ago", &[&(s / 60)]),
        3600..86_400 => od_i18n::trf("{} h ago", &[&(s / 3600)]),
        _ => od_i18n::trf("{} days ago", &[&(s / 86_400)]),
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The bar: state ("" hides it), text, progress.
pub(crate) fn bar(phase: &Phase) -> (&'static str, String, f32) {
    match phase {
        Phase::Idle | Phase::Checking => ("", String::new(), 0.0),
        Phase::Available(u) => {
            let mut t = od_i18n::trf("OpenDecks Session {} is available", &[&u.version]);
            if u.prerelease {
                t.push_str(&od_i18n::tr(" (pre-release)"));
            }
            ("available", t, 0.0)
        }
        Phase::Installing(u, done, total) => {
            let p = total.filter(|t| *t > 0).map(|t| (*done as f64 / t as f64).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
            let t = match total {
                Some(t) if *done >= *t => od_i18n::trf("Installing OpenDecks Session {} …", &[&u.version]),
                Some(_) => od_i18n::trf("Downloading OpenDecks Session {} — {} %", &[&u.version, &((p * 100.0).round() as u32)]),
                None => od_i18n::trf("Downloading OpenDecks Session {} …", &[&u.version]),
            };
            ("installing", t, p)
        }
        Phase::Ready(u, _) => ("ready", od_i18n::trf("OpenDecks Session {} is ready — restart to finish", &[&u.version]), 1.0),
        Phase::Message(t, MessageKind::Info, _) => ("info", t.clone(), 0.0),
        Phase::Message(t, MessageKind::Error, _) => ("error", t.clone(), 0.0),
    }
}

fn cache_dir() -> PathBuf {
    od_library::data_dir().join("updates")
}

fn policy(app: &App, manual: bool) -> Policy {
    let s = &app.state.borrow().settings.update;
    // Asking by hand shows a skipped version again.
    Policy { include_prereleases: s.prereleases, skip: (!manual && !s.skip_version.is_empty()).then(|| s.skip_version.clone()) }
}

fn set_phase(app: &App, phase: Phase) {
    app.update.borrow_mut().phase = phase;
    refresh(app);
}

/// Pushes the state into the bar.
pub(crate) fn refresh(app: &App) {
    let (state, text, progress) = bar(&app.update.borrow().phase);
    app.ui.set_update_state(state.into());
    app.ui.set_update_text(text.into());
    app.ui.set_update_progress(progress);
}

/// Preferences > Advanced: the last check.
pub(crate) fn last_checked_text(app: &App) -> String {
    let st = app.state.borrow();
    let checking = matches!(app.update.borrow().phase, Phase::Checking);
    if checking {
        return od_i18n::tr("Checking …").to_string();
    }
    od_i18n::trf("Version {} · last checked {}", &[&env!("CARGO_PKG_VERSION"), &ago(unix_now(), st.settings.update.last_check)])
}

/// Starts a check on a background thread.
pub(crate) fn check_now(app: &App, manual: bool) {
    let busy = matches!(app.update.borrow().phase, Phase::Checking | Phase::Installing(..) | Phase::Ready(..));
    if busy {
        if manual {
            app.status("An update is already in progress");
        }
        return;
    }
    {
        let mut u = app.update.borrow_mut();
        u.phase = Phase::Checking;
        u.next_check = Instant::now() + INTERVAL;
    }
    if manual {
        app.status("Checking for updates …");
    }
    let policy = policy(app, manual);
    let tx = app.update.borrow().tx.clone();
    let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let result = Updater::new(cache_dir()).check(&policy).map_err(|e| e.to_string());
        let _ = tx.send(Msg::Checked { manual, result });
    });
    if spawned.is_err() {
        app.update.borrow_mut().phase = Phase::Idle;
    }
    refresh(app);
    crate::preferences::refresh_updates(app);
}

fn install(app: &App) {
    let update = match &app.update.borrow().phase {
        Phase::Available(u) => u.clone(),
        _ => return,
    };
    let (tx, cancel) = {
        let u = app.update.borrow();
        u.cancel.store(false, Ordering::Relaxed);
        (u.tx.clone(), u.cancel.clone())
    };
    set_phase(app, Phase::Installing(update.clone(), 0, update.asset.as_ref().map(|a| a.size).filter(|s| *s > 0)));
    let _ = std::thread::Builder::new().name("update-install".into()).spawn(move || {
        let updater = Updater::new(cache_dir());
        let ptx = tx.clone();
        let mut last = Instant::now() - Duration::from_secs(1);
        let r = updater
            .prepare(
                &update,
                &mut |done, total| {
                    // A few progress messages per second are plenty.
                    if last.elapsed() > Duration::from_millis(150) || total.is_some_and(|t| done >= t) {
                        last = Instant::now();
                        let _ = ptx.send(Msg::Progress(done, total));
                    }
                },
                &cancel,
            )
            .map_err(|e| e.to_string());
        let _ = tx.send(Msg::Prepared(r));
    });
}

/// Quits so the helper can swap the versions and start the new one.
fn restart(app: &App) {
    let finish = match &app.update.borrow().phase {
        Phase::Ready(_, f) => f.clone(),
        _ => return,
    };
    match Updater::new(cache_dir()).finish(&finish) {
        Ok(()) => {
            tracing::info!("update staged, restarting");
            let _ = slint::quit_event_loop();
        }
        Err(e) => set_phase(app, Phase::Message(od_i18n::trf("Update failed: {} — this version keeps working", &[&e]), MessageKind::Error, None)),
    }
}

fn open_page(url: &str) {
    if let Err(e) = od_update::install::open(url.as_ref()) {
        tracing::warn!(error = %e, "could not open the release page");
    }
}

/// A button of the bar.
fn act(app: &App, what: &str) {
    let phase = app.update.borrow().phase.clone();
    match (what, &phase) {
        ("install", Phase::Available(_)) => install(app),
        ("notes", Phase::Available(u)) => open_page(&u.page_url),
        ("later", Phase::Available(_)) => set_phase(app, Phase::Idle),
        ("skip", Phase::Available(u)) => {
            let v = u.version.to_string();
            app.state.borrow_mut().change_settings(|s| s.update.skip_version = v.clone());
            app.status(od_i18n::trf("Version {} skipped — newer versions are offered again", &[&v]));
            set_phase(app, Phase::Idle);
        }
        ("cancel", Phase::Installing(..)) => app.update.borrow().cancel.store(true, Ordering::Relaxed),
        ("restart", Phase::Ready(..)) => restart(app),
        ("later", Phase::Ready(..)) => app.status("The update is installed when you restart"),
        ("page", Phase::Message(_, _, Some(url))) => open_page(url),
        ("close", Phase::Message(..)) => set_phase(app, Phase::Idle),
        _ => tracing::debug!(what, "update action ignored in {phase:?}"),
    }
}

fn playing(app: &App) -> bool {
    crate::engine::snapshot(app).is_some_and(|s| s.decks.iter().any(|d| d.playing))
}

/// Periodic work (from the app tick): results of the worker and the
/// daily schedule.
pub(crate) fn poll(app: &App) {
    let msgs: Vec<Msg> = app.update.borrow().rx.try_iter().collect();
    for m in msgs {
        receive(app, m);
    }
    let (due, idle) = {
        let u = app.update.borrow();
        (Instant::now() >= u.next_check, matches!(u.phase, Phase::Idle | Phase::Message(..)))
    };
    if !due {
        return;
    }
    let (auto, kiosk) = {
        let st = app.state.borrow();
        (st.settings.update.auto_check, st.config.kiosk)
    };
    let performing = app.ui.get_mode() == "perform" && playing(app);
    match gate(auto, kiosk, due, idle, performing) {
        Gate::Run => check_now(app, false),
        Gate::Defer => app.update.borrow_mut().next_check = Instant::now() + DEFER,
        // Busy with an update or switched off: look again in a minute.
        Gate::Wait => app.update.borrow_mut().next_check = Instant::now() + DEFER,
    }
}

fn receive(app: &App, m: Msg) {
    match m {
        Msg::Checked { manual, result } => {
            match result {
                Ok(found) => {
                    app.state.borrow_mut().change_settings(|s| s.update.last_check = unix_now());
                    match found {
                        Some(u) => {
                            tracing::info!(version = %u.version, strategy = ?u.strategy, "update available");
                            set_phase(app, Phase::Available(u));
                        }
                        None => {
                            set_phase(app, Phase::Idle);
                            if manual {
                                app.status(od_i18n::trf("OpenDecks Session {} is up to date", &[&env!("CARGO_PKG_VERSION")]));
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::info!(error = %e, "update check failed");
                    set_phase(app, Phase::Idle);
                    if manual {
                        app.status(od_i18n::trf("Could not check for updates: {}", &[&e]));
                    }
                }
            }
            crate::preferences::refresh_updates(app);
        }
        Msg::Progress(done, total) => {
            let mut u = app.update.borrow_mut();
            if let Phase::Installing(up, _, _) = &u.phase {
                u.phase = Phase::Installing(up.clone(), done, total.or(up.asset.as_ref().map(|a| a.size).filter(|s| *s > 0)));
            }
            drop(u);
            refresh(app);
        }
        Msg::Prepared(r) => {
            let update = match &app.update.borrow().phase {
                Phase::Installing(u, _, _) => u.clone(),
                _ => return,
            };
            match r {
                Ok(Prepared::Restart(f)) => {
                    set_phase(app, Phase::Ready(update, f));
                    let recording = app.recorder.borrow().is_recording();
                    if may_restart_now(app.ui.get_mode() == "perform", playing(app), recording) {
                        restart(app);
                    }
                }
                Ok(Prepared::OpenedDmg(_)) => set_phase(
                    app,
                    Phase::Message(
                        od_i18n::trf("OpenDecks Session {} is verified and open — drag it to Applications, then restart", &[&update.version]),
                        MessageKind::Info,
                        None,
                    ),
                ),
                Ok(Prepared::OpenedPage(url)) => set_phase(
                    app,
                    Phase::Message(od_i18n::trf("This copy cannot update itself — download OpenDecks Session {} from the release page", &[&update.version]), MessageKind::Info, Some(url)),
                ),
                Err(e) if e == "cancelled" => set_phase(app, Phase::Available(update)),
                Err(e) => {
                    tracing::warn!(error = %e, "update failed");
                    set_phase(app, Phase::Message(od_i18n::trf("Update failed: {} — this version keeps working", &[&e]), MessageKind::Error, Some(update.page_url)));
                }
            }
        }
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    app.ui.on_update_action(handler!(app, |a, what: SharedString| act(&a, what.as_str())));
    app.ui.on_check_updates(handler!(app, |a| check_now(&a, true)));
    refresh(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_update::{Strategy, Version};

    fn update(pre: bool) -> Update {
        Update {
            version: Version::new(0, 2, 0),
            tag: "v0.2.0".into(),
            prerelease: pre,
            notes: String::new(),
            page_url: "https://github.com/nurkert/opendecks-session/releases/tag/v0.2.0".into(),
            asset: None,
            sums: None,
            strategy: Strategy::OpenReleasePage,
        }
    }

    #[test]
    fn schedule_never_interrupts_a_performance() {
        assert_eq!(gate(true, false, true, true, false), Gate::Run);
        assert_eq!(gate(true, false, true, true, true), Gate::Defer);
        // Switched off, kiosk, not due, or busy with an update.
        assert_eq!(gate(false, false, true, true, false), Gate::Wait);
        assert_eq!(gate(true, true, true, true, false), Gate::Wait);
        assert_eq!(gate(true, false, false, true, false), Gate::Wait);
        assert_eq!(gate(true, false, true, false, false), Gate::Wait);
        assert!(may_restart_now(false, false, false));
        assert!(!may_restart_now(true, false, false));
        assert!(!may_restart_now(false, true, false));
        assert!(!may_restart_now(false, false, true));
    }

    #[test]
    fn bar_texts_per_phase() {
        od_i18n::set_language(od_i18n::Language::English);
        assert_eq!(bar(&Phase::Idle).0, "");
        assert_eq!(bar(&Phase::Checking).0, "");
        assert_eq!(bar(&Phase::Available(update(false))), ("available", "OpenDecks Session 0.2.0 is available".into(), 0.0));
        assert_eq!(bar(&Phase::Available(update(true))).1, "OpenDecks Session 0.2.0 is available (pre-release)");
        let (s, t, p) = bar(&Phase::Installing(update(false), 25, Some(100)));
        assert_eq!((s, t.as_str(), p), ("installing", "Downloading OpenDecks Session 0.2.0 — 25 %", 0.25));
        assert_eq!(bar(&Phase::Installing(update(false), 100, Some(100))).1, "Installing OpenDecks Session 0.2.0 …");
        assert_eq!(bar(&Phase::Installing(update(false), 5, None)).2, 0.0);
        let f = od_update::Finish { mode: "wait", current: PathBuf::new(), new: PathBuf::new(), relaunch: vec![] };
        assert_eq!(bar(&Phase::Ready(update(false), f)).0, "ready");
        assert_eq!(bar(&Phase::Message("x".into(), MessageKind::Error, None)).0, "error");
        assert_eq!(bar(&Phase::Message("x".into(), MessageKind::Info, None)).0, "info");
    }

    #[test]
    fn bar_texts_have_german() {
        for s in [
            "OpenDecks Session {} is available",
            " (pre-release)",
            "Installing OpenDecks Session {} …",
            "Downloading OpenDecks Session {} — {} %",
            "Downloading OpenDecks Session {} …",
            "OpenDecks Session {} is ready — restart to finish",
            "Update failed: {} — this version keeps working",
            "OpenDecks Session {} is up to date",
            "Could not check for updates: {}",
            "Checking for updates …",
            "An update is already in progress",
            "Version {} skipped — newer versions are offered again",
            "The update is installed when you restart",
            "OpenDecks Session {} is verified and open — drag it to Applications, then restart",
            "This copy cannot update itself — download OpenDecks Session {} from the release page",
            "Version {} · last checked {}",
            "Checking …",
            "never",
            "just now",
            "{} min ago",
            "{} h ago",
            "{} days ago",
        ] {
            assert!(od_i18n::has_german(s), "no German for {s:?}");
        }
    }

    #[test]
    fn last_checked_is_relative() {
        od_i18n::set_language(od_i18n::Language::English);
        assert_eq!(ago(1000, 0), "never");
        assert_eq!(ago(1000, 990), "just now");
        assert_eq!(ago(10_000, 10_000 - 150), "2 min ago");
        assert_eq!(ago(100_000, 100_000 - 7200), "2 h ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3 days ago");
        // A clock that went backwards.
        assert_eq!(ago(1000, 2000), "just now");
    }
}
