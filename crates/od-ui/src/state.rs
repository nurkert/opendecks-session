//! The domain state of the UI: one `AppState` owned by the app, with a part
//! per feature module. Modules change it through their commands; the view
//! models are projections of it.

use crate::browser::BrowserState;
use crate::devices::DevicesState;
use crate::explorer::ExplorerState;
use crate::player::PlayerState;
use od_core::KeyNotation;
use od_library::{Library, TrackRecord};
use std::path::{Path, PathBuf};

/// Settings the app starts with.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub library_path: PathBuf,
    /// Raspberry Pi / weak hardware.
    pub lite: bool,
    pub kiosk: bool,
    pub key_notation: KeyNotation,
    /// "de" selects German; anything else English.
    pub language: String,
    /// Fixed window size in logical pixels (e.g. to emulate a Pi display);
    /// disables full screen in kiosk mode.
    pub window: Option<(u32, u32)>,
    pub colorblind_safe: bool,
    pub high_contrast: bool,
    /// Animations; always off in the Lite profile.
    pub motion: bool,
}

pub(crate) struct AppState {
    pub lib: Library,
    pub config: AppConfig,
    pub browser: BrowserState,
    pub player: PlayerState,
    pub devices: DevicesState,
    pub explorer: ExplorerState,
    /// Analysis worker progress (done, total).
    pub analysis: (usize, usize),
    /// Last seen library change sequence (changes by other connections).
    pub last_seq: i64,
}

impl AppState {
    pub fn new(lib: Library, config: AppConfig) -> Self {
        let explorer = ExplorerState::load(&lib);
        Self {
            lib,
            config,
            browser: BrowserState::default(),
            player: PlayerState::default(),
            devices: DevicesState::default(),
            explorer,
            analysis: (0, 0),
            last_seq: -1,
        }
    }

    /// The library the current view reads from: a browsed stick or the main
    /// library.
    pub fn src(&self) -> &Library {
        self.browser.stick.as_ref().map(|(_, s)| &s.library).unwrap_or(&self.lib)
    }

    /// Absolute path of a track of the current source.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        match &self.browser.stick {
            Some((_, s)) => s.resolve(path),
            None => path.to_path_buf(),
        }
    }

    /// Browsing a stick: no editing.
    pub fn read_only(&self) -> bool {
        self.browser.stick.is_some()
    }

    pub fn key_text(&self, t: &TrackRecord) -> String {
        t.key.map(|k| k.format(self.config.key_notation)).unwrap_or_default()
    }
}
