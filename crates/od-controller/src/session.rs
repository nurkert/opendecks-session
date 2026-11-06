//! I/O-free controller state machine: raw MIDI in → actions out, feedback in
//! → MIDI out. The I/O layer (`io`) and the tests drive it directly.

use crate::action::{ControlAction, DeckAction, ParamTarget};
use crate::device_module::{module_for, DeviceModule, ModuleContext};
use crate::feedback::ControlFeedback;
use crate::mapping::{InputMode, LayerMode, Mapping};
use crate::midi::MidiMessage;
use crate::output::OutputEngine;
use crate::takeover::SoftTakeover;
use crate::templates::InputValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// What happened to one mapping binding for an incoming message.
#[derive(Clone, Debug, PartialEq)]
pub struct MappingHit {
    pub binding: String,
    pub action: Option<ControlAction>,
    /// Why the action was not emitted, if it was not.
    pub suppressed: Option<&'static str>,
    pub verify: bool,
}

/// Result of processing one raw MIDI message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Processed {
    pub message: Option<MidiMessage>,
    pub actions: Vec<ControlAction>,
    pub hits: Vec<MappingHit>,
}

pub struct ControllerSession {
    mapping: Arc<Mapping>,
    module: Box<dyn DeviceModule>,
    takeover: SoftTakeover,
    output: OutputEngine,
    /// Pending MSB per (binding index) of 14-bit pairs.
    msb: HashMap<usize, u8>,
    /// Shift held per deck slot.
    shift: HashMap<u8, bool>,
    /// Active deck per side (from `[layers]`).
    active: HashMap<u8, u8>,
    keepalive_due: Vec<Option<Instant>>,
}

impl ControllerSession {
    pub fn new(mapping: Arc<Mapping>) -> Self {
        let module = module_for(mapping.device().module.as_deref());
        Self::with_module(mapping, module)
    }

    pub fn with_module(mapping: Arc<Mapping>, module: Box<dyn DeviceModule>) -> Self {
        let mut active = HashMap::new();
        if let Some(l) = &mapping.file.layers {
            for (i, side) in l.sides.iter().enumerate() {
                active.insert(i as u8 + 1, side[0]);
            }
        }
        Self {
            takeover: SoftTakeover::new(mapping.options().takeover_threshold),
            output: OutputEngine::new(mapping.clone()),
            keepalive_due: vec![None; mapping.file.keepalive.len()],
            mapping,
            module,
            msb: HashMap::new(),
            shift: HashMap::new(),
            active,
        }
    }

    pub fn mapping(&self) -> &Arc<Mapping> {
        &self.mapping
    }

    pub fn output_mut(&mut self) -> &mut OutputEngine {
        &mut self.output
    }

    /// Deck currently active on `side` (1-based), for 4-deck layouts.
    pub fn active_deck(&self, side: u8) -> Option<u8> {
        self.active.get(&side).copied()
    }

    fn software_layers(&self) -> bool {
        self.mapping.file.layers.as_ref().is_some_and(|l| l.mode == LayerMode::Software)
    }

    fn resolve_deck(&self, slot: Option<u8>) -> Option<u8> {
        let slot = slot?;
        if self.software_layers() { self.active.get(&slot).copied().or(Some(slot)) } else { Some(slot) }
    }

    fn shift_held(&self, slot: Option<u8>) -> bool {
        match slot {
            Some(s) => self.shift.get(&s).copied().unwrap_or(false),
            None => self.shift.values().any(|v| *v),
        }
    }

    /// Messages to send right after connecting: init sequence and all
    /// outputs reset to off.
    pub fn connect_messages(&mut self) -> Vec<Vec<u8>> {
        self.output.reset();
        self.mapping.file.init.iter().map(|m| m.bytes.clone()).collect()
    }

    /// Processes one raw MIDI message.
    pub fn handle_midi(&mut self, bytes: &[u8]) -> Processed {
        let mut res = Processed { message: MidiMessage::parse(bytes), ..Default::default() };
        let mapping = self.mapping.clone();
        let mut raw_actions = Vec::new();
        let ctx = ModuleContext { mapping: &mapping };
        if self.module.on_raw(&ctx, bytes, &mut raw_actions) {
            res.actions = raw_actions;
            return res;
        }
        let Some((addr, value)) = res.message.as_ref().and_then(MidiMessage::addr_value) else {
            return res;
        };
        for &idx in mapping.inputs_at(addr) {
            let b = &mapping.inputs[idx];
            let iv = match b.mode {
                InputMode::Button => InputValue::Button { pressed: value > 0, velocity: value },
                InputMode::Absolute => {
                    let v = value as f32 / 127.0;
                    InputValue::Absolute(if b.invert { 1.0 - v } else { v })
                }
                InputMode::Absolute14 => {
                    if addr == b.addr {
                        // MSB first; the value is emitted when the LSB arrives.
                        self.msb.insert(idx, value);
                        continue;
                    }
                    let Some(msb) = self.msb.get(&idx).copied() else { continue };
                    let v = (((msb as u16) << 7) | value as u16) as f32 / 16383.0;
                    InputValue::Absolute(if b.invert { 1.0 - v } else { v })
                }
                InputMode::Relative => {
                    let d = b.encoding.decode(value);
                    InputValue::Relative(if b.invert { -d } else { d })
                }
            };
            if let Some(want) = b.shift
                && self.shift_held(b.slot) != want {
                    continue;
                }
            let deck = self.resolve_deck(b.slot);
            let Some(action) = b.template.produce(deck, iv) else { continue };
            if b.template.is_shift
                && let (Some(slot), InputValue::Button { pressed, .. }) = (b.slot, iv) {
                    self.shift.insert(slot, pressed);
                }
            if b.takeover
                && let Some((target, v)) = action.param()
                    && !self.takeover.input(target, target.to_unit(v)) {
                        res.hits.push(MappingHit {
                            binding: b.name.clone(),
                            action: Some(action),
                            suppressed: Some("soft takeover"),
                            verify: b.verify,
                        });
                        continue;
                    }
            if let ControlAction::Deck { deck, action: DeckAction::DeckSelect } = action {
                self.select_deck(deck);
            }
            res.hits.push(MappingHit {
                binding: b.name.clone(),
                action: Some(action.clone()),
                suppressed: None,
                verify: b.verify,
            });
            raw_actions.push(action);
        }
        for a in raw_actions {
            self.module.on_action(&ctx, a, &mut res.actions);
        }
        res
    }

    /// Makes `deck` the active deck of its side.
    pub fn select_deck(&mut self, deck: u8) {
        let Some(layers) = &self.mapping.file.layers else { return };
        let Some(side) = layers.sides.iter().position(|s| s.contains(&deck)) else { return };
        let side = side as u8 + 1;
        if self.active.insert(side, deck) != Some(deck) {
            // Physical controls now address another deck: pick up again.
            self.takeover.invalidate_deck(deck);
            self.msb.clear();
            if self.software_layers() {
                self.output.set_active_deck(side, deck);
            }
        }
    }

    /// Applies feedback from the application.
    pub fn feedback(&mut self, fb: &ControlFeedback) {
        if let ControlFeedback::Parameter { target, value } = fb {
            self.takeover.set_engine(*target, target.to_unit(*value));
        }
        self.output.apply(fb);
    }

    /// Whether a parameter currently follows its physical control.
    pub fn is_caught(&self, target: ParamTarget) -> bool {
        self.takeover.is_caught(target)
    }

    /// Messages due now: changed LEDs/meters and keepalives.
    pub fn poll_output(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        for (i, k) in self.mapping.file.keepalive.iter().enumerate() {
            let due = self.keepalive_due[i].is_none_or(|d| now >= d);
            if due {
                out.push(k.bytes.clone());
                self.keepalive_due[i] = Some(now + Duration::from_millis(k.interval_ms as u64));
            }
        }
        out.extend(self.output.poll(now).into_iter().map(|m| m.to_vec()));
        out
    }
}
