//! Real-time side plumbing: event output and deferred deallocation.

use std::sync::Arc;

use crate::command::Command;
use crate::state::Event;
use crate::track::{SampleData, TrackData};

/// Objects the processor no longer needs. They are handed back to the control
/// side, which drops them (freeing memory off the audio thread).
#[allow(dead_code)]
pub(crate) enum Garbage {
    Track(Arc<TrackData>),
    Sample(Arc<SampleData>),
    Commands(rtrb::Consumer<Command>),
    Input(rtrb::Consumer<f32>),
}

pub(crate) struct RtCtx<'a> {
    pub quantize: bool,
    pub events: &'a mut rtrb::Producer<Event>,
    pub garbage: &'a mut rtrb::Producer<Garbage>,
    pub leaked: &'a mut u64,
}

impl RtCtx<'_> {
    pub fn event(&mut self, e: Event) {
        // A full event queue means nobody is listening; dropping is fine.
        let _ = self.events.push(e);
    }

    pub fn dispose(&mut self, g: Garbage) {
        if let Err(rtrb::PushError::Full(g)) = self.garbage.push(g) {
            // Never free on the audio thread: leak instead (diagnostic counter).
            std::mem::forget(g);
            *self.leaked += 1;
        }
    }

    pub fn dispose_track(&mut self, t: Arc<TrackData>) {
        self.dispose(Garbage::Track(t));
    }

    pub fn dispose_sample(&mut self, s: Arc<SampleData>) {
        self.dispose(Garbage::Sample(s));
    }
}
