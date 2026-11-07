//! Background separation queue.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use od_core::{ContentHash, TrackId};
use parking_lot::{Condvar, Mutex};

use crate::{Separator, SeparatorOptions, StemCache};

/// One track to separate.
#[derive(Clone, Debug, PartialEq)]
pub struct StemJob {
    pub track: TrackId,
    pub path: PathBuf,
    /// Content hash of the file (cache key), as stored in the library.
    pub hash: ContentHash,
}

/// Progress reports from the worker (drain [`StemWorker::events`]).
#[derive(Clone, Debug, PartialEq)]
pub enum StemEvent {
    /// The model is being downloaded (first use); bytes done / total.
    ModelDownload {
        done: u64,
        total: u64,
    },
    Started {
        track: TrackId,
    },
    /// 0..=1 for the running job.
    Progress {
        track: TrackId,
        fraction: f32,
    },
    /// Stems are in the cache: reload the track (or load it) to use them.
    Done {
        track: TrackId,
        hash: ContentHash,
        seconds: f64,
    },
    /// Already cached; nothing done.
    AlreadyCached {
        track: TrackId,
        hash: ContentHash,
    },
    Failed {
        track: TrackId,
        error: String,
    },
    Cancelled {
        track: TrackId,
    },
}

/// Options of the worker.
#[derive(Clone, Debug)]
pub struct StemWorkerOptions {
    /// Download the model on first use if it is missing.
    pub fetch_model: bool,
    pub separator: SeparatorOptions,
}

impl Default for StemWorkerOptions {
    fn default() -> Self {
        Self {
            fetch_model: true,
            separator: SeparatorOptions::default(),
        }
    }
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<StemJob>,
    current: Option<TrackId>,
    shutdown: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Cancels the running job.
    cancel_current: AtomicBool,
    /// Cancels the running job and stops the worker.
    stop: AtomicBool,
}

/// A background thread separating queued tracks one at a time (the model
/// already uses all cores) into the [`StemCache`].
pub struct StemWorker {
    shared: Arc<Shared>,
    events: Receiver<StemEvent>,
    thread: Option<JoinHandle<()>>,
}

impl StemWorker {
    pub fn spawn(cache: StemCache, opts: StemWorkerOptions) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            cancel_current: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let (tx, rx) = crossbeam_channel::unbounded();
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("od-stems".into())
            .spawn(move || run(s, cache, opts, tx))
            .expect("spawn stem worker");
        Self {
            shared,
            events: rx,
            thread: Some(thread),
        }
    }

    /// Queues a track (ignored if it is already queued or running).
    pub fn enqueue(&self, job: StemJob) {
        let mut q = self.shared.queue.lock();
        if q.current == Some(job.track) || q.jobs.iter().any(|j| j.track == job.track) {
            return;
        }
        q.jobs.push_back(job);
        self.shared.wake.notify_one();
    }

    /// Moves a queued track to the front of the queue (e.g. just loaded).
    pub fn prioritize(&self, track: TrackId) {
        let mut q = self.shared.queue.lock();
        if let Some(i) = q.jobs.iter().position(|j| j.track == track) {
            let j = q.jobs.remove(i).unwrap();
            q.jobs.push_front(j);
        }
    }

    /// Removes a queued track or aborts it if it is running.
    pub fn cancel(&self, track: TrackId) {
        let mut q = self.shared.queue.lock();
        q.jobs.retain(|j| j.track != track);
        if q.current == Some(track) {
            self.shared.cancel_current.store(true, Ordering::Relaxed);
        }
    }

    /// Clears the queue and aborts the running job.
    pub fn cancel_all(&self) {
        let mut q = self.shared.queue.lock();
        q.jobs.clear();
        if q.current.is_some() {
            self.shared.cancel_current.store(true, Ordering::Relaxed);
        }
    }

    /// The running track, if any.
    pub fn current(&self) -> Option<TrackId> {
        self.shared.queue.lock().current
    }

    /// Queued (not yet running) tracks in order.
    pub fn queued(&self) -> Vec<TrackId> {
        self.shared
            .queue
            .lock()
            .jobs
            .iter()
            .map(|j| j.track)
            .collect()
    }

    pub fn is_idle(&self) -> bool {
        let q = self.shared.queue.lock();
        q.current.is_none() && q.jobs.is_empty()
    }

    /// Event stream (cloneable receiver; poll from the UI thread).
    pub fn events(&self) -> &Receiver<StemEvent> {
        &self.events
    }
}

impl Drop for StemWorker {
    fn drop(&mut self) {
        {
            let mut q = self.shared.queue.lock();
            q.shutdown = true;
            q.jobs.clear();
        }
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.cancel_current.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(shared: Arc<Shared>, cache: StemCache, opts: StemWorkerOptions, tx: Sender<StemEvent>) {
    let mut separator: Option<Separator> = None;
    loop {
        let job = {
            let mut q = shared.queue.lock();
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(j) = q.jobs.pop_front() {
                    q.current = Some(j.track);
                    shared.cancel_current.store(false, Ordering::Relaxed);
                    break j;
                }
                q.current = None;
                shared.wake.wait(&mut q);
            }
        };
        let track = job.track;
        let result = process(&shared, &cache, &opts, &mut separator, &job, &tx);
        let ev = match result {
            Ok(Some(seconds)) => StemEvent::Done {
                track,
                hash: job.hash.clone(),
                seconds,
            },
            Ok(None) => StemEvent::AlreadyCached {
                track,
                hash: job.hash.clone(),
            },
            Err(crate::Error::Cancelled) => StemEvent::Cancelled { track },
            Err(e) => {
                tracing::warn!("stem separation of track {track} failed: {e}");
                StemEvent::Failed {
                    track,
                    error: e.to_string(),
                }
            }
        };
        shared.queue.lock().current = None;
        let _ = tx.send(ev);
    }
}

fn process(
    shared: &Shared,
    cache: &StemCache,
    opts: &StemWorkerOptions,
    separator: &mut Option<Separator>,
    job: &StemJob,
    tx: &Sender<StemEvent>,
) -> Result<Option<f64>, crate::Error> {
    if cache.exists(&job.hash) {
        return Ok(None);
    }
    let cancel = &shared.cancel_current;
    if separator.is_none() {
        if !crate::model_available() {
            if !opts.fetch_model {
                return Err(crate::Error::ModelMissing(crate::model_path()));
            }
            crate::fetch_model(
                &mut |done, total| {
                    let _ = tx.send(StemEvent::ModelDownload { done, total });
                },
                cancel,
            )?;
        }
        *separator = Some(Separator::new(&crate::model_path(), &opts.separator)?);
    }
    let sep = separator.as_mut().unwrap();
    let _ = tx.send(StemEvent::Started { track: job.track });
    let started = std::time::Instant::now();
    let mut last = -1.0f32;
    let stems = sep.separate_file(
        &job.path,
        &mut |f| {
            if f - last >= 0.01 || f >= 1.0 {
                last = f;
                let _ = tx.send(StemEvent::Progress {
                    track: job.track,
                    fraction: f,
                });
            }
        },
        cancel,
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err(crate::Error::Cancelled);
    }
    cache.store(&job.hash, &stems)?;
    Ok(Some(started.elapsed().as_secs_f64()))
}
