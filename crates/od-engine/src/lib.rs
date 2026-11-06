//! Real-time audio engine of OpenDecks Session: decks, mixer, sync, FX,
//! sampler (SPEC §4.2, §10, §11).
//!
//! The engine is pure DSP without I/O. It is split into two halves that talk
//! through lock-free queues (`rtrb`) and a triple-buffered state snapshot:
//!
//! * [`Engine`] — the control-side handle. Send [`Command`]s, read the
//!   latest [`EngineState`], drain [`Event`]s, take the [`RecordTap`].
//! * [`EngineProcessor`] — lives on the audio thread. Its
//!   [`EngineProcessor::process`] never allocates, locks, blocks or logs.
//!
//! ```no_run
//! use od_engine::*;
//! let (mut engine, mut processor) = Engine::new(EngineConfig::default());
//! # let track: std::sync::Arc<TrackData> = unimplemented!();
//! engine.deck(0, DeckCommand::Load(track)).unwrap();
//! engine.deck(0, DeckCommand::Play).unwrap();
//! let mut master = vec![0.0; 256];
//! let mut phones = vec![0.0; 256];
//! processor.process(128, &mut Outputs { master: &mut master, headphones: Some(&mut phones) });
//! println!("{:?}", engine.state().decks[0].position);
//! ```
//!
//! Signal flow per channel: deck (stem levels → resampler or Rubber Band
//! keylock) → trim /
//! auto-gain → 3-band EQ → colour FX → Beat FX (if targeted) → PFL tap →
//! channel fader → crossfader → master bus. Master bus: + sampler → Beat FX
//! (if targeted) → master level → limiter/soft clip → meters → record tap.
//! Headphones: cue bus blended with master.

mod beatfx;
mod colorfx;
pub mod command;
pub mod config;
mod deck;
pub mod dsp;
mod engine;
pub mod keylock;
pub mod mixer;
mod rt;
mod sampler;
pub mod state;
pub mod stems;
pub mod track;

pub use command::*;
pub use config::*;
pub use deck::SECONDS_PER_REVOLUTION;
pub use dsp::eq::EqMode;
pub use dsp::meter::MeterReading;
pub use engine::{CommandSender, Engine, EngineError, EngineProcessor, Outputs, RecordTap};
pub use state::*;
pub use stems::{Stem, StemData, StemInput};
pub use track::{HotCue, SampleData, TrackData};
