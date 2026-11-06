//! Colour FX: one bipolar knob per channel (−1..=1, 0 = bypass).
//!
//! Echo, dub echo and space feed their wet path with the knob amount, so
//! their tails ring out after the knob returns to the centre; the channel
//! is bit-exact dry again once the tail is silent.

use crate::beatfx::BeatClock;
use crate::command::ColorFxKind;
use crate::dsp::Noise;
use crate::dsp::delay::DelayLine;
use crate::dsp::pitchshift::PitchShifter;
use crate::dsp::reverb::Reverb;
use crate::dsp::smooth::Smoothed;
use crate::dsp::svf::Svf;

const UPDATE_EVERY: usize = 16;
const DEAD_ZONE: f32 = 0.01;
/// Wet energy per frame below which a tail counts as silent.
const QUIET: f32 = 1e-10;

pub struct ColorFx {
    sr: f32,
    kind: ColorFxKind,
    amount: Smoothed,
    svf: Svf,
    noise_svf: Svf,
    noise: Noise,
    reverb: Reverb,
    delay: DelayLine,
    shifter: PitchShifter,
    crush_hold: [f32; 2],
    crush_phase: f32,
    gate: f32,
    /// Feedback amount held after the knob returns (dub echo).
    fb_hold: f32,
    /// Tail still ringing.
    ringing: bool,
    quiet_frames: f32,
    counter: usize,
}

impl ColorFx {
    pub fn new(sample_rate: u32, seed: u32) -> Self {
        let sr = sample_rate as f32;
        Self {
            sr,
            kind: ColorFxKind::Filter,
            amount: Smoothed::new(0.0, (sr * 0.02) as u32),
            svf: Svf::new(1000.0, 0.7, sr),
            noise_svf: Svf::new(1000.0, 0.7, sr),
            noise: Noise::new(seed),
            reverb: Reverb::new(sample_rate),
            delay: DelayLine::new((sr * 2.5) as usize),
            shifter: PitchShifter::new((sr * 0.04) as usize, sr),
            crush_hold: [0.0; 2],
            crush_phase: 0.0,
            gate: 1.0,
            fb_hold: 0.0,
            ringing: false,
            quiet_frames: 0.0,
            counter: 0,
        }
    }

    pub fn set_kind(&mut self, kind: ColorFxKind) {
        if kind != self.kind {
            self.kind = kind;
            self.svf.reset();
            self.noise_svf.reset();
            if self.ringing {
                self.reverb.clear();
                self.delay.clear();
                self.ringing = false;
            }
            self.gate = 1.0;
            self.fb_hold = 0.0;
        }
    }

    pub fn kind(&self) -> ColorFxKind {
        self.kind
    }

    pub fn set_amount(&mut self, a: f32) {
        self.amount.set(a.clamp(-1.0, 1.0));
    }

    pub fn amount(&self) -> f32 {
        self.amount.target()
    }

    fn has_tail(kind: ColorFxKind) -> bool {
        matches!(kind, ColorFxKind::Echo | ColorFxKind::DubEcho | ColorFxKind::Space)
    }

    fn bypassed(&self) -> bool {
        self.amount.is_settled() && self.amount.current().abs() < DEAD_ZONE && !self.ringing
    }

    /// Filter cutoff for a knob value: negative sweeps a low-pass from 20 kHz
    /// down to 60 Hz, positive a high-pass from 20 Hz up to 10 kHz.
    fn cutoff(a: f32) -> f32 {
        if a < 0.0 {
            20_000.0 * (60.0f32 / 20_000.0).powf(-a)
        } else {
            20.0 * (10_000.0f32 / 20.0).powf(a)
        }
    }

    fn update_filters(&mut self, a: f32) {
        let mag = a.abs();
        match self.kind {
            ColorFxKind::Sweep if a > 0.0 => {
                // Band-pass rising 250 Hz → 8 kHz.
                let fc = 250.0 * 32f32.powf(mag);
                self.svf.set(fc, 1.2 + 3.0 * mag, self.sr);
            }
            ColorFxKind::Sweep => self.svf.set(120.0 + 400.0 * mag, 0.7, self.sr),
            ColorFxKind::DubEcho => {
                // Feedback filter: dark on the left, thin on the right.
                let fc = if a < 0.0 { 3000.0 * (0.15f32).powf(mag) } else { 150.0 * 8f32.powf(mag) };
                self.svf.set(fc, 0.9, self.sr);
            }
            _ => {
                let q = 0.707 + 0.9 * mag;
                self.svf.set(Self::cutoff(a), q, self.sr);
                self.noise_svf.set(Self::cutoff(a), q, self.sr);
            }
        }
    }

    /// Processes interleaved stereo in place, synced to `clock` (the
    /// channel's deck or the master clock).
    pub fn process(&mut self, buf: &mut [f32], clock: &BeatClock) {
        if self.bypassed() {
            if self.kind == ColorFxKind::Pitch {
                // Keep the grain history continuous for a click-free start.
                for f in buf.as_chunks::<2>().0 {
                    self.shifter.push(*f);
                }
            }
            return;
        }
        let beat_frames = clock.beat_frames();
        let frames = buf.len() / 2;
        let release = (-1.0 / (self.sr * 1.5)).exp();
        let mut wet_energy = 0.0f32;
        for (i, f) in buf.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let a = self.amount.next();
            if self.counter == 0 {
                self.update_filters(a);
            }
            self.counter = (self.counter + 1) % UPDATE_EVERY;
            let mag = a.abs();
            // Blend in over the first 5 % so the centre is seamless.
            let w = (mag / 0.05).min(1.0);
            let lp = a < 0.0;
            let x = [f[0], f[1]];
            let y = match self.kind {
                ColorFxKind::Filter => {
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, x[ch]);
                        let filtered = if lp { o.low } else { o.high };
                        y[ch] = x[ch] + (filtered - x[ch]) * w;
                    }
                    y
                }
                ColorFxKind::Noise => {
                    let n = self.noise.next() * mag * 0.25;
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.noise_svf.tick(ch, n);
                        let nf = if lp { o.low } else { o.high };
                        y[ch] = x[ch] + nf;
                    }
                    y
                }
                ColorFxKind::Space => {
                    self.reverb.room = 0.8 + 0.17 * mag;
                    let wet = self.reverb.process([x[0] * mag * 2.0, x[1] * mag * 2.0]);
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, wet[ch]);
                        let wf = if a < 0.0 { o.low } else { o.high };
                        y[ch] = x[ch] + wf;
                        wet_energy += wf * wf;
                    }
                    y
                }
                ColorFxKind::Crush => {
                    let factor = 1.0 + 24.0 * mag;
                    self.crush_phase += 1.0;
                    if self.crush_phase >= factor {
                        self.crush_phase -= factor;
                        self.crush_hold = x;
                    }
                    let levels = 2f32.powf(16.0 - 13.0 * mag);
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let q = (self.crush_hold[ch] * levels).round() / levels;
                        let o = self.svf.tick(ch, q);
                        // Mild tone: low-pass side darkens, high-pass side thins.
                        let t = if lp { o.low } else { q };
                        y[ch] = x[ch] + (t - x[ch]) * (mag / 0.1).min(1.0);
                    }
                    y
                }
                ColorFxKind::Echo => {
                    let d = (beat_frames * 0.75).clamp(1.0, self.delay.capacity() as f32);
                    let wet = self.delay.read(d);
                    self.fb_hold = (self.fb_hold * release).max(mag);
                    let fb = 0.3 + 0.5 * self.fb_hold;
                    let mut fw = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, wet[ch]);
                        fw[ch] = if a < 0.0 { o.low } else { o.high };
                        wet_energy += fw[ch] * fw[ch];
                    }
                    self.delay
                        .push([x[0] * mag + fw[0] * fb, x[1] * mag + fw[1] * fb]);
                    [x[0] + fw[0], x[1] + fw[1]]
                }
                ColorFxKind::DubEcho => {
                    let d = (beat_frames * 0.5).clamp(1.0, self.delay.capacity() as f32);
                    let wet = self.delay.read(d);
                    self.fb_hold = (self.fb_hold * release).max(mag);
                    let fb = 0.6 + 0.32 * self.fb_hold;
                    let mut fw = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, wet[ch]);
                        let filtered = if a < 0.0 { o.low } else { o.high };
                        // Tape-like saturation keeps the feedback bounded.
                        fw[ch] = filtered.tanh();
                        wet_energy += fw[ch] * fw[ch];
                    }
                    self.delay.push([
                        x[0] * mag + fw[0] * fb,
                        x[1] * mag + fw[1] * fb,
                    ]);
                    [x[0] + fw[0], x[1] + fw[1]]
                }
                ColorFxKind::Pitch => {
                    let y = self.shifter.process(x, 2f32.powf(a));
                    [x[0] + (y[0] - x[0]) * w, x[1] + (y[1] - x[1]) * w]
                }
                ColorFxKind::Sweep => {
                    if a < 0.0 {
                        // 1/16 gate locked to the beat, depth = knob.
                        let beat = clock.beat + i as f64 * clock.beats_per_frame;
                        let ph = (beat * 4.0).rem_euclid(1.0);
                        let target = if ph < 0.5 { 1.0 } else { 1.0 - mag };
                        self.gate += (target - self.gate) * (1.0 / (self.sr * 0.001)).min(1.0);
                        let mut y = [0.0; 2];
                        for ch in 0..2 {
                            // Slight high-pass tightens the gated sound.
                            let o = self.svf.tick(ch, x[ch]);
                            let thin = x[ch] + (o.high - x[ch]) * w;
                            y[ch] = thin * self.gate;
                        }
                        y
                    } else {
                        self.gate = 1.0;
                        let mut y = [0.0; 2];
                        for ch in 0..2 {
                            let o = self.svf.tick(ch, x[ch]);
                            y[ch] = x[ch] + (o.band * 1.5 - x[ch]) * w;
                        }
                        y
                    }
                }
            };
            f[0] = y[0];
            f[1] = y[1];
        }
        // Tails: keep processing until the wet path is silent.
        if Self::has_tail(self.kind) {
            let centred = self.amount.is_settled() && self.amount.current().abs() < DEAD_ZONE;
            if !centred {
                self.ringing = true;
                self.quiet_frames = 0.0;
            } else if wet_energy / frames.max(1) as f32 <= QUIET {
                self.quiet_frames += frames as f32;
                if self.quiet_frames > beat_frames + 4096.0 {
                    self.ringing = false;
                    self.quiet_frames = 0.0;
                }
            } else {
                self.quiet_frames = 0.0;
            }
        } else {
            self.ringing = false;
        }
    }
}
