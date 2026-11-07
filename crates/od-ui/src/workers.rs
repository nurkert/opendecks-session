//! Background work with its own library connections: adding music and
//! analysing tracks. Results reach the UI through the library itself (the
//! UI polls the change log) plus small progress messages.

use crossbeam_channel::{Receiver, Sender, unbounded};
use od_core::{DataSource, TrackId};
use od_library::{Library, NewTrack};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum WorkerEvent {
    ImportProgress { done: usize, total: usize },
    ImportFinished { added: usize, failed: usize },
    AnalysisProgress { done: usize, total: usize },
    AnalysisIdle,
    Message(String),
}

/// Collects supported audio files below `root`.
pub fn scan(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in read.flatten() {
            let p = e.path();
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && od_core::AudioFormat::is_supported_extension(&p) => {
                    out.push(p)
                }
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// Adds one audio file to the library (probe, content hash, tags,
/// artwork) and returns its id; an already known path returns its id.
/// Used by ADD MUSIC and by loading a file from the Explorer.
pub fn import_file(lib: &mut Library, path: &Path) -> anyhow::Result<TrackId> {
    if let Some(t) = lib.track_by_path(path)? {
        return Ok(t.id);
    }
    let info = od_decode::probe(path)?;
    let hash = od_decode::content_hash(path)?;
    Ok(lib.add_track(&NewTrack {
        path: path.to_path_buf(),
        content_hash: hash,
        file_size: std::fs::metadata(path)?.len(),
        format: info.format,
        duration: info.duration.unwrap_or(0.0),
        sample_rate: info.sample_rate,
        channels: info.channels,
        bit_rate: info.bit_rate_kbps,
        bits_per_sample: info.bits_per_sample,
        tags: info.tags,
        artwork: info.artwork,
    })?)
}

/// Adds every audio file below `roots` to the library (in a thread).
pub fn spawn_import(
    library_path: PathBuf,
    roots: Vec<PathBuf>,
    events: Sender<WorkerEvent>,
    wake_analysis: Sender<()>,
) {
    std::thread::spawn(move || {
        let mut lib = match Library::open(&library_path) {
            Ok(l) => l,
            Err(e) => {
                let _ = events.send(WorkerEvent::Message(format!("Cannot open library: {e}")));
                return;
            }
        };
        let files: Vec<PathBuf> = roots.iter().flat_map(|r| scan(r)).collect();
        let total = files.len();
        let (mut added, mut failed) = (0, 0);
        for (i, path) in files.iter().enumerate() {
            if lib.track_by_path(path).ok().flatten().is_some() {
                continue;
            }
            let result = import_file(&mut lib, path).map(|_| ());
            match result {
                Ok(()) => added += 1,
                Err(e) => {
                    failed += 1;
                    tracing::warn!(path = %path.display(), error = %e, "import failed");
                }
            }
            if i % 8 == 0 || i + 1 == total {
                let _ = events.send(WorkerEvent::ImportProgress { done: i + 1, total });
            }
            if added % 32 == 1 {
                let _ = wake_analysis.send(());
            }
        }
        let _ = events.send(WorkerEvent::ImportFinished { added, failed });
        let _ = wake_analysis.send(());
    });
}

/// Analysis settings shared with the worker (Preferences > Analysis).
#[derive(Clone, Debug)]
pub struct AnalysisControl {
    pub options: od_analysis::AnalysisOptions,
    pub detect_key: bool,
    /// Analyse every track that needs it; otherwise only `requested`.
    pub auto: bool,
    pub requested: Vec<TrackId>,
}

/// Analyses tracks whose analysis is missing or outdated, forever.
pub struct AnalysisWorker {
    pub wake: Sender<()>,
    pub control: Arc<Mutex<AnalysisControl>>,
    stop: Arc<AtomicBool>,
}

impl AnalysisWorker {
    pub fn start(
        library_path: PathBuf,
        control: AnalysisControl,
        events: Sender<WorkerEvent>,
    ) -> Self {
        let lite = control.options.lite;
        let control = Arc::new(Mutex::new(control));
        let control2 = control.clone();
        let (wake_tx, wake_rx): (Sender<()>, Receiver<()>) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::Builder::new()
            .name("od-analysis".into())
            .spawn(move || {
                let Ok(mut lib) = Library::open(&library_path) else {
                    return;
                };
                let threads = if lite {
                    1
                } else {
                    std::thread::available_parallelism()
                        .map(|n| n.get().saturating_sub(2).max(1))
                        .unwrap_or(2)
                };
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("analysis pool");
                while !stop2.load(Ordering::Relaxed) {
                    let (opts, detect_key, todo) = {
                        let mut c = control2.lock().unwrap_or_else(|e| e.into_inner());
                        let requested = std::mem::take(&mut c.requested);
                        let todo = if c.auto {
                            lib.tracks_needing_analysis(od_analysis::ANALYSIS_VERSION)
                                .unwrap_or_default()
                        } else {
                            requested
                        };
                        (c.options.clone(), c.detect_key, todo)
                    };
                    if todo.is_empty() {
                        let _ = events.send(WorkerEvent::AnalysisIdle);
                        let _ = wake_rx.recv_timeout(Duration::from_secs(30));
                        continue;
                    }
                    let total = todo.len();
                    let mut done = 0;
                    for chunk in todo.chunks(threads * 2) {
                        if stop2.load(Ordering::Relaxed) {
                            return;
                        }
                        let paths: Vec<_> = chunk
                            .iter()
                            .filter_map(|id| lib.track(*id).ok().map(|t| (*id, t.path)))
                            .collect();
                        let results: Vec<_> = pool.install(|| {
                            use rayon::prelude::*;
                            paths
                                .par_iter()
                                .map(|(id, p)| (*id, od_analysis::analyze_file(p, &opts)))
                                .collect()
                        });
                        for (id, r) in results {
                            match r {
                                Ok(mut a) => {
                                    if !detect_key {
                                        a.key = None;
                                    }
                                    let _ = lib.store_analysis(id, &a, DataSource::Analysis);
                                }
                                Err(e) => {
                                    tracing::warn!(track = %id, error = %e, "analysis failed");
                                    let _ = lib.set_missing(
                                        id,
                                        !lib.track(id).map(|t| t.path.exists()).unwrap_or(false),
                                    );
                                    // Mark as analysed with an empty result so it is not retried forever.
                                    let _ = lib.store_analysis(
                                        id,
                                        &od_core::TrackAnalysis {
                                            analysis_version: od_analysis::ANALYSIS_VERSION,
                                            ..Default::default()
                                        },
                                        DataSource::Analysis,
                                    );
                                }
                            }
                            done += 1;
                        }
                        let _ = events.send(WorkerEvent::AnalysisProgress { done, total });
                    }
                }
            })
            .expect("analysis thread");
        Self {
            wake: wake_tx,
            control,
            stop,
        }
    }
}

impl Drop for AnalysisWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(());
    }
}

#[cfg(test)]
mod tests {
    /// Importing a file adds it with its audio facts; importing it again
    /// finds the same track.
    #[test]
    fn import_adds_each_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("music/a.wav");
        crate::ui_tests::harness::write_wav(&path, 2.0, 440.0);
        let mut lib = od_library::Library::open(&dir.path().join("lib/library.sqlite")).unwrap();
        let id = super::import_file(&mut lib, &path).unwrap();
        assert_eq!(
            super::import_file(&mut lib, &path).unwrap(),
            id,
            "a second import finds the track"
        );
        let t = lib.track(id).unwrap();
        assert!(
            t.duration > 1.9 && t.sample_rate > 0,
            "duration and sample rate are read: {} s, {} Hz",
            t.duration,
            t.sample_rate
        );
        assert_eq!(lib.track_count().unwrap(), 1);
    }
}
