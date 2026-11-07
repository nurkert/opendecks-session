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
    pub update: UpdateSettings,
    pub perform: PerformSettings,
    pub browser: BrowserSettings,
    /// Deck defaults applied when audio starts (Preferences > Controller).
    pub deck: DeckSettings,
    /// Mixer behaviour (Preferences > Controller > Mixer).
    pub mixer: MixerSettings,
}

/// Deck defaults: sent to every deck when the engine starts and when they
/// change (`preferences::deck_defaults`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckSettings {
    /// Tempo fader range: "six", "ten", "sixteen" or "wide".
    pub tempo_range: String,
    /// Quantize on at start.
    pub quantize: bool,
    /// Jog wheel: "vinyl" (touch holds and scratches) or "bend".
    pub jog_mode: String,
    /// Auto cue: a loaded track waits at its first sound / first memory cue.
    pub auto_cue: bool,
    /// SYNC matches "beat" (tempo and phase) or "tempo" only.
    pub sync_mode: String,
}

impl Default for DeckSettings {
    fn default() -> Self {
        Self { tempo_range: "ten".into(), quantize: true, jog_mode: "vinyl".into(), auto_cue: true, sync_mode: "beat".into() }
    }
}

/// Mixer behaviour: EQ type, curves and auto gain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerSettings {
    /// "isolator" (full kill) or "classic" (−26 dB shelf EQ).
    pub eq_mode: String,
    /// Channel fader curve: "linear", "smooth" or "sharp".
    pub fader_curve: String,
    /// Crossfader curve: "constant_power", "linear" or "cut".
    pub crossfader_curve: String,
    /// Level every track by its analysed loudness.
    pub auto_gain: bool,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self { eq_mode: "isolator".into(), fader_curve: "smooth".into(), crossfader_curve: "constant_power".into(), auto_gain: true }
    }
}

pub const TEMPO_RANGES: [&str; 4] = ["six", "ten", "sixteen", "wide"];
pub const JOG_MODES: [&str; 2] = ["vinyl", "bend"];
pub const SYNC_MODES: [&str; 2] = ["beat", "tempo"];
pub const EQ_MODES: [&str; 2] = ["isolator", "classic"];
pub const FADER_CURVES: [&str; 3] = ["linear", "smooth", "sharp"];
pub const CROSSFADER_CURVES: [&str; 3] = ["constant_power", "linear", "cut"];
/// Track table text sizes (px) and row heights (px) of Preferences > View.
pub const FONT_SIZES: [u32; 4] = [12, 14, 16, 18];
pub const LINE_SPACINGS: [&str; 3] = ["compact", "normal", "roomy"];
/// Seconds across the Prepare player's zoomed waveform (Preferences > View).
pub const ZOOM_SPANS: [f32; 4] = [4.0, 8.0, 16.0, 32.0];
/// Recording gain steps (dB) of Preferences > Recording.
pub const REC_GAINS: [f32; 7] = [-12.0, -6.0, -3.0, 0.0, 3.0, 6.0, 12.0];

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
    /// "dark", "light" or "system" (follow the OS appearance).
    pub appearance: String,
    /// "3band", "blue" or "rgb".
    pub waveform: String,
    /// Phrases on the enlarged (zoomed) waveforms: a colour strip along
    /// the bottom.
    pub phrases: bool,
    /// The phrase bar under the overviews (decks and the Prepare player).
    pub phrases_overview: bool,
    /// Phrase types on the phrase bar: "names" (labelled colour blocks) or
    /// "colors" (colour only).
    pub phrase_types: String,
    /// Bar counter on decks: "to-phrase" (bars left in the phrase, the
    /// default), "to-memory-cue" (bars left to the next memory cue) or "up".
    pub bar_counter: String,
    /// Seconds across the zoomed waveform at start (Prepare player; the
    /// decks start at the matching zoom).
    pub zoom_span: f32,
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
    /// DDJ-FLX10 utility settings (written to the unit on APPLY).
    pub flx10: Flx10Prefs,
}

/// The player preferences sticks carry, as the JSON the Pioneer sync reads
/// (`od_devices::pioneer::StickPlayerSettings`).
pub(crate) fn stick_player_settings(s: &Settings) -> String {
    let p = od_devices::pioneer::StickPlayerSettings {
        quantize: s.deck.quantize,
        auto_cue: s.deck.auto_cue,
        master_tempo: false,
        tempo_range: s.deck.tempo_range.clone(),
        jog_vinyl: s.deck.jog_mode != "bend",
        language: s.language(),
    };
    serde_json::to_string(&p).unwrap_or_default()
}

/// DDJ-FLX10 utility settings (`midi_tools::flx10_settings`). The defaults
/// are the vendor utility's defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Flx10Prefs {
    /// "off", "on" or "on-sync" (fader start also turns SYNC on).
    pub fader_start: String,
    /// REC OUT: "with-mic" or "without-mic".
    pub rec_out: String,
    /// Jog display time: "remaining" or "elapsed".
    pub time_mode: String,
    pub slip_flashing: bool,
    /// SLIP button while slip is on: "lit" or "blink".
    pub slip_led: String,
    /// Virtual deck mode (hot cue indicator on the jog display).
    pub virtual_deck: bool,
    /// Jog ring colour mode 1..=3.
    pub jog_ring: u8,
    /// USB output boost in dB (0, 3, 6, 9, 12, 15).
    pub boost_db: i32,
}

impl Default for Flx10Prefs {
    fn default() -> Self {
        Self {
            fader_start: "on".into(),
            rec_out: "with-mic".into(),
            time_mode: "remaining".into(),
            slip_flashing: true,
            slip_led: "lit".into(),
            virtual_deck: true,
            jog_ring: 1,
            boost_db: 0,
        }
    }
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
    /// Stop recording after this long silence (with automatic start).
    pub silence_stop: bool,
    /// File name pattern: `{date}` (2026-11-07), `{time}` (21-30-00),
    /// `{n}` (number of the recording that day); empty = "Mix {date} {time}".
    pub name_pattern: String,
    /// Recording level in dB (−12 … +12).
    pub gain_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Back up the library at every start.
    pub backup_on_start: bool,
}

/// Perform view state that persists: which panels are open and the Colour
/// FX the mixer's CFX knobs drive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformSettings {
    /// Beat FX panel above the waveforms (top bar FX toggle).
    pub fx_panel: bool,
    /// Sampler panel under the decks (top bar SAMPLER toggle).
    pub sampler_panel: bool,
    /// Colour FX of the CFX knobs: "filter", "echo", "noise", "space",
    /// "crush", "pitch", "dub_echo" or "sweep".
    pub color_fx: String,
    /// Mixer column between the decks (top bar MIXER toggle).
    pub mixer_panel: bool,
    /// Recording section / bar (top bar REC toggle).
    pub rec_panel: bool,
    /// Mix Point Link panel above the waveforms (top bar MIX POINT toggle).
    pub mpl_panel: bool,
}

impl Default for PerformSettings {
    fn default() -> Self {
        Self { fx_panel: false, sampler_panel: false, color_fx: "filter".into(), mixer_panel: true, rec_panel: true, mpl_panel: false }
    }
}

/// The track table: shown columns in order and the secondary sort.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserSettings {
    /// Column ids in display order (`browser::columns::COLUMNS`); empty =
    /// the default set.
    pub columns: Vec<String>,
    /// Secondary sort columns (⇧-click in the header), in order.
    pub sort_then: Vec<SortSpec>,
    /// Text size of the track table in px (0 = the default, 14).
    pub font_size: u32,
    /// Row height of the track table: "compact", "normal" or "roomy".
    pub line_spacing: String,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self { columns: Vec::new(), sort_then: Vec::new(), font_size: 14, line_spacing: "normal".into() }
    }
}

/// One sort column and its direction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SortSpec {
    pub column: String,
    pub descending: bool,
}

/// Colour FX names stored in [`PerformSettings::color_fx`], in panel order.
pub const COLOR_FX: [&str; 9] = ["filter", "echo", "noise", "space", "crush", "pitch", "dub_echo", "sweep", "smart"];

/// The update check (SPEC §14.1; Preferences > Advanced).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check GitHub for a new version at start and once a day.
    pub auto_check: bool,
    /// Offer pre-releases too.
    pub prereleases: bool,
    /// The version the user chose to skip ("SKIP THIS VERSION").
    pub skip_version: String,
    /// Last successful check, seconds since 1970 (0 = never).
    pub last_check: i64,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true, prereleases: false, skip_version: String::new(), last_check: 0 }
    }
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
            update: UpdateSettings::default(),
            perform: PerformSettings::default(),
            browser: BrowserSettings::default(),
            deck: DeckSettings::default(),
            mixer: MixerSettings::default(),
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
            appearance: "dark".into(),
            waveform: "3band".into(),
            phrases: true,
            phrases_overview: true,
            phrase_types: "names".into(),
            bar_counter: "to-phrase".into(),
            zoom_span: 8.0,
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
        Self { format: "wav".into(), folder: String::new(), auto_split: false, silence_start: false, silence_stop: false, name_pattern: String::new(), gain_db: 0.0 }
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
/// Bar counter modes of the decks, in the order Preferences lists them.
pub const BAR_COUNTERS: [&str; 3] = ["to-phrase", "to-memory-cue", "up"];
/// How phrase types show on the phrase bar: names on the colour blocks, or
/// colours only.
pub const PHRASE_TYPES: [&str; 2] = ["names", "colors"];
/// Appearance modes, in the order the top-bar button cycles them.
pub const APPEARANCES: [&str; 3] = ["dark", "light", "system"];
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
        if !BAR_COUNTERS.contains(&v.bar_counter.as_str()) {
            v.bar_counter = "to-phrase".into();
        }
        if !APPEARANCES.contains(&v.appearance.as_str()) {
            v.appearance = "dark".into();
        }
        if !["3band", "blue", "rgb"].contains(&v.waveform.as_str()) {
            v.waveform = "3band".into();
        }
        if !PHRASE_TYPES.contains(&v.phrase_types.as_str()) {
            v.phrase_types = "names".into();
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
        if !COLOR_FX.contains(&self.perform.color_fx.as_str()) {
            self.perform.color_fx = "filter".into();
        }
        if !self.browser.columns.is_empty() {
            self.browser.columns = crate::browser::sanitize_columns(&self.browser.columns);
        }
        self.browser.sort_then.retain(|s| crate::browser::sort_key_known(&s.column));
        self.browser.sort_then.truncate(3);
        if !FONT_SIZES.contains(&self.browser.font_size) {
            self.browser.font_size = 14;
        }
        if !LINE_SPACINGS.contains(&self.browser.line_spacing.as_str()) {
            self.browser.line_spacing = "normal".into();
        }
        if !ZOOM_SPANS.contains(&self.view.zoom_span) {
            self.view.zoom_span = 8.0;
        }
        let d = &mut self.deck;
        if !TEMPO_RANGES.contains(&d.tempo_range.as_str()) {
            d.tempo_range = "ten".into();
        }
        if !JOG_MODES.contains(&d.jog_mode.as_str()) {
            d.jog_mode = "vinyl".into();
        }
        if !SYNC_MODES.contains(&d.sync_mode.as_str()) {
            d.sync_mode = "beat".into();
        }
        let m = &mut self.mixer;
        if !EQ_MODES.contains(&m.eq_mode.as_str()) {
            m.eq_mode = "isolator".into();
        }
        if !FADER_CURVES.contains(&m.fader_curve.as_str()) {
            m.fader_curve = "smooth".into();
        }
        if !CROSSFADER_CURVES.contains(&m.crossfader_curve.as_str()) {
            m.crossfader_curve = "constant_power".into();
        }
        let r = &mut self.recording;
        r.gain_db = if r.gain_db.is_finite() { r.gain_db.clamp(-12.0, 12.0) } else { 0.0 };
        r.name_pattern = r.name_pattern.trim().chars().filter(|c| !matches!(c, '/' | '\\' | ':') && !c.is_control()).collect();
        self
    }

    /// Row height of the track table (px) for the line spacing.
    pub fn row_height(&self) -> f32 {
        match self.browser.line_spacing.as_str() {
            "compact" => 22.0,
            "roomy" => 34.0,
            _ => 28.0,
        }
    }

    /// Applies preference values read from another application's settings
    /// (`(json path, value)` as `od_import_rekordbox::settings` writes
    /// them). Unknown keys and values are skipped; returns the keys applied.
    pub fn apply_values(&mut self, values: &[(String, String)]) -> Vec<String> {
        let mut applied = Vec::new();
        let flag = |v: &str| match v {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        for (k, v) in values {
            // Per-deck values: the first deck that carries one wins.
            let key = match k.split_once('.') {
                Some((d, rest)) if d.starts_with("deck") && d[4..].parse::<u8>().is_ok() => format!("deck.{rest}"),
                _ => k.clone(),
            };
            if key.starts_with("deck.") && applied.contains(&key) {
                continue;
            }
            let ok = match key.as_str() {
                "view.language" => ["en", "de"].contains(&v.as_str()).then(|| self.view.language = v.clone()).is_some(),
                "view.key_notation" => ["camelot", "openkey", "musical"].contains(&v.as_str()).then(|| self.view.key_notation = v.clone()).is_some(),
                "view.waveform" => ["3band", "blue", "rgb"].contains(&v.as_str()).then(|| self.view.waveform = v.clone()).is_some(),
                "view.phrases" => flag(v).map(|b| self.view.phrases = b).is_some(),
                "view.bar_counter" => BAR_COUNTERS.contains(&v.as_str()).then(|| self.view.bar_counter = v.clone()).is_some(),
                "analysis.bpm_min" => v.parse().map(|n| self.analysis.bpm_min = n).is_ok(),
                "analysis.bpm_max" => v.parse().map(|n| self.analysis.bpm_max = n).is_ok(),
                "analysis.detect_key" => flag(v).map(|b| self.analysis.detect_key = b).is_some(),
                "analysis.detect_phrases" => flag(v).map(|b| self.analysis.detect_phrases = b).is_some(),
                "analysis.auto_analyse" => flag(v).map(|b| self.analysis.auto_analyse = b).is_some(),
                "recording.folder" => (!v.is_empty()).then(|| self.recording.folder = v.clone()).is_some(),
                "recording.auto_split" => flag(v).map(|b| self.recording.auto_split = b).is_some(),
                "perform.quantize" => flag(v).map(|b| self.deck.quantize = b).is_some(),
                "deck.tempo_range" => TEMPO_RANGES.contains(&v.as_str()).then(|| self.deck.tempo_range = v.clone()).is_some(),
                "deck.auto_cue" => flag(v).map(|b| self.deck.auto_cue = b).is_some(),
                "mixer.eq_mode" => EQ_MODES.contains(&v.as_str()).then(|| self.mixer.eq_mode = v.clone()).is_some(),
                "mixer.fader_curve" => FADER_CURVES.contains(&v.as_str()).then(|| self.mixer.fader_curve = v.clone()).is_some(),
                "mixer.crossfader_curve" => CROSSFADER_CURVES.contains(&v.as_str()).then(|| self.mixer.crossfader_curve = v.clone()).is_some(),
                _ => false,
            };
            if ok {
                applied.push(key);
            }
        }
        *self = self.clone().sanitized();
        applied
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
    pub appearance: Option<String>,
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
        if let Some(a) = &self.appearance {
            s.view.appearance = a.clone();
            s = s.sanitized();
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
    fn stick_player_settings_follow_the_deck_defaults() {
        let mut s = Settings::default();
        s.deck.quantize = false;
        s.deck.tempo_range = "sixteen".into();
        s.deck.jog_mode = "bend".into();
        s.view.language = "de".into();
        let p = od_devices::pioneer::player_settings(Some(&stick_player_settings(&s)));
        assert!(!p.quantize && !p.jog_vinyl && p.auto_cue);
        assert_eq!(p.tempo_range, od_format_pioneer::mysetting::TempoRange::Sixteen);
        assert_eq!(p.language, od_format_pioneer::mysetting::Language::German);
    }

    #[test]
    fn json_round_trip() {
        let mut s = Settings::default();
        s.audio.device = "DDJ-FLX10".into();
        s.audio.buffer = 256;
        s.view.language = "de".into();
        s.view.key_notation = "openkey".into();
        s.view.scale = 1.25;
        s.view.high_contrast = true;
        s.view.appearance = "system".into();
        s.analysis.bpm_min = 78;
        s.analysis.bpm_max = 155;
        s.recording.format = "flac".into();
        s.recording.auto_split = true;
        s.controller.mapping_file = "/tmp/my.toml".into();
        s.update.auto_check = false;
        s.update.prereleases = true;
        s.update.skip_version = "0.2.0".into();
        s.update.last_check = 1_790_000_000;
        s.perform.fx_panel = true;
        s.perform.sampler_panel = true;
        s.perform.color_fx = "dub_echo".into();
        s.browser.columns = vec!["title".into(), "album".into(), "bpm".into()];
        s.browser.sort_then = vec![SortSpec { column: "bpm".into(), descending: true }];
        s.browser.font_size = 16;
        s.browser.line_spacing = "roomy".into();
        s.view.zoom_span = 16.0;
        s.deck.tempo_range = "sixteen".into();
        s.deck.quantize = false;
        s.deck.jog_mode = "bend".into();
        s.deck.auto_cue = false;
        s.mixer.eq_mode = "classic".into();
        s.mixer.fader_curve = "linear".into();
        s.mixer.crossfader_curve = "cut".into();
        s.mixer.auto_gain = false;
        s.recording.silence_stop = true;
        s.recording.name_pattern = "Club {date}".into();
        s.recording.gain_db = -6.0;
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json(&Settings::default().to_json()), Settings::default());
    }

    #[test]
    fn missing_and_broken_values_fall_back() {
        let s = Settings::from_json(r#"{"view": {"scale": 9.0, "key_notation": "solfege"}, "audio": {"buffer": 512}}"#);
        assert_eq!(s.view.scale, 1.0);
        assert_eq!(s.view.key_notation, "camelot");
        assert_eq!(s.view.appearance, "dark");
        assert_eq!(s.view.bar_counter, "to-phrase");
        assert_eq!(Settings::from_json(r#"{"view": {"bar_counter": "sideways"}}"#).view.bar_counter, "to-phrase");
        assert_eq!(Settings::from_json(r#"{"view": {"bar_counter": "up"}}"#).view.bar_counter, "up");
        assert_eq!(Settings::from_json(r#"{"view": {"appearance": "sepia"}}"#).view.appearance, "dark");
        assert_eq!(Settings::from_json(r#"{"view": {"appearance": "light"}}"#).view.appearance, "light");
        assert_eq!(s.audio.buffer, 512);
        assert_eq!(s.audio.headphones, "auto");
        assert!(s.analysis.auto_analyse);
        // Settings stored before the update check existed: checks are on.
        assert!(s.update.auto_check && !s.update.prereleases);
        assert_eq!(Settings::from_json("not json"), Settings::default());
        // Panels start closed; an unknown colour FX falls back to the filter.
        assert!(!s.perform.fx_panel && !s.perform.sampler_panel);
        assert_eq!(Settings::from_json(r#"{"perform": {"color_fx": "laser"}}"#).perform.color_fx, "filter");
    }

    #[test]
    fn phrase_display_options() {
        let d = Settings::default();
        assert!(d.view.phrases && d.view.phrases_overview, "phrases on both waveforms by default");
        assert_eq!(d.view.phrase_types, "names");
        // Settings stored before the options existed keep phrases everywhere.
        let old = Settings::from_json(r#"{"view": {"phrases": false}}"#);
        assert!(!old.view.phrases && old.view.phrases_overview && old.view.phrase_types == "names");
        assert_eq!(Settings::from_json(r#"{"view": {"phrase_types": "colors"}}"#).view.phrase_types, "colors");
        assert_eq!(Settings::from_json(r#"{"view": {"phrase_types": "glyphs"}}"#).view.phrase_types, "names");
    }

    #[test]
    fn new_values_fall_back_and_row_heights() {
        let s = Settings::from_json(r#"{"deck": {"tempo_range": "huge", "jog_mode": "spin"}, "mixer": {"eq_mode": "x", "fader_curve": "y", "crossfader_curve": "z"}, "browser": {"font_size": 99, "line_spacing": "airy"}, "view": {"zoom_span": 3.0}, "recording": {"gain_db": 40.0, "name_pattern": " a/b:c "}}"#);
        assert_eq!((s.deck.tempo_range.as_str(), s.deck.jog_mode.as_str()), ("ten", "vinyl"));
        assert_eq!((s.mixer.eq_mode.as_str(), s.mixer.fader_curve.as_str(), s.mixer.crossfader_curve.as_str()), ("isolator", "smooth", "constant_power"));
        assert_eq!((s.browser.font_size, s.browser.line_spacing.as_str(), s.view.zoom_span), (14, "normal", 8.0));
        assert_eq!(s.recording.gain_db, 12.0);
        assert_eq!(s.recording.name_pattern, "abc");
        // Settings stored before these existed keep the defaults.
        let old = Settings::from_json(r#"{"view": {"language": "de"}}"#);
        assert!(old.deck.quantize && old.deck.auto_cue && old.mixer.auto_gain);
        let mut r = Settings::default();
        assert_eq!(r.row_height(), 28.0);
        r.browser.line_spacing = "compact".into();
        assert_eq!(r.row_height(), 22.0);
        r.browser.line_spacing = "roomy".into();
        assert_eq!(r.row_height(), 34.0);
    }

    #[test]
    fn values_from_other_software_are_applied() {
        let mut s = Settings::default();
        let v = |k: &str, x: &str| (k.to_string(), x.to_string());
        let applied = s.apply_values(&[
            v("view.key_notation", "musical"),
            v("view.phrases", "false"),
            v("analysis.bpm_min", "78"),
            v("analysis.bpm_max", "155"),
            v("perform.quantize", "false"),
            v("deck1.tempo_range", "six"),
            v("deck2.tempo_range", "wide"),
            v("deck1.auto_cue", "false"),
            v("deck1.keylock", "true"),
            v("mixer.eq_mode", "classic"),
            v("mixer.fader_curve", "sharp"),
            v("mixer.crossfader_curve", "cut"),
            v("recording.auto_split", "maybe"),
            v("view.time_display", "remain"),
        ]);
        assert_eq!(s.view.key_notation, "musical");
        assert!(!s.view.phrases && !s.deck.quantize && !s.deck.auto_cue);
        assert_eq!((s.analysis.bpm_min, s.analysis.bpm_max), (78, 155));
        assert_eq!(s.deck.tempo_range, "six", "the first deck wins");
        assert_eq!((s.mixer.eq_mode.as_str(), s.mixer.fader_curve.as_str(), s.mixer.crossfader_curve.as_str()), ("classic", "sharp", "cut"));
        assert!(!applied.contains(&"recording.auto_split".to_string()), "broken values are skipped");
        assert!(!applied.iter().any(|k| k.contains("keylock") || k.contains("time_display")), "unknown keys are skipped");
        assert_eq!(applied.len(), 10);
    }

    #[test]
    fn library_meta_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = od_library::Library::open(&dir.path().join("library.sqlite")).unwrap();
        let mut s = Settings::default();
        s.view.phrases = false;
        s.view.phrases_overview = false;
        s.view.phrase_types = "colors".into();
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
