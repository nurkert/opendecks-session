//! Headless UI tests (SPEC §16.2 "UI" layer, §16.3 view completeness,
//! §16.4 flows).
//!
//! The real [`App`](crate::App) window runs on Slint's testing backend: no
//! display, no event loop, mock time. Each test thread gets its own backend
//! (`init_no_event_loop`), a temporary library with generated audio files
//! ([`Fixture`]) and an app whose engine renders into nothing
//! (`AppConfig::null_audio`), so decks load, play and record without an
//! audio device. Elements are found by their `accessible-label`
//! (docs/parity.md); input is synthesised at the element's centre (clicks,
//! double-clicks, right-clicks, drags, wheel, keys) and the resulting
//! library / engine state is asserted.
//!
//! - [`harness`]: fixture, app start-up, time, element lookup, input
//! - [`views`]: SPEC §16.3 view completeness against `tests/parity/views.toml`
//! - [`flows`]: SPEC §16.4 flows from `tests/parity/flows.toml`, counting steps
//! - [`features`]: behaviour tests for features of `tests/parity/features.toml`
//! - [`prepare`]: Prepare / browser behaviour tests (file drops, controller
//!   browsing, source rail, layouts, panel toggles, batch separation)
//! - [`editing`]: phrase display and editing, edit mode, sub-browser

mod alignment;
mod browser;
mod editing;
mod features;
mod flows;
pub(crate) mod harness;
mod perform;
mod prepare;
mod remote;
mod waveform;
mod session;
mod stage;
mod views;
