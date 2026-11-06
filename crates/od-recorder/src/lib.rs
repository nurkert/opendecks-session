//! Mix recording (SPEC §11): the master output from the engine's record tap
//! is written to a WAV file by a background thread; the tracklist is built
//! from the engine's "audible track" logic and written as a `.cue` sheet and
//! plain text, and the recording can be split into one file per track.

use od_core::TrackId;
use od_engine::RecordTap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("wav error: {0}")]
    Wav(#[from] hound::Error),
    #[error("already recording")]
    Busy,
    #[error("not recording")]
    Idle,
}

pub type Result<T> = std::result::Result<T, RecordError>;

/// One tracklist entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TracklistEntry {
    /// Seconds from the start of the recording.
    pub offset: f64,
    pub track: Option<TrackId>,
    pub title: String,
    pub artist: String,
}

/// A finished recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub path: PathBuf,
    pub duration: f64,
    pub tracklist: Vec<TracklistEntry>,
    pub cue_path: PathBuf,
    pub text_path: PathBuf,
}

struct Active {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    thread: JoinHandle<Result<()>>,
    tracklist: Vec<TracklistEntry>,
}

/// Owns the record tap for the lifetime of the app and records on demand.
pub struct Recorder {
    tap: Arc<parking::Tap>,
    sample_rate: u32,
    active: Option<Active>,
}

mod parking {
    use od_engine::RecordTap;
    use std::sync::Mutex;

    /// The tap moves between recording threads; only one uses it at a time.
    pub struct Tap(pub Mutex<RecordTap>);
}

impl Recorder {
    pub fn new(tap: RecordTap, sample_rate: u32) -> Self {
        Self { tap: Arc::new(parking::Tap(std::sync::Mutex::new(tap))), sample_rate, active: None }
    }

    pub fn is_recording(&self) -> bool {
        self.active.is_some()
    }

    /// Seconds recorded so far.
    pub fn elapsed(&self) -> f64 {
        self.active
            .as_ref()
            .map(|a| a.frames.load(Ordering::Relaxed) as f64 / self.sample_rate as f64)
            .unwrap_or(0.0)
    }

    /// Starts writing `dir/<name>.wav` (24-bit stereo). The caller must also
    /// enable the engine's record tap.
    pub fn start(&mut self, dir: &Path, name: &str) -> Result<PathBuf> {
        if self.active.is_some() {
            return Err(RecordError::Busy);
        }
        std::fs::create_dir_all(dir)?;
        let path = unique_path(dir, name, "wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: self.sample_rate,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec)?;
        let stop = Arc::new(AtomicBool::new(false));
        let frames = Arc::new(AtomicU64::new(0));
        let tap = self.tap.clone();
        let (stop2, frames2) = (stop.clone(), frames.clone());
        // Discard whatever accumulated before the start.
        {
            let mut t = tap.0.lock().unwrap_or_else(|e| e.into_inner());
            let mut scratch = Vec::new();
            t.drain_into(&mut scratch);
        }
        let thread = std::thread::Builder::new().name("od-recorder".into()).spawn(move || -> Result<()> {
            let mut buf: Vec<f32> = Vec::with_capacity(48_000);
            loop {
                let done = stop2.load(Ordering::Relaxed);
                buf.clear();
                tap.0.lock().unwrap_or_else(|e| e.into_inner()).drain_into(&mut buf);
                for s in &buf {
                    writer.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0) as i32)?;
                }
                frames2.fetch_add(buf.len() as u64 / 2, Ordering::Relaxed);
                if done {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            writer.finalize()?;
            Ok(())
        })?;
        self.active = Some(Active { path: path.clone(), stop, frames, thread, tracklist: Vec::new() });
        Ok(path)
    }

    /// Records that `track` became the audible track now.
    pub fn mark_track(&mut self, track: Option<TrackId>, title: &str, artist: &str) {
        let elapsed = self.elapsed();
        let Some(a) = self.active.as_mut() else { return };
        if a.tracklist.last().is_some_and(|l| l.track == track && track.is_some()) {
            return;
        }
        a.tracklist.push(TracklistEntry { offset: elapsed, track, title: title.into(), artist: artist.into() });
    }

    /// Stops the recording and writes the tracklist files.
    pub fn stop(&mut self) -> Result<Recording> {
        let a = self.active.take().ok_or(RecordError::Idle)?;
        a.stop.store(true, Ordering::Relaxed);
        a.thread.join().map_err(|_| std::io::Error::other("recorder thread panicked"))??;
        let duration = a.frames.load(Ordering::Relaxed) as f64 / self.sample_rate as f64;
        let tracklist: Vec<TracklistEntry> = a.tracklist.into_iter().filter(|e| e.track.is_some()).collect();
        let cue_path = a.path.with_extension("cue");
        let text_path = a.path.with_extension("txt");
        std::fs::write(&cue_path, cue_sheet(&a.path, &tracklist))?;
        std::fs::write(&text_path, text_tracklist(&tracklist))?;
        Ok(Recording { path: a.path, duration, tracklist, cue_path, text_path })
    }
}

fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{name}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{name} ({n}).{ext}"));
        n += 1;
    }
    p
}

fn cue_time(secs: f64) -> String {
    // CUE frames are 1/75 s.
    let total_frames = (secs.max(0.0) * 75.0).round() as u64;
    format!("{:02}:{:02}:{:02}", total_frames / 75 / 60, total_frames / 75 % 60, total_frames % 75)
}

fn escape(s: &str) -> String {
    s.replace('"', "'")
}

/// A CUE sheet for the recording.
pub fn cue_sheet(audio: &Path, tracklist: &[TracklistEntry]) -> String {
    let file = audio.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = format!("PERFORMER \"OpenDecks Session\"\nTITLE \"{}\"\nFILE \"{}\" WAVE\n", escape(&file), escape(&file));
    for (i, e) in tracklist.iter().enumerate() {
        s.push_str(&format!(
            "  TRACK {:02} AUDIO\n    TITLE \"{}\"\n    PERFORMER \"{}\"\n    INDEX 01 {}\n",
            i + 1,
            escape(&e.title),
            escape(&e.artist),
            cue_time(if i == 0 { 0.0 } else { e.offset })
        ));
    }
    s
}

/// Plain tracklist in the "MM:SS Artist - Title" form used by Mixcloud and
/// SoundCloud descriptions.
pub fn text_tracklist(tracklist: &[TracklistEntry]) -> String {
    tracklist
        .iter()
        .map(|e| {
            let t = e.offset.max(0.0).round() as u64;
            let stamp = if t >= 3600 { format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60) } else { format!("{:02}:{:02}", t / 60, t % 60) };
            if e.artist.is_empty() { format!("{stamp} {}", e.title) } else { format!("{stamp} {} - {}", e.artist, e.title) }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Splits a recording into one WAV per tracklist entry next to it, in a
/// folder named after the recording. Returns the written files.
pub fn split(recording: &Recording) -> Result<Vec<PathBuf>> {
    let mut reader = hound::WavReader::open(&recording.path)?;
    let spec = reader.spec();
    let rate = spec.sample_rate as f64;
    let dir = recording.path.with_extension("");
    std::fs::create_dir_all(&dir)?;
    let samples: Vec<i32> = reader.samples::<i32>().collect::<std::result::Result<_, _>>()?;
    let frames = samples.len() / spec.channels as usize;
    let mut out = Vec::new();
    for (i, e) in recording.tracklist.iter().enumerate() {
        let start = if i == 0 { 0 } else { (e.offset * rate) as usize };
        let end = recording.tracklist.get(i + 1).map(|n| (n.offset * rate) as usize).unwrap_or(frames).min(frames);
        if end <= start {
            continue;
        }
        let name: String = format!("{:02} {} - {}", i + 1, e.artist, e.title)
            .chars()
            .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
            .collect();
        let path = dir.join(format!("{name}.wav"));
        let mut w = hound::WavWriter::create(&path, spec)?;
        for s in &samples[start * spec.channels as usize..end * spec.channels as usize] {
            w.write_sample(*s)?;
        }
        w.finalize()?;
        out.push(path);
    }
    Ok(out)
}

/// Re-export for callers that only need the tap type.
pub type Tap = RecordTap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_and_text_format() {
        let tl = vec![
            TracklistEntry { offset: 0.4, track: Some(TrackId(1)), title: "One".into(), artist: "A".into() },
            TracklistEntry { offset: 3725.5, track: Some(TrackId(2)), title: "Two \"x\"".into(), artist: "B".into() },
        ];
        let cue = cue_sheet(Path::new("/x/mix.wav"), &tl);
        assert!(cue.contains("INDEX 01 00:00:00"));
        assert!(cue.contains("INDEX 01 62:05:38"));
        assert!(cue.contains("TITLE \"Two 'x'\""));
        assert_eq!(text_tracklist(&tl), "00:00 A - One\n1:02:06 B - Two \"x\"\n");
    }

    #[test]
    fn records_engine_output_and_splits() {
        use od_engine::{Command, Engine, EngineConfig, Outputs};
        let (mut engine, mut proc) = Engine::new(EngineConfig { sample_rate: 48_000, max_block: 256, decks: 2, ..Default::default() });
        let tap = engine.take_record_tap().unwrap();
        engine.send(Command::SetRecordTap(true)).unwrap();
        let mut rec = Recorder::new(tap, 48_000);
        let dir = tempfile::tempdir().unwrap();
        rec.start(dir.path(), "mix").unwrap();
        rec.mark_track(Some(TrackId(1)), "One", "A");
        // Wait until the writer thread has caught up with `frames` rendered
        // frames (CI machines can be slow; never rely on fixed sleeps).
        let wait_for = |rec: &Recorder, frames: usize| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while rec.elapsed() * 48_000.0 < frames as f64 && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let mut master = vec![0.0; 512];
        for i in 0..400 {
            proc.process(256, &mut Outputs { master: &mut master, headphones: None, booth: None });
            if i % 50 == 49 {
                wait_for(&rec, (i + 1) * 256);
            }
            if i == 199 {
                rec.mark_track(Some(TrackId(2)), "Two", "B");
            }
        }
        wait_for(&rec, 400 * 256);
        let r = rec.stop().unwrap();
        assert!(r.duration > 1.0, "duration {}", r.duration);
        assert_eq!(r.tracklist.len(), 2);
        assert!(r.cue_path.exists() && r.text_path.exists());
        let parts = split(&r).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(matches!(rec.stop(), Err(RecordError::Idle)));
    }
}
