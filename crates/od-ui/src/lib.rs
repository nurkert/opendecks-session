//! Slint user interface of OpenDecks Session.
//!
//! Organised by feature (SPEC §4.1a): each module owns its part of the
//! state, its view-model projection and its callbacks (`wire`).
//!
//! - core: [`app`] (window, wiring, periodic tick), [`state`] (`AppState`),
//!   [`commands`] (status, errors, refresh after edits, undo), [`models`],
//!   [`format`]
//! - features: [`shell`], [`browser`], [`explorer`], [`player`], [`decks`],
//!   [`mixer`], [`fx`], [`sampler`], [`recorder`], [`devices`],
//!   [`preferences`], plus [`engine`] (audio lifecycle, frame tick) and
//!   [`controller`] (MIDI)
//! - [`render`]: waveform images

slint::include_modules!();

mod app;
mod browser;
mod commands;
mod controller;
mod decks;
mod devices;
mod engine;
mod explorer;
mod format;
mod fx;
mod import;
mod mixer;
mod models;
mod player;
mod preferences;
mod recorder;
mod render;
mod sampler;
mod shell;
mod smart_text;
mod state;
mod workers;

pub use app::App;
pub use state::AppConfig;

/// Runs the application until the window closes.
pub fn run(config: AppConfig) -> anyhow::Result<()> {
    let app = App::new(config)?;
    app.run()
}
