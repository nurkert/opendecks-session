//! Beat FX units (two on the mixer, one Pad FX unit per channel), synced to
//! a beat clock, with release FX.
//!
//! Each unit processes its target's interleaved stereo buffer in place:
//! Beat FX kind → tape (history, vinyl brake, backspin, echo out) → release
//! crossfade. Buffers are allocated in [`FxUnit::new`] only.

use std::f32::consts::{PI, TAU};

use crate::command::{BeatFxCommand, BeatFxKind, FxTarget, PadFxCommand, PadFxEffect, ReleaseFxKind};
use crate::dsp::delay::DelayLine;
use crate::dsp::pitchshift::PitchShifter;
use crate::dsp::reverb::Reverb;
use crate::dsp::smooth::Smoothed;
use crate::dsp::svf::Svf;
use crate::state::{BeatFxState, PadFxState};

/// Beat clock for one block.
#[derive(Clone, Copy, Debug)]
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

const PHASER_STAGES: usize = 6;
const KINDS: usize = BeatFxKind::ALL.len();
/// Wet energy per frame below which a tail counts as silent.
const QUIET: f32 = 1e-10;
const FLANGER_MAX_S: f32 = 0.02;
const SHIFT_WINDOW_S: f32 = 0.045;

#[derive(Clone, Copy, Debug, PartialEq)]
enum TapeMode {
    Brake,
    Backspin,
    EchoOut,
}

/// History ring of the unit's output; plays it back slowed down, backwards
/// or as an echo for vinyl brake, backspin and echo out.
struct Tape {
    line: DelayLine,
    mode: Option<TapeMode>,
    /// Frames the read head lags behind the newest frame.
    lag: f64,
    elapsed: f64,
    duration: f64,
    echo_period: f32,
}

impl Tape {
    fn new(frames: usize) -> Self {
        Self {
            line: DelayLine::new(frames),
            mode: None,
            lag: 0.0,
            elapsed: 0.0,
            duration: 1.0,
            echo_period: 1.0,
        }
    }

    fn start(&mut self, mode: TapeMode, beats: f32, beat_frames: f32) {
        let cap = self.line.capacity() as f64;
        let len = (beats.max(1.0 / 32.0) * beat_frames) as f64;
        self.duration = match mode {
            // The lag grows by at most the elapsed time / 2 (linear brake).
            TapeMode::Brake => len.clamp(64.0, cap * 1.8),
            // Backwards at up to 3× plus the writer moving on.
            TapeMode::Backspin => len.clamp(64.0, cap / 3.0),
            TapeMode::EchoOut => len,
        };
        self.echo_period = (beats.clamp(1.0 / 32.0, 1.0) * beat_frames).clamp(16.0, cap as f32);
        self.mode = Some(mode);
        self.lag = 0.0;
        self.elapsed = 0.0;
    }

    /// Records `x` and returns the tape output (or `x` without a mode).
    #[inline]
    fn tick(&mut self, x: [f32; 2]) -> [f32; 2] {
        let Some(mode) = self.mode else {
            self.line.push(x);
            return x;
        };
        let t = (self.elapsed / self.duration).min(1.0);
        self.elapsed += 1.0;
        match mode {
            TapeMode::Brake | TapeMode::Backspin => {
                self.line.push(x);
                let speed = if mode == TapeMode::Brake {
                    1.0 - t
                } else {
                    // 1 → −3 within 30 ms, then decelerating to a stop.
                    let swing = (self.elapsed / 1440.0).min(1.0);
                    let back = -3.0 * (1.0 - t) * (1.0 - t);
                    1.0 + (back - 1.0) * swing
                };
                let cap = (self.line.capacity() - 2) as f64;
                self.lag = (self.lag + 1.0 - speed).clamp(0.0, cap);
                // Fade over the last 15 % so the stop is click-free.
                let amp = ((1.0 - t) / 0.15).min(1.0) as f32;
                let y = self.line.read(1.0 + self.lag as f32);
                [y[0] * amp, y[1] * amp]
            }
            TapeMode::EchoOut => {
                let e = self.line.read(self.echo_period);
                self.line.push([e[0] * 0.6, e[1] * 0.6]);
                e
            }
        }
    }
}

pub struct FxUnit {
    sr: f32,
    kind: BeatFxKind,
    target: FxTarget,
    on: bool,
    level: f32,
    beats: f32,
    params: [f32; KINDS],
    param_s: f32,
    tap_bpm: Option<f32>,
    tap_beat: f64,
    last_bpm: f32,
    /// 0 = dry … 1 = effect fully engaged (on/off ramp × level).
    mix: Smoothed,
    delay: DelayLine,
    /// Glided delay time in frames (0 = not initialised).
    delay_len: f32,
    short: DelayLine,
    reverb: Reverb,
    svf: Svf,
    fb_filter: [f32; 2],
    gate: f32,
    phaser_z: [[f32; PHASER_STAGES]; 2],
    phaser_fb: [f32; 2],
    roll_buf: Vec<[f32; 2]>,
    roll_len: usize,
    roll_pos: usize,
    roll_filled: usize,
    roll_resize: bool,
    shifter: PitchShifter,
    tape: Tape,
    /// Pending tape start (computed with the block's beat length).
    tape_start: Option<(TapeMode, f32)>,
    release: Option<ReleaseFxKind>,
    rel_mix: Smoothed,
    /// A tail kind is still ringing after switching off.
    ringing: bool,
    quiet_frames: f32,
    counter: usize,
    pad: PadFxState,
}

impl FxUnit {
    /// `max_seconds`: longest delay / roll / tape history.
    pub fn new(sample_rate: u32, max_seconds: f32, target: FxTarget) -> Self {
        let sr = sample_rate as f32;
        let max = (sr * max_seconds) as usize;
        Self {
            sr,
            kind: BeatFxKind::Echo,
            target,
            on: false,
            level: 0.5,
            beats: 0.75,
            params: std::array::from_fn(|i| BeatFxKind::ALL[i].default_param()),
            param_s: 0.0,
            tap_bpm: None,
            tap_beat: 0.0,
            last_bpm: 120.0,
            mix: Smoothed::new(0.0, (sr * 0.01) as u32),
            delay: DelayLine::new(max),
            delay_len: 0.0,
            short: DelayLine::new((sr * FLANGER_MAX_S) as usize + 8),
            reverb: Reverb::new(sample_rate),
            svf: Svf::new(1000.0, 0.7, sr),
            fb_filter: [0.0; 2],
            gate: 1.0,
            phaser_z: [[0.0; PHASER_STAGES]; 2],
            phaser_fb: [0.0; 2],
            roll_buf: vec![[0.0; 2]; max],
            roll_len: 0,
            roll_pos: 0,
            roll_filled: 0,
            roll_resize: false,
            shifter: PitchShifter::new((sr * SHIFT_WINDOW_S) as usize, sr),
            tape: Tape::new(max),
            tape_start: None,
            release: None,
            rel_mix: Smoothed::new(0.0, (sr * 0.008) as u32),
            ringing: false,
            quiet_frames: 0.0,
            counter: 0,
            pad: PadFxState::default(),
        }
    }

    pub fn target(&self) -> FxTarget {
        self.target
    }

    fn param(&self) -> f32 {
        self.params[self.kind.index()]
    }

    fn set_kind(&mut self, k: BeatFxKind) {
        if k == self.kind {
            return;
        }
        // The long delay only ever holds gated input: clear it only while a
        // tail is still in it (rare; keeps kind changes cheap).
        if self.ringing {
            self.delay.clear();
            self.reverb.clear();
            self.ringing = false;
        }
        self.kind = k;
        self.delay_len = 0.0;
        self.svf.reset();
        self.fb_filter = [0.0; 2];
        self.gate = 1.0;
        self.phaser_z = [[0.0; PHASER_STAGES]; 2];
        self.phaser_fb = [0.0; 2];
        self.param_s = self.param();
        self.roll_filled = 0;
        self.roll_len = 0;
        self.roll_pos = 0;
        if self.on {
            self.engage();
        }
    }

    /// Effect switched on (or kind changed while on).
    fn engage(&mut self) {
        self.roll_filled = 0;
        self.roll_pos = 0;
        self.roll_len = 0;
        if self.kind.has_tail() {
            self.ringing = true;
        }
        if self.kind == BeatFxKind::VinylBrake && self.release.is_none() {
            self.tape_start = Some((TapeMode::Brake, self.beats));
        }
    }

    fn set_on(&mut self, on: bool) {
        if on && !self.on {
            self.on = true;
            self.engage();
        }
        self.on = on;
    }

    fn set_release(&mut self, kind: ReleaseFxKind, beats: f32, on: bool) {
        if on {
            self.on = false;
            self.release = Some(kind);
            let mode = match kind {
                ReleaseFxKind::VinylBrake => TapeMode::Brake,
                ReleaseFxKind::Backspin => TapeMode::Backspin,
                ReleaseFxKind::EchoOut => TapeMode::EchoOut,
            };
            self.tape_start = Some((mode, beats));
            self.rel_mix.set(1.0);
        } else if self.release.is_some() {
            self.rel_mix.set(0.0);
        }
    }

    pub fn apply(&mut self, cmd: BeatFxCommand) {
        match cmd {
            BeatFxCommand::SetKind(k) => self.set_kind(k),
            BeatFxCommand::SetTarget(t) => self.target = t,
            BeatFxCommand::SetOn(on) => self.set_on(on),
            BeatFxCommand::SetLevel(l) => self.level = l.clamp(0.0, 1.0),
            BeatFxCommand::SetBeats(b) => {
                let b = b.clamp(1.0 / 32.0, 16.0);
                if b != self.beats {
                    self.beats = b;
                    self.roll_resize = true;
                }
            }
            BeatFxCommand::SetParam(p) => self.params[self.kind.index()] = p.clamp(-1.0, 1.0),
            BeatFxCommand::SetTapBpm(b) => self.tap_bpm = b.map(|b| b.clamp(40.0, 300.0)),
            BeatFxCommand::Release { kind, beats, on } => self.set_release(kind, beats, on),
        }
    }

    /// Pad FX (units owned by a channel).
    pub fn apply_pad(&mut self, cmd: PadFxCommand) {
        match cmd {
            PadFxCommand::Press(p) => {
                self.pad = PadFxState {
                    held: true,
                    preset: Some(p),
                };
                match p.effect {
                    PadFxEffect::Beat(kind) => {
                        if self.release.is_some() {
                            self.rel_mix.set(0.0);
                        }
                        self.set_kind(kind);
                        self.params[kind.index()] = p.param.clamp(-1.0, 1.0);
                        self.param_s = self.param();
                        self.level = p.level.clamp(0.0, 1.0);
                        self.beats = p.beats.clamp(1.0 / 32.0, 16.0);
                        self.set_on(false);
                        self.set_on(true);
                    }
                    PadFxEffect::Release(kind) => self.set_release(kind, p.beats, true),
                }
            }
            PadFxCommand::Release => {
                self.pad.held = false;
                if self.release.is_some() {
                    self.set_release(ReleaseFxKind::EchoOut, 1.0, false);
                }
                self.set_on(false);
            }
        }
    }

    pub fn pad_state(&self) -> PadFxState {
        self.pad
    }

    pub fn state(&self) -> BeatFxState {
        BeatFxState {
            kind: self.kind,
            target: self.target,
            on: self.on,
            level: self.level,
            beats: self.beats,
            param: self.param(),
            tap_bpm: self.tap_bpm,
            bpm: self.last_bpm,
            release: self.release,
            ringing: !self.on && (self.ringing || self.release.is_some()),
        }
    }

    pub fn is_idle(&self) -> bool {
        !self.on
            && self.mix.is_settled()
            && self.mix.current() == 0.0
            && !self.ringing
            && self.release.is_none()
            && self.tape_start.is_none()
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32], clock_in: &BeatClock) {
        let clock = match self.tap_bpm {
            Some(bpm) => {
                let bpf = bpm as f64 / 60.0 / self.sr as f64;
                BeatClock {
                    bpm: bpm as f64,
                    beat: self.tap_beat,
                    beats_per_frame: bpf,
                }
            }
            None => *clock_in,
        };
        let frames = buf.len() / 2;
        self.tap_beat = clock.beat + clock.beats_per_frame * frames as f64;
        self.last_bpm = clock.bpm as f32;
        self.mix.set(if self.on { self.level } else { 0.0 });
        if self.is_idle() {
            // Keep the history for backspin, and the short buffers current
            // so switching on later starts from continuous audio (no click).
            for f in buf.as_chunks_mut::<2>().0 {
                let x = [f[0], f[1]];
                self.tape.line.push(x);
                self.short.push(x);
                self.shifter.push(x);
            }
            return;
        }
        let beat_frames = clock.beat_frames();
        if let Some((mode, beats)) = self.tape_start.take() {
            self.tape.start(mode, beats, beat_frames);
        }
        let cap = self.delay.capacity() as f32;
        let target_len = (self.beats * beat_frames).clamp(16.0, cap);
        let glide_s = if self.kind == BeatFxKind::Spiral { 0.35 } else { 0.03 };
        let glide = 1.0 - (-1.0 / (self.sr * glide_s)).exp();
        if self.delay_len <= 0.0 {
            self.delay_len = target_len;
        }
        if self.roll_resize {
            self.roll_resize = false;
            let len = (target_len as usize).clamp(16, self.roll_buf.len());
            match self.kind {
                BeatFxKind::SlipRoll => {
                    self.roll_filled = 0;
                    self.roll_pos = 0;
                    self.roll_len = len;
                }
                BeatFxKind::Roll if self.roll_len > 0 => {
                    self.roll_len = len;
                    self.roll_pos %= len;
                }
                _ => {}
            }
        }
        let param_a = 1.0 - (-1.0 / (self.sr * 0.02)).exp();
        let param_t = self.param();
        let mut wet_energy = 0.0f32;
        for (i, f) in buf.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let m = self.mix.next();
            self.param_s += (param_t - self.param_s) * param_a;
            self.delay_len += (target_len - self.delay_len) * glide;
            let beat = clock.beat + i as f64 * clock.beats_per_frame;
            // LFO phase locked to the beat, cycle = `beats` beats.
            let phase = ((beat / self.beats as f64).rem_euclid(1.0)) as f32;
            let x = [f[0], f[1]];
            let (y, wet) = self.kind_frame(x, m, phase);
            if self.kind != BeatFxKind::Flanger {
                self.short.push(x);
            }
            if !matches!(self.kind, BeatFxKind::Pitch | BeatFxKind::Helix) {
                self.shifter.push(x);
            }
            wet_energy += wet[0] * wet[0] + wet[1] * wet[1];
            let t = self.tape.tick(y);
            let mut out = y;
            if self.kind == BeatFxKind::VinylBrake {
                out = [y[0] + (t[0] - y[0]) * m, y[1] + (t[1] - y[1]) * m];
            }
            if self.release.is_some() {
                let r = self.rel_mix.next();
                out = [out[0] + (t[0] - out[0]) * r, out[1] + (t[1] - out[1]) * r];
            }
            f[0] = out[0];
            f[1] = out[1];
        }
        // Release finished fading back to dry.
        if self.release.is_some() && self.rel_mix.is_settled() && self.rel_mix.current() == 0.0 {
            self.release = None;
            self.tape.mode = None;
        }
        if !self.on && self.mix.current() == 0.0 && self.kind == BeatFxKind::VinylBrake && self.release.is_none() {
            self.tape.mode = None;
        }
        // Tail detection: silent for longer than the delay time.
        if self.ringing && !self.on {
            if wet_energy / frames.max(1) as f32 <= QUIET {
                self.quiet_frames += frames as f32;
            } else {
                self.quiet_frames = 0.0;
            }
            if self.quiet_frames > self.delay_len + 2048.0 {
                self.ringing = false;
                self.quiet_frames = 0.0;
            }
        } else {
            self.quiet_frames = 0.0;
        }
    }

    /// One frame of the Beat FX kind: `(output, wet part for tail tracking)`.
    #[inline]
    fn kind_frame(&mut self, x: [f32; 2], m: f32, phase: f32) -> ([f32; 2], [f32; 2]) {
        let d = self.delay_len;
        match self.kind {
            BeatFxKind::Echo | BeatFxKind::Delay => {
                let wet = self.delay.read(d);
                let fb = if self.kind == BeatFxKind::Echo { 0.55 } else { 0.0 };
                let mut w = [0.0; 2];
                for ch in 0..2 {
                    // Gentle damping in the feedback path.
                    self.fb_filter[ch] += (wet[ch] - self.fb_filter[ch]) * 0.45;
                    w[ch] = self.fb_filter[ch];
                }
                // Input gated by `m` so tails ring out after switching off.
                self.delay.push([x[0] * m + w[0] * fb, x[1] * m + w[1] * fb]);
                ([x[0] + w[0], x[1] + w[1]], w)
            }
            BeatFxKind::PingPong => {
                let wet = self.delay.read(d);
                let mono = (x[0] + x[1]) * 0.5 * m;
                // Left repeats after d, then crosses to the right after 2d …
                self.delay.push([mono + wet[1] * 0.6, wet[0] * 0.75]);
                ([x[0] + wet[0], x[1] + wet[1]], wet)
            }
            BeatFxKind::Reverb => {
                self.reverb.room = 0.75 + 0.22 * (self.beats / 4.0).min(1.0);
                self.reverb.damp = (0.3 - 0.28 * self.param_s).clamp(0.02, 0.9);
                let wet = self.reverb.process([x[0] * m, x[1] * m]);
                let w = [wet[0] * 2.5, wet[1] * 2.5];
                ([x[0] + w[0], x[1] + w[1]], w)
            }
            BeatFxKind::Spiral => {
                let wet = self.delay.read(d);
                for ch in 0..2 {
                    self.fb_filter[ch] += (wet[ch] - self.fb_filter[ch]) * 0.3;
                }
                let [a, b] = self.fb_filter;
                let fb = 0.72;
                self.delay.push([
                    x[0] * m + (0.8 * a + 0.2 * b) * fb,
                    x[1] * m + (0.8 * b + 0.2 * a) * fb,
                ]);
                ([x[0] + a, x[1] + b], [a, b])
            }
            BeatFxKind::Helix => {
                let wet = self.delay.read(d);
                let ratio = 2f32.powf(self.param_s);
                let shifted = self.shifter.process(wet, ratio);
                self.delay.push([x[0] * m + shifted[0] * 0.7, x[1] * m + shifted[1] * 0.7]);
                ([x[0] + wet[0], x[1] + wet[1]], wet)
            }
            BeatFxKind::Flanger => {
                let lfo = 0.5 - 0.5 * (phase * TAU).cos();
                let dd = self.sr * (0.0005 + 0.006 * lfo);
                let wet = self.short.read(dd);
                self.short.push([x[0] + wet[0] * 0.6, x[1] + wet[1] * 0.6]);
                let y = [
                    x[0] * (1.0 - 0.5 * m) + wet[0] * m,
                    x[1] * (1.0 - 0.5 * m) + wet[1] * m,
                ];
                (y, [0.0; 2])
            }
            BeatFxKind::Phaser => {
                let lfo = 0.5 - 0.5 * (phase * TAU).cos();
                let fc = 200.0 * (16.0f32).powf(lfo);
                let t = (PI * fc / self.sr).tan();
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
                    y[ch] = x[ch] * (1.0 - 0.5 * m) + v * m;
                }
                (y, [0.0; 2])
            }
            BeatFxKind::Roll | BeatFxKind::SlipRoll => {
                if self.roll_len == 0 {
                    self.roll_len = (d as usize).clamp(16, self.roll_buf.len());
                }
                let y = if self.roll_filled < self.roll_len {
                    self.roll_buf[self.roll_filled] = x;
                    self.roll_filled += 1;
                    x
                } else {
                    let r = self.roll_buf[self.roll_pos];
                    self.roll_pos += 1;
                    if self.roll_pos >= self.roll_len {
                        self.roll_pos = 0;
                    }
                    r
                };
                (Self::blend(x, y, m), [0.0; 2])
            }
            BeatFxKind::Filter => {
                if self.counter == 0 {
                    let lfo = 0.5 - 0.5 * (phase * TAU).cos();
                    let fc = if self.param_s > 0.0 {
                        40.0 * (150.0f32).powf(lfo)
                    } else {
                        150.0 * (100.0f32).powf(1.0 - lfo)
                    };
                    self.svf.set(fc, 1.6, self.sr);
                }
                self.counter = (self.counter + 1) % 16;
                let hp = self.param_s > 0.0;
                let a = self.svf.tick(0, x[0]);
                let b = self.svf.tick(1, x[1]);
                let y = if hp { [a.high, b.high] } else { [a.low, b.low] };
                (Self::blend(x, y, m), [0.0; 2])
            }
            BeatFxKind::Trans => {
                let target = if phase < 0.5 { 1.0 } else { 0.0 };
                // 1 ms edges.
                self.gate += (target - self.gate) * (1.0 / (self.sr * 0.001)).min(1.0);
                let g = self.gate;
                (Self::blend(x, [x[0] * g, x[1] * g], m), [0.0; 2])
            }
            BeatFxKind::Pitch => {
                let y = self.shifter.process(x, 2f32.powf(self.param_s));
                (Self::blend(x, y, m), [0.0; 2])
            }
            // Mixed with the tape output in `process`.
            BeatFxKind::VinylBrake => (x, [0.0; 2]),
        }
    }

    #[inline]
    fn blend(x: [f32; 2], y: [f32; 2], m: f32) -> [f32; 2] {
        [x[0] + (y[0] - x[0]) * m, x[1] + (y[1] - x[1]) * m]
    }
}
