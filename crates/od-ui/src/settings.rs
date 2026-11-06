//! Persisted settings (the Preferences window). Stored as JSON in the
//! library meta (`settings`), the keymap overrides as TOML (`keymap`). The
//! library location itself lives outside the library, in `config.toml` in
//! the data directory. Command-line flags override settings for one run
//! without changing what is stored.

use od_core::KeyNotation;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const META_SETTINGS: &str = "settings";
pub const META_KEYMAP: &str = "keymap";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioSettings,
    pub view: ViewSettings,
    pub analysis: AnalysisSettings,
    pub controller: ControllerSettings,
    pub recording: RecordingSettings,
    pub advanced: AdvancedSettings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    /// Output device name; empty = automatic (the controller's interface,
    /// else the system default).
    pub device: String,
    /// Buffer size in frames; 0 = automatic (128, 256 in Lite).
    pub buffer: u32,
    /// Sample rate in Hz; 0 = the device's default.
    pub sample_rate: u32,
    /// "auto" (channels 3/4 when the device has them), "34" or "off".
    pub headphones: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewSettings {
    /// "" = from the system, "en" or "de".
    pub language: String,
    /// "camelot", "openkey" or "musical".
    pub key_notation: String,
    /// UI scale factor (0.75–2.0).
    pub scale: f32,
    pub colorblind_safe: bool,
    pub high_contrast: bool,
    pub reduced_motion: bool,
    /// "3band" or "blue".
    pub waveform: String,
    pub phrases: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisSettings {
    pub bpm_min: u32,
    pub bpm_max: u32,
    pub detect_key: bool,
    pub detect_phrases: bool,
    /// Analyse new tracks automatically; otherwise only on request.
    pub auto_analyse: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ControllerSettings {
    /// An extra mapping file loaded with the built-in ones.
    pub mapping_file: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingSettings {
    /// "wav", "flac" or "wav+mp3".
    pub format: String,
    /// Empty = ~/Music/OpenDecks Recordings.
    pub folder: String,
    /// Split into one file per track when the recording stops.
    pub auto_split: bool,
    /// Start recording when sound starts after silence.
    pub silence_start: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Back up the library at every start.
    pub backup_on_start: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioSettings::default(),
            view: ViewSettings::default(),
            analysis: AnalysisSettings::default(),
            controller: ControllerSettings::default(),
            recording: RecordingSettings::default(),
            advanced: AdvancedSettings::default(),
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { device: String::new(), buffer: 0, sample_rate: 0, headphones: "auto".into() }
    }
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            language: String::new(),
            key_notation: "camelot".into(),
            scale: 1.0,
            colorblind_safe: false,
            high_contrast: false,
            reduced_motion: false,
            waveform: "3band".into(),
            phrases: true,
        }
    }
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        let d = od_analysis::AnalysisOptions::default();
        Self { bpm_min: d.min_bpm as u32, bpm_max: d.max_bpm as u32, detect_key: true, detect_phrases: d.detect_phrases, auto_analyse: true }
    }
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self { format: "wav".into(), folder: String::new(), auto_split: false, silence_start: false }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self { backup_on_start: true }
    }
}

pub const BUFFER_SIZES: [u32; 6] = [0, 64, 128, 256, 512, 1024];
pub const SAMPLE_RATES: [u32; 3] = [0, 44_100, 48_000];
pub const SCALES: [f32; 6] = [0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
pub const BPM_RANGES: [(u32, u32); 5] = [(58, 115), (68, 135), (78, 155), (87, 175), (98, 195)];

impl Settings {
    /// Reads the settings from JSON; unknown or broken values fall back to
    /// their defaults.
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str::<Settings>(text).map(Settings::sanitized).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Clamps values into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        let v = &mut self.view;
        if !(0.75..=2.0).contains(&v.scale) || !v.scale.is_finite() {
            v.scale = 1.0;
        }
        if !["camelot", "openkey", "musical"].contains(&v.key_notation.as_str()) {
            v.key_notation = "camelot".into();
        }
        if !["", "en", "de"].contains(&v.language.as_str()) {
            v.language = String::new();
        }
        if !["3band", "blue"].contains(&v.waveform.as_str()) {
            v.waveform = "3band".into();
        }
        let a = &mut self.analysis;
        a.bpm_min = a.bpm_min.clamp(40, 200);
        a.bpm_max = a.bpm_max.clamp(a.bpm_min + 20, 300);
        if !["auto", "34", "off"].contains(&self.audio.headphones.as_str()) {
            self.audio.headphones = "auto".into();
        }
        if !["wav", "flac", "wav+mp3"].contains(&self.recording.format.as_str()) {
            self.recording.format = "wav".into();
        }
        self
    }

    pub fn key_notation(&self) -> KeyNotation {
        match self.view.key_notation.as_str() {
            "openkey" => KeyNotation::OpenKey,
            "musical" => KeyNotation::Musical,
            _ => KeyNotation::Camelot,
        }
    }

    /// The interface language: the setting, else the system's.
    pub fn language(&self) -> String {
        if self.view.language.is_empty() { std::env::var("LANG").unwrap_or_default() } else { self.view.language.clone() }
    }

    pub fn analysis_options(&self, lite: bool) -> od_analysis::AnalysisOptions {
        od_analysis::AnalysisOptions {
            min_bpm: self.analysis.bpm_min as f64,
            max_bpm: self.analysis.bpm_max as f64,
            detect_phrases: self.analysis.detect_phrases,
            lite,
        }
    }

    pub fn recordings_dir(&self) -> PathBuf {
        if self.recording.folder.is_empty() {
            std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Music/OpenDecks Recordings")
        } else {
            PathBuf::from(&self.recording.folder)
        }
    }
}

pub fn notation_name(n: KeyNotation) -> &'static str {
    match n {
        KeyNotation::Camelot => "camelot",
        KeyNotation::OpenKey => "openkey",
        KeyNotation::Musical => "musical",
    }
}

/// Command-line flags that override the stored settings for this run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub language: Option<String>,
    pub key_notation: Option<KeyNotation>,
    pub scale: Option<f32>,
    pub colorblind_safe: Option<bool>,
    pub high_contrast: Option<bool>,
    pub reduced_motion: Option<bool>,
}

impl Overrides {
    /// The settings in effect: `stored` with the overrides applied.
    pub fn apply(&self, stored: &Settings) -> Settings {
        let mut s = stored.clone();
        if let Some(l) = &self.language {
            s.view.language = if l.starts_with("de") { "de".into() } else { "en".into() };
        }
        if let Some(k) = self.key_notation {
            s.view.key_notation = notation_name(k).into();
        }
        if let Some(f) = self.scale {
            s.view.scale = f;
        }
        if let Some(b) = self.colorblind_safe {
            s.view.colorblind_safe = b;
        }
        if let Some(b) = self.high_contrast {
            s.view.high_contrast = b;
        }
        if let Some(b) = self.reduced_motion {
            s.view.reduced_motion = b;
        }
        s
    }
}

/// App-wide configuration outside the library: where the library is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub library: Option<PathBuf>,
}

impl GlobalConfig {
    pub fn path() -> PathBuf {
        od_library::data_dir().join("config.toml")
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string(self).unwrap_or_default())
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }
}

/// The library the app opens: the `--library` flag, else the configured
/// location, else the default.
pub fn library_path(flag: Option<PathBuf>) -> PathBuf {
    flag.or_else(|| GlobalConfig::load().library).unwrap_or_else(od_library::Library::default_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut s = Settings::default();
        s.audio.device = "DDJ-FLX10".into();
        s.audio.buffer = 256;
        s.view.language = "de".into();
        s.view.key_notation = "openkey".into();
        s.view.scale = 1.25;
        s.view.high_contrast = true;
        s.analysis.bpm_min = 78;
        s.analysis.bpm_max = 155;
        s.recording.format = "flac".into();
        s.recording.auto_split = true;
        s.controller.mapping_file = "/tmp/my.toml".into();
        assert_eq!(Settings::from_json(&s.to_json()), s);
    }

    #[test]
    fn missing_and_broken_values_fall_back() {
        let s = Settings::from_json(r#"{"view": {"scale": 9.0, "key_notation": "solfege"}, "audio": {"buffer": 512}}"#);
        assert_eq!(s.view.scale, 1.0);
        assert_eq!(s.view.key_notation, "camelot");
        assert_eq!(s.audio.buffer, 512);
        assert_eq!(s.audio.headphones, "auto");
        assert!(s.analysis.auto_analyse);
        assert_eq!(Settings::from_json("not json"), Settings::default());
    }

    #[test]
    fn library_meta_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = od_library::Library::open(&dir.path().join("library.sqlite")).unwrap();
        let mut s = Settings::default();
        s.view.phrases = false;
        s.analysis.detect_key = false;
        lib.set_meta(META_SETTINGS, &s.to_json()).unwrap();
        drop(lib);
        let lib = od_library::Library::open(&dir.path().join("library.sqlite")).unwrap();
        let back = Settings::from_json(&lib.meta(META_SETTINGS).unwrap().unwrap());
        assert_eq!(back, s);
    }

    #[test]
    fn overrides_win_without_changing_the_stored_settings() {
        let mut stored = Settings::default();
        stored.view.key_notation = "musical".into();
        let o = Overrides { key_notation: Some(KeyNotation::OpenKey), language: Some("de_DE.UTF-8".into()), high_contrast: Some(true), ..Default::default() };
        let eff = o.apply(&stored);
        assert_eq!(eff.key_notation(), KeyNotation::OpenKey);
        assert_eq!(eff.view.language, "de");
        assert!(eff.view.high_contrast);
        assert_eq!(stored.view.key_notation, "musical");
        assert_eq!(Overrides::default().apply(&stored), stored);
    }

    #[test]
    fn global_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/config.toml");
        assert_eq!(GlobalConfig::load_from(&p), GlobalConfig::default());
        let c = GlobalConfig { library: Some("/music/lib.sqlite".into()) };
        c.save_to(&p).unwrap();
        assert_eq!(GlobalConfig::load_from(&p), c);
    }
}
