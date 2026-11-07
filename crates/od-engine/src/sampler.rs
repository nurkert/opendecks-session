//! Sampler: `SAMPLER_TOTAL` slots (banks × 16 pads), one-shot / loop / gate,
//! quantised triggering, tempo sync, and a 16-step sequencer locked to the
//! master beat clock.
//!
//! Blocks are rendered in segments between events (sequencer steps,
//! quantize boundaries), so triggers are sample accurate. Restarting or
//! stopping a voice crossfades it out over [`FADE`] frames (no clicks).

use std::sync::Arc;

use crate::beatfx::BeatClock;
use crate::command::{SampleMode, SamplerCommand};
use crate::config::{SAMPLER_TOTAL, SEQ_STEPS};
use crate::dsp::smooth::Smoothed;
use crate::rt::RtCtx;
use crate::state::{SamplerSlotState, SequencerState};
use crate::track::SampleData;

/// Declick fade length in frames.
const FADE: u32 = 128;

#[derive(Clone, Copy, Debug, Default)]
struct Tail {
    pos: f64,
    remaining: u32,
}

struct Slot {
    data: Option<Arc<SampleData>>,
    playing: bool,
    pending: bool,
    pos: f64,
    mode: SampleMode,
    gain: Smoothed,
    bpm: Option<f64>,
    sync: bool,
    tail: Tail,
    pattern: u16,
}

pub struct Sampler {
    slots: [Slot; SAMPLER_TOTAL],
    quantize: Option<f64>,
    seq_playing: bool,
    seq_recording: bool,
    /// Master beat at the end of the last block.
    last_beat: f64,
    /// Last sequencer boundary triggered (16th-note index).
    last_step: f64,
    step: u8,
}

#[inline]
fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

#[inline]
fn frame_at(s: &[f32], pos: f64) -> [f32; 2] {
    let total = (s.len() / 2) as isize;
    let ip = pos.floor();
    let t = (pos - ip) as f32;
    let i = ip as isize;
    if t == 0.0 && i >= 0 && i < total {
        let k = i as usize * 2;
        return [s[k], s[k + 1]];
    }
    let get = |j: isize, c: usize| -> f32 {
        if j >= 0 && j < total {
            s[j as usize * 2 + c]
        } else {
            0.0
        }
    };
    let mut out = [0.0; 2];
    for (c, o) in out.iter_mut().enumerate() {
        *o = hermite(get(i - 1, c), get(i, c), get(i + 1, c), get(i + 2, c), t);
    }
    out
}

impl Slot {
    fn start(&mut self) {
        if self.data.is_none() {
            return;
        }
        if self.playing {
            self.tail = Tail {
                pos: self.pos,
                remaining: FADE,
            };
        }
        self.playing = true;
        self.pending = false;
        self.pos = 0.0;
    }

    fn stop(&mut self) {
        self.pending = false;
        if self.playing {
            self.tail = Tail {
                pos: self.pos,
                remaining: FADE,
            };
        }
        self.playing = false;
    }

    fn speed(&self, clock: &BeatClock) -> f64 {
        match (self.sync, self.bpm) {
            (true, Some(bpm)) if bpm > 1.0 && clock.bpm > 1.0 => clock.bpm / bpm,
            _ => 1.0,
        }
    }

    /// Mixes frames `out` (interleaved) of this slot.
    fn render(&mut self, out: &mut [f32], level: f32, speed: f64) {
        let Some(d) = &self.data else {
            self.playing = false;
            self.tail.remaining = 0;
            return;
        };
        let total = d.frames() as f64;
        if total <= 0.0 {
            self.playing = false;
            self.tail.remaining = 0;
            return;
        }
        let looping = matches!(self.mode, SampleMode::Loop | SampleMode::Gate);
        for f in out.as_chunks_mut::<2>().0 {
            let g = self.gain.next() * level;
            if self.tail.remaining > 0 {
                let w = self.tail.remaining as f32 / FADE as f32;
                if self.tail.pos < total {
                    let v = frame_at(&d.samples, self.tail.pos);
                    f[0] += v[0] * g * w;
                    f[1] += v[1] * g * w;
                }
                self.tail.pos += speed;
                if looping && self.tail.pos >= total {
                    self.tail.pos -= total;
                }
                self.tail.remaining -= 1;
            }
            if self.playing {
                if self.pos >= total {
                    if looping {
                        self.pos -= total * (self.pos / total).floor();
                    } else {
                        self.playing = false;
                        continue;
                    }
                }
                let v = frame_at(&d.samples, self.pos);
                f[0] += v[0] * g;
                f[1] += v[1] * g;
                self.pos += speed;
            }
        }
    }
}

impl Sampler {
    pub fn new(sample_rate: u32) -> Self {
        let ramp = (sample_rate as f32 * 0.01) as u32;
        Self {
            slots: std::array::from_fn(|_| Slot {
                data: None,
                playing: false,
                pending: false,
                pos: 0.0,
                mode: SampleMode::OneShot,
                gain: Smoothed::new(1.0, ramp),
                bpm: None,
                sync: false,
                tail: Tail::default(),
                pattern: 0,
            }),
            quantize: None,
            seq_playing: false,
            seq_recording: false,
            last_beat: 0.0,
            last_step: -1e9,
            step: 0,
        }
    }

    fn trigger(&mut self, slot: usize) {
        let quantize = self.quantize;
        let s = &mut self.slots[slot];
        if s.data.is_none() {
            return;
        }
        if self.seq_recording && self.seq_playing {
            let pos = self.last_beat * 4.0;
            let nearest = pos.round();
            s.pattern |= 1 << (nearest as i64).rem_euclid(SEQ_STEPS as i64);
            if nearest > pos {
                // The sequencer plays the upcoming step itself.
                return;
            }
            s.start();
            return;
        }
        if quantize.is_some() {
            s.pending = true;
        } else {
            s.start();
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
                    s.pending = false;
                    s.tail.remaining = 0;
                    s.pos = 0.0;
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
                    s.pending = false;
                    s.tail.remaining = 0;
                }
            }
            SamplerCommand::Trigger { slot } if slot < SAMPLER_TOTAL => self.trigger(slot),
            SamplerCommand::Trigger { .. } => {}
            SamplerCommand::Release { slot } => {
                if let Some(s) = self.slots.get_mut(slot)
                    && s.mode == SampleMode::Gate
                {
                    s.stop();
                }
            }
            SamplerCommand::Stop { slot } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.stop();
                }
            }
            SamplerCommand::StopAll => self.slots.iter_mut().for_each(Slot::stop),
            SamplerCommand::SetMode { slot, mode } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.mode = mode;
                }
            }
            SamplerCommand::SetGain { slot, gain } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.gain.set(gain.clamp(0.0, 2.0));
                }
            }
            SamplerCommand::SetBpm { slot, bpm } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.bpm = bpm.filter(|b| b.is_finite() && *b > 1.0);
                }
            }
            SamplerCommand::SetSync { slot, on } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.sync = on;
                }
            }
            SamplerCommand::SetQuantize(q) => {
                self.quantize = q
                    .filter(|q| q.is_finite() && *q > 0.0)
                    .map(|q| q.clamp(1.0 / 16.0, 16.0));
                if self.quantize.is_none() {
                    for s in &mut self.slots {
                        if s.pending {
                            s.start();
                        }
                    }
                }
            }
            SamplerCommand::SeqStep { slot, step, on } => {
                if let Some(s) = self.slots.get_mut(slot)
                    && step < SEQ_STEPS
                {
                    if on {
                        s.pattern |= 1 << step;
                    } else {
                        s.pattern &= !(1 << step);
                    }
                }
            }
            SamplerCommand::SeqPattern { slot, mask } => {
                if let Some(s) = self.slots.get_mut(slot) {
                    s.pattern = mask;
                }
            }
            SamplerCommand::SeqClear => self.slots.iter_mut().for_each(|s| s.pattern = 0),
            SamplerCommand::SeqPlay(on) => {
                if on && !self.seq_playing {
                    self.last_step = -1e9;
                }
                self.seq_playing = on;
            }
            SamplerCommand::SeqRecord(on) => self.seq_recording = on,
        }
    }

    /// Frames from the block start until beat `b` (rounded up), if within
    /// reach.
    fn offset_of(b: f64, clock: &BeatClock) -> usize {
        ((b - clock.beat) / clock.beats_per_frame - 1e-6)
            .ceil()
            .max(0.0) as usize
    }

    /// Mixes all slots into `out` (interleaved stereo, added), scaled by
    /// `level`; `clock` is the master beat clock of the block.
    pub fn mix_into(&mut self, out: &mut [f32], level: f32, clock: &BeatClock) {
        let n = out.len() / 2;
        let bpf = clock.beats_per_frame;
        let timed = bpf > 0.0 && bpf.is_finite();
        let mut next_step = (clock.beat * 4.0 - 1e-9).ceil();
        // Never trigger a step twice when the master beat jitters backwards
        // across a boundary (sync phase correction); a real jump back resets.
        if next_step <= self.last_step && next_step > self.last_step - 2.0 {
            next_step = self.last_step + 1.0;
        }
        let mut cursor = 0;
        loop {
            let step_off = if timed && self.seq_playing {
                Self::offset_of(next_step / 4.0, clock).max(cursor)
            } else {
                usize::MAX
            };
            let quant_off = if self.slots.iter().any(|s| s.pending) {
                match self.quantize {
                    Some(q) if timed => {
                        let beat_now = clock.beat + cursor as f64 * bpf;
                        let b = (beat_now / q - 1e-9).ceil() * q;
                        Self::offset_of(b, clock).max(cursor)
                    }
                    _ => cursor,
                }
            } else {
                usize::MAX
            };
            let ev = n.min(step_off).min(quant_off);
            let step_ev = ev < n && step_off == ev;
            let quant_ev = ev < n && quant_off == ev;
            if ev > cursor {
                let seg = &mut out[cursor * 2..ev * 2];
                for s in &mut self.slots {
                    if s.playing || s.tail.remaining > 0 {
                        let speed = s.speed(clock);
                        s.render(seg, level, speed);
                    }
                }
            }
            if ev >= n {
                break;
            }
            cursor = ev;
            if step_ev {
                let step = (next_step as i64).rem_euclid(SEQ_STEPS as i64) as u16;
                for s in &mut self.slots {
                    if s.pattern & (1 << step) != 0 {
                        s.start();
                    }
                }
                self.last_step = next_step;
                next_step += 1.0;
            }
            if quant_ev {
                for s in &mut self.slots {
                    if s.pending {
                        s.start();
                    }
                }
            }
        }
        self.last_beat = clock.beat + n as f64 * bpf;
        self.step = ((self.last_beat * 4.0).floor() as i64).rem_euclid(SEQ_STEPS as i64) as u8;
    }

    pub fn sequencer(&self) -> SequencerState {
        SequencerState {
            playing: self.seq_playing,
            recording: self.seq_recording,
            step: self.step,
            quantize: self.quantize,
        }
    }

    pub fn state(&self) -> [SamplerSlotState; SAMPLER_TOTAL] {
        std::array::from_fn(|i| {
            let s = &self.slots[i];
            let frames = s.data.as_ref().map_or(0, |d| d.frames());
            SamplerSlotState {
                loaded: s.data.is_some(),
                playing: s.playing,
                pending: s.pending,
                mode: s.mode,
                gain: s.gain.target(),
                progress: if frames > 0 {
                    (s.pos / frames as f64).min(1.0) as f32
                } else {
                    0.0
                },
                bpm: s.bpm,
                sync: s.sync,
                pattern: s.pattern,
            }
        })
    }
}
