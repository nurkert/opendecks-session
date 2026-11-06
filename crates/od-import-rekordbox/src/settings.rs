//! One-time import of rekordbox preferences (SPEC §7, part of the
//! migration; triggered by the user from the import flow).
//!
//! This module only *reads* and *maps*: [`read`] / [`parse`] turn the
//! rekordbox preference files into a [`RekordboxSettings`] value plus a
//! [`SettingsReport`] that lists every recognised setting as mapped (with
//! the OpenDecks target), engine-only (OpenDecks has the feature but no
//! persisted preference for it yet) or unmapped (with the reason).
//! Applying the values is left to the preferences module, which consumes
//! [`RekordboxSettings::preference_values`] (keys are the JSON paths of the
//! OpenDecks settings, e.g. `view.key_notation`) and
//! [`RekordboxSettings::keymap_toml`] (the `[bindings]` format of the
//! OpenDecks keymap). Nothing in the rekordbox directories is ever written.
//!
//! # Files
//!
//! Found in the rekordbox preference folder (macOS:
//! `~/Library/Application Support/Pioneer/rekordbox6`, also used by
//! rekordbox 7; on Linux pass a copied folder explicitly):
//!
//! * `rekordbox3.settings`, `PlaySettings.xml`: XML of the form
//!   `<PROPERTIES><VALUE name="…" val="…"/>…</PROPERTIES>`. The meaning of
//!   individual names was established by looking at their names and at the
//!   values of a real installation against the options of the rekordbox
//!   preferences window; where the numeric encoding is inferred rather than
//!   documented the report says so (`note`).
//! * `KeyMappings/rekordbox_<id>.mappings`: the keyboard preset selected by
//!   `performaceKeyMapping` in `rekordbox3.settings`. Each
//!   `<MAPPING commandId=… description=… key=…/>` is translated to an
//!   OpenDecks command id where one exists.
//! * `MYSETTING.DAT`, `MYSETTING2.DAT`, `DJMMYSETTING.DAT` ("My Settings"
//!   for players and mixers, also found under `/PIONEER/` on exported
//!   sticks): binary, layout as publicly documented by the pyrekordbox
//!   project (`docs/source/formats/mysetting.md`,
//!   <https://github.com/dylanljones/pyrekordbox>): 104-byte header
//!   (`len_strings` = 96, three 32-byte strings, `len_data` u32 LE at 100),
//!   then the body; every setting is one byte, enums start at `0x80`.
//! * `DEVSETTING.DAT`: not understood publicly; reported as unmapped.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

/// Key display of rekordbox ("Classic" `Am` or "Alphanumeric" `8A`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyDisplay {
    Classic,
    Alphanumeric,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveformColor {
    Blue,
    Rgb,
    ThreeBand,
}

/// Tempo fader range in ± percent; `Wide` is ±100 %.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoRange {
    Six,
    Ten,
    Sixteen,
    Wide,
}

impl TempoRange {
    fn opendecks(self) -> &'static str {
        match self {
            TempoRange::Six => "six",
            TempoRange::Ten => "ten",
            TempoRange::Sixteen => "sixteen",
            TempoRange::Wide => "wide",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotCueAutoLoad {
    Off,
    On,
    /// Follow the per-track setting stored in rekordbox.
    RekordboxSetting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqType {
    /// Classic shelving EQ.
    Eq,
    Isolator,
}

/// Channel fader curve of the DJM "My Settings" (short-throw faders).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelFaderCurve {
    /// Volume rises late, near the top.
    SteepTop,
    Linear,
    /// Volume rises early, near the bottom.
    SteepBottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossfaderCurve {
    Constant,
    SlowCut,
    FastCut,
}

/// One keyboard shortcut of the selected rekordbox keyboard preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// rekordbox command id (hex, as in the file).
    pub command_id: String,
    pub description: String,
    /// The key as rekordbox writes it, e.g. `shift + command + 1`.
    pub key: String,
    /// The same key in OpenDecks' portable notation (`Mod+Shift+1`).
    pub binding: Option<String>,
    /// The OpenDecks command id, if there is an equivalent.
    pub target: Option<String>,
}

/// Everything recognised in the rekordbox preference files. `None` means
/// the setting was not found (or its value was not understood). Per-deck
/// arrays are indexed by deck 1..=4 → 0..=3.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RekordboxSettings {
    /// ISO 639-1 code (only from the documented "My Settings" encoding).
    pub language: Option<String>,
    pub key_notation: Option<KeyDisplay>,
    pub waveform_color: Option<WaveformColor>,
    pub phrase_display: Option<bool>,
    pub quantize: Option<bool>,
    /// Quantize resolution in beats (1, 0.5, 0.25, 0.125).
    pub quantize_beats: Option<f64>,
    /// Raw rekordbox sync mode value (`BeatSyncType`); the encoding is not
    /// documented.
    pub sync_mode: Option<i64>,
    pub tempo_range: [Option<TempoRange>; 4],
    pub auto_cue: [Option<bool>; 4],
    /// Auto cue level in dB (negative), or `None` with `auto_cue_memory`.
    pub auto_cue_level_db: Option<i32>,
    pub auto_cue_memory: bool,
    pub master_tempo: [Option<bool>; 4],
    pub load_lock: Option<bool>,
    pub needle_lock: Option<bool>,
    pub beat_jump_beats: [Option<f64>; 4],
    pub hot_cue_auto_load: Option<HotCueAutoLoad>,
    /// Hot cue colour setting: raw value of `HotCueColorType`
    /// (rekordbox3.settings, undocumented) …
    pub hot_cue_color_type: Option<i64>,
    /// … or the "My Settings" on/off switch for hot cue colours on players.
    pub hot_cue_color: Option<bool>,
    pub eq_type: Option<EqType>,
    pub channel_fader_curve: Option<ChannelFaderCurve>,
    pub crossfader_curve: Option<CrossfaderCurve>,
    /// Headphone cue: pre-EQ (true) or post-EQ.
    pub headphones_pre_eq: Option<bool>,
    /// Headphone mono split (cue left, master right).
    pub headphones_mono_split: Option<bool>,
    /// rekordbox records WAV only; there is no format setting.
    pub recording_folder: Option<PathBuf>,
    pub recording_auto_split: Option<bool>,
    pub analysis_bpm_range: Option<(u32, u32)>,
    pub analysis_key: Option<bool>,
    pub analysis_phrases: Option<bool>,
    pub auto_analyse: Option<bool>,
    pub keymap_name: Option<String>,
    pub shortcuts: Vec<Shortcut>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStatus {
    /// Stored as an OpenDecks preference (`target` is its key).
    Mapped,
    /// OpenDecks has the function (`target` names the engine command) but no
    /// persisted preference yet.
    EngineOnly,
    /// No OpenDecks equivalent (`note` says why).
    Unmapped,
}

impl fmt::Display for MapStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MapStatus::Mapped => "mapped",
            MapStatus::EngineOnly => "engine-only",
            MapStatus::Unmapped => "unmapped",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingEntry {
    /// Human name of the setting.
    pub setting: String,
    /// `file: name` it came from.
    pub source: String,
    /// The decoded value.
    pub value: String,
    pub status: MapStatus,
    /// OpenDecks preference key or engine command (empty when unmapped).
    pub target: String,
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsReport {
    /// Files that were read.
    pub files: Vec<PathBuf>,
    pub entries: Vec<MappingEntry>,
    /// Files present but not imported, with the reason.
    pub unmapped_files: Vec<(String, String)>,
    /// Names in the XML preference files that are not preferences OpenDecks
    /// could use (window geometry, accounts, video, lighting, streaming …).
    /// Values are never kept: some of them are account tokens.
    pub ignored_keys: Vec<String>,
}

impl SettingsReport {
    pub fn count(&self, status: MapStatus) -> usize {
        self.entries.iter().filter(|e| e.status == status).count()
    }
}

/// The contents of the preference files (whatever exists).
#[derive(Clone, Debug, Default)]
pub struct SettingsFiles {
    pub rekordbox3_settings: Option<String>,
    pub play_settings: Option<String>,
    /// The selected `KeyMappings/*.mappings` file.
    pub key_mappings: Option<String>,
    pub mysetting: Option<Vec<u8>>,
    pub mysetting2: Option<Vec<u8>>,
    pub djmmysetting: Option<Vec<u8>>,
    pub devsetting: Option<Vec<u8>>,
    /// Other files present (names only), reported as not imported.
    pub other: Vec<String>,
    pub paths: Vec<PathBuf>,
}

/// The rekordbox preference folder of this user (macOS only).
pub fn default_settings_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    ["rekordbox7", "rekordbox6"]
        .iter()
        .map(|v| home.join("Library/Application Support/Pioneer").join(v))
        .find(|d| d.join("rekordbox3.settings").is_file())
}

/// Files that hold settings OpenDecks has no equivalent for.
const OTHER_FILES: &[(&str, &str)] = &[
    ("MidiMappings", "controller MIDI mappings: OpenDecks ships its own controller mappings"),
    ("pad", "custom pad layouts: no equivalent"),
    ("FxUnitSettings7.xml", "effect unit presets: OpenDecks effects differ"),
    ("PadFxSettings.xml", "Pad FX presets: OpenDecks effects differ"),
    ("MFXSettings.xml", "Merge FX presets: no equivalent"),
    ("SamplerSettings1.xml", "sampler bank: OpenDecks sampler banks are stored in the library"),
    ("GrooveCircuitSettings.xml", "Groove Circuit: no equivalent"),
    ("VideoSettings.xml", "video: out of scope"),
    ("LightingDB", "lighting: out of scope"),
    ("EditSettings.xml", "edit mode: no equivalent"),
    ("browseSetting.xml", "browser column layout: OpenDecks has its own browser layout"),
];

/// Reads the preference files of `dir` (and of `dir/PIONEER`, where a stick
/// keeps its "My Settings"). Read-only.
pub fn load_files(dir: &Path) -> std::io::Result<SettingsFiles> {
    let mut f = SettingsFiles::default();
    let read_text = |name: &str, paths: &mut Vec<PathBuf>| -> Option<String> {
        let p = dir.join(name);
        let bytes = std::fs::read(&p).ok()?;
        paths.push(p);
        Some(String::from_utf8_lossy(&bytes).into_owned())
    };
    f.rekordbox3_settings = read_text("rekordbox3.settings", &mut f.paths);
    f.play_settings = read_text("PlaySettings.xml", &mut f.paths);
    if let Some(id) = f.rekordbox3_settings.as_deref().and_then(|s| values(s).remove("performaceKeyMapping"))
        && !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        f.key_mappings = read_text(&format!("KeyMappings/rekordbox_{id}.mappings"), &mut f.paths);
    }
    let mut read_bin = |name: &str| -> Option<Vec<u8>> {
        for d in [dir.to_path_buf(), dir.join("PIONEER")] {
            let p = d.join(name);
            if let Ok(b) = std::fs::read(&p) {
                f.paths.push(p);
                return Some(b);
            }
        }
        None
    };
    let my = read_bin("MYSETTING.DAT");
    let my2 = read_bin("MYSETTING2.DAT");
    let djm = read_bin("DJMMYSETTING.DAT");
    let dev = read_bin("DEVSETTING.DAT");
    f.mysetting = my;
    f.mysetting2 = my2;
    f.djmmysetting = djm;
    f.devsetting = dev;
    for (name, _) in OTHER_FILES {
        if dir.join(name).exists() {
            f.other.push((*name).to_string());
        }
    }
    if f.paths.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no rekordbox preference files in {}", dir.display()),
        ));
    }
    Ok(f)
}

/// Reads and maps the settings of `dir`, or of [`default_settings_dir`].
pub fn read(dir: Option<&Path>) -> std::io::Result<(RekordboxSettings, SettingsReport)> {
    let dir = match dir {
        Some(d) => d.to_path_buf(),
        None => default_settings_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no rekordbox preference folder found")
        })?,
    };
    Ok(parse(&load_files(&dir)?))
}

// ───────────────────────────── XML ─────────────────────────────

fn unescape(s: &str) -> String {
    let mut out = s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
    // Numeric entities (`&#96;` is how rekordbox writes a backtick).
    while let Some(i) = out.find("&#") {
        let Some(len) = out[i..].find(';') else { break };
        let num = &out[i + 2..i + len];
        let code = if let Some(h) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(h, 16).ok()
        } else {
            num.parse().ok()
        };
        let Some(c) = code.and_then(char::from_u32) else { break };
        out.replace_range(i..i + len + 1, &c.to_string());
    }
    out.replace("&amp;", "&")
}

/// Attributes of every element named `tag`, in document order.
fn elements(xml: &str, tag: &str) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        // `<VALUE` must not match `<VALUES`.
        if !after.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else { break };
        let mut attrs = HashMap::new();
        let mut s = &after[..end];
        while let Some(eq) = s.find('=') {
            let name = s[..eq].trim().to_string();
            let v = s[eq + 1..].trim_start();
            let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(close) = v[1..].find(q) else { break };
            attrs.insert(name, unescape(&v[1..1 + close]));
            s = &v[close + 2..];
        }
        out.push(attrs);
        rest = &after[end..];
    }
    out
}

/// `name → val` of a `<PROPERTIES>` file (entries without `val` are skipped).
fn values(xml: &str) -> HashMap<String, String> {
    elements(xml, "VALUE")
        .into_iter()
        .filter_map(|mut a| Some((a.remove("name")?, a.remove("val")?)))
        .collect()
}

// ───────────────────────── My Settings ─────────────────────────

/// The data block of a "My Settings" file, after validating the header.
fn my_settings_body(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 104 || u32::from_le_bytes(bytes[0..4].try_into().ok()?) != 96 {
        return None;
    }
    let len = u32::from_le_bytes(bytes[100..104].try_into().ok()?) as usize;
    bytes.get(104..104 + len)
}

/// Byte `n` (1-based, as in the documentation) of a body.
fn byte(body: &[u8], n: usize) -> Option<u8> {
    body.get(n - 1).copied()
}

fn on_off(v: u8) -> Option<bool> {
    match v {
        0x80 => Some(false),
        0x81 => Some(true),
        _ => None,
    }
}

const MY_LANGUAGES: &[(u8, &str)] = &[
    (0x80, "en"),
    (0x81, "fr"),
    (0x82, "en"),
    (0x83, "de"),
    (0x84, "it"),
    (0x85, "nl"),
    (0x86, "es"),
    (0x87, "ru"),
    (0x88, "ko"),
    (0x89, "zh-Hans"),
    (0x8A, "zh-Hant"),
    (0x8B, "ja"),
    (0x8C, "pt"),
    (0x8D, "sv"),
    (0x8E, "cs"),
    (0x8F, "hu"),
    (0x90, "da"),
    (0x91, "el"),
    (0x92, "tr"),
];

/// rekordbox analysis BPM ranges in the order of its preferences menu.
const BPM_RANGES: [(u32, u32); 5] = [(58, 115), (68, 135), (78, 155), (88, 175), (98, 195)];

// ─────────────────────────── Keymap ────────────────────────────

/// rekordbox deck command (low byte of `commandId`) → OpenDecks deck
/// command suffix. Deck 1 is `30xx`, deck 2 `31xx` (decks 3/4 are assumed
/// to continue as `32xx`/`33xx`).
const DECK_COMMANDS: &[(u8, &str)] = &[
    (0x06, "play"),
    (0x07, "cue"),
    (0x57, "start"),
    (0x08, "jump.fwd"),
    (0x09, "jump.back"),
    (0x4b, "sync"),
    (0x4c, "master"),
    (0x4d, "keylock"),
    (0x4f, "bend.up"),
    (0x50, "bend.down"),
    (0x5b, "tempo.up"),
    (0x5e, "tempo.down"),
    (0x39, "memory.next"),
    (0x3a, "memory.prev"),
    (0x1e, "hotcue.a"),
    (0x1f, "hotcue.b"),
    (0x20, "hotcue.c"),
    (0x54, "hotcue.d"),
    (0x21, "hotcue.clear.a"),
    (0x22, "hotcue.clear.b"),
    (0x23, "hotcue.clear.c"),
    (0x55, "hotcue.clear.d"),
    (0x0a, "loop.in"),
    (0x0b, "loop.out"),
    (0x0c, "loop.exit"),
    (0x18, "loop.half"),
    (0x19, "loop.double"),
    (0xa5, "loop.auto"),
    (0x11, "loop.1_2"),
    (0x12, "loop.1"),
    (0x13, "loop.2"),
    (0x14, "loop.4"),
    (0x15, "loop.8"),
];

/// Global rekordbox commands with an OpenDecks equivalent.
const GLOBAL_COMMANDS: &[(u16, &str)] = &[
    (0x7000, "browser.search"),
    (0x7003, "browser.search"),
    (0x500a, "browser.new.playlist"),
    (0xb129, "browser.load.deck1"),
    (0xb12a, "browser.load.deck2"),
    (0xd0f0, "record.toggle"),
    // rekordbox's 16-slot sampler: slots 1-4 sit under deck 1, 9-12 under
    // deck 2; OpenDecks has 8 slots in one row.
    (0xf000, "sampler.slot1"),
    (0xf001, "sampler.slot2"),
    (0xf002, "sampler.slot3"),
    (0xf003, "sampler.slot4"),
    (0xf008, "sampler.slot5"),
    (0xf009, "sampler.slot6"),
    (0xf00a, "sampler.slot7"),
    (0xf00b, "sampler.slot8"),
];

fn command_target(id: &str) -> Option<String> {
    let n = u16::from_str_radix(id.trim(), 16).ok()?;
    if let Some((_, t)) = GLOBAL_COMMANDS.iter().find(|(c, _)| *c == n) {
        return Some((*t).to_string());
    }
    let deck = match n >> 8 {
        0x30..=0x33 => (n >> 8) - 0x2f,
        _ => return None,
    };
    DECK_COMMANDS.iter().find(|(c, _)| *c as u16 == n & 0xff).map(|(_, s)| format!("deck{deck}.{s}"))
}

/// rekordbox key text (`shift + command + cursor left`) → OpenDecks
/// portable binding (`Mod+Shift+Left`). macOS: `command` is the primary
/// modifier (`Mod`), `ctrl` the secondary (`Meta`).
pub fn translate_key(key: &str) -> Option<String> {
    let (mut primary, mut secondary, mut alt, mut shift) = (false, false, false, false);
    let mut main = None;
    for part in key.split(" + ").map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "command" | "cmd" => primary = true,
            "ctrl" | "control" => secondary = true,
            "option" | "alt" => alt = true,
            "shift" => shift = true,
            other => {
                if main.is_some() {
                    return None;
                }
                main = Some(match other {
                    "cursor left" | "left" => "Left".to_string(),
                    "cursor right" | "right" => "Right".to_string(),
                    "cursor up" | "up" => "Up".to_string(),
                    "cursor down" | "down" => "Down".to_string(),
                    "spacebar" | "space" => "Space".to_string(),
                    "return" | "enter" => "Return".to_string(),
                    "tab" => "Tab".to_string(),
                    "escape" | "esc" => "Escape".to_string(),
                    "backspace" => "Backspace".to_string(),
                    "delete" | "del" => "Delete".to_string(),
                    "home" => "Home".to_string(),
                    "end" => "End".to_string(),
                    "page up" => "PageUp".to_string(),
                    "page down" => "PageDown".to_string(),
                    f if f.len() >= 2
                        && f.starts_with('f')
                        && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) =>
                    {
                        f.to_ascii_uppercase()
                    }
                    _ if part.chars().count() == 1 => part.to_uppercase(),
                    _ => return None,
                });
            }
        }
    }
    let mut out = String::new();
    for (on, m) in [(primary, "Mod+"), (secondary, "Meta+"), (alt, "Alt+"), (shift, "Shift+")] {
        if on {
            out.push_str(m);
        }
    }
    out.push_str(&main?);
    Some(out)
}

// ─────────────────────────── Parsing ───────────────────────────

struct Builder {
    s: RekordboxSettings,
    r: SettingsReport,
}

impl Builder {
    fn entry(&mut self, setting: &str, source: &str, value: impl Into<String>, status: MapStatus, target: &str, note: &str) {
        self.r.entries.push(MappingEntry {
            setting: setting.into(),
            source: source.into(),
            value: value.into(),
            status,
            target: target.into(),
            note: note.into(),
        });
    }
}

fn int(v: &HashMap<String, String>, name: &str) -> Option<i64> {
    v.get(name)?.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f as i64)
}

fn flag(v: &HashMap<String, String>, name: &str) -> Option<bool> {
    int(v, name).map(|n| n != 0)
}

fn yes(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Names in `rekordbox3.settings` that [`parse`] interprets.
const HANDLED: &[&str] = &[
    "KeyStringSetting",
    "ColorType",
    "showPhrase",
    "BeatSyncType",
    "DeckLoadLock",
    "DeckNeedleLock",
    "hotCueAutoLoad",
    "HotCueColorType",
    "EqualizerType",
    "ChFaderCurve",
    "CrossFaderCurve",
    "HeadphoneStereoMonoSplit",
    "RecordSaveDirectory",
    "RecAutoTrackSplit_PerformanceMode",
    "trackAnalysisBpmRange",
    "enableKeyAnalysis",
    "trackAnalysisSettingsSongStruct",
    "AutoAnalysis",
    "AutoCueLeveldB",
    "language",
    "performaceKeyMapping",
];

/// Maps the contents of the preference files. Pure: no file access.
pub fn parse(files: &SettingsFiles) -> (RekordboxSettings, SettingsReport) {
    let mut b = Builder { s: RekordboxSettings::default(), r: SettingsReport::default() };
    b.r.files = files.paths.clone();
    if let Some(xml) = &files.rekordbox3_settings {
        parse_rekordbox3(&mut b, &values(xml));
    }
    if let Some(xml) = &files.play_settings {
        parse_play_settings(&mut b, &values(xml));
    }
    if let Some(xml) = &files.key_mappings {
        parse_key_mappings(&mut b, xml);
    }
    if let Some(body) = files.mysetting.as_deref().and_then(my_settings_body) {
        parse_mysetting(&mut b, body);
    } else if files.mysetting.is_some() {
        b.r.unmapped_files.push(("MYSETTING.DAT".into(), "unexpected header".into()));
    }
    if let Some(body) = files.mysetting2.as_deref().and_then(my_settings_body) {
        parse_mysetting2(&mut b, body);
    } else if files.mysetting2.is_some() {
        b.r.unmapped_files.push(("MYSETTING2.DAT".into(), "unexpected header".into()));
    }
    if let Some(body) = files.djmmysetting.as_deref().and_then(my_settings_body) {
        parse_djm(&mut b, body);
    } else if files.djmmysetting.is_some() {
        b.r.unmapped_files.push(("DJMMYSETTING.DAT".into(), "unexpected header".into()));
    }
    if files.devsetting.is_some() {
        b.r.unmapped_files
            .push(("DEVSETTING.DAT".into(), "device settings: layout not publicly documented".into()));
    }
    for name in &files.other {
        if let Some((_, why)) = OTHER_FILES.iter().find(|(n, _)| n == name) {
            b.r.unmapped_files.push((name.clone(), (*why).into()));
        }
    }
    b.r.ignored_keys.sort();
    b.r.ignored_keys.dedup();
    (b.s, b.r)
}

fn is_per_deck_handled(name: &str) -> bool {
    let deck_keys = ["_Quantize", "_BeatJumpBeat", "_MasterTempo"];
    (name.starts_with("Player") && deck_keys.iter().any(|k| name.ends_with(k)))
        || name.starts_with("AutoCueEnableDeck")
}

fn parse_rekordbox3(b: &mut Builder, v: &HashMap<String, String>) {
    const F: &str = "rekordbox3.settings";
    let src = |n: &str| format!("{F}: {n}");

    if let Some(n) = int(v, "language") {
        b.entry(
            "Language",
            &src("language"),
            n.to_string(),
            MapStatus::Unmapped,
            "",
            "rekordbox's language index is not documented; set the language in OpenDecks (en/de)",
        );
    }
    if let Some(n) = int(v, "KeyStringSetting") {
        let k = match n {
            1 => Some(KeyDisplay::Classic),
            2 => Some(KeyDisplay::Alphanumeric),
            _ => None,
        };
        match k {
            Some(k) => {
                b.s.key_notation = Some(k);
                let (val, od) =
                    if k == KeyDisplay::Classic { ("Classic", "musical") } else { ("Alphanumeric", "camelot") };
                b.entry(
                    "Key notation",
                    &src("KeyStringSetting"),
                    format!("{val} ({n})"),
                    MapStatus::Mapped,
                    &format!("view.key_notation = {od}"),
                    "encoding inferred: 1 = Classic, 2 = Alphanumeric",
                );
            }
            None => b.entry("Key notation", &src("KeyStringSetting"), n.to_string(), MapStatus::Unmapped, "", "unknown value"),
        }
    }
    if let Some(n) = int(v, "ColorType") {
        let c = match n {
            1 => Some(WaveformColor::Blue),
            2 => Some(WaveformColor::Rgb),
            3 => Some(WaveformColor::ThreeBand),
            _ => None,
        };
        match c {
            Some(WaveformColor::Blue) => b.entry(
                "Waveform colour",
                &src("ColorType"),
                "Blue (1)",
                MapStatus::Mapped,
                "view.waveform = blue",
                "encoding inferred from the menu order Blue/RGB/3Band",
            ),
            Some(WaveformColor::ThreeBand) => b.entry(
                "Waveform colour",
                &src("ColorType"),
                "3Band (3)",
                MapStatus::Mapped,
                "view.waveform = 3band",
                "encoding inferred from the menu order Blue/RGB/3Band",
            ),
            Some(WaveformColor::Rgb) => b.entry(
                "Waveform colour",
                &src("ColorType"),
                "RGB (2)",
                MapStatus::Mapped,
                "view.waveform = 3band",
                "OpenDecks has no RGB view; 3-band is the closest coloured view",
            ),
            None => b.entry("Waveform colour", &src("ColorType"), n.to_string(), MapStatus::Unmapped, "", "unknown value"),
        }
        b.s.waveform_color = c;
    }
    if let Some(on) = flag(v, "showPhrase") {
        b.s.phrase_display = Some(on);
        b.entry("Phrase display", &src("showPhrase"), yes(on), MapStatus::Mapped, &format!("view.phrases = {on}"), "");
    }

    // Quantize: one flag per deck (Player1..4); OpenDecks quantize is global.
    let q: Vec<bool> = (1..=4).filter_map(|d| flag(v, &format!("Player{d}_Quantize"))).collect();
    if !q.is_empty() {
        let on = q.iter().any(|x| *x);
        b.s.quantize = Some(on);
        b.entry(
            "Quantize",
            &src("Player1..4_Quantize"),
            q.iter().map(|x| yes(*x)).collect::<Vec<_>>().join("/"),
            MapStatus::EngineOnly,
            &format!("perform.quantize = {on} (Command::SetQuantize)"),
            "OpenDecks quantize is global: on if any deck had it on",
        );
    }
    if let Some(n) = int(v, "BeatSyncType") {
        b.s.sync_mode = Some(n);
        b.entry(
            "Sync mode",
            &src("BeatSyncType"),
            n.to_string(),
            MapStatus::Unmapped,
            "",
            "OpenDecks has one sync mode (beat sync with phase); rekordbox's alternatives have no equivalent",
        );
    }
    let ac: Vec<(usize, bool)> =
        (1..=4).filter_map(|d| flag(v, &format!("AutoCueEnableDeck{d}")).map(|x| (d, x))).collect();
    for (d, on) in &ac {
        b.s.auto_cue[d - 1] = Some(*on);
    }
    if !ac.is_empty() {
        b.entry(
            "Auto cue",
            &src("AutoCueEnableDeck1..4"),
            ac.iter().map(|(_, x)| yes(*x)).collect::<Vec<_>>().join("/"),
            MapStatus::EngineOnly,
            "deck.auto_cue (DeckCommand::SetAutoCue)",
            "per deck",
        );
    }
    if let Some(n) = int(v, "AutoCueLeveldB") {
        b.entry(
            "Auto cue level",
            &src("AutoCueLeveldB"),
            n.to_string(),
            MapStatus::Unmapped,
            "",
            "OpenDecks uses a fixed auto-cue threshold",
        );
    }
    let mt: Vec<(usize, bool)> =
        (0..4).filter_map(|d| flag(v, &format!("Player{d}_MasterTempo")).map(|x| (d, x))).collect();
    for (d, on) in &mt {
        b.s.master_tempo[*d] = Some(*on);
    }
    if !mt.is_empty() {
        b.entry(
            "Master tempo (key lock)",
            &src("Player0..3_MasterTempo"),
            mt.iter().map(|(_, x)| yes(*x)).collect::<Vec<_>>().join("/"),
            MapStatus::EngineOnly,
            "deck.keylock (DeckCommand::SetKeylock)",
            "per deck",
        );
    }
    let bj: Vec<(usize, i64)> = (1..=4)
        .filter_map(|d| int(v, &format!("Player{d}_BeatJumpBeat")).filter(|n| *n > 0).map(|x| (d, x)))
        .collect();
    for (d, n) in &bj {
        b.s.beat_jump_beats[d - 1] = Some(*n as f64);
    }
    if !bj.is_empty() {
        b.entry(
            "Beat jump size",
            &src("Player1..4_BeatJumpBeat"),
            bj.iter().map(|(_, n)| n.to_string()).collect::<Vec<_>>().join("/"),
            MapStatus::EngineOnly,
            "deck.beat_jump (DeckCommand::SetBeatJumpSize)",
            "beats, per deck",
        );
    }
    if let Some(on) = flag(v, "DeckLoadLock") {
        b.s.load_lock = Some(on);
        b.entry(
            "Load lock (no loading on a playing deck)",
            &src("DeckLoadLock"),
            yes(on),
            MapStatus::Unmapped,
            "",
            "OpenDecks has no load lock yet",
        );
    }
    if let Some(on) = flag(v, "DeckNeedleLock") {
        b.s.needle_lock = Some(on);
        b.entry(
            "Needle lock",
            &src("DeckNeedleLock"),
            yes(on),
            MapStatus::Unmapped,
            "",
            "OpenDecks has no needle search lock",
        );
    }
    if let Some(on) = flag(v, "hotCueAutoLoad") {
        b.s.hot_cue_auto_load = Some(if on { HotCueAutoLoad::On } else { HotCueAutoLoad::Off });
        b.entry(
            "Hot cue auto load",
            &src("hotCueAutoLoad"),
            yes(on),
            MapStatus::Unmapped,
            "",
            "OpenDecks always loads a track's stored hot cues (behaves like On)",
        );
    }
    if let Some(n) = int(v, "HotCueColorType") {
        b.s.hot_cue_color_type = Some(n);
        b.entry(
            "Default hot cue colour",
            &src("HotCueColorType"),
            n.to_string(),
            MapStatus::Unmapped,
            "",
            "encoding not documented; OpenDecks colours new hot cues by slot",
        );
    }
    if let Some(n) = int(v, "EqualizerType") {
        let t = match n {
            0 => Some(EqType::Eq),
            1 => Some(EqType::Isolator),
            _ => None,
        };
        b.s.eq_type = t;
        match t {
            Some(t) => {
                let (val, od) = if t == EqType::Eq { ("EQ", "classic") } else { ("Isolator", "isolator") };
                b.entry(
                    "Mixer EQ type",
                    &src("EqualizerType"),
                    format!("{val} ({n})"),
                    MapStatus::EngineOnly,
                    &format!("mixer.eq_mode = {od} (ChannelCommand::SetEqMode)"),
                    "encoding inferred: 0 = EQ, 1 = Isolator",
                );
            }
            None => b.entry("Mixer EQ type", &src("EqualizerType"), n.to_string(), MapStatus::Unmapped, "", "unknown value"),
        }
    }
    for (name, setting) in [("ChFaderCurve", "Channel fader curve (internal mixer)"), ("CrossFaderCurve", "Crossfader curve (internal mixer)")] {
        if let Some(n) = int(v, name) {
            b.entry(
                setting,
                &src(name),
                n.to_string(),
                MapStatus::Unmapped,
                "",
                "rekordbox's internal-mixer curve value is not documented; the DJMMYSETTING.DAT curves are used when present",
            );
        }
    }
    if let Some(on) = flag(v, "HeadphoneStereoMonoSplit") {
        b.s.headphones_mono_split = Some(on);
        b.entry(
            "Headphones mono split",
            &src("HeadphoneStereoMonoSplit"),
            yes(on),
            MapStatus::Unmapped,
            "",
            "OpenDecks has a cue/master mix knob, no mono split mode",
        );
    }
    if let Some(dir) = v.get("RecordSaveDirectory").filter(|d| !d.trim().is_empty()) {
        b.s.recording_folder = Some(PathBuf::from(dir));
        b.entry(
            "Recording folder",
            &src("RecordSaveDirectory"),
            dir.clone(),
            MapStatus::Mapped,
            &format!("recording.folder = {dir}"),
            "rekordbox records WAV only (no format setting); recording.format stays as is",
        );
    }
    if let Some(on) = flag(v, "RecAutoTrackSplit_PerformanceMode") {
        b.s.recording_auto_split = Some(on);
        b.entry(
            "Recording: split into tracks",
            &src("RecAutoTrackSplit_PerformanceMode"),
            yes(on),
            MapStatus::Mapped,
            &format!("recording.auto_split = {on}"),
            "",
        );
    }
    if let Some(n) = int(v, "trackAnalysisBpmRange") {
        match BPM_RANGES.get(n as usize).filter(|_| n >= 0) {
            Some(&(lo, hi)) => {
                b.s.analysis_bpm_range = Some((lo, hi));
                b.entry(
                    "Analysis BPM range",
                    &src("trackAnalysisBpmRange"),
                    format!("{lo}-{hi} ({n})"),
                    MapStatus::Mapped,
                    &format!("analysis.bpm_min = {lo}, analysis.bpm_max = {hi}"),
                    "encoding inferred: index into 58-115/68-135/78-155/88-175/98-195",
                );
            }
            None => b.entry(
                "Analysis BPM range",
                &src("trackAnalysisBpmRange"),
                n.to_string(),
                MapStatus::Unmapped,
                "",
                "unknown value",
            ),
        }
    }
    for (name, setting, target) in [
        ("enableKeyAnalysis", "Analyse key", "analysis.detect_key"),
        ("trackAnalysisSettingsSongStruct", "Analyse phrases", "analysis.detect_phrases"),
        ("AutoAnalysis", "Analyse new tracks automatically", "analysis.auto_analyse"),
    ] {
        if let Some(on) = flag(v, name) {
            match name {
                "enableKeyAnalysis" => b.s.analysis_key = Some(on),
                "trackAnalysisSettingsSongStruct" => b.s.analysis_phrases = Some(on),
                _ => b.s.auto_analyse = Some(on),
            }
            b.entry(setting, &src(name), yes(on), MapStatus::Mapped, &format!("{target} = {on}"), "");
        }
    }
    for k in v.keys() {
        if !HANDLED.contains(&k.as_str()) && !is_per_deck_handled(k) {
            b.r.ignored_keys.push(format!("{F}: {k}"));
        }
    }
}

fn parse_play_settings(b: &mut Builder, v: &HashMap<String, String>) {
    let mut vals = Vec::new();
    for d in 0..4 {
        let Some(n) = int(v, &format!("TempoRange{d}")) else { continue };
        let r = match n {
            6 => Some(TempoRange::Six),
            10 => Some(TempoRange::Ten),
            16 => Some(TempoRange::Sixteen),
            100 => Some(TempoRange::Wide),
            _ => None,
        };
        b.s.tempo_range[d] = r;
        vals.push(match r {
            Some(TempoRange::Wide) => "wide".to_string(),
            Some(_) => format!("±{n}%"),
            None => format!("?{n}"),
        });
    }
    if !vals.is_empty() {
        b.entry(
            "Tempo range",
            "PlaySettings.xml: TempoRange0..3",
            vals.join("/"),
            MapStatus::EngineOnly,
            "deck.tempo_range (DeckCommand::SetTempoRange)",
            "per deck",
        );
    }
    for k in v.keys() {
        if !k.starts_with("TempoRange") {
            // Last deck state (play status, loop, tempo …): session state,
            // not preferences.
            b.r.ignored_keys.push(format!("PlaySettings.xml: {k}"));
        }
    }
}

fn parse_key_mappings(b: &mut Builder, xml: &str) {
    let name = values(xml).remove("keyMappingName");
    let mut mapped = 0;
    for m in elements(xml, "MAPPING") {
        let (Some(id), Some(key)) = (m.get("commandId"), m.get("key")) else { continue };
        let description = m.get("description").map(|d| d.trim().to_string()).unwrap_or_default();
        let binding = translate_key(key);
        let target = command_target(id);
        if binding.is_some() && target.is_some() {
            mapped += 1;
        }
        b.s.shortcuts.push(Shortcut {
            command_id: id.clone(),
            description,
            key: key.clone(),
            binding,
            target,
        });
    }
    let total = b.s.shortcuts.len();
    b.entry(
        "Keyboard shortcuts",
        &format!("KeyMappings: {}", name.as_deref().unwrap_or("?")),
        format!("{mapped} of {total} shortcuts"),
        if mapped > 0 { MapStatus::Mapped } else { MapStatus::Unmapped },
        "keymap [bindings]",
        "only shortcuts with an OpenDecks command; see the shortcut list",
    );
    b.s.keymap_name = name;
}

fn parse_mysetting(b: &mut Builder, body: &[u8]) {
    const F: &str = "MYSETTING.DAT";
    let src = |n: &str| format!("{F}: {n}");
    if let Some(on) = byte(body, 11).and_then(on_off)
        && b.s.quantize.is_none()
    {
        b.s.quantize = Some(on);
        b.entry("Quantize", &src("quantize"), yes(on), MapStatus::EngineOnly, &format!("perform.quantize = {on}"), "");
    }
    if let Some(v) = byte(body, 12) {
        match v {
            0x80..=0x87 => {
                let db = -36 - 6 * (v - 0x80) as i32;
                b.s.auto_cue_level_db = Some(db);
                b.entry("Auto cue level", &src("auto_cue_level"), format!("{db} dB"), MapStatus::Unmapped, "", "OpenDecks uses a fixed auto-cue threshold");
            }
            0x88 => {
                b.s.auto_cue_memory = true;
                b.entry("Auto cue level", &src("auto_cue_level"), "memory", MapStatus::Unmapped, "", "OpenDecks uses a fixed auto-cue threshold");
            }
            _ => {}
        }
    }
    if let Some((_, code)) = byte(body, 13).and_then(|v| MY_LANGUAGES.iter().find(|(c, _)| *c == v)) {
        b.s.language = Some((*code).to_string());
        if *code == "en" || *code == "de" {
            b.entry("Language", &src("language"), *code, MapStatus::Mapped, &format!("view.language = {code}"), "");
        } else {
            b.entry("Language", &src("language"), *code, MapStatus::Unmapped, "", "OpenDecks is translated to English and German only");
        }
    }
    if let Some(on) = byte(body, 23).and_then(on_off) {
        b.entry("Sync on load", &src("sync"), yes(on), MapStatus::Unmapped, "", "OpenDecks does not enable sync on load");
    }
    if let Some(v) = byte(body, 25)
        && let Some(q) = [1.0, 0.5, 0.25, 0.125].get(v.wrapping_sub(0x80) as usize)
    {
        b.s.quantize_beats = Some(*q);
        b.entry("Quantize beat value", &src("quantize_beat_value"), format!("{q} beat"), MapStatus::Unmapped, "", "OpenDecks quantizes to whole beats");
    }
    if let Some(v) = byte(body, 26) {
        let a = match v {
            0x80 => Some(HotCueAutoLoad::Off),
            0x81 => Some(HotCueAutoLoad::On),
            0x83 => Some(HotCueAutoLoad::RekordboxSetting),
            _ => None,
        };
        if let (Some(a), None) = (a, b.s.hot_cue_auto_load) {
            b.s.hot_cue_auto_load = Some(a);
            b.entry("Hot cue auto load", &src("hotcue_autoload"), format!("{a:?}"), MapStatus::Unmapped, "", "OpenDecks always loads a track's stored hot cues");
        }
    }
    if let Some(on) = byte(body, 27).and_then(on_off) {
        b.s.hot_cue_color = Some(on);
        b.entry("Hot cue colours on players", &src("hotcue_color"), yes(on), MapStatus::Unmapped, "", "OpenDecks always shows hot cue colours");
    }
    if let Some(on) = byte(body, 30).and_then(on_off)
        && b.s.needle_lock.is_none()
    {
        b.s.needle_lock = Some(on);
        b.entry("Needle lock", &src("needle_lock"), yes(on), MapStatus::Unmapped, "", "OpenDecks has no needle search lock");
    }
    if let Some(on) = byte(body, 35).and_then(on_off)
        && b.s.auto_cue.iter().all(Option::is_none)
    {
        b.s.auto_cue = [Some(on); 4];
        b.entry("Auto cue", &src("auto_cue"), yes(on), MapStatus::EngineOnly, "deck.auto_cue (DeckCommand::SetAutoCue)", "all decks");
    }
    if let Some(on) = byte(body, 36).and_then(on_off)
        && b.s.master_tempo.iter().all(Option::is_none)
    {
        b.s.master_tempo = [Some(on); 4];
        b.entry("Master tempo (key lock)", &src("master_tempo"), yes(on), MapStatus::EngineOnly, "deck.keylock (DeckCommand::SetKeylock)", "all decks");
    }
    if let Some(v) = byte(body, 37) {
        let r = [TempoRange::Six, TempoRange::Ten, TempoRange::Sixteen, TempoRange::Wide].get(v.wrapping_sub(0x80) as usize).copied();
        if let (Some(r), true) = (r, b.s.tempo_range.iter().all(Option::is_none)) {
            b.s.tempo_range = [Some(r); 4];
            b.entry("Tempo range", &src("tempo_range"), r.opendecks(), MapStatus::EngineOnly, "deck.tempo_range (DeckCommand::SetTempoRange)", "all decks");
        }
    }
    for (n, name) in [(9, "on_air_display"), (10, "lcd_brightness"), (15, "jog_ring_brightness"), (16, "jog_ring_indicator"), (17, "slip_flashing"), (21, "disc_slot_illumination"), (22, "eject_lock"), (24, "play_mode"), (33, "time_mode"), (34, "jog_mode"), (38, "phase_meter")] {
        if byte(body, n).is_some_and(|v| v >= 0x80) {
            b.r.ignored_keys.push(format!("{F}: {name}"));
        }
    }
}

fn parse_mysetting2(b: &mut Builder, body: &[u8]) {
    const F: &str = "MYSETTING2.DAT";
    if let Some(v) = byte(body, 5)
        && b.s.phrase_display.is_none() && (v == 0x80 || v == 0x81)
    {
        let on = v == 0x81;
        b.s.phrase_display = Some(on);
        b.entry("Phrase display", &format!("{F}: waveform_divisions"), if on { "phrase" } else { "time scale" }, MapStatus::Mapped, &format!("view.phrases = {on}"), "");
    }
    if let Some(v) = byte(body, 13) {
        let beats = [0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0].get(v.wrapping_sub(0x80) as usize).copied();
        if let (Some(n), true) = (beats, b.s.beat_jump_beats.iter().all(Option::is_none)) {
            b.s.beat_jump_beats = [Some(n); 4];
            b.entry("Beat jump size", &format!("{F}: beat_jump_beat_value"), format!("{n}"), MapStatus::EngineOnly, "deck.beat_jump (DeckCommand::SetBeatJumpSize)", "all decks");
        }
    }
    for (n, name) in [(1, "vinyl_speed_adjust"), (2, "jog_display_mode"), (3, "pad_button_brightness"), (4, "jog_lcd_brightness"), (11, "waveform")] {
        if byte(body, n).is_some_and(|v| v >= 0x80) {
            b.r.ignored_keys.push(format!("{F}: {name}"));
        }
    }
}

fn parse_djm(b: &mut Builder, body: &[u8]) {
    const F: &str = "DJMMYSETTING.DAT";
    let src = |n: &str| format!("{F}: {n}");
    if let Some(v) = byte(body, 13) {
        let c = match v {
            0x80 => Some((ChannelFaderCurve::SteepTop, "smooth")),
            0x81 => Some((ChannelFaderCurve::Linear, "linear")),
            0x82 => Some((ChannelFaderCurve::SteepBottom, "sharp")),
            _ => None,
        };
        if let Some((c, od)) = c {
            b.s.channel_fader_curve = Some(c);
            b.entry(
                "Channel fader curve",
                &src("channel_fader_curve"),
                format!("{c:?}"),
                MapStatus::EngineOnly,
                &format!("mixer.fader_curve = {od} (ChannelCommand::SetFaderCurve)"),
                "steep top ≈ OpenDecks smooth (x²), steep bottom ≈ sharp",
            );
        }
    }
    if let Some(v) = byte(body, 14) {
        let c = match v {
            0x80 => Some((CrossfaderCurve::Constant, "constant_power")),
            0x81 => Some((CrossfaderCurve::SlowCut, "linear")),
            0x82 => Some((CrossfaderCurve::FastCut, "cut")),
            _ => None,
        };
        if let Some((c, od)) = c {
            b.s.crossfader_curve = Some(c);
            b.entry(
                "Crossfader curve",
                &src("cross_fader_curve"),
                format!("{c:?}"),
                MapStatus::EngineOnly,
                &format!("mixer.crossfader_curve = {od} (MixerCommand::SetCrossfaderCurve)"),
                "slow cut ≈ OpenDecks linear",
            );
        }
    }
    if let Some(pre) = byte(body, 15).and_then(on_off) {
        b.s.headphones_pre_eq = Some(pre);
        b.entry(
            "Headphones cue pre/post EQ",
            &src("headphones_pre_eq"),
            if pre { "pre EQ" } else { "post EQ" },
            MapStatus::Unmapped,
            "",
            "OpenDecks headphone cue is always post EQ",
        );
    }
    if let Some(split) = byte(body, 16).and_then(on_off)
        && b.s.headphones_mono_split.is_none()
    {
        b.s.headphones_mono_split = Some(split);
        b.entry(
            "Headphones mono split",
            &src("headphones_mono_split"),
            if split { "mono split" } else { "stereo" },
            MapStatus::Unmapped,
            "",
            "OpenDecks has a cue/master mix knob, no mono split mode",
        );
    }
    for (n, name) in [(17, "beat_fx_quantize"), (18, "mic_low_cut"), (19, "talk_over_mode"), (20, "talk_over_level"), (21, "midi_channel"), (22, "midi_button_type"), (23, "display_brightness"), (24, "indicator_brightness"), (25, "channel_fader_curve_long")] {
        if byte(body, n).is_some_and(|v| v >= 0x80) {
            b.r.ignored_keys.push(format!("{F}: {name}"));
        }
    }
}

// ─────────────────────────── Output ────────────────────────────

impl RekordboxSettings {
    /// Values for the OpenDecks preferences, as `(json path, value)` with
    /// the value spelled as in the settings file (`"camelot"`, `"true"`,
    /// `"78"`). Includes the deck/mixer defaults the engine supports but
    /// that have no persisted preference yet (`deck.*`, `mixer.*`,
    /// `perform.quantize`); the preferences module ignores keys it does not
    /// know.
    pub fn preference_values(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        let mut put = |k: &str, v: String| out.push((k.to_string(), v));
        if let Some(l) = self.language.as_deref().filter(|l| *l == "en" || *l == "de") {
            put("view.language", l.into());
        }
        if let Some(k) = self.key_notation {
            put("view.key_notation", if k == KeyDisplay::Classic { "musical" } else { "camelot" }.into());
        }
        if let Some(w) = self.waveform_color {
            put("view.waveform", if w == WaveformColor::Blue { "blue" } else { "3band" }.into());
        }
        if let Some(p) = self.phrase_display {
            put("view.phrases", p.to_string());
        }
        if let Some((lo, hi)) = self.analysis_bpm_range {
            put("analysis.bpm_min", lo.to_string());
            put("analysis.bpm_max", hi.to_string());
        }
        if let Some(x) = self.analysis_key {
            put("analysis.detect_key", x.to_string());
        }
        if let Some(x) = self.analysis_phrases {
            put("analysis.detect_phrases", x.to_string());
        }
        if let Some(x) = self.auto_analyse {
            put("analysis.auto_analyse", x.to_string());
        }
        if let Some(f) = &self.recording_folder {
            put("recording.folder", f.display().to_string());
        }
        if let Some(x) = self.recording_auto_split {
            put("recording.auto_split", x.to_string());
        }
        if let Some(q) = self.quantize {
            put("perform.quantize", q.to_string());
        }
        for d in 0..4 {
            if let Some(r) = self.tempo_range[d] {
                put(&format!("deck{}.tempo_range", d + 1), r.opendecks().into());
            }
            if let Some(a) = self.auto_cue[d] {
                put(&format!("deck{}.auto_cue", d + 1), a.to_string());
            }
            if let Some(k) = self.master_tempo[d] {
                put(&format!("deck{}.keylock", d + 1), k.to_string());
            }
            if let Some(j) = self.beat_jump_beats[d] {
                put(&format!("deck{}.beat_jump", d + 1), j.to_string());
            }
        }
        if let Some(e) = self.eq_type {
            put("mixer.eq_mode", if e == EqType::Eq { "classic" } else { "isolator" }.into());
        }
        if let Some(c) = self.channel_fader_curve {
            let v = match c {
                ChannelFaderCurve::SteepTop => "smooth",
                ChannelFaderCurve::Linear => "linear",
                ChannelFaderCurve::SteepBottom => "sharp",
            };
            put("mixer.fader_curve", v.into());
        }
        if let Some(c) = self.crossfader_curve {
            let v = match c {
                CrossfaderCurve::Constant => "constant_power",
                CrossfaderCurve::SlowCut => "linear",
                CrossfaderCurve::FastCut => "cut",
            };
            put("mixer.crossfader_curve", v.into());
        }
        out
    }

    /// OpenDecks command id → bindings, for every shortcut with an
    /// equivalent (several rekordbox keys for one command are kept).
    pub fn keymap_bindings(&self) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for s in &self.shortcuts {
            if let (Some(t), Some(k)) = (&s.target, &s.binding) {
                let list = out.entry(t.clone()).or_default();
                if !list.contains(k) {
                    list.push(k.clone());
                }
            }
        }
        out
    }

    /// The shortcuts as an OpenDecks keymap file (`[bindings]` table).
    pub fn keymap_toml(&self) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
        let mut out = String::from("# Imported from rekordbox");
        if let Some(n) = &self.keymap_name {
            out.push_str(&format!(" ({n})"));
        }
        out.push_str("\n[bindings]\n");
        for (id, keys) in self.keymap_bindings() {
            let list: Vec<String> = keys.iter().map(|k| q(k)).collect();
            out.push_str(&format!("{} = [{}]\n", q(&id), list.join(", ")));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invented values in the shape of rekordbox's preference XML.
    const RB3: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<PROPERTIES>
  <VALUE name="KeyStringSetting" val="1"/>
  <VALUE name="ColorType" val="1"/>
  <VALUE name="showPhrase" val="1"/>
  <VALUE name="Player1_Quantize" val="0"/>
  <VALUE name="Player2_Quantize" val="1"/>
  <VALUE name="BeatSyncType" val="0"/>
  <VALUE name="AutoCueEnableDeck1" val="1"/>
  <VALUE name="AutoCueEnableDeck2" val="0"/>
  <VALUE name="Player0_MasterTempo" val="1"/>
  <VALUE name="Player1_BeatJumpBeat" val="16"/>
  <VALUE name="Player2_BeatJumpBeat" val="4"/>
  <VALUE name="DeckLoadLock" val="0"/>
  <VALUE name="hotCueAutoLoad" val="0"/>
  <VALUE name="HotCueColorType" val="2"/>
  <VALUE name="EqualizerType" val="0"/>
  <VALUE name="HeadphoneStereoMonoSplit" val="0"/>
  <VALUE name="RecordSaveDirectory" val="/tmp/My &amp; Recordings"/>
  <VALUE name="RecAutoTrackSplit_PerformanceMode" val="1"/>
  <VALUE name="trackAnalysisBpmRange" val="3"/>
  <VALUE name="enableKeyAnalysis" val="0"/>
  <VALUE name="AutoAnalysis" val="1"/>
  <VALUE name="WindowWidth" val="800"/>
  <VALUE name="SomeAccountToken" val="secret-value"/>
  <VALUE name="performaceKeyMapping" val="0000000000042"/>
</PROPERTIES>"#;

    const PLAY: &str = r#"<PROPERTIES>
  <VALUE name="TempoRange0" val="6"/>
  <VALUE name="TempoRange1" val="100"/>
  <VALUE name="PlayStatus0" val="0"/>
</PROPERTIES>"#;

    const KEYS: &str = r#"<PROPERTIES>
  <VALUE name="keyMappingName" val="My Keys"/>
  <VALUE name="keyMappingXml">
    <KEYMAPPINGS basedOnDefaults="0">
      <MAPPING commandId="3006" description="Play" key="X"/>
      <MAPPING commandId="3107" description="Cue" key="shift + command + J"/>
      <MAPPING commandId="3220" description="Hot cue C deck 3" key="option + cursor up"/>
      <MAPPING commandId="3015" description="8 beat loop" key="ctrl + F2"/>
      <MAPPING commandId="b129" description="Load 1" key="&#96;"/>
      <MAPPING commandId="b129" description="Load 1" key="spacebar"/>
      <MAPPING commandId="7003" description="Search" key="command + F"/>
      <MAPPING commandId="b04d" description="Something without equivalent" key="K"/>
      <MAPPING commandId="3006" description="Play" key="weird key name"/>
    </KEYMAPPINGS>
  </VALUE>
</PROPERTIES>"#;

    fn dat(len_data_body: &[u8]) -> Vec<u8> {
        let mut v = vec![0u8; 104];
        v[0] = 96;
        v[4..13].copy_from_slice(b"PioneerDJ");
        v[100..104].copy_from_slice(&(len_data_body.len() as u32).to_le_bytes());
        v.extend_from_slice(len_data_body);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }

    fn files() -> SettingsFiles {
        SettingsFiles {
            rekordbox3_settings: Some(RB3.into()),
            play_settings: Some(PLAY.into()),
            key_mappings: Some(KEYS.into()),
            ..Default::default()
        }
    }

    fn entry<'a>(r: &'a SettingsReport, setting: &str) -> &'a MappingEntry {
        r.entries.iter().find(|e| e.setting == setting).unwrap_or_else(|| panic!("no entry {setting}"))
    }

    #[test]
    fn xml_settings_are_mapped() {
        let (s, r) = parse(&files());
        assert_eq!(s.key_notation, Some(KeyDisplay::Classic));
        assert_eq!(s.waveform_color, Some(WaveformColor::Blue));
        assert_eq!(s.phrase_display, Some(true));
        assert_eq!(s.quantize, Some(true));
        assert_eq!(s.sync_mode, Some(0));
        assert_eq!(s.auto_cue, [Some(true), Some(false), None, None]);
        assert_eq!(s.master_tempo[0], Some(true));
        assert_eq!(s.beat_jump_beats, [Some(16.0), Some(4.0), None, None]);
        assert_eq!(s.tempo_range, [Some(TempoRange::Six), Some(TempoRange::Wide), None, None]);
        assert_eq!(s.load_lock, Some(false));
        assert_eq!(s.hot_cue_auto_load, Some(HotCueAutoLoad::Off));
        assert_eq!(s.hot_cue_color_type, Some(2));
        assert_eq!(s.eq_type, Some(EqType::Eq));
        assert_eq!(s.headphones_mono_split, Some(false));
        assert_eq!(s.recording_folder, Some(PathBuf::from("/tmp/My & Recordings")));
        assert_eq!(s.recording_auto_split, Some(true));
        assert_eq!(s.analysis_bpm_range, Some((88, 175)));
        assert_eq!(s.analysis_key, Some(false));
        assert_eq!(s.auto_analyse, Some(true));

        assert_eq!(entry(&r, "Key notation").status, MapStatus::Mapped);
        assert_eq!(entry(&r, "Sync mode").status, MapStatus::Unmapped);
        assert_eq!(entry(&r, "Tempo range").status, MapStatus::EngineOnly);
        assert_eq!(entry(&r, "Load lock (no loading on a playing deck)").status, MapStatus::Unmapped);
        // Unknown names are listed by name only, never with their value.
        assert!(r.ignored_keys.contains(&"rekordbox3.settings: SomeAccountToken".to_string()));
        assert!(r.ignored_keys.contains(&"PlaySettings.xml: PlayStatus0".to_string()));
        assert!(!format!("{r:?}").contains("secret-value"));
        assert!(!r.ignored_keys.iter().any(|k| k.contains("Player1_Quantize") || k.contains("KeyStringSetting")));

        let prefs = s.preference_values();
        let get = |k: &str| prefs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        assert_eq!(get("view.key_notation"), Some("musical"));
        assert_eq!(get("view.waveform"), Some("blue"));
        assert_eq!(get("view.phrases"), Some("true"));
        assert_eq!(get("analysis.bpm_min"), Some("88"));
        assert_eq!(get("analysis.bpm_max"), Some("175"));
        assert_eq!(get("recording.folder"), Some("/tmp/My & Recordings"));
        assert_eq!(get("deck2.tempo_range"), Some("wide"));
        assert_eq!(get("deck1.beat_jump"), Some("16"));
        assert_eq!(get("mixer.eq_mode"), Some("classic"));
        assert_eq!(get("view.language"), None);
    }

    #[test]
    fn rgb_and_alphanumeric() {
        let xml = RB3.replace(r#"name="KeyStringSetting" val="1""#, r#"name="KeyStringSetting" val="2""#).replace(
            r#"name="ColorType" val="1""#,
            r#"name="ColorType" val="2""#,
        );
        let (s, r) = parse(&SettingsFiles { rekordbox3_settings: Some(xml), ..Default::default() });
        assert_eq!(s.key_notation, Some(KeyDisplay::Alphanumeric));
        assert_eq!(s.waveform_color, Some(WaveformColor::Rgb));
        assert_eq!(entry(&r, "Waveform colour").target, "view.waveform = 3band");
        let prefs = s.preference_values();
        assert!(prefs.contains(&("view.key_notation".into(), "camelot".into())));
    }

    #[test]
    fn keyboard_shortcuts_translate() {
        assert_eq!(translate_key("shift + command + cursor left").as_deref(), Some("Mod+Shift+Left"));
        assert_eq!(translate_key("ctrl + shift + F").as_deref(), Some("Meta+Shift+F"));
        assert_eq!(translate_key("option + 3").as_deref(), Some("Alt+3"));
        assert_eq!(translate_key("command + return").as_deref(), Some("Mod+Return"));
        assert_eq!(translate_key("spacebar").as_deref(), Some("Space"));
        assert_eq!(translate_key("shift + :").as_deref(), Some("Shift+:"));
        assert_eq!(translate_key("z").as_deref(), Some("Z"));
        assert_eq!(translate_key("F12").as_deref(), Some("F12"));
        assert_eq!(translate_key("weird key name"), None);

        let (s, r) = parse(&files());
        assert_eq!(s.keymap_name.as_deref(), Some("My Keys"));
        assert_eq!(s.shortcuts.len(), 9);
        let k = s.keymap_bindings();
        assert_eq!(k["deck1.play"], vec!["X"]);
        assert_eq!(k["deck2.cue"], vec!["Mod+Shift+J"]);
        assert_eq!(k["deck3.hotcue.c"], vec!["Alt+Up"]);
        assert_eq!(k["deck1.loop.8"], vec!["Meta+F2"]);
        assert_eq!(k["browser.load.deck1"], vec!["`", "Space"]);
        assert_eq!(k["browser.search"], vec!["Mod+F"]);
        assert_eq!(k.len(), 6);
        assert_eq!(entry(&r, "Keyboard shortcuts").value, "7 of 9 shortcuts");
        let toml = s.keymap_toml();
        assert!(toml.contains("[bindings]\n"));
        assert!(toml.contains("\"browser.load.deck1\" = [\"`\", \"Space\"]\n"));
    }

    #[test]
    fn my_settings_files() {
        // MYSETTING.DAT: quantize on, auto cue level -48 dB, German,
        // quantize 1/2, hot cue autoload "rekordbox", needle lock on, auto
        // cue on, master tempo off, tempo ±16.
        let mut my = vec![0u8; 40];
        my[..8].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 2, 0, 0, 0]);
        let set = |b: &mut Vec<u8>, n: usize, v: u8| b[n - 1] = v;
        for (n, v) in [(11, 0x81), (12, 0x82), (13, 0x83), (25, 0x81), (26, 0x83), (27, 0x81), (30, 0x81), (35, 0x81), (36, 0x80), (37, 0x82), (9, 0x80)] {
            set(&mut my, n, v);
        }
        // MYSETTING2.DAT: phrase divisions, beat jump 32.
        let mut my2 = vec![0u8; 40];
        set(&mut my2, 5, 0x81);
        set(&mut my2, 13, 0x86);
        // DJMMYSETTING.DAT: linear fader, fast cut, pre EQ, mono split.
        let mut djm = vec![0u8; 52];
        djm[..12].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 1, 0, 0, 0, 0x20, 0, 0, 0]);
        for (n, v) in [(13, 0x81), (14, 0x82), (15, 0x81), (16, 0x81), (17, 0x81)] {
            set(&mut djm, n, v);
        }
        let f = SettingsFiles {
            mysetting: Some(dat(&my)),
            mysetting2: Some(dat(&my2)),
            djmmysetting: Some(dat(&djm)),
            devsetting: Some(dat(&[0; 32])),
            ..Default::default()
        };
        let (s, r) = parse(&f);
        assert_eq!(s.quantize, Some(true));
        assert_eq!(s.auto_cue_level_db, Some(-48));
        assert_eq!(s.language.as_deref(), Some("de"));
        assert_eq!(s.quantize_beats, Some(0.5));
        assert_eq!(s.hot_cue_auto_load, Some(HotCueAutoLoad::RekordboxSetting));
        assert_eq!(s.hot_cue_color, Some(true));
        assert_eq!(s.needle_lock, Some(true));
        assert_eq!(s.auto_cue, [Some(true); 4]);
        assert_eq!(s.master_tempo, [Some(false); 4]);
        assert_eq!(s.tempo_range, [Some(TempoRange::Sixteen); 4]);
        assert_eq!(s.phrase_display, Some(true));
        assert_eq!(s.beat_jump_beats, [Some(32.0); 4]);
        assert_eq!(s.channel_fader_curve, Some(ChannelFaderCurve::Linear));
        assert_eq!(s.crossfader_curve, Some(CrossfaderCurve::FastCut));
        assert_eq!(s.headphones_pre_eq, Some(true));
        assert_eq!(s.headphones_mono_split, Some(true));
        assert_eq!(entry(&r, "Language").status, MapStatus::Mapped);
        assert!(r.unmapped_files.iter().any(|(n, _)| n == "DEVSETTING.DAT"));
        assert!(r.ignored_keys.contains(&"MYSETTING.DAT: on_air_display".to_string()));
        assert!(r.ignored_keys.contains(&"DJMMYSETTING.DAT: beat_fx_quantize".to_string()));
        let prefs = s.preference_values();
        assert!(prefs.contains(&("view.language".into(), "de".into())));
        assert!(prefs.contains(&("mixer.crossfader_curve".into(), "cut".into())));
        assert!(prefs.contains(&("mixer.fader_curve".into(), "linear".into())));
        assert!(prefs.contains(&("deck4.tempo_range".into(), "sixteen".into())));
    }

    #[test]
    fn xml_settings_win_over_my_settings() {
        let mut my = vec![0u8; 40];
        my[36] = 0x80; // tempo ±6 for all decks
        my[10] = 0x80; // quantize off
        let mut f = files();
        f.mysetting = Some(dat(&my));
        let (s, _) = parse(&f);
        assert_eq!(s.tempo_range[0], Some(TempoRange::Six));
        assert_eq!(s.tempo_range[1], Some(TempoRange::Wide));
        assert_eq!(s.quantize, Some(true));
    }

    #[test]
    fn broken_my_settings_header_is_reported() {
        let f = SettingsFiles { djmmysetting: Some(vec![1, 2, 3]), ..Default::default() };
        let (s, r) = parse(&f);
        assert_eq!(s, RekordboxSettings::default());
        assert_eq!(r.unmapped_files, vec![("DJMMYSETTING.DAT".to_string(), "unexpected header".to_string())]);
    }

    #[test]
    fn loads_files_from_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rekordbox3.settings"), RB3).unwrap();
        std::fs::write(dir.path().join("PlaySettings.xml"), PLAY).unwrap();
        std::fs::create_dir(dir.path().join("KeyMappings")).unwrap();
        std::fs::write(dir.path().join("KeyMappings/rekordbox_0000000000042.mappings"), KEYS).unwrap();
        std::fs::create_dir(dir.path().join("PIONEER")).unwrap();
        std::fs::write(dir.path().join("PIONEER/DEVSETTING.DAT"), dat(&[0; 32])).unwrap();
        std::fs::write(dir.path().join("VideoSettings.xml"), "<PROPERTIES/>").unwrap();
        let (s, r) = read(Some(dir.path())).unwrap();
        assert_eq!(r.files.len(), 4);
        assert_eq!(s.keymap_name.as_deref(), Some("My Keys"));
        assert!(r.unmapped_files.iter().any(|(n, _)| n == "VideoSettings.xml"));
        assert!(r.unmapped_files.iter().any(|(n, _)| n == "DEVSETTING.DAT"));
        let empty = tempfile::tempdir().unwrap();
        assert!(read(Some(empty.path())).is_err());
    }
}
