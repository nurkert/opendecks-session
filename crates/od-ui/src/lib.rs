//! Slint user interface of OpenDecks Session.

slint::include_modules!();

mod app;
mod import;
mod perform;
mod render;
mod smart_text;
mod workers;

pub use app::{App, AppConfig};

/// Runs the application until the window closes.
pub fn run(config: AppConfig) -> anyhow::Result<()> {
    let app = App::new(config)?;
    app.run()
}
