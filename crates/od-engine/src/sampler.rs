//! Sampler: one-shot or looped samples triggered from pads.

use std::sync::Arc;

use crate::command::{SampleMode, SamplerCommand};
use crate::config::SAMPLER_SLOTS;
use crate::rt::RtCtx;
use crate::state::SamplerSlotState;
use crate::track::SampleData;

#[derive(Default)]
struct Slot {
    data: Option<Arc<SampleData>>,
    playing: bool,
    pos: usize,
    mode: SampleMode,
    gain: f32,
}

pub struct Sampler {
    slots: [Slot; SAMPLER_SLOTS],
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot {
                gain: 1.0,
                ..Slot::default()
            }),
        }
    }

    pub fn apply(&mut self, cmd: SamplerCommand, ctx: &mut RtCtx) {
        match cmd {
            SamplerCommand::Load { slot, sample } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    if let Some(old) = s.data.replace(sample) {
                        ctx.dispose_sample(old);
                    }
                    s.playing = false;
                    s.pos = 0;
                } else {
                    ctx.dispose_sample(sample);
                }
            }
            SamplerCommand::Unload { slot } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    if let Some(old) = s.data.take() {
                        ctx.dispose_sample(old);
                    }
                    s.playing = false;
                }
            }
            SamplerCommand::Trigger { slot } => {
                if let Some(s) = self.slots.get_mut(slot)
                    && s.data.is_some()
                {
                    s.playing = true;
                    s.pos = 0;
                }
            }
            SamplerCommand::Stop { slot } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.playing = false;
                }
            }
            SamplerCommand::StopAll => self.slots.iter_mut().for_each(|s| s.playing = false),
            SamplerCommand::SetMode { slot, mode } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.mode = mode;
                }
            }
            SamplerCommand::SetGain { slot, gain } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.gain = gain.clamp(0.0, 2.0);
                }
            }
        }
    }

    /// Mixes all playing slots into `out` (interleaved stereo), scaled by `level`.
    pub fn mix_into(&mut self, out: &mut [f32], level: f32) {
        for s in &mut self.slots {
            if !s.playing {
                continue;
            }
            let Some(d) = &s.data else {
                s.playing = false;
                continue;
            };
            let total = d.frames();
            if total == 0 {
                s.playing = false;
                continue;
            }
            let g = s.gain * level;
            for f in out.as_chunks_mut::<2>().0 {
                if s.pos >= total {
                    if s.mode == SampleMode::Loop {
                        s.pos = 0;
                    } else {
                        s.playing = false;
                        break;
                    }
                }
                f[0] += d.samples[s.pos * 2] * g;
                f[1] += d.samples[s.pos * 2 + 1] * g;
                s.pos += 1;
            }
        }
    }

    pub fn state(&self) -> [SamplerSlotState; SAMPLER_SLOTS] {
        std::array::from_fn(|i| {
            let s = &self.slots[i];
            let frames = s.data.as_ref().map_or(0, |d| d.frames());
            SamplerSlotState {
                loaded: s.data.is_some(),
                playing: s.playing,
                mode: s.mode,
                gain: s.gain,
                progress: if frames > 0 {
                    s.pos as f32 / frames as f32
                } else {
                    0.0
                },
            }
        })
    }
}
