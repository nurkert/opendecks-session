//! Recorder: recording the master, the tracklist, and the recordings list.

use crate::app::{App, handler};
use crate::models::model;
use crate::{RecordingRow, engine, format};
use od_engine::{Command, EngineState};
use od_recorder::Recorder;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct RecorderState {
    recorder: Option<Recorder>,
    /// Deck that was audible last (tracklist marks).
    last_dominant: Option<usize>,
    /// Start automatically when sound starts (Preferences > Recording).
    armed: bool,
}

impl RecorderState {
    pub fn is_recording(&self) -> bool {
        self.recorder.as_ref().is_some_and(|r| r.is_recording())
    }
}

/// Hands the engine's record tap to the recorder (once audio starts).
pub(crate) fn attach(app: &App, tap: od_engine::RecordTap, sample_rate: u32) {
    app.recorder.borrow_mut().recorder = Some(Recorder::new(tap, sample_rate));
}

/// "Mix 2026-11-06 18-30-00" from a library timestamp.
pub(crate) fn mix_name(now: &str) -> String {
    format!("Mix {}", now.replace(':', "-").trim_end_matches('Z').replace('T', " "))
}

/// "2026-11-06 18:30:00  ·  1:02:03" for the recordings list.
pub(crate) fn recording_detail(started: &str, duration: f64) -> String {
    format!("{}  ·  {}", started.replace('T', " ").trim_end_matches('Z'), format::time(duration))
}

/// Arms (or disarms) the automatic start.
pub(crate) fn arm(app: &App, on: bool) {
    app.recorder.borrow_mut().armed = on;
}

/// Master level above which the automatic start fires (about −40 dBFS).
const SOUND_THRESHOLD: f32 = 0.01;

/// Whether ffmpeg is available for FLAC / MP3.
pub(crate) fn encoder_available() -> bool {
    std::process::Command::new("ffmpeg").arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// The ffmpeg runs for a recording format: (extension, extra arguments).
pub(crate) fn conversions(format: &str) -> Vec<(&'static str, &'static [&'static str])> {
    match format {
        "flac" => vec![("flac", &["-c:a", "flac"])],
        "wav+mp3" => vec![("mp3", &["-c:a", "libmp3lame", "-b:a", "320k"])],
        _ => Vec::new(),
    }
}

/// After a recording stops: split into tracks and convert, off the UI
/// thread. A FLAC recording replaces its WAV file.
fn finish(rec: od_recorder::Recording, settings: crate::settings::RecordingSettings, library: PathBuf, events: crossbeam_channel::Sender<crate::workers::WorkerEvent>) {
    use crate::workers::WorkerEvent::Message;
    std::thread::spawn(move || {
        if settings.auto_split {
            match od_recorder::split(&rec) {
                Ok(files) => {
                    let _ = events.send(Message(format!("Split into {} track files next to the recording", files.len())));
                }
                Err(e) => {
                    let _ = events.send(Message(format!("Split failed: {e}")));
                }
            }
        }
        for (ext, args) in conversions(&settings.format) {
            let out = rec.path.with_extension(ext);
            let ok = std::process::Command::new("ffmpeg")
                .args(["-y", "-loglevel", "error", "-i"])
                .arg(&rec.path)
                .args(args)
                .arg(&out)
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                let _ = events.send(Message("Converting the recording failed — the WAV file is kept".into()));
                continue;
            }
            if ext == "flac" {
                if let Ok(lib) = od_library::Library::open(&library) {
                    let _ = lib.conn().execute("UPDATE recording SET path = ?1 WHERE path = ?2", (out.display().to_string(), rec.path.display().to_string()));
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
        engine::send(app, Command::SetRecordTap(false));
        let result = app.recorder.borrow_mut().recorder.as_mut().map(|r| r.stop());
        match result {
            Some(Ok(rec)) => {
                let _ = app.state.borrow().lib.conn().execute(
                    "INSERT INTO recording (path, started, duration, tracklist) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2, ?3)",
                    (rec.path.display().to_string(), rec.duration, serde_json::to_string(&rec.tracklist).unwrap_or_default()),
                );
                app.status(format!("Recording saved: {} ({} tracks)", rec.path.display(), rec.tracklist.len()));
                let (settings, library) = {
                    let st = app.state.borrow();
                    (st.settings.recording.clone(), st.config.library_path.clone())
                };
                if settings.auto_split || settings.format != "wav" {
                    finish(rec, settings, library, app.worker_tx.clone());
                }
            }
            Some(Err(e)) => app.status(format!("Recording failed: {e}")),
            None => {}
        }
        app.recorder.borrow_mut().last_dominant = None;
    } else {
        let dir = app.state.borrow().settings.recordings_dir();
        let name = mix_name(&app.state.borrow().lib.now().unwrap_or_default());
        engine::send(app, Command::SetRecordTap(true));
        let r = app.recorder.borrow_mut().recorder.as_mut().map(|r| r.start(&dir, &name));
        if let Some(Err(e)) = r {
            app.status(format!("Recording could not start: {e}"));
        }
    }
}

/// Frame tick: tracklist marks, play history and the REC clock.
pub(crate) fn frame(app: &App, state: &EngineState) {
    let armed = app.recorder.borrow().armed;
    if armed && !app.recorder.borrow().is_recording() && state.master_meter.peak > SOUND_THRESHOLD {
        toggle(app);
        // Still armed for this run: a stop by hand disarms.
        app.recorder.borrow_mut().armed = true;
    }
    let recording = app.recorder.borrow().is_recording();
    if recording {
        let dom = state.audible.dominant;
        if dom != app.recorder.borrow().last_dominant {
            app.recorder.borrow_mut().last_dominant = dom;
            if let Some(d) = dom {
                let meta = app.decks.borrow().loaded(d).map(|m| (m.id, m.title.clone(), m.artist.clone()));
                if let Some((id, title, artist)) = meta {
                    if let Some(r) = app.recorder.borrow_mut().recorder.as_mut() {
                        r.mark_track(Some(id), &title, &artist);
                    }
                    if !app.state.borrow().read_only() {
                        let _ = app.state.borrow_mut().lib.record_play(id, "recording", "opendecks");
                    }
                }
            }
        }
        let elapsed = app.recorder.borrow().recorder.as_ref().map(|r| r.elapsed()).unwrap_or(0.0);
        app.ui.set_recording_time(format::deck_time(elapsed).into());
    }
    app.ui.set_recording(recording);
}

pub(crate) fn refresh_list(app: &App) {
    let st = app.state.borrow();
    let mut rows = Vec::new();
    if let Ok(mut stmt) = st.lib.conn().prepare("SELECT id, path, started, duration FROM recording ORDER BY started DESC") {
        if let Ok(iter) = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, f64>(3)?))) {
            for (id, path, started, duration) in iter.flatten() {
                let name = PathBuf::from(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(path.clone());
                rows.push(RecordingRow { id: id as i32, name: name.into(), detail: recording_detail(&started, duration).into(), path: path.into() });
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
    ui.on_reveal_recording(handler!(app, |a, id: i32| {
        let path: Option<String> = a.state.borrow().lib.conn().query_row("SELECT path FROM recording WHERE id = ?1", [id], |r| r.get(0)).ok();
        if let Some(p) = path {
            let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
            let target = if cfg!(target_os = "macos") { vec!["-R".to_string(), p] } else { vec![PathBuf::from(p).parent().map(|d| d.display().to_string()).unwrap_or_default()] };
            let _ = std::process::Command::new(opener).args(target).spawn();
        }
    }));
    ui.on_split_recording(handler!(app, |a, id: i32| {
        let row: Option<(String, f64, String)> = a
            .state
            .borrow()
            .lib
            .conn()
            .query_row("SELECT path, duration, tracklist FROM recording WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .ok();
        let Some((path, duration, tracklist)) = row else { return };
        let path = PathBuf::from(path);
        let rec = od_recorder::Recording {
            cue_path: path.with_extension("cue"),
            text_path: path.with_extension("txt"),
            tracklist: serde_json::from_str(&tracklist).unwrap_or_default(),
            duration,
            path,
        };
        match od_recorder::split(&rec) {
            Ok(files) => a.status(format!("Split into {} track files next to the recording", files.len())),
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
        assert_eq!(conversions("flac")[0].0, "flac");
        assert_eq!(conversions("wav+mp3")[0].0, "mp3");
    }

    #[test]
    fn names_and_details() {
        assert_eq!(mix_name("2026-11-06T18:30:00Z"), "Mix 2026-11-06 18-30-00");
        assert_eq!(recording_detail("2026-11-06T18:30:00Z", 3723.0), "2026-11-06 18:30:00  ·  1:02:03");
    }
}
