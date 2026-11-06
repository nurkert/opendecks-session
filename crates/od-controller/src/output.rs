//! Feedback → MIDI rendering with diffing, software blinking and rate limits.
//!
//! The application pushes [`ControlFeedback`] whenever it likes; nothing is
//! sent immediately. [`OutputEngine::poll`] is called periodically (e.g.
//! every 5–10 ms) and returns only the messages whose byte value actually
//! changed since it was last sent, honouring the per-output minimum interval
//! (meters) and a global per-poll budget.

use crate::feedback::{ControlFeedback, FeedbackValue, LedState};
use crate::mapping::{Mapping, OutputBinding, OutputMode};
use crate::midi::MidiAddr;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Render {
    Static(u8),
    Blink { on: u8, off: u8, period_ms: u32 },
}

#[derive(Clone, Copy, Debug)]
struct AddrState {
    desired: Render,
    min_interval: Duration,
    /// Level outputs are lower priority than LEDs within the poll budget.
    is_level: bool,
    sent: Option<u8>,
    sent_at: Option<Instant>,
}

pub struct OutputEngine {
    mapping: Arc<Mapping>,
    /// Last value per output binding (kept even while hidden by a layer).
    values: Vec<Option<FeedbackValue>>,
    addrs: BTreeMap<MidiAddr, AddrState>,
    /// Active deck per side (software layers).
    active: HashMap<u8, u8>,
    epoch: Instant,
}

impl OutputEngine {
    pub fn new(mapping: Arc<Mapping>) -> Self {
        let n = mapping.outputs.len();
        let mut active = HashMap::new();
        if let Some(l) = &mapping.file.layers {
            for (i, side) in l.sides.iter().enumerate() {
                active.insert(i as u8 + 1, side[0]);
            }
        }
        Self {
            mapping,
            values: vec![None; n],
            addrs: BTreeMap::new(),
            active,
            epoch: Instant::now(),
        }
    }

    /// Sets the blink phase reference (for deterministic tests).
    pub fn set_epoch(&mut self, t: Instant) {
        self.epoch = t;
    }

    fn is_visible(&self, b: &OutputBinding) -> bool {
        match b.active_when {
            Some((side, deck)) => self.active.get(&side) == Some(&deck),
            None => true,
        }
    }

    fn render(&self, b: &OutputBinding, v: Option<FeedbackValue>) -> Render {
        let opts = self.mapping.options();
        let led = |state: LedState, on: u8| match state {
            LedState::Off => Render::Static(b.off),
            LedState::On => Render::Static(on),
            LedState::Dim => Render::Static(b.dim.unwrap_or(b.off)),
            LedState::Blink | LedState::BlinkFast => match b.blink {
                Some(hw) => Render::Static(hw),
                None => Render::Blink {
                    on,
                    off: b.off,
                    period_ms: if state == LedState::Blink { opts.blink_ms } else { opts.blink_fast_ms },
                },
            },
        };
        match (b.mode, v) {
            (_, None) => Render::Static(b.off),
            (OutputMode::Level, Some(FeedbackValue::Level(l))) => {
                let l = l.clamp(0.0, 1.0);
                Render::Static((b.min as f32 + l * (b.max as f32 - b.min as f32)).round() as u8)
            }
            (OutputMode::Level, Some(FeedbackValue::Led(s))) => led(s, b.max),
            (_, Some(FeedbackValue::Level(l))) => led(LedState::from_bool(l >= 0.5), b.on),
            (_, Some(FeedbackValue::Led(s))) => led(s, b.on),
            (_, Some(FeedbackValue::Color { color, state })) => match color {
                None => Render::Static(b.off),
                Some(c) => {
                    let on = match &b.palette {
                        Some(p) if !p.is_empty() => {
                            let colors: Vec<_> = p.iter().map(|(c, _)| *c).collect();
                            p[c.nearest(&colors)].1
                        }
                        _ => b.on,
                    };
                    led(state, on)
                }
            },
        }
    }

    fn refresh_binding(&mut self, idx: usize) {
        let b = &self.mapping.outputs[idx];
        if !self.is_visible(b) {
            return;
        }
        let desired = self.render(b, self.values[idx]);
        let min_interval = Duration::from_millis(b.min_interval_ms as u64);
        let is_level = b.mode == OutputMode::Level;
        let st = self.addrs.entry(b.addr).or_insert(AddrState {
            desired,
            min_interval,
            is_level,
            sent: None,
            sent_at: None,
        });
        st.desired = desired;
        st.min_interval = min_interval;
        st.is_level = is_level;
    }

    /// Applies one feedback message.
    pub fn apply(&mut self, fb: &ControlFeedback) {
        match fb {
            ControlFeedback::Set { target, value } => {
                let idxs: Vec<usize> = self.mapping.outputs_for(target).to_vec();
                for i in idxs {
                    self.values[i] = Some(*value);
                    self.refresh_binding(i);
                }
            }
            ControlFeedback::Parameter { .. } => {}
            ControlFeedback::ResendAll => {
                for st in self.addrs.values_mut() {
                    st.sent = None;
                    st.sent_at = None;
                }
            }
            ControlFeedback::AllOff => {
                for v in self.values.iter_mut() {
                    *v = None;
                }
                for i in 0..self.mapping.outputs.len() {
                    self.refresh_binding(i);
                }
            }
        }
    }

    /// Initialises every output to "off" so the first poll clears the
    /// controller's LEDs.
    pub fn reset(&mut self) {
        self.addrs.clear();
        for i in 0..self.mapping.outputs.len() {
            self.refresh_binding(i);
        }
    }

    /// Software deck layer switch: `side` now shows `deck`.
    pub fn set_active_deck(&mut self, side: u8, deck: u8) {
        if self.active.insert(side, deck) == Some(deck) {
            return;
        }
        for i in 0..self.mapping.outputs.len() {
            if self.mapping.outputs[i].active_when == Some((side, deck)) {
                self.refresh_binding(i);
            }
        }
    }

    /// Returns the messages to send now.
    pub fn poll(&mut self, now: Instant) -> Vec<[u8; 3]> {
        let budget = self.mapping.options().max_messages_per_poll;
        let elapsed_ms = now.saturating_duration_since(self.epoch).as_millis() as u64;
        let mut out = Vec::new();
        for pass_levels in [false, true] {
            for (addr, st) in self.addrs.iter_mut() {
                if st.is_level != pass_levels {
                    continue;
                }
                let value = match st.desired {
                    Render::Static(v) => v,
                    Render::Blink { on, off, period_ms } => {
                        let half = (period_ms / 2).max(1) as u64;
                        if (elapsed_ms / half).is_multiple_of(2) { on } else { off }
                    }
                };
                if st.sent == Some(value) {
                    continue;
                }
                if let (Some(at), Some(_)) = (st.sent_at, st.sent)
                    && now.saturating_duration_since(at) < st.min_interval {
                        continue;
                    }
                if out.len() >= budget {
                    // Out of budget: stays dirty and goes out on the next poll.
                    continue;
                }
                st.sent = Some(value);
                st.sent_at = Some(now);
                out.push(addr.message(value));
            }
        }
        out
    }
}
