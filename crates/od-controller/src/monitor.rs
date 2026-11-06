//! MIDI monitor: a bounded log of raw messages with decoded mapping hits.

use crate::midi::{hex, MidiMessage};
use crate::session::MappingHit;
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorEvent {
    /// Time since the monitor was created.
    pub at: Duration,
    pub port: String,
    pub direction: Direction,
    pub bytes: Vec<u8>,
    pub message: Option<MidiMessage>,
    pub hits: Vec<MappingHit>,
}

impl fmt::Display for MonitorEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dir = match self.direction {
            Direction::In => "IN ",
            Direction::Out => "OUT",
        };
        write!(f, "{:>9.3}s {dir} [{:<8}] ", self.at.as_secs_f64(), hex(&self.bytes))?;
        match &self.message {
            Some(m) => write!(f, "{m}")?,
            None => write!(f, "(undecodable)")?,
        }
        if self.direction == Direction::In && self.hits.is_empty() {
            write!(f, "  -> (unmapped)")?;
        }
        for h in &self.hits {
            write!(f, "\n            -> {}", h.binding)?;
            if let Some(a) = &h.action {
                write!(f, ": {a:?}")?;
            }
            if let Some(s) = h.suppressed {
                write!(f, " [suppressed: {s}]")?;
            }
            if h.verify {
                write!(f, " [unverified]")?;
            }
        }
        Ok(())
    }
}

/// Keeps the last `capacity` events and fans them out to subscribers.
/// Cheap to clone; all clones share one log.
#[derive(Clone)]
pub struct MidiMonitor {
    inner: Arc<Mutex<Inner>>,
    start: std::time::Instant,
}

struct Inner {
    capacity: usize,
    log: VecDeque<MonitorEvent>,
    subscribers: Vec<Sender<MonitorEvent>>,
    enabled: bool,
}

impl MidiMonitor {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                capacity,
                log: VecDeque::with_capacity(capacity.min(4096)),
                subscribers: Vec::new(),
                enabled: true,
            })),
            start: std::time::Instant::now(),
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.inner.lock().unwrap().enabled = on;
    }

    pub fn record(
        &self,
        port: &str,
        direction: Direction,
        bytes: &[u8],
        hits: Vec<MappingHit>,
    ) {
        let mut g = self.inner.lock().unwrap();
        if !g.enabled {
            return;
        }
        let ev = MonitorEvent {
            at: self.start.elapsed(),
            port: port.to_string(),
            direction,
            bytes: bytes.to_vec(),
            message: MidiMessage::parse(bytes),
            hits,
        };
        // Drop subscribers that went away; never block on slow ones.
        g.subscribers.retain(|s| !matches!(s.try_send(ev.clone()), Err(TrySendError::Disconnected(_))));
        if g.log.len() == g.capacity {
            g.log.pop_front();
        }
        g.log.push_back(ev);
    }

    /// A live stream of new events (bounded; events are dropped if the
    /// receiver lags behind by more than 4096 events).
    pub fn subscribe(&self) -> Receiver<MonitorEvent> {
        let (tx, rx) = crossbeam_channel::bounded(4096);
        self.inner.lock().unwrap().subscribers.push(tx);
        rx
    }

    pub fn snapshot(&self) -> Vec<MonitorEvent> {
        self.inner.lock().unwrap().log.iter().cloned().collect()
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().log.clear();
    }
}

impl Default for MidiMonitor {
    fn default() -> Self {
        Self::new(10_000)
    }
}
