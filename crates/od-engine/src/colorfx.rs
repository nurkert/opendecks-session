//! Colour FX: one knob per channel (−1..=1, 0 = bypass).

use crate::command::ColorFxKind;
use crate::dsp::Noise;
use crate::dsp::delay::DelayLine;
use crate::dsp::reverb::Reverb;
use crate::dsp::smooth::Smoothed;
use crate::dsp::svf::Svf;

const UPDATE_EVERY: usize = 16;
const DEAD_ZONE: f32 = 0.01;

pub struct ColorFx {
    sr: f32,
    kind: ColorFxKind,
    amount: Smoothed,
    svf: Svf,
    noise_svf: Svf,
    noise: Noise,
    reverb: Reverb,
    delay: DelayLine,
    crush_hold: [f32; 2],
    crush_phase: f32,
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
            crush_hold: [0.0; 2],
            crush_phase: 0.0,
            counter: 0,
        }
    }

    pub fn set_kind(&mut self, kind: ColorFxKind) {
        if kind != self.kind {
            self.kind = kind;
            self.svf.reset();
            self.noise_svf.reset();
            self.reverb.clear();
            self.delay.clear();
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

    fn bypassed(&self) -> bool {
        self.amount.is_settled() && self.amount.current().abs() < DEAD_ZONE
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
        let q = 0.707 + 0.9 * a.abs();
        self.svf.set(Self::cutoff(a), q, self.sr);
        self.noise_svf.set(Self::cutoff(a), q, self.sr);
    }

    /// Processes interleaved stereo in place. `beat_frames` is the length of
    /// one beat (for the echo).
    pub fn process(&mut self, buf: &mut [f32], beat_frames: f32) {
        if self.bypassed() {
            // Keep reverb/echo tails ringing out naturally is not needed for
            // a one-knob FX: once at zero it is fully dry.
            return;
        }
        for f in buf.as_chunks_mut::<2>().0 {
            let a = self.amount.next();
            if self.counter == 0 {
                self.update_filters(a);
            }
            self.counter = (self.counter + 1) % UPDATE_EVERY;
            let mag = a.abs();
            let lp = a < 0.0;
            let x = [f[0], f[1]];
            let y = match self.kind {
                ColorFxKind::Filter => {
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, x[ch]);
                        let filtered = if lp { o.low } else { o.high };
                        // Blend in the first percent so the centre is seamless.
                        let w = (mag / 0.05).min(1.0);
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
                    let wet = self.reverb.process(x);
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, wet[ch]);
                        let wf = if lp { o.low } else { o.high };
                        y[ch] = x[ch] + wf * mag * 2.0;
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
                    let fb = 0.3 + 0.5 * mag;
                    let mut fw = [0.0; 2];
                    for ch in 0..2 {
                        let o = self.svf.tick(ch, wet[ch]);
                        fw[ch] = if lp { o.low } else { o.high };
                    }
                    self.delay.push([x[0] + fw[0] * fb, x[1] + fw[1] * fb]);
                    [x[0] + fw[0] * mag, x[1] + fw[1] * mag]
                }
            };
            f[0] = y[0];
            f[1] = y[1];
        }
    }
}
