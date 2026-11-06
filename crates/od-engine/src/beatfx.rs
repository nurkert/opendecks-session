//! Beat FX units, synced to the master tempo and beat phase.

use crate::command::{BeatFxCommand, BeatFxKind, FxTarget};
use crate::dsp::delay::DelayLine;
use crate::dsp::reverb::Reverb;
use crate::dsp::smooth::Smoothed;
use crate::dsp::svf::Svf;
use crate::state::BeatFxState;

/// Master beat clock for one block.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct BeatClock {
    pub bpm: f64,
    /// Beat position at the first frame of the block.
    pub beat: f64,
    /// Beats advanced per output frame.
    pub beats_per_frame: f64,
}

impl BeatClock {
    pub fn beat_frames(&self) -> f32 {
        if self.beats_per_frame > 0.0 {
            (1.0 / self.beats_per_frame) as f32
        } else {
            24_000.0
        }
    }
}

const MAX_DELAY_S: f32 = 8.0;
const PHASER_STAGES: usize = 6;

pub struct BeatFxUnit {
    sr: f32,
    kind: BeatFxKind,
    target: FxTarget,
    on: bool,
    level: f32,
    beats: f32,
    /// 0 = dry … 1 = effect fully engaged (on/off ramp × level).
    mix: Smoothed,
    delay: DelayLine,
    reverb: Reverb,
    svf: Svf,
    fb_filter: [f32; 2],
    phaser_z: [[f32; PHASER_STAGES]; 2],
    phaser_fb: [f32; 2],
    roll_buf: Vec<[f32; 2]>,
    roll_len: usize,
    roll_pos: usize,
    roll_filled: usize,
    counter: usize,
}

impl BeatFxUnit {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let max = (sr * MAX_DELAY_S) as usize;
        Self {
            sr,
            kind: BeatFxKind::Echo,
            target: FxTarget::Master,
            on: false,
            level: 0.5,
            beats: 0.75,
            mix: Smoothed::new(0.0, (sr * 0.01) as u32),
            delay: DelayLine::new(max),
            reverb: Reverb::new(sample_rate),
            svf: Svf::new(1000.0, 0.7, sr),
            fb_filter: [0.0; 2],
            phaser_z: [[0.0; PHASER_STAGES]; 2],
            phaser_fb: [0.0; 2],
            roll_buf: vec![[0.0; 2]; max],
            roll_len: 0,
            roll_pos: 0,
            roll_filled: 0,
            counter: 0,
        }
    }

    pub fn target(&self) -> FxTarget {
        self.target
    }

    pub fn apply(&mut self, cmd: BeatFxCommand) {
        match cmd {
            BeatFxCommand::SetKind(k) => {
                if k != self.kind {
                    self.kind = k;
                    self.reset_state();
                }
            }
            BeatFxCommand::SetTarget(t) => self.target = t,
            BeatFxCommand::SetOn(on) => {
                if on && !self.on {
                    // Roll captures from the moment it is switched on.
                    self.roll_filled = 0;
                    self.roll_pos = 0;
                    self.roll_len = 0;
                }
                self.on = on;
            }
            BeatFxCommand::SetLevel(l) => self.level = l.clamp(0.0, 1.0),
            BeatFxCommand::SetBeats(b) => self.beats = b.clamp(1.0 / 32.0, 16.0),
        }
    }

    fn reset_state(&mut self) {
        self.delay.clear();
        self.reverb.clear();
        self.svf.reset();
        self.fb_filter = [0.0; 2];
        self.phaser_z = [[0.0; PHASER_STAGES]; 2];
        self.phaser_fb = [0.0; 2];
        self.roll_filled = 0;
        self.roll_len = 0;
    }

    pub fn state(&self) -> BeatFxState {
        BeatFxState {
            kind: self.kind,
            target: self.target,
            on: self.on,
            level: self.level,
            beats: self.beats,
        }
    }

    pub fn is_idle(&self) -> bool {
        !self.on && self.mix.is_settled() && self.mix.current() == 0.0
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32], clock: &BeatClock) {
        self.mix.set(if self.on { self.level } else { 0.0 });
        if self.is_idle() {
            return;
        }
        let beat_frames = clock.beat_frames();
        let period_frames = (self.beats * beat_frames).clamp(16.0, self.delay.capacity() as f32);
        for (i, f) in buf.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let m = self.mix.next();
            let beat = clock.beat + i as f64 * clock.beats_per_frame;
            // LFO phase locked to the master beat, cycle = `beats` beats.
            let phase = ((beat / self.beats as f64).rem_euclid(1.0)) as f32;
            let x = [f[0], f[1]];
            let y = match self.kind {
                BeatFxKind::Echo | BeatFxKind::Delay => {
                    let wet = self.delay.read(period_frames);
                    let fb = if self.kind == BeatFxKind::Echo {
                        0.55
                    } else {
                        0.0
                    };
                    let mut w = [0.0; 2];
                    for ch in 0..2 {
                        // Gentle damping in the feedback path.
                        self.fb_filter[ch] += (wet[ch] - self.fb_filter[ch]) * 0.45;
                        w[ch] = self.fb_filter[ch];
                    }
                    // Input gated by `m` so tails ring out after switching off.
                    self.delay
                        .push([x[0] * m + w[0] * fb, x[1] * m + w[1] * fb]);
                    [x[0] + w[0], x[1] + w[1]]
                }
                BeatFxKind::Reverb => {
                    self.reverb.room = 0.75 + 0.22 * (self.beats / 4.0).min(1.0);
                    let wet = self.reverb.process(x);
                    [x[0] + wet[0] * m * 2.5, x[1] + wet[1] * m * 2.5]
                }
                BeatFxKind::Flanger => {
                    let lfo = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
                    let d = self.sr * (0.0005 + 0.006 * lfo);
                    let wet = self.delay.read(d);
                    self.delay.push([x[0] + wet[0] * 0.6, x[1] + wet[1] * 0.6]);
                    [
                        x[0] * (1.0 - 0.5 * m) + wet[0] * 0.5 * m * 2.0,
                        x[1] * (1.0 - 0.5 * m) + wet[1] * 0.5 * m * 2.0,
                    ]
                }
                BeatFxKind::Phaser => {
                    let lfo = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
                    let fc = 200.0 * (16.0f32).powf(lfo);
                    let t = (std::f32::consts::PI * fc / self.sr).tan();
                    let a = (t - 1.0) / (t + 1.0);
                    let mut y = [0.0; 2];
                    for ch in 0..2 {
                        let mut v = x[ch] + self.phaser_fb[ch] * 0.5;
                        for s in 0..PHASER_STAGES {
                            let z = self.phaser_z[ch][s];
                            let out = a * v + z;
                            self.phaser_z[ch][s] = v - a * out;
                            v = out;
                        }
                        self.phaser_fb[ch] = v;
                        y[ch] = x[ch] * (1.0 - 0.5 * m) + v * 0.5 * m * 2.0;
                    }
                    y
                }
                BeatFxKind::Roll => {
                    let len = (period_frames as usize).clamp(16, self.roll_buf.len());
                    if self.roll_len == 0 {
                        self.roll_len = len;
                    }
                    if self.roll_filled < self.roll_len {
                        self.roll_buf[self.roll_filled] = x;
                        self.roll_filled += 1;
                        x
                    } else {
                        let r = self.roll_buf[self.roll_pos];
                        self.roll_pos = (self.roll_pos + 1) % self.roll_len;
                        r
                    }
                }
                BeatFxKind::FilterSweep => {
                    if self.counter == 0 {
                        let lfo = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
                        let fc = 150.0 * (100.0f32).powf(1.0 - lfo);
                        self.svf.set(fc, 1.6, self.sr);
                    }
                    self.counter = (self.counter + 1) % 16;
                    let a = self.svf.tick(0, x[0]).low;
                    let b = self.svf.tick(1, x[1]).low;
                    [a, b]
                }
                BeatFxKind::Trans => {
                    let gate = if phase < 0.5 { 1.0 } else { 0.0 };
                    // Short 1 ms edges via the filter state.
                    self.fb_filter[0] +=
                        (gate - self.fb_filter[0]) * (1.0 / (self.sr * 0.001)).min(1.0);
                    let g = self.fb_filter[0];
                    [x[0] * g, x[1] * g]
                }
            };
            match self.kind {
                // These already mix internally with `m`.
                BeatFxKind::Echo
                | BeatFxKind::Delay
                | BeatFxKind::Reverb
                | BeatFxKind::Flanger
                | BeatFxKind::Phaser => {
                    f[0] = y[0];
                    f[1] = y[1];
                }
                _ => {
                    f[0] = x[0] + (y[0] - x[0]) * m;
                    f[1] = x[1] + (y[1] - x[1]) * m;
                }
            }
        }
    }
}
