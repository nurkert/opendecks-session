//! Separating the parts of tracks ahead of time (Prepare, SPEC §10.3): the
//! selected tracks are queued on a background worker that writes into the
//! same stem cache the decks read, so a track loads with its parts ready.
//! Progress and the result show in the status line.

use crate::app::App;
use od_core::TrackId;
use std::cell::RefCell;

/// A finished or running batch: (done, failed, total).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BatchCount {
    pub done: usize,
    pub failed: usize,
    pub total: usize,
}

impl BatchCount {
    pub fn finished(&self) -> bool {
        self.total > 0 && self.done + self.failed >= self.total
    }

    /// The status line for the batch (pure).
    pub fn message(&self, last_error: &str) -> String {
        if !self.finished() {
            od_i18n::trf(
                "Separating parts: {} of {} tracks …",
                &[&(self.done + self.failed + 1).min(self.total), &self.total],
            )
        } else if self.failed == 0 {
            od_i18n::trf("Parts ready for {} tracks", &[&self.done])
        } else {
            od_i18n::trf(
                "Parts ready for {} tracks, {} failed: {}",
                &[&self.done, &self.failed, &last_error],
            )
        }
    }
}

#[derive(Default)]
struct Batch {
    #[cfg(feature = "separation")]
    worker: Option<od_stems::StemWorker>,
    queued: Vec<TrackId>,
    count: BatchCount,
    last_error: String,
}

thread_local! {
    static BATCH: RefCell<Batch> = RefCell::new(Batch::default());
    static TIMER: slint::Timer = slint::Timer::default();
}

/// The current batch count (tests, status).
pub(crate) fn count() -> BatchCount {
    BATCH.with(|b| b.borrow().count)
}

/// "Separate parts" for the selected tracks of the library.
pub(crate) fn separate_selected(app: &App) {
    if app.guard_read_only() {
        return;
    }
    let ids = app.state.borrow().selected_ids();
    separate(app, &ids);
}

#[cfg(not(feature = "separation"))]
pub(crate) fn separate(app: &App, _ids: &[TrackId]) {
    app.status("This build separates no parts (feature separation)");
}

/// Queues `ids` (tracks already queued are skipped).
#[cfg(feature = "separation")]
pub(crate) fn separate(app: &App, ids: &[TrackId]) {
    if ids.is_empty() {
        app.status("Select the tracks to separate first");
        return;
    }
    if app.state.borrow().config.lite {
        app.status("Parts are separated on a desktop computer; the stick brings them along");
        return;
    }
    let fetch_model = !app.state.borrow().config.headless;
    let jobs: Vec<od_stems::StemJob> = {
        let st = app.state.borrow();
        ids.iter()
            .filter_map(|id| st.lib.track(*id).ok())
            .map(|t| od_stems::StemJob {
                track: t.id,
                path: t.path.clone(),
                hash: t.content_hash.clone(),
            })
            .collect()
    };
    BATCH.with(|b| {
        let mut b = b.borrow_mut();
        if b.count.finished() {
            b.count = BatchCount::default();
            b.queued.clear();
        }
        let b = &mut *b;
        let worker = b.worker.get_or_insert_with(|| {
            od_stems::StemWorker::spawn(
                od_stems::StemCache::open_default(),
                od_stems::StemWorkerOptions {
                    fetch_model,
                    ..Default::default()
                },
            )
        });
        let mut added = Vec::new();
        for j in jobs {
            if !b.queued.contains(&j.track) {
                added.push(j.track);
                worker.enqueue(j);
            }
        }
        b.count.total += added.len();
        b.queued.extend(added);
    });
    app.status(count().message(""));
    start_polling(app);
}

#[cfg(feature = "separation")]
fn start_polling(app: &App) {
    let ui = slint::ComponentHandle::as_weak(&app.ui);
    TIMER.with(|t| {
        if t.running() {
            return;
        }
        t.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(200),
            move || {
                let Some(ui) = ui.upgrade() else { return };
                let (changed, message, finished) = poll();
                if changed {
                    ui.set_status_text(message.into());
                }
                if finished {
                    TIMER.with(|t| t.stop());
                }
            },
        );
    });
}

/// Takes the worker's events: (changed, status message, batch finished).
#[cfg(feature = "separation")]
fn poll() -> (bool, String, bool) {
    use od_stems::StemEvent;
    BATCH.with(|b| {
        let mut b = b.borrow_mut();
        let events: Vec<StemEvent> = b
            .worker
            .as_ref()
            .map(|w| w.events().try_iter().collect())
            .unwrap_or_default();
        let mut changed = false;
        for e in events {
            match e {
                StemEvent::Done { .. } | StemEvent::AlreadyCached { .. } => {
                    b.count.done += 1;
                    changed = true;
                }
                StemEvent::Failed { error, .. } => {
                    b.count.failed += 1;
                    b.last_error = error;
                    changed = true;
                }
                StemEvent::Cancelled { .. } => {
                    b.count.failed += 1;
                    changed = true;
                }
                StemEvent::ModelDownload { done, total } => {
                    let pct = if total > 0 { done * 100 / total } else { 0 };
                    return (
                        true,
                        od_i18n::trf("Downloading the stem model … {} %", &[&pct]),
                        false,
                    );
                }
                StemEvent::Started { .. } | StemEvent::Progress { .. } => {}
            }
        }
        let msg = b.count.message(&b.last_error);
        (changed, msg, b.count.finished())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_messages() {
        let mut c = BatchCount {
            done: 0,
            failed: 0,
            total: 3,
        };
        assert!(!c.finished());
        assert!(c.message("").contains('1') && c.message("").contains('3'));
        c.done = 2;
        c.failed = 1;
        assert!(c.finished());
        assert!(c.message("model missing").contains("model missing"));
        c.failed = 0;
        c.done = 3;
        assert!(c.message("").contains('3'));
    }
}
