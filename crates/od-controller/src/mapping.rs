//! TOML mapping files: schema (`MappingFile`) and compiled form (`Mapping`).
//!
//! The schema is documented in `docs/controller-mappings.md`.

use crate::action::{CrossfaderSide, PadMode};
use crate::feedback::{DeckElement, FeedbackTarget, MixerElement};
use crate::midi::{MidiAddr, MsgKind};
use crate::templates::{ActionTemplate, Natural, parse_action};
use od_core::CueColor;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use toml::{Table, Value};

/// Current mapping schema version.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum MappingError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported schema version {0} (expected {SCHEMA_VERSION})")]
    Schema(u32),
    #[error("unknown action `{0}`")]
    UnknownAction(String),
    #[error("action `{action}`: {msg}")]
    Action { action: String, msg: String },
    #[error("unknown feedback target `{0}`")]
    UnknownTarget(String),
    #[error("{entry}: {msg}")]
    Entry { entry: String, msg: String },
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingFile {
    pub schema: u32,
    pub device: DeviceSection,
    /// Named channel sets: entry `i` is the wire channel (0..15) of deck slot `i + 1`.
    #[serde(default)]
    pub channels: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    pub layers: Option<LayerSection>,
    #[serde(default)]
    pub options: Options,
    /// Named colour palettes: velocity value ↔ RGB, for pad colour outputs.
    #[serde(default)]
    pub palettes: BTreeMap<String, Vec<PaletteEntry>>,
    /// Raw messages sent once after connecting.
    #[serde(default)]
    pub init: Vec<RawMessageSpec>,
    /// Raw messages sent periodically while connected.
    #[serde(default)]
    pub keepalive: Vec<KeepaliveSpec>,
    #[serde(default, rename = "input")]
    pub inputs: Vec<InputSpec>,
    #[serde(default, rename = "output")]
    pub outputs: Vec<OutputSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSection {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub model: String,
    pub decks: u8,
    /// Optional built-in Rust device module (see `device_module`).
    #[serde(default)]
    pub module: Option<String>,
    #[serde(rename = "match")]
    pub matching: MatchSpec,
    /// Where the message facts came from.
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// Case-insensitive substrings of the MIDI port name.
    pub port_names: Vec<String>,
    #[serde(default)]
    pub usb_vendor_id: Option<u16>,
    #[serde(default)]
    pub usb_product_id: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerMode {
    /// The hardware changes MIDI channel per deck; channel sets name decks
    /// directly and layers only track which deck is active on each side.
    Hardware,
    /// The hardware always sends on the same channel per side; the software
    /// redirects slot `i` to the active deck of side `i`.
    Software,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSection {
    pub mode: LayerMode,
    /// For each side, the decks it can control. The first entry is active
    /// initially.
    pub sides: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Options {
    pub soft_takeover: bool,
    /// Normalised distance at which a knob picks up the engine value.
    pub takeover_threshold: f32,
    pub jog_ticks_per_revolution: u32,
    pub blink_ms: u32,
    pub blink_fast_ms: u32,
    /// Minimum interval between two messages to the same level output.
    pub meter_interval_ms: u32,
    /// Upper bound of messages emitted per output poll.
    pub max_messages_per_poll: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            soft_takeover: true,
            takeover_threshold: 0.03,
            jog_ticks_per_revolution: 720,
            blink_ms: 500,
            blink_fast_ms: 200,
            meter_interval_ms: 33,
            max_messages_per_poll: 256,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteEntry {
    pub value: u8,
    pub color: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawMessageSpec {
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub verify: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeepaliveSpec {
    pub bytes: Vec<u8>,
    pub interval_ms: u32,
    #[serde(default)]
    pub verify: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ChannelRef {
    Fixed(u8),
    Set(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    Button,
    Absolute,
    Absolute14,
    Relative,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelEncoding {
    /// 64 = no movement, 65 = +1, 63 = -1.
    #[default]
    Offset64,
    /// 1 = +1, 127 = -1.
    TwosComplement,
    /// bit 6 = negative, bits 0..5 = magnitude.
    SignMagnitude,
}

impl RelEncoding {
    pub fn decode(self, v: u8) -> i32 {
        let v = (v & 0x7F) as i32;
        match self {
            RelEncoding::Offset64 => v - 64,
            RelEncoding::TwosComplement => {
                if v >= 64 {
                    v - 128
                } else {
                    v
                }
            }
            RelEncoding::SignMagnitude => {
                if v & 0x40 != 0 {
                    -(v & 0x3F)
                } else {
                    v & 0x3F
                }
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputSpec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub note: Option<u8>,
    #[serde(default)]
    pub cc: Option<u8>,
    /// LSB controller of a 14-bit pair. Defaults to `cc + 0x20` in `absolute14` mode.
    #[serde(default)]
    pub lsb: Option<u8>,
    pub channel: ChannelRef,
    /// Explicit deck/channel (1-based) for inputs on a fixed channel.
    #[serde(default)]
    pub deck: Option<u8>,
    #[serde(default)]
    pub mode: Option<InputMode>,
    #[serde(default)]
    pub encoding: Option<RelEncoding>,
    pub action: String,
    #[serde(default)]
    pub args: Table,
    #[serde(default)]
    pub repeat: Option<u8>,
    #[serde(default = "one")]
    pub step: u8,
    #[serde(default)]
    pub index_arg: Option<String>,
    #[serde(default)]
    pub index_base: i64,
    /// Software shift condition: only active while shift is (not) held.
    #[serde(default)]
    pub shift: Option<bool>,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub takeover: Option<bool>,
    /// The entry still has to be verified on real hardware.
    #[serde(default)]
    pub verify: bool,
}

fn one() -> u8 {
    1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    Led,
    Level,
    Color,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    pub target: String,
    #[serde(default)]
    pub note: Option<u8>,
    #[serde(default)]
    pub cc: Option<u8>,
    pub channel: ChannelRef,
    #[serde(default)]
    pub deck: Option<u8>,
    #[serde(default)]
    pub mode: Option<OutputMode>,
    #[serde(default = "full")]
    pub on: u8,
    #[serde(default)]
    pub off: u8,
    #[serde(default)]
    pub dim: Option<u8>,
    /// Value that makes the hardware blink by itself; otherwise blinking is
    /// done in software.
    #[serde(default)]
    pub blink: Option<u8>,
    #[serde(default)]
    pub min: u8,
    #[serde(default = "full")]
    pub max: u8,
    #[serde(default)]
    pub palette: Option<String>,
    #[serde(default)]
    pub args: Table,
    #[serde(default)]
    pub repeat: Option<u8>,
    #[serde(default = "one")]
    pub step: u8,
    #[serde(default)]
    pub index_arg: Option<String>,
    #[serde(default)]
    pub index_base: i64,
    #[serde(default)]
    pub min_interval_ms: Option<u32>,
    #[serde(default)]
    pub verify: bool,
}

fn full() -> u8 {
    0x7F
}

// ---------------------------------------------------------------------------
// Compiled form
// ---------------------------------------------------------------------------

/// One expanded input binding.
#[derive(Debug, Clone)]
pub struct InputBinding {
    pub name: String,
    pub addr: MidiAddr,
    pub lsb: Option<MidiAddr>,
    pub mode: InputMode,
    pub encoding: RelEncoding,
    /// Deck slot (1-based) the binding is attached to.
    pub slot: Option<u8>,
    pub shift: Option<bool>,
    pub invert: bool,
    pub takeover: bool,
    pub verify: bool,
    pub template: Arc<ActionTemplate>,
}

/// One expanded output binding.
#[derive(Debug, Clone)]
pub struct OutputBinding {
    pub name: String,
    pub target: FeedbackTarget,
    pub addr: MidiAddr,
    pub mode: OutputMode,
    pub on: u8,
    pub off: u8,
    pub dim: Option<u8>,
    pub blink: Option<u8>,
    pub min: u8,
    pub max: u8,
    pub palette: Option<Arc<Vec<(CueColor, u8)>>>,
    pub min_interval_ms: u32,
    /// Software layers: only drive the LED while `deck` is active on `side`.
    pub active_when: Option<(u8, u8)>,
    pub verify: bool,
}

/// A compiled, ready-to-use mapping.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub file: MappingFile,
    pub inputs: Vec<InputBinding>,
    pub outputs: Vec<OutputBinding>,
    input_index: HashMap<MidiAddr, Vec<usize>>,
    output_index: HashMap<FeedbackTarget, Vec<usize>>,
}

impl Mapping {
    pub fn from_toml_str(s: &str) -> Result<Self, MappingError> {
        let file: MappingFile = toml::from_str(s)?;
        Self::compile(file)
    }

    pub fn load(path: &Path) -> Result<Self, MappingError> {
        let s = std::fs::read_to_string(path).map_err(|source| MappingError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&s)
    }

    /// Loads every `*.toml` below `dir` (recursively). Errors are returned per file.
    pub fn load_dir(dir: &Path) -> Vec<(PathBuf, Result<Mapping, MappingError>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "toml") {
                    let r = Mapping::load(&p);
                    out.push((p, r));
                }
            }
        }
        out
    }

    pub fn device(&self) -> &DeviceSection {
        &self.file.device
    }

    pub fn options(&self) -> &Options {
        &self.file.options
    }

    /// Input bindings listening on `addr` (MSB or LSB of 14-bit pairs included).
    pub fn inputs_at(&self, addr: MidiAddr) -> &[usize] {
        self.input_index
            .get(&addr)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn outputs_for(&self, target: &FeedbackTarget) -> &[usize] {
        self.output_index
            .get(target)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Does a MIDI port name match this mapping?
    pub fn matches_port(&self, port_name: &str) -> bool {
        let p = port_name.to_lowercase();
        self.file
            .device
            .matching
            .port_names
            .iter()
            .any(|n| p.contains(&n.to_lowercase()))
    }

    /// Does a USB VID/PID pair match this mapping (if it declares one)?
    pub fn matches_usb(&self, vid: u16, pid: u16) -> bool {
        let m = &self.file.device.matching;
        m.usb_vendor_id == Some(vid) && m.usb_product_id == Some(pid)
    }

    /// Entries flagged `verify = true`, as (kind, name).
    pub fn unverified(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .inputs
            .iter()
            .filter(|b| b.verify)
            .map(|b| format!("in  {} [{}]", b.name, b.addr))
            .chain(
                self.outputs
                    .iter()
                    .filter(|b| b.verify)
                    .map(|b| format!("out {} [{}]", b.name, b.addr)),
            )
            .collect();
        v.dedup();
        v
    }

    pub fn compile(file: MappingFile) -> Result<Self, MappingError> {
        if file.schema != SCHEMA_VERSION {
            return Err(MappingError::Schema(file.schema));
        }
        let palettes: HashMap<String, Arc<Vec<(CueColor, u8)>>> = file
            .palettes
            .iter()
            .map(|(name, entries)| {
                let v = entries
                    .iter()
                    .map(|e| {
                        CueColor::from_hex(&e.color)
                            .map(|c| (c, e.value))
                            .ok_or_else(|| MappingError::Entry {
                                entry: format!("palette {name}"),
                                msg: format!("bad colour `{}`", e.color),
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((name.clone(), Arc::new(v)))
            })
            .collect::<Result<_, MappingError>>()?;

        if let Some(l) = &file.layers
            && l.sides.iter().any(|s| s.is_empty())
        {
            return Err(MappingError::Entry {
                entry: "layers".into(),
                msg: "every side needs at least one deck".into(),
            });
        }
        let software_layers = file
            .layers
            .as_ref()
            .filter(|l| l.mode == LayerMode::Software)
            .cloned();

        let mut inputs = Vec::new();
        for (n, spec) in file.inputs.iter().enumerate() {
            let entry = spec
                .name
                .clone()
                .unwrap_or_else(|| format!("input #{n} ({})", spec.action));
            let err = |msg: String| MappingError::Entry {
                entry: entry.clone(),
                msg,
            };
            let (kind, base) = match (spec.note, spec.cc) {
                (Some(n), None) => (MsgKind::Note, n),
                (None, Some(c)) => (MsgKind::Cc, c),
                _ => return Err(err("exactly one of `note` or `cc` is required".into())),
            };
            for (slot, channel) in expand_channels(&file, &spec.channel, spec.deck).map_err(err)? {
                for rep in 0..spec.repeat.unwrap_or(1) {
                    let args = repeat_args(&spec.args, rep, &spec.index_arg, spec.index_base);
                    let template = parse_action(&spec.action, &args)?;
                    if template.needs_deck && slot.is_none() {
                        return Err(err("deck action needs a channel set or `deck`".into()));
                    }
                    let number = base
                        .checked_add(rep * spec.step)
                        .filter(|n| *n < 0x80)
                        .ok_or_else(|| err("MIDI number out of range".into()))?;
                    let addr = MidiAddr {
                        kind,
                        channel,
                        number,
                    };
                    let mode = spec.mode.unwrap_or(if spec.lsb.is_some() {
                        InputMode::Absolute14
                    } else {
                        match template.natural {
                            Natural::Button => InputMode::Button,
                            Natural::Absolute => InputMode::Absolute,
                            Natural::Relative => InputMode::Relative,
                        }
                    });
                    let lsb = match mode {
                        InputMode::Absolute14 => {
                            if kind != MsgKind::Cc {
                                return Err(err("absolute14 requires `cc`".into()));
                            }
                            let l = spec
                                .lsb
                                .map(|l| l + rep * spec.step)
                                .unwrap_or(number + 0x20);
                            Some(MidiAddr::cc(channel, l))
                        }
                        _ => None,
                    };
                    let takeover = spec.takeover.unwrap_or(
                        file.options.soft_takeover
                            && matches!(mode, InputMode::Absolute | InputMode::Absolute14),
                    );
                    let name = match (&spec.name, spec.repeat) {
                        (Some(n), Some(_)) => format!("{n} {}", rep as i64 + spec.index_base + 1),
                        (Some(n), None) => n.clone(),
                        (None, _) => spec.action.clone(),
                    };
                    let name = match slot {
                        Some(s) if spec.deck.is_none() => format!("{name} (deck {s})"),
                        _ => name,
                    };
                    inputs.push(InputBinding {
                        name,
                        addr,
                        lsb,
                        mode,
                        encoding: spec.encoding.unwrap_or_default(),
                        slot,
                        shift: spec.shift,
                        invert: spec.invert,
                        takeover,
                        verify: spec.verify,
                        template: Arc::new(template),
                    });
                }
            }
        }

        let mut outputs = Vec::new();
        for (n, spec) in file.outputs.iter().enumerate() {
            let entry = format!("output #{n} ({})", spec.target);
            let err = |msg: String| MappingError::Entry {
                entry: entry.clone(),
                msg,
            };
            let (kind, base) = match (spec.note, spec.cc) {
                (Some(n), None) => (MsgKind::Note, n),
                (None, Some(c)) => (MsgKind::Cc, c),
                _ => return Err(err("exactly one of `note` or `cc` is required".into())),
            };
            let palette = match &spec.palette {
                Some(p) => Some(
                    palettes
                        .get(p)
                        .cloned()
                        .ok_or_else(|| err(format!("unknown palette `{p}`")))?,
                ),
                None => None,
            };
            for (slot, channel) in expand_channels(&file, &spec.channel, spec.deck).map_err(err)? {
                for rep in 0..spec.repeat.unwrap_or(1) {
                    let args = repeat_args(&spec.args, rep, &spec.index_arg, spec.index_base);
                    let number = base
                        .checked_add(rep * spec.step)
                        .filter(|n| *n < 0x80)
                        .ok_or_else(|| err("MIDI number out of range".into()))?;
                    let addr = MidiAddr {
                        kind,
                        channel,
                        number,
                    };
                    // With software layers a slot shows any deck of its side.
                    let decks: Vec<DeckVariant> = match (slot, &software_layers) {
                        (Some(s), Some(l)) => {
                            let side = l
                                .sides
                                .get(s as usize - 1)
                                .ok_or_else(|| err(format!("slot {s} has no side in [layers]")))?;
                            side.iter().map(|d| (Some(*d), Some((s, *d)))).collect()
                        }
                        _ => vec![(slot, None)],
                    };
                    for (deck, active_when) in decks {
                        let target = parse_target(&spec.target, deck, &args)?;
                        let mode = spec.mode.unwrap_or(if palette.is_some() {
                            OutputMode::Color
                        } else if matches!(
                            target,
                            FeedbackTarget::MasterVuLeft
                                | FeedbackTarget::MasterVuRight
                                | FeedbackTarget::Mixer {
                                    element: MixerElement::Vu,
                                    ..
                                }
                                | FeedbackTarget::Deck {
                                    element: DeckElement::JogIndicator,
                                    ..
                                }
                        ) {
                            OutputMode::Level
                        } else {
                            OutputMode::Led
                        });
                        let min_interval_ms = spec.min_interval_ms.unwrap_or(match mode {
                            OutputMode::Level => file.options.meter_interval_ms,
                            _ => 0,
                        });
                        outputs.push(OutputBinding {
                            name: spec.target.clone(),
                            target,
                            addr,
                            mode,
                            on: spec.on,
                            off: spec.off,
                            dim: spec.dim,
                            blink: spec.blink,
                            min: spec.min,
                            max: spec.max,
                            palette: palette.clone(),
                            min_interval_ms,
                            active_when,
                            verify: spec.verify,
                        });
                    }
                }
            }
        }

        let mut input_index: HashMap<MidiAddr, Vec<usize>> = HashMap::new();
        for (i, b) in inputs.iter().enumerate() {
            input_index.entry(b.addr).or_default().push(i);
            if let Some(l) = b.lsb {
                input_index.entry(l).or_default().push(i);
            }
        }
        let mut output_index: HashMap<FeedbackTarget, Vec<usize>> = HashMap::new();
        for (i, b) in outputs.iter().enumerate() {
            output_index.entry(b.target.clone()).or_default().push(i);
        }
        Ok(Mapping {
            file,
            inputs,
            outputs,
            input_index,
            output_index,
        })
    }
}

/// (deck, `active_when` side/deck) an output binding is expanded for.
type DeckVariant = (Option<u8>, Option<(u8, u8)>);

/// Expands a channel reference to (deck slot, wire channel) pairs.
fn expand_channels(
    file: &MappingFile,
    ch: &ChannelRef,
    deck: Option<u8>,
) -> Result<Vec<(Option<u8>, u8)>, String> {
    match ch {
        ChannelRef::Fixed(c) if *c < 16 => Ok(vec![(deck, *c)]),
        ChannelRef::Fixed(c) => Err(format!("channel {c} out of range 0..15")),
        ChannelRef::Set(name) => {
            let set = file
                .channels
                .get(name)
                .ok_or_else(|| format!("unknown channel set `{name}`"))?;
            if deck.is_some() {
                return Err("`deck` cannot be combined with a channel set".into());
            }
            set.iter()
                .enumerate()
                .map(|(i, c)| {
                    if *c < 16 {
                        Ok((Some(i as u8 + 1), *c))
                    } else {
                        Err(format!("channel {c} out of range"))
                    }
                })
                .collect()
        }
    }
}

/// Resolves array arguments and the index argument for repetition `rep`.
fn repeat_args(args: &Table, rep: u8, index_arg: &Option<String>, base: i64) -> Table {
    let mut t: Table = args
        .iter()
        .map(|(k, v)| {
            let v = match v {
                Value::Array(a) => a.get(rep as usize).cloned().unwrap_or(Value::Integer(0)),
                v => v.clone(),
            };
            (k.clone(), v)
        })
        .collect();
    if let Some(k) = index_arg {
        t.insert(k.clone(), Value::Integer(rep as i64 + base));
    }
    t
}

fn targ_err(name: &str, msg: &str) -> MappingError {
    MappingError::Action {
        action: name.to_string(),
        msg: msg.to_string(),
    }
}

/// Parses an output target name.
pub fn parse_target(
    name: &str,
    deck: Option<u8>,
    args: &Table,
) -> Result<FeedbackTarget, MappingError> {
    let s = |k: &str| match args.get(k) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(targ_err(name, &format!("missing string argument `{k}`"))),
    };
    let u = |k: &str| match args.get(k) {
        Some(Value::Integer(i)) if (0..=255).contains(i) => Ok(*i as u8),
        _ => Err(targ_err(name, &format!("missing integer argument `{k}`"))),
    };
    let pad_mode = || {
        let m = s("mode")?;
        PadMode::parse(&m).ok_or_else(|| targ_err(name, &format!("unknown pad mode `{m}`")))
    };
    if let Some(el) = name.strip_prefix("deck.") {
        let deck =
            deck.ok_or_else(|| targ_err(name, "deck target needs a channel set or `deck`"))?;
        let element = match el {
            "play" => DeckElement::Play,
            "cue" => DeckElement::Cue,
            "sync" => DeckElement::Sync,
            "master" => DeckElement::Master,
            "keylock" => DeckElement::KeyLock,
            "key_sync" => DeckElement::KeySync,
            "slip" => DeckElement::Slip,
            "quantize" => DeckElement::Quantize,
            "reverse" => DeckElement::Reverse,
            "censor" => DeckElement::Censor,
            "vinyl" => DeckElement::Vinyl,
            "tempo_reset" => DeckElement::TempoReset,
            "tempo_range" => DeckElement::TempoRange,
            "loop_in" => DeckElement::LoopIn,
            "loop_out" => DeckElement::LoopOut,
            "reloop" => DeckElement::Reloop,
            "loop_active" => DeckElement::LoopActive,
            "loaded" => DeckElement::Loaded,
            "shift" => DeckElement::Shift,
            "jog_indicator" => DeckElement::JogIndicator,
            "selected" => DeckElement::Selected,
            "pad_mode" => DeckElement::PadMode(pad_mode()?),
            "pad" => DeckElement::Pad {
                mode: pad_mode()?,
                index: u("index")?,
            },
            _ => return Err(MappingError::UnknownTarget(name.into())),
        };
        return Ok(FeedbackTarget::Deck { deck, element });
    }
    if let Some(el) = name.strip_prefix("mixer.") {
        let channel =
            deck.ok_or_else(|| targ_err(name, "mixer target needs a channel set or `deck`"))?;
        let element = match el {
            "cue" => MixerElement::Cue,
            "vu" => MixerElement::Vu,
            "crossfader_assign" => MixerElement::CrossfaderAssign(match s("side")?.as_str() {
                "a" => CrossfaderSide::A,
                "thru" => CrossfaderSide::Thru,
                "b" => CrossfaderSide::B,
                _ => return Err(targ_err(name, "side must be a, thru or b")),
            }),
            _ => return Err(MappingError::UnknownTarget(name.into())),
        };
        return Ok(FeedbackTarget::Mixer { channel, element });
    }
    Ok(match name {
        "master.vu_left" => FeedbackTarget::MasterVuLeft,
        "master.vu_right" => FeedbackTarget::MasterVuRight,
        "master_cue" => FeedbackTarget::MasterCue,
        "beat_fx.on" => FeedbackTarget::BeatFxOn {
            unit: u("unit").unwrap_or(1),
        },
        "beat_fx.release" => FeedbackTarget::BeatFxRelease {
            unit: u("unit").unwrap_or(1),
        },
        "color_fx" => FeedbackTarget::ColorFx(s("effect")?),
        "sampler.cue" => FeedbackTarget::SamplerCue,
        "recording" => FeedbackTarget::Recording,
        _ => return Err(MappingError::UnknownTarget(name.into())),
    })
}
