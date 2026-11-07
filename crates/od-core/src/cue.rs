use crate::{CueColor, DataSource};
use serde::{Deserialize, Serialize};

/// Hot cues are numbered 0..HOT_CUE_SLOTS (shown as A..H).
pub const HOT_CUE_SLOTS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "slot", rename_all = "snake_case")]
pub enum CueKind {
    Hot(u8),
    Memory,
}

impl CueKind {
    pub fn hot_slot_letter(self) -> Option<char> {
        match self {
            CueKind::Hot(n) if n < 26 => Some((b'A' + n) as char),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub kind: CueKind,
    /// Start in seconds.
    pub start: f64,
    /// Loop end in seconds; `Some` makes this cue a stored loop.
    pub loop_end: Option<f64>,
    pub label: String,
    pub color: Option<CueColor>,
    pub source: DataSource,
    /// Active loop: a stored loop that engages by itself when playback
    /// reaches it (loops only).
    #[serde(default)]
    pub active: bool,
}

impl Cue {
    pub fn hot(slot: u8, start: f64) -> Self {
        Self {
            kind: CueKind::Hot(slot),
            start,
            loop_end: None,
            label: String::new(),
            color: None,
            source: DataSource::Manual,
            active: false,
        }
    }

    pub fn memory(start: f64) -> Self {
        Self {
            kind: CueKind::Memory,
            ..Self::hot(0, start)
        }
    }

    pub fn is_loop(&self) -> bool {
        self.loop_end.is_some()
    }
}
