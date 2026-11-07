//! The domain state of the UI: one `AppState` owned by the app, with a part
//! per feature module. Modules change it through their commands; the view
//! models are projections of it.

use crate::browser::BrowserState;
use crate::devices::DevicesState;
use crate::explorer::ExplorerState;
use crate::keymap::Keymap;
use crate::player::PlayerState;
use crate::settings::{Overrides, Settings};
use od_library::{Library, TrackRecord};
use std::path::{Path, PathBuf};

/// How the app starts. Everything the user can change lives in the
/// persisted [`Settings`]; command-line flags override them for one run.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub library_path: PathBuf,
    /// Raspberry Pi / weak hardware.
    pub lite: bool,
    pub kiosk: bool,
    /// Fixed window size in logical pixels (e.g. to emulate a Pi display);
    /// disables full screen in kiosk mode.
    pub window: Option<(u32, u32)>,
    pub overrides: Overrides,
    /// Run the engine without an audio device (output discarded): headless
    /// UI tests and machines without audio. Also `OPENDECKS_NULL_AUDIO=1`.
    pub null_audio: bool,
    /// Headless run (UI tests): no controller (MIDI) service and no
    /// automatic update check.
    pub headless: bool,
}

impl AppConfig {
    pub fn new(library_path: PathBuf) -> Self {
        Self {
            library_path,
            lite: false,
            kiosk: false,
            window: None,
            overrides: Overrides::default(),
            null_audio: std::env::var_os("OPENDECKS_NULL_AUDIO").is_some_and(|v| v == "1"),
            headless: false,
        }
    }
}

pub(crate) struct AppState {
    pub lib: Library,
    pub config: AppConfig,
    /// Settings in effect (stored settings plus command-line overrides).
    pub settings: Settings,
    /// Settings as stored in the library.
    pub stored: Settings,
    pub keymap: Keymap,
    pub browser: BrowserState,
    pub player: PlayerState,
    pub devices: DevicesState,
    pub explorer: ExplorerState,
    /// Analysis worker progress (done, total).
    pub analysis: (usize, usize),
}

impl AppState {
    pub fn new(lib: Library, config: AppConfig) -> Self {
        let explorer = ExplorerState::load(&lib);
        let stored = lib
            .meta(crate::settings::META_SETTINGS)
            .ok()
            .flatten()
            .map(|j| Settings::from_json(&j))
            .unwrap_or_default();
        let settings = config.overrides.apply(&stored);
        let mut keymap = Keymap::new(crate::keymap::Platform::current());
        if let Some(t) = lib.meta(crate::settings::META_KEYMAP).ok().flatten() {
            if let Err(e) = keymap.import_toml(&t) {
                tracing::warn!(error = %e, "stored keymap unreadable");
            }
        }
        Self {
            lib,
            config,
            settings,
            stored,
            keymap,
            browser: BrowserState::default(),
            player: PlayerState::default(),
            devices: DevicesState::default(),
            explorer,
            analysis: (0, 0),
        }
    }

    /// The library the current view reads from: a browsed stick or the main
    /// library.
    pub fn src(&self) -> &Library {
        self.browser
            .stick
            .as_ref()
            .map(|(_, s)| &s.library)
            .unwrap_or(&self.lib)
    }

    /// Root of the stick being browsed, if any.
    pub fn stick_root(&self) -> Option<&Path> {
        self.browser.stick.as_ref().map(|(_, s)| s.root.as_path())
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

    /// Changes a setting in effect and in storage, and stores it.
    pub fn change_settings(&mut self, f: impl Fn(&mut Settings)) {
        f(&mut self.settings);
        f(&mut self.stored);
        self.settings = self.settings.clone().sanitized();
        self.stored = self.stored.clone().sanitized();
        if let Err(e) = self
            .lib
            .set_meta(crate::settings::META_SETTINGS, &self.stored.to_json())
        {
            tracing::warn!(error = %e, "settings not saved");
        }
    }

    /// Stores the keymap overrides.
    pub fn save_keymap(&mut self) {
        let t = self.keymap.overrides_toml();
        if let Err(e) = self.lib.set_meta(crate::settings::META_KEYMAP, &t) {
            tracing::warn!(error = %e, "keymap not saved");
        }
    }

    pub fn key_text(&self, t: &TrackRecord) -> String {
        t.key
            .map(|k| k.format(self.settings.key_notation()))
            .unwrap_or_default()
    }
}
