//! Domain types shared by all OpenDecks Session crates.
//!
//! Time positions inside a track are expressed in seconds (`f64`) from the
//! start of the decoded audio. Conversions to sample frames or to the
//! millisecond units used by the Pioneer formats happen at the edges.

pub mod beatgrid;
pub mod color;
pub mod cue;
pub mod ids;
pub mod key;
pub mod phrase;
pub mod track;
pub mod waveform;

pub use beatgrid::{Beat, Beatgrid};
pub use color::CueColor;
pub use cue::{Cue, CueKind};
pub use ids::*;
pub use key::{KeyNotation, Mode, MusicalKey};
pub use phrase::{Phrase, PhraseKind, PhraseMood};
pub use track::{AudioFormat, DataSource, TrackAnalysis, TrackTags};
pub use waveform::{Waveform, WaveformColumn};
