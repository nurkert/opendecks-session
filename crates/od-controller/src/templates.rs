//! Translation of mapping-file action names (`"deck.play_pause"`) and their
//! arguments into functions producing [`ControlAction`]s from input values.

use crate::action::*;
use crate::mapping::MappingError;
use std::fmt;
use toml::{Table, Value};

/// Interpreted value of an input after decoding the MIDI message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputValue {
    Button { pressed: bool, velocity: u8 },
    /// Normalised absolute position 0..=1.
    Absolute(f32),
    /// Signed relative movement in hardware ticks.
    Relative(i32),
}

impl InputValue {
    fn pressed(self) -> bool {
        match self {
            InputValue::Button { pressed, .. } => pressed,
            InputValue::Absolute(v) => v > 0.5,
            InputValue::Relative(d) => d != 0,
        }
    }
    fn unit(self) -> f32 {
        match self {
            InputValue::Absolute(v) => v,
            InputValue::Button { pressed, .. } => {
                if pressed {
                    1.0
                } else {
                    0.0
                }
            }
            InputValue::Relative(_) => 0.0,
        }
    }
    fn ticks(self) -> i32 {
        match self {
            InputValue::Relative(d) => d,
            _ => 0,
        }
    }
    fn velocity(self) -> u8 {
        match self {
            InputValue::Button { velocity, .. } => velocity,
            _ => 0,
        }
    }
}

/// The value type an action naturally consumes; used as the default input mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Natural {
    Button,
    Absolute,
    Relative,
}

type Producer = Box<dyn Fn(Option<u8>, InputValue) -> Option<ControlAction> + Send + Sync>;

pub struct ActionTemplate {
    pub name: String,
    pub natural: Natural,
    /// The action needs a deck / channel / unit binding.
    pub needs_deck: bool,
    /// For `deck.shift`: the input updates the shift modifier.
    pub is_shift: bool,
    produce: Producer,
}

impl fmt::Debug for ActionTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionTemplate").field("name", &self.name).finish()
    }
}

impl ActionTemplate {
    pub fn produce(&self, deck: Option<u8>, v: InputValue) -> Option<ControlAction> {
        if self.needs_deck && deck.is_none() {
            return None;
        }
        (self.produce)(deck, v)
    }
}

struct Args<'a> {
    name: &'a str,
    t: &'a Table,
}

impl Args<'_> {
    fn err(&self, msg: impl Into<String>) -> MappingError {
        MappingError::Action { action: self.name.to_string(), msg: msg.into() }
    }
    fn f32(&self, key: &str) -> Result<f32, MappingError> {
        match self.t.get(key) {
            Some(Value::Float(f)) => Ok(*f as f32),
            Some(Value::Integer(i)) => Ok(*i as f32),
            Some(_) => Err(self.err(format!("argument `{key}` must be a number"))),
            None => Err(self.err(format!("missing argument `{key}`"))),
        }
    }
    fn u8(&self, key: &str) -> Result<u8, MappingError> {
        match self.t.get(key) {
            Some(Value::Integer(i)) if (0..=255).contains(i) => Ok(*i as u8),
            Some(_) => Err(self.err(format!("argument `{key}` must be an integer 0..255"))),
            None => Err(self.err(format!("missing argument `{key}`"))),
        }
    }
    fn u8_or(&self, key: &str, d: u8) -> Result<u8, MappingError> {
        if self.t.contains_key(key) { self.u8(key) } else { Ok(d) }
    }
    fn str(&self, key: &str) -> Result<String, MappingError> {
        match self.t.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            Some(_) => Err(self.err(format!("argument `{key}` must be a string"))),
            None => Err(self.err(format!("missing argument `{key}`"))),
        }
    }
    fn bool_or(&self, key: &str, d: bool) -> Result<bool, MappingError> {
        match self.t.get(key) {
            Some(Value::Boolean(b)) => Ok(*b),
            Some(_) => Err(self.err(format!("argument `{key}` must be a boolean"))),
            None => Ok(d),
        }
    }
}

/// All action names understood in mapping files.
pub const ACTION_NAMES: &[&str] = &[
    "ignore",
    "deck.play_pause", "deck.cue", "deck.jump_to_start", "deck.sync", "deck.sync_off",
    "deck.master", "deck.keylock", "deck.key_sync", "deck.key_reset", "deck.tempo",
    "deck.tempo_range", "deck.tempo_reset", "deck.tap_bpm", "deck.quantize", "deck.jog_touch",
    "deck.jog_turn", "deck.hot_cue", "deck.hot_cue_delete", "deck.memory_cue_set",
    "deck.memory_cue_delete", "deck.memory_cue_prev", "deck.memory_cue_next", "deck.loop_in",
    "deck.loop_out", "deck.auto_loop", "deck.loop_roll", "deck.loop_halve", "deck.loop_double",
    "deck.reloop_exit", "deck.beat_jump", "deck.search", "deck.slip", "deck.vinyl",
    "deck.shift", "deck.pad_mode", "deck.pad", "deck.load", "deck.censor", "deck.reverse",
    "deck.needle_search", "deck.select", "deck.stem_level", "deck.stem_mute",
    "mixer.trim", "mixer.eq_high", "mixer.eq_mid", "mixer.eq_low", "mixer.color_fx",
    "mixer.fader", "mixer.cue", "mixer.crossfader_assign",
    "crossfader", "master_level", "booth_level", "headphone_mix", "headphone_level",
    "master_cue",
    "beat_fx.toggle", "beat_fx.release", "beat_fx.level", "beat_fx.beat_down",
    "beat_fx.beat_up", "beat_fx.auto_beat", "beat_fx.tap", "beat_fx.select_next",
    "beat_fx.select_prev", "beat_fx.select", "beat_fx.assign",
    "color_fx.select", "color_fx.next", "color_fx.toggle",
    "browse.rotate", "browse.press", "browse.back", "browse.view_toggle", "browse.related",
    "sampler.pad", "sampler.stop", "sampler.bank_next", "sampler.bank_prev", "sampler.volume",
    "sampler.cue",
    "recording.toggle",
    "lighting.look", "lighting.strobe", "lighting.flash", "lighting.fog", "lighting.blackout",
    "lighting.master", "lighting.cue_go", "lighting.cue_back", "lighting.mode",
];

fn on_press(f: impl Fn(Option<u8>) -> ControlAction + Send + Sync + 'static) -> Producer {
    Box::new(move |d, v| v.pressed().then(|| f(d)))
}

fn deck_press(a: DeckAction) -> Producer {
    on_press(move |d| ControlAction::Deck { deck: d.unwrap_or(0), action: a.clone() })
}

fn deck_with(f: impl Fn(InputValue) -> Option<DeckAction> + Send + Sync + 'static) -> Producer {
    Box::new(move |d, v| f(v).map(|action| ControlAction::Deck { deck: d.unwrap_or(0), action }))
}

fn mixer_abs(f: fn(f32) -> MixerAction) -> Producer {
    Box::new(move |d, v| Some(ControlAction::Mixer { channel: d.unwrap_or(0), action: f(v.unit()) }))
}

fn stem_arg(a: &Args) -> Result<StemPart, MappingError> {
    let s = a.str("stem")?;
    StemPart::parse(&s).ok_or_else(|| a.err(format!("unknown stem `{s}` (vocals, drums, bass, inst)")))
}

fn bipolar(u: f32) -> f32 {
    (u * 2.0 - 1.0).clamp(-1.0, 1.0)
}

/// Parses an action name with its arguments.
pub fn parse_action(name: &str, args: &Table) -> Result<ActionTemplate, MappingError> {
    use Natural::*;
    let a = Args { name, t: args };
    let deck_scoped = name.starts_with("deck.") || name.starts_with("mixer.");
    let mut needs_deck = deck_scoped;
    let mut is_shift = false;
    let (natural, produce): (Natural, Producer) = match name {
        "ignore" => (Button, Box::new(|_, _| Some(ControlAction::Ignored))),
        "deck.play_pause" => (Button, deck_press(DeckAction::PlayPause)),
        "deck.cue" => (
            Button,
            deck_with(|v| Some(if v.pressed() { DeckAction::CuePress } else { DeckAction::CueRelease })),
        ),
        "deck.jump_to_start" => (Button, deck_press(DeckAction::JumpToStart)),
        "deck.sync" => (Button, deck_press(DeckAction::Sync)),
        "deck.sync_off" => (Button, deck_press(DeckAction::SyncOff)),
        "deck.master" => (Button, deck_press(DeckAction::SetMaster)),
        "deck.keylock" => (Button, deck_press(DeckAction::KeyLock)),
        "deck.key_sync" => (Button, deck_press(DeckAction::KeySync)),
        "deck.key_reset" => (Button, deck_press(DeckAction::KeyReset)),
        "deck.tempo" => (Absolute, deck_with(|v| Some(DeckAction::Tempo(bipolar(v.unit()))))),
        "deck.tempo_range" => (Button, deck_press(DeckAction::TempoRange)),
        "deck.tempo_reset" => (Button, deck_press(DeckAction::TempoReset)),
        "deck.tap_bpm" => (Button, deck_press(DeckAction::TapBpm)),
        "deck.quantize" => (Button, deck_press(DeckAction::Quantize)),
        "deck.jog_touch" => (Button, deck_with(|v| Some(DeckAction::JogTouch(v.pressed())))),
        "deck.jog_turn" => {
            let mode = match a.str("mode")?.as_str() {
                "scratch" => JogMode::Scratch,
                "bend" => JogMode::Bend,
                "search" => JogMode::Search,
                other => return Err(a.err(format!("unknown jog mode `{other}`"))),
            };
            (
                Relative,
                deck_with(move |v| {
                    let ticks = v.ticks();
                    (ticks != 0).then_some(DeckAction::JogTurn { ticks, mode })
                }),
            )
        }
        "deck.hot_cue" => {
            let slot = a.u8("slot")?;
            (Button, deck_with(move |v| Some(DeckAction::HotCue { slot, pressed: v.pressed() })))
        }
        "deck.hot_cue_delete" => {
            let slot = a.u8("slot")?;
            (Button, deck_press(DeckAction::HotCueDelete { slot }))
        }
        "deck.memory_cue_set" => (Button, deck_press(DeckAction::MemoryCueSet)),
        "deck.memory_cue_delete" => (Button, deck_press(DeckAction::MemoryCueDelete)),
        "deck.memory_cue_prev" => (Button, deck_press(DeckAction::MemoryCuePrev)),
        "deck.memory_cue_next" => (Button, deck_press(DeckAction::MemoryCueNext)),
        "deck.loop_in" => (Button, deck_press(DeckAction::LoopIn)),
        "deck.loop_out" => (Button, deck_press(DeckAction::LoopOut)),
        "deck.auto_loop" => {
            let beats = a.f32("beats")?;
            (Button, deck_press(DeckAction::AutoLoop { beats }))
        }
        "deck.loop_roll" => {
            let beats = a.f32("beats")?;
            (Button, deck_with(move |v| Some(DeckAction::LoopRoll { beats, pressed: v.pressed() })))
        }
        "deck.loop_halve" => (Button, deck_press(DeckAction::LoopHalve)),
        "deck.loop_double" => (Button, deck_press(DeckAction::LoopDouble)),
        "deck.reloop_exit" => (Button, deck_press(DeckAction::ReloopExit)),
        "deck.beat_jump" => {
            let beats = a.f32("beats")?;
            (Button, deck_press(DeckAction::BeatJump { beats }))
        }
        "deck.search" => {
            let forward = a.bool_or("forward", true)?;
            (Button, deck_with(move |v| Some(DeckAction::Search { forward, pressed: v.pressed() })))
        }
        "deck.slip" => (Button, deck_press(DeckAction::Slip)),
        "deck.vinyl" => (Button, deck_press(DeckAction::VinylMode)),
        "deck.shift" => {
            is_shift = true;
            (Button, deck_with(|v| Some(DeckAction::Shift(v.pressed()))))
        }
        "deck.pad_mode" => {
            let m = a.str("mode")?;
            let mode = PadMode::parse(&m).ok_or_else(|| a.err(format!("unknown pad mode `{m}`")))?;
            (Button, deck_press(DeckAction::PadMode(mode)))
        }
        "deck.pad" => {
            let index = a.u8("index")?;
            (
                Button,
                deck_with(move |v| {
                    Some(DeckAction::Pad { index, pressed: v.pressed(), velocity: v.velocity() })
                }),
            )
        }
        "deck.load" => (Button, deck_press(DeckAction::Load)),
        "deck.censor" => (Button, deck_with(|v| Some(DeckAction::Censor(v.pressed())))),
        "deck.reverse" => (Button, deck_press(DeckAction::Reverse)),
        "deck.needle_search" => (Absolute, deck_with(|v| Some(DeckAction::NeedleSearch(v.unit())))),
        "deck.select" => {
            // With `deck = n` the input selects that deck (software layers);
            // otherwise it reports the deck it is bound to (hardware layers).
            if args.contains_key("deck") {
                let target = a.u8("deck")?;
                needs_deck = false;
                (Button, on_press(move |_| ControlAction::Deck { deck: target, action: DeckAction::DeckSelect }))
            } else {
                (Button, deck_press(DeckAction::DeckSelect))
            }
        }

        // Part isolator knob: centre and above = full level, fully left = muted.
        "deck.stem_level" => {
            let stem = stem_arg(&a)?;
            (Absolute, deck_with(move |v| Some(DeckAction::StemLevel { stem, level: (v.unit() * 2.0).min(1.0) })))
        }
        "deck.stem_mute" => {
            let stem = stem_arg(&a)?;
            (Button, deck_press(DeckAction::StemMuteToggle { stem }))
        }

        "mixer.trim" => (Absolute, mixer_abs(MixerAction::Trim)),
        "mixer.eq_high" => (Absolute, mixer_abs(MixerAction::EqHigh)),
        "mixer.eq_mid" => (Absolute, mixer_abs(MixerAction::EqMid)),
        "mixer.eq_low" => (Absolute, mixer_abs(MixerAction::EqLow)),
        "mixer.color_fx" => (Absolute, mixer_abs(|u| MixerAction::ColorFx(bipolar(u)))),
        "mixer.fader" => (Absolute, mixer_abs(MixerAction::Fader)),
        "mixer.cue" => (
            Button,
            on_press(|d| ControlAction::Mixer { channel: d.unwrap_or(0), action: MixerAction::CueToggle }),
        ),
        "mixer.crossfader_assign" => {
            let side = match a.str("side")?.as_str() {
                "a" => CrossfaderSide::A,
                "thru" => CrossfaderSide::Thru,
                "b" => CrossfaderSide::B,
                o => return Err(a.err(format!("unknown crossfader side `{o}`"))),
            };
            (
                Button,
                on_press(move |d| ControlAction::Mixer {
                    channel: d.unwrap_or(0),
                    action: MixerAction::CrossfaderAssign(side),
                }),
            )
        }
        "crossfader" => (Absolute, Box::new(|_, v| Some(ControlAction::Crossfader(v.unit())))),
        "master_level" => (Absolute, Box::new(|_, v| Some(ControlAction::MasterLevel(v.unit())))),
        "booth_level" => (Absolute, Box::new(|_, v| Some(ControlAction::BoothLevel(v.unit())))),
        "headphone_mix" => (Absolute, Box::new(|_, v| Some(ControlAction::HeadphoneMix(v.unit())))),
        "headphone_level" => {
            (Absolute, Box::new(|_, v| Some(ControlAction::HeadphoneLevel(v.unit()))))
        }
        "master_cue" => (Button, on_press(|_| ControlAction::MasterCue)),

        n if n.starts_with("beat_fx.") => {
            let unit = a.u8_or("unit", 1)?;
            let fx = move |action: BeatFxAction| ControlAction::BeatFx { unit, action };
            match n {
                "beat_fx.toggle" => (Button, on_press(move |_| fx(BeatFxAction::Toggle))),
                "beat_fx.release" => (
                    Button,
                    Box::new(move |_, v: InputValue| Some(fx(BeatFxAction::Release(v.pressed())))) as Producer,
                ),
                "beat_fx.level" => {
                    (Absolute, Box::new(move |_, v: InputValue| Some(fx(BeatFxAction::Level(v.unit())))) as Producer)
                }
                "beat_fx.beat_down" => (Button, on_press(move |_| fx(BeatFxAction::BeatDown))),
                "beat_fx.beat_up" => (Button, on_press(move |_| fx(BeatFxAction::BeatUp))),
                "beat_fx.auto_beat" => (Button, on_press(move |_| fx(BeatFxAction::AutoBeat))),
                "beat_fx.tap" => (Button, on_press(move |_| fx(BeatFxAction::Tap))),
                "beat_fx.select_next" => (Button, on_press(move |_| fx(BeatFxAction::SelectNext))),
                "beat_fx.select_prev" => (Button, on_press(move |_| fx(BeatFxAction::SelectPrev))),
                "beat_fx.select" => {
                    let e = a.str("effect")?;
                    (Button, on_press(move |_| fx(BeatFxAction::Select(e.clone()))))
                }
                "beat_fx.assign" => {
                    let ch = a.u8("channel")?;
                    (Button, on_press(move |_| fx(BeatFxAction::Assign(ch))))
                }
                _ => return Err(MappingError::UnknownAction(name.to_string())),
            }
        }
        "color_fx.select" => {
            let e = a.str("effect")?;
            (Button, on_press(move |_| ControlAction::ColorFx(ColorFxAction::Select(e.clone()))))
        }
        "color_fx.next" => (Button, on_press(|_| ControlAction::ColorFx(ColorFxAction::Next))),
        "color_fx.toggle" => (Button, on_press(|_| ControlAction::ColorFx(ColorFxAction::Toggle))),

        "browse.rotate" => {
            let shifted = a.bool_or("shifted", false)?;
            (
                Relative,
                Box::new(move |_, v| {
                    let delta = v.ticks();
                    (delta != 0)
                        .then_some(ControlAction::Browse(BrowseAction::Rotate { delta, shifted }))
                }),
            )
        }
        "browse.press" => {
            let shifted = a.bool_or("shifted", false)?;
            (Button, on_press(move |_| ControlAction::Browse(BrowseAction::Press { shifted })))
        }
        "browse.back" => (Button, on_press(|_| ControlAction::Browse(BrowseAction::Back))),
        "browse.view_toggle" => (Button, on_press(|_| ControlAction::Browse(BrowseAction::ViewToggle))),
        "browse.related" => (Button, on_press(|_| ControlAction::Browse(BrowseAction::Related))),

        "sampler.pad" => {
            let slot = a.u8("slot")?;
            (
                Button,
                Box::new(move |_, v| {
                    Some(ControlAction::Sampler(SamplerAction::Pad {
                        slot,
                        pressed: v.pressed(),
                        velocity: v.velocity(),
                    }))
                }),
            )
        }
        "sampler.stop" => {
            let slot = a.u8("slot")?;
            (Button, on_press(move |_| ControlAction::Sampler(SamplerAction::Stop { slot })))
        }
        "sampler.bank_next" => (Button, on_press(|_| ControlAction::Sampler(SamplerAction::BankNext))),
        "sampler.bank_prev" => (Button, on_press(|_| ControlAction::Sampler(SamplerAction::BankPrev))),
        "sampler.volume" => (
            Absolute,
            Box::new(|_, v| Some(ControlAction::Sampler(SamplerAction::Volume(v.unit())))),
        ),
        "sampler.cue" => (Button, on_press(|_| ControlAction::Sampler(SamplerAction::CueToggle))),
        "recording.toggle" => (Button, on_press(|_| ControlAction::RecordingToggle)),
        "lighting.look" => {
            let look = a.u8("look")?.min(5);
            (Button, on_press(move |_| ControlAction::Lighting(LightingAction::Look(look))))
        }
        "lighting.strobe" => (Button, Box::new(|_, v| Some(ControlAction::Lighting(LightingAction::Strobe(v.pressed()))))),
        "lighting.flash" => (Button, Box::new(|_, v| Some(ControlAction::Lighting(LightingAction::Flash(v.pressed()))))),
        "lighting.fog" => (Button, Box::new(|_, v| Some(ControlAction::Lighting(LightingAction::Fog(v.pressed()))))),
        "lighting.blackout" => (Button, on_press(|_| ControlAction::Lighting(LightingAction::BlackoutToggle))),
        "lighting.master" => (Absolute, Box::new(|_, v| Some(ControlAction::Lighting(LightingAction::Master(v.unit()))))),
        "lighting.cue_go" => (Button, on_press(|_| ControlAction::Lighting(LightingAction::CueGo))),
        "lighting.cue_back" => (Button, on_press(|_| ControlAction::Lighting(LightingAction::CueBack))),
        "lighting.mode" => {
            let m = a.str("mode")?;
            if !["auto", "easy", "pro"].contains(&m.as_str()) {
                return Err(a.err(format!("unknown lighting mode `{m}` (auto, easy, pro)")));
            }
            (Button, on_press(move |_| ControlAction::Lighting(LightingAction::Mode(m.clone()))))
        }
        _ => return Err(MappingError::UnknownAction(name.to_string())),
    };
    if name == "ignore" {
        needs_deck = false;
    }
    Ok(ActionTemplate { name: name.to_string(), natural, needs_deck, is_shift, produce })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_action_parses_with_sample_args() {
        let mut t = Table::new();
        t.insert("slot".into(), Value::Integer(1));
        t.insert("beats".into(), Value::Float(4.0));
        t.insert("mode".into(), Value::String("scratch".into()));
        t.insert("index".into(), Value::Integer(0));
        t.insert("side".into(), Value::String("a".into()));
        t.insert("effect".into(), Value::String("echo".into()));
        t.insert("channel".into(), Value::Integer(1));
        t.insert("stem".into(), Value::String("vocals".into()));
        for n in ACTION_NAMES {
            let mut args = t.clone();
            if *n == "deck.pad_mode" {
                args.insert("mode".into(), Value::String("hot_cue".into()));
            }
            if *n == "lighting.mode" {
                args.insert("mode".into(), Value::String("pro".into()));
            }
            args.insert("look".into(), Value::Integer(2));
            parse_action(n, &args).unwrap_or_else(|e| panic!("{n}: {e}"));
        }
    }

    #[test]
    fn cue_press_and_release() {
        let t = parse_action("deck.cue", &Table::new()).unwrap();
        let p = t.produce(Some(2), InputValue::Button { pressed: true, velocity: 127 });
        assert_eq!(p, Some(ControlAction::Deck { deck: 2, action: DeckAction::CuePress }));
        let r = t.produce(Some(2), InputValue::Button { pressed: false, velocity: 0 });
        assert_eq!(r, Some(ControlAction::Deck { deck: 2, action: DeckAction::CueRelease }));
        assert_eq!(t.produce(None, InputValue::Button { pressed: true, velocity: 1 }), None);
    }

    #[test]
    fn lighting_controls_map() {
        let mut look = Table::new();
        look.insert("look".into(), Value::Integer(3));
        let t = parse_action("lighting.look", &look).unwrap();
        assert_eq!(t.produce(None, InputValue::Button { pressed: true, velocity: 127 }), Some(ControlAction::Lighting(LightingAction::Look(3))));
        let t = parse_action("lighting.strobe", &Table::new()).unwrap();
        assert_eq!(t.produce(None, InputValue::Button { pressed: false, velocity: 0 }), Some(ControlAction::Lighting(LightingAction::Strobe(false))));
        let mut bad = Table::new();
        bad.insert("mode".into(), Value::String("disco".into()));
        assert!(parse_action("lighting.mode", &bad).is_err());
    }
}
