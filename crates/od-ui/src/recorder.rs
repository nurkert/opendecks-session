//! Recorder: recording the master, the tracklist, the recording panel
//! (level, meter, elapsed time, file size, live tracklist; `ui/rec.slint`),
//! automatic start / stop on silence, and the recordings list.

use crate::app::{App, handler};
use crate::models::model;
use crate::{RecTrack, RecordingRow, engine, format};
use od_engine::{Command, EngineState};
use od_recorder::{AutoAction, Recorder, SilenceDetector};
use slint::{ComponentHandle, Model};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

/// Master level (dBFS) that counts as sound for the automatic start / stop.
pub(crate) const SILENCE_DB: f32 = -40.0;
/// Seconds of sound before the automatic start fires.
pub(crate) const START_AFTER: f32 = 2.0;
/// Seconds of silence before the automatic stop fires.
pub(crate) const STOP_AFTER: f32 = 30.0;

pub(crate) struct RecorderState {
    recorder: Option<Recorder>,
    /// Deck that was audible last (tracklist marks).
    last_dominant: Option<usize>,
    /// Start automatically when sound starts (Preferences > Recording).
    armed: bool,
    /// Deck that was audible last (play history), and this run's history
    /// session ("Session 2026-11-07 21:30"), named at the first play.
    last_played: Option<usize>,
    history_session: Option<String>,
    /// Silence detection while armed.
    detector: SilenceDetector,
    /// Last frame (time step of the detector).
    last_frame: Option<Instant>,
    /// The file being written and the tracklist so far (offset, title,
    /// artist) for the panel.
    path: Option<PathBuf>,
    tracks: Vec<(f64, String, String)>,
}

impl Default for RecorderState {
    fn default() -> Self {
        Self {
            recorder: None,
            last_dominant: None,
            armed: false,
            last_played: None,
            history_session: None,
            detector: SilenceDetector::new(SILENCE_DB, START_AFTER, STOP_AFTER),
            last_frame: None,
            path: None,
            tracks: Vec::new(),
        }
    }
}

impl RecorderState {
    pub fn is_recording(&self) -> bool {
        self.recorder.as_ref().is_some_and(|r| r.is_recording())
    }
}

/// Hands the engine's record tap to the recorder (once audio starts).
pub(crate) fn attach(app: &App, tap: od_engine::RecordTap, sample_rate: u32) {
    app.recorder.borrow_mut().recorder = Some(Recorder::new(tap, sample_rate));
    apply_gain(app);
}

/// Recording level from Preferences > Recording (also while recording).
pub(crate) fn apply_gain(app: &App) {
    let db = app.state.borrow().settings.recording.gain_db;
    if let Some(r) = app.recorder.borrow().recorder.as_ref() {
        r.set_gain_db(db);
    }
    let options = crate::settings::REC_GAINS;
    let g = app.ui.global::<crate::Rec>();
    g.set_gain_options(model(
        options
            .iter()
            .map(|g| slint::SharedString::from(gain_text(*g)))
            .collect(),
    ));
    g.set_gain(
        options
            .iter()
            .position(|x| (*x - db).abs() < 0.01)
            .unwrap_or(3) as i32,
    );
}

/// "0 dB", "+3 dB", "−6 dB".
pub(crate) fn gain_text(db: f32) -> String {
    if db == 0.0 {
        "0 dB".into()
    } else if db > 0.0 {
        format!("+{db} dB")
    } else {
        format!("−{} dB", -db)
    }
}

/// The file name of a recording from the pattern (Preferences >
/// Recording): `{date}`, `{time}` and `{n}` (number of the recording that
/// day) from a library timestamp; empty = "Mix {date} {time}".
pub(crate) fn recording_name(pattern: &str, now: &str, n: usize) -> String {
    let t = now.trim_end_matches('Z');
    let (date, time) = t.split_once('T').unwrap_or((t, ""));
    let pattern = if pattern.trim().is_empty() {
        "Mix {date} {time}"
    } else {
        pattern.trim()
    };
    let name = pattern
        .replace("{date}", date)
        .replace("{time}", &time.replace(':', "-"))
        .replace("{n}", &n.to_string());
    let clean: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    if clean.trim().is_empty() {
        mix_name(now)
    } else {
        clean.trim().to_string()
    }
}

/// Meter position (0..=1) of a linear peak: −48 dBFS … 0 dBFS.
pub(crate) fn meter(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() + 48.0) / 48.0).clamp(0.0, 1.0)
}

/// What the silence detection asks for this frame (pure): `armed` =
/// automatic start on, `stop` = automatic stop on.
pub(crate) fn auto_step(
    detector: &mut SilenceDetector,
    peak: f32,
    dt: f32,
    recording: bool,
    armed: bool,
    stop: bool,
) -> Option<AutoAction> {
    if !armed {
        return None;
    }
    let db = if peak > 0.0 {
        20.0 * peak.log10()
    } else {
        -120.0
    };
    match detector.update(db, dt, recording) {
        Some(AutoAction::Stop) if !stop => None,
        a => a,
    }
}

/// Recordings started today in `dir` (for `{n}`).
fn recordings_today(lib: &od_library::Library, now: &str) -> usize {
    let day = now.get(..10).unwrap_or_default().to_string();
    lib.conn()
        .query_row(
            "SELECT count(*) FROM recording WHERE substr(started, 1, 10) = ?1",
            [day],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
}

/// History session name of a performance from a library timestamp:
/// "Session 2026-11-07 21:30".
pub(crate) fn session_name(now: &str) -> String {
    let t = now.trim_end_matches('Z').replace('T', " ");
    format!("Session {}", t.get(..16).unwrap_or(&t))
}

/// Play history: every track that becomes the audible one is logged in this
/// run's session (SPEC §5.5: every performance session is a history list).
fn log_play(app: &App, state: &EngineState) {
    let dom = state.audible.dominant;
    if dom == app.recorder.borrow().last_played {
        return;
    }
    app.recorder.borrow_mut().last_played = dom;
    let Some(d) = dom else { return };
    let id = {
        let decks = app.decks.borrow();
        if decks.source_is_stick() {
            return;
        }
        decks.loaded(d).map(|m| m.id)
    };
    let Some(id) = id else { return };
    if app.state.borrow().read_only() {
        return;
    }
    let session = {
        let mut r = app.recorder.borrow_mut();
        r.history_session
            .get_or_insert_with(|| session_name(&app.state.borrow().lib.now().unwrap_or_default()))
            .clone()
    };
    if app
        .state
        .borrow_mut()
        .lib
        .record_play(id, &session, "opendecks")
        .is_ok()
    {
        crate::browser::refresh_tree(app);
    }
}

/// "Mix 2026-11-06 18-30-00" from a library timestamp.
pub(crate) fn mix_name(now: &str) -> String {
    format!(
        "Mix {}",
        now.replace(':', "-")
            .trim_end_matches('Z')
            .replace('T', " ")
    )
}

/// "2026-11-06 18:30:00  ·  1:02:03" for the recordings list.
pub(crate) fn recording_detail(started: &str, duration: f64) -> String {
    format!(
        "{}  ·  {}",
        started.replace('T', " ").trim_end_matches('Z'),
        format::time(duration)
    )
}

/// Shorter silence detection times (UI tests).
#[cfg(test)]
pub(crate) fn set_silence_times(app: &App, start_after: f32, stop_after: f32) {
    app.recorder.borrow_mut().detector = SilenceDetector::new(SILENCE_DB, start_after, stop_after);
}

/// Arms (or disarms) the automatic start.
pub(crate) fn arm(app: &App, on: bool) {
    app.recorder.borrow_mut().armed = on;
}

/// The extra files a recording format produces after the WAV.
pub(crate) fn conversions(format: &str) -> Vec<&'static str> {
    match format {
        "flac" => vec!["flac"],
        "wav+mp3" => vec!["mp3"],
        _ => Vec::new(),
    }
}

/// After a recording stops: split into tracks and convert, off the UI
/// thread. A FLAC recording replaces its WAV file.
fn finish(
    rec: od_recorder::Recording,
    settings: crate::settings::RecordingSettings,
    library: PathBuf,
    events: crossbeam_channel::Sender<crate::workers::WorkerEvent>,
) {
    use crate::workers::WorkerEvent::Message;
    std::thread::spawn(move || {
        if settings.auto_split {
            match od_recorder::split(&rec) {
                Ok(files) => {
                    let _ = events.send(Message(format!(
                        "Split into {} track files next to the recording",
                        files.len()
                    )));
                }
                Err(e) => {
                    let _ = events.send(Message(format!("Split failed: {e}")));
                }
            }
        }
        for ext in conversions(&settings.format) {
            let out = rec.path.with_extension(ext);
            let result = if ext == "flac" {
                od_recorder::wav_to_flac(&rec.path, &out)
            } else {
                od_recorder::wav_to_mp3(&rec.path, &out)
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, "encoding the recording failed");
                let _ = events.send(Message(
                    "Converting the recording failed — the WAV file is kept".into(),
                ));
                continue;
            }
            if ext == "flac" {
                if let Ok(lib) = od_library::Library::open(&library) {
                    let _ = lib.conn().execute(
                        "UPDATE recording SET path = ?1 WHERE path = ?2",
                        (out.display().to_string(), rec.path.display().to_string()),
                    );
                }
                let _ = std::fs::remove_file(&rec.path);
            }
            let _ = events.send(Message(format!("Recording saved as {}", out.display())));
        }
    });
}

fn toggle(app: &App) {
    if !engine::ensure_engine(app) {
        return;
    }
    if app.recorder.borrow().is_recording() {
        // Stopped by hand: no automatic restart.
        app.recorder.borrow_mut().armed = false;
        stop(app);
    } else {
        start(app);
    }
}

/// Starts a recording of the master into the recordings folder.
fn start(app: &App) {
    let dir = app.state.borrow().settings.recordings_dir();
    let name = {
        let st = app.state.borrow();
        let now = st.lib.now().unwrap_or_default();
        recording_name(
            &st.settings.recording.name_pattern,
            &now,
            recordings_today(&st.lib, &now) + 1,
        )
    };
    engine::send(app, Command::SetRecordTap(true));
    let r = app
        .recorder
        .borrow_mut()
        .recorder
        .as_mut()
        .map(|r| r.start(&dir, &name));
    match r {
        Some(Ok(path)) => {
            let mut rs = app.recorder.borrow_mut();
            rs.path = Some(path);
            rs.tracks.clear();
            rs.last_dominant = None;
        }
        Some(Err(e)) => app.status(format!("Recording could not start: {e}")),
        None => {}
    }
}

/// Stops the recording, stores it and starts the split / conversion.
fn stop(app: &App) {
    engine::send(app, Command::SetRecordTap(false));
    let result = app
        .recorder
        .borrow_mut()
        .recorder
        .as_mut()
        .map(|r| r.stop());
    match result {
        Some(Ok(rec)) => {
            let _ = app.state.borrow().lib.conn().execute(
                "INSERT INTO recording (path, started, duration, tracklist) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2, ?3)",
                (rec.path.display().to_string(), rec.duration, serde_json::to_string(&rec.tracklist).unwrap_or_default()),
            );
            app.status(format!(
                "Recording saved: {} ({} tracks)",
                rec.path.display(),
                rec.tracklist.len()
            ));
            let (settings, library) = {
                let st = app.state.borrow();
                (
                    st.settings.recording.clone(),
                    st.config.library_path.clone(),
                )
            };
            if settings.auto_split || settings.format != "wav" {
                finish(rec, settings, library, app.worker_tx.clone());
            }
            refresh_list(app);
        }
        Some(Err(e)) => app.status(format!("Recording failed: {e}")),
        None => {}
    }
    let mut rs = app.recorder.borrow_mut();
    rs.last_dominant = None;
    rs.path = None;
}

/// Frame tick: automatic start / stop, tracklist marks, play history, the
/// REC clock and the recording panel.
pub(crate) fn frame(app: &App, state: &EngineState) {
    log_play(app, state);
    let now = Instant::now();
    let dt = app
        .recorder
        .borrow_mut()
        .last_frame
        .replace(now)
        .map(|t| (now - t).as_secs_f32().min(0.5))
        .unwrap_or(0.0);
    let (armed, stop_on) = (
        app.recorder.borrow().armed,
        app.state.borrow().settings.recording.silence_stop,
    );
    let recording = app.recorder.borrow().is_recording();
    let action = {
        let mut rs = app.recorder.borrow_mut();
        auto_step(
            &mut rs.detector,
            state.master_meter.peak,
            dt,
            recording,
            armed,
            stop_on,
        )
    };
    match action {
        Some(AutoAction::Start) => {
            start(app);
            app.status("Recording started automatically");
        }
        Some(AutoAction::Stop) => {
            // Stays armed: the next sound starts a new recording.
            stop(app);
        }
        None => {}
    }
    let recording = app.recorder.borrow().is_recording();
    if recording {
        let dom = state.audible.dominant;
        if dom != app.recorder.borrow().last_dominant {
            app.recorder.borrow_mut().last_dominant = dom;
            if let Some(d) = dom {
                let meta = app
                    .decks
                    .borrow()
                    .loaded(d)
                    .map(|m| (m.id, m.title.clone(), m.artist.clone()));
                if let Some((id, title, artist)) = meta {
                    let mut rs = app.recorder.borrow_mut();
                    let elapsed = rs.recorder.as_ref().map(|r| r.elapsed()).unwrap_or(0.0);
                    if let Some(r) = rs.recorder.as_mut() {
                        r.mark_track(Some(id), &title, &artist);
                    }
                    if rs
                        .tracks
                        .last()
                        .is_none_or(|l| l.1 != title || l.2 != artist)
                    {
                        rs.tracks.push((elapsed, title, artist));
                    }
                }
            }
        }
        let elapsed = app
            .recorder
            .borrow()
            .recorder
            .as_ref()
            .map(|r| r.elapsed())
            .unwrap_or(0.0);
        app.ui.set_recording_time(format::deck_time(elapsed).into());
    }
    app.ui.set_recording(recording);
    project_panel(app, state);
}

/// The recording panel (`Rec` global): level, meter, elapsed time, file
/// size and the tracklist so far.
fn project_panel(app: &App, state: &EngineState) {
    let g = app.ui.global::<crate::Rec>();
    let rs = app.recorder.borrow();
    let recording = rs.is_recording();
    let gain = 10f32.powf(app.state.borrow().settings.recording.gain_db / 20.0);
    let peak = if recording {
        rs.recorder
            .as_ref()
            .map(|r| r.level())
            .unwrap_or(0.0)
            .max(state.master_meter.peak * gain)
    } else {
        state.master_meter.peak * gain
    };
    g.set_level(meter(peak));
    g.set_clip(peak >= 1.0);
    g.set_recording(recording);
    g.set_armed(rs.armed);
    let elapsed = rs.recorder.as_ref().map(|r| r.elapsed()).unwrap_or(0.0);
    let new_elapsed: slint::SharedString = if recording {
        format::deck_time(elapsed).into()
    } else {
        slint::SharedString::new()
    };
    if g.get_elapsed() != new_elapsed {
        g.set_elapsed(new_elapsed);
        let size = rs
            .path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        g.set_size(if recording {
            format::bytes(size).into()
        } else {
            slint::SharedString::new()
        });
        g.set_file(
            rs.path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        );
    }
    if g.get_tracks().row_count() != rs.tracks.len()
        || (!recording && rs.tracks.is_empty() && g.get_tracks().row_count() > 0)
    {
        let rows: Vec<RecTrack> = rs
            .tracks
            .iter()
            .enumerate()
            .map(|(i, (t, title, artist))| RecTrack {
                index: i as i32 + 1,
                time: format::deck_time(*t).into(),
                title: title.into(),
                artist: artist.into(),
            })
            .collect();
        g.set_tracks(model(rows));
    }
}

pub(crate) fn refresh_list(app: &App) {
    let st = app.state.borrow();
    let mut rows = Vec::new();
    if let Ok(mut stmt) = st
        .lib
        .conn()
        .prepare("SELECT id, path, started, duration FROM recording ORDER BY started DESC")
    {
        if let Ok(iter) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
            ))
        }) {
            for (id, path, started, duration) in iter.flatten() {
                let name = PathBuf::from(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or(path.clone());
                rows.push(RecordingRow {
                    id: id as i32,
                    name: name.into(),
                    detail: recording_detail(&started, duration).into(),
                    path: path.into(),
                });
            }
        }
    }
    app.ui.set_recordings(model(rows));
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    let armed = app.state.borrow().settings.recording.silence_start;
    arm(app, armed);
    ui.on_toggle_recording(handler!(app, |a| toggle(&a)));
    let g = ui.global::<crate::Rec>();
    g.on_toggle(handler!(app, |a| toggle(&a)));
    g.on_set_gain(handler!(app, |a, i: i32| {
        let db = crate::settings::REC_GAINS
            .get(i.max(0) as usize)
            .copied()
            .unwrap_or(0.0);
        a.state
            .borrow_mut()
            .change_settings(|s| s.recording.gain_db = db);
        apply_gain(&a);
    }));
    g.on_step_gain(handler!(app, |a, d: i32| {
        let gains = crate::settings::REC_GAINS;
        let cur = a.state.borrow().settings.recording.gain_db;
        let i = gains
            .iter()
            .position(|x| (*x - cur).abs() < 0.01)
            .unwrap_or(3) as i32;
        let db = gains[(i + d).clamp(0, gains.len() as i32 - 1) as usize];
        a.state
            .borrow_mut()
            .change_settings(|s| s.recording.gain_db = db);
        apply_gain(&a);
    }));
    apply_gain(app);
    ui.on_reveal_recording(handler!(app, |a, id: i32| {
        let path: Option<String> = a
            .state
            .borrow()
            .lib
            .conn()
            .query_row("SELECT path FROM recording WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .ok();
        if let Some(p) = path {
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            let target = if cfg!(target_os = "macos") {
                vec!["-R".to_string(), p]
            } else {
                vec![
                    PathBuf::from(p)
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default(),
                ]
            };
            let _ = std::process::Command::new(opener).args(target).spawn();
        }
    }));
    ui.on_split_recording(handler!(app, |a, id: i32| {
        let row: Option<(String, f64, String)> = a
            .state
            .borrow()
            .lib
            .conn()
            .query_row(
                "SELECT path, duration, tracklist FROM recording WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((path, duration, tracklist)) = row else {
            return;
        };
        let path = PathBuf::from(path);
        let rec = od_recorder::Recording {
            cue_path: path.with_extension("cue"),
            text_path: path.with_extension("txt"),
            tracklist: serde_json::from_str(&tracklist).unwrap_or_default(),
            duration,
            path,
        };
        match od_recorder::split(&rec) {
            Ok(files) => a.status(format!(
                "Split into {} track files next to the recording",
                files.len()
            )),
            Err(e) => a.status(format!("Split failed: {e}")),
        }
    }));
}

/// Controller REC button.
pub(crate) fn toggle_from_controller(app: &App) {
    toggle(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_convert_with_ffmpeg() {
        assert!(conversions("wav").is_empty());
        assert_eq!(conversions("flac"), vec!["flac"]);
        assert_eq!(conversions("wav+mp3"), vec!["mp3"]);
    }

    #[test]
    fn file_name_pattern() {
        let now = "2026-11-07T21:30:05Z";
        assert_eq!(recording_name("", now, 1), "Mix 2026-11-07 21-30-05");
        assert_eq!(
            recording_name("Club {date} #{n}", now, 3),
            "Club 2026-11-07 #3"
        );
        assert_eq!(recording_name("set/{time}", now, 1), "set-21-30-05");
        assert_eq!(recording_name("   ", now, 1), "Mix 2026-11-07 21-30-05");
        assert_eq!(gain_text(0.0), "0 dB");
        assert_eq!(gain_text(3.0), "+3 dB");
        assert_eq!(gain_text(-6.0), "−6 dB");
    }

    #[test]
    fn meter_scale() {
        assert_eq!(meter(0.0), 0.0);
        assert_eq!(meter(1.0), 1.0);
        assert!((meter(10f32.powf(-24.0 / 20.0)) - 0.5).abs() < 1e-3);
        assert_eq!(meter(4.0), 1.0);
    }

    #[test]
    fn silence_starts_and_stops_only_when_enabled() {
        let mut d = SilenceDetector::new(SILENCE_DB, START_AFTER, STOP_AFTER);
        // Not armed: nothing happens, however loud.
        assert_eq!(auto_step(&mut d, 0.5, 5.0, false, false, true), None);
        // Armed: two seconds of sound start a recording.
        assert_eq!(auto_step(&mut d, 0.5, 1.0, false, true, true), None);
        assert_eq!(
            auto_step(&mut d, 0.5, 1.0, false, true, true),
            Some(AutoAction::Start)
        );
        // Silence while recording: stop after 30 s, only with automatic stop on.
        assert_eq!(auto_step(&mut d, 0.0, 29.0, true, true, true), None);
        assert_eq!(
            auto_step(&mut d, 0.0, 1.0, true, true, true),
            Some(AutoAction::Stop)
        );
        let mut d = SilenceDetector::new(SILENCE_DB, START_AFTER, STOP_AFTER);
        assert_eq!(
            auto_step(&mut d, 0.001, 60.0, true, true, false),
            None,
            "no automatic stop without the setting"
        );
        // Quiet (−60 dB) does not count as sound.
        let mut d = SilenceDetector::new(SILENCE_DB, START_AFTER, STOP_AFTER);
        assert_eq!(auto_step(&mut d, 0.001, 10.0, false, true, true), None);
    }

    #[test]
    fn names_and_details() {
        assert_eq!(mix_name("2026-11-06T18:30:00Z"), "Mix 2026-11-06 18-30-00");
        assert_eq!(
            session_name("2026-11-07T21:30:12Z"),
            "Session 2026-11-07 21:30"
        );
        assert_eq!(
            recording_detail("2026-11-06T18:30:00Z", 3723.0),
            "2026-11-06 18:30:00  ·  1:02:03"
        );
    }
}
