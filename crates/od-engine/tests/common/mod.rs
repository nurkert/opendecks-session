//! Shared helpers for offline-render tests.
#![allow(dead_code)]

use std::sync::Arc;

use od_core::{Beatgrid, TrackId};
use od_decode::DecodedAudio;
use od_engine::*;

pub const SR: u32 = 48_000;
pub const BLOCK: usize = 128;
/// Scale of the position-encoding ramp: sample value × RAMP_SCALE = frame index.
pub const RAMP_SCALE: f32 = 1_048_576.0;

pub fn track_from(id: i64, samples: Vec<f32>, bpm: f64, first_beat: f64) -> Arc<TrackData> {
    let frames = samples.len() / 2;
    let dur = frames as f64 / SR as f64;
    let grid = if bpm > 0.0 {
        Beatgrid::constant(bpm, first_beat, 1, dur)
    } else {
        Beatgrid::default()
    };
    TrackData::new(
        TrackId(id),
        DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples,
        },
        SR,
        grid,
        &[],
        0.0,
    )
}

/// Left channel = frame index / RAMP_SCALE (exactly representable), right = same.
pub fn ramp_track(id: i64, seconds: f64, bpm: f64) -> Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let s = (0..n).flat_map(|i| {
        let v = i as f32 / RAMP_SCALE;
        [v, v]
    });
    track_from(id, s.collect(), bpm, 0.0)
}

pub fn sine_track(id: i64, seconds: f64, freq: f64, amp: f32, bpm: f64) -> Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let s = (0..n).flat_map(|i| {
        let v = (std::f64::consts::TAU * freq * i as f64 / SR as f64).sin() as f32 * amp;
        [v, v]
    });
    track_from(id, s.collect(), bpm, 0.0)
}

pub struct Rig {
    pub engine: Engine,
    pub proc: EngineProcessor,
    pub master: Vec<f32>,
    pub phones: Vec<f32>,
    pub booth: Vec<f32>,
}

impl Rig {
    pub fn new(decks: usize) -> Self {
        Self::with(EngineConfig {
            sample_rate: SR,
            max_block: 512,
            decks,
            profile: Profile::Full,
        })
    }

    pub fn with(config: EngineConfig) -> Self {
        let (mut engine, proc) = Engine::new(config);
        // Neutral mixer for tests: limiter off so samples pass bit-exact.
        engine
            .send(Command::Mixer(MixerCommand::SetLimiter(false)))
            .unwrap();
        // Tests position explicitly: load at the track start.
        for d in 0..config.decks {
            engine.deck(d, DeckCommand::SetAutoCue(false)).unwrap();
        }
        Self {
            engine,
            proc,
            master: vec![0.0; 8192 * 2],
            phones: vec![0.0; 8192 * 2],
            booth: vec![0.0; 8192 * 2],
        }
    }

    pub fn send(&mut self, c: Command) {
        self.engine.send(c).unwrap();
    }

    pub fn deck(&mut self, d: usize, c: DeckCommand) {
        self.engine.deck(d, c).unwrap();
    }

    /// Renders one block and returns the master output of that block.
    pub fn block(&mut self, frames: usize) -> &[f32] {
        let mut out = Outputs {
            master: &mut self.master[..frames * 2],
            headphones: Some(&mut self.phones[..frames * 2]),
            booth: Some(&mut self.booth[..frames * 2]),
        };
        self.proc.process(frames, &mut out);
        &self.master[..frames * 2]
    }

    /// Renders `blocks` blocks of `BLOCK` frames, collecting master output.
    pub fn render(&mut self, blocks: usize) -> Vec<f32> {
        let mut all = Vec::with_capacity(blocks * BLOCK * 2);
        for _ in 0..blocks {
            let b = self.block(BLOCK).to_vec();
            all.extend_from_slice(&b);
        }
        all
    }

    pub fn state(&mut self) -> EngineState {
        *self.engine.state()
    }
}

/// Frequency estimate via zero crossings of the left channel (with linear
/// interpolation of the crossing instants).
pub fn estimate_freq(interleaved: &[f32]) -> f64 {
    let l: Vec<f32> = interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .map(|f| f[0])
        .collect();
    let mut first = None;
    let mut last = 0.0;
    let mut count = 0;
    for i in 1..l.len() {
        if l[i - 1] < 0.0 && l[i] >= 0.0 {
            let t = (i - 1) as f64 + (-l[i - 1] / (l[i] - l[i - 1])) as f64;
            if first.is_none() {
                first = Some(t);
            } else {
                count += 1;
            }
            last = t;
        }
    }
    let first = first.unwrap_or(0.0);
    if count == 0 {
        0.0
    } else {
        count as f64 * SR as f64 / (last - first)
    }
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn tone(n: usize, freq: f64, amp: f32) -> Vec<f32> {
    (0..n)
        .flat_map(|i| {
            let v = (std::f64::consts::TAU * freq * i as f64 / SR as f64).sin() as f32 * amp;
            [v, v]
        })
        .collect()
}

/// A track whose mix is `vocals + drums + other` (three sines), with stems
/// for vocals (`f_vocals`) and drums (`f_drums`) attached and silent bass;
/// `f_other` only lives in the implicit "inst" remainder.
pub fn stem_track(
    id: i64,
    seconds: f64,
    f_vocals: f64,
    f_drums: f64,
    f_other: f64,
) -> Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let v = tone(n, f_vocals, 0.25);
    let d = tone(n, f_drums, 0.25);
    let o = tone(n, f_other, 0.25);
    let mix: Vec<f32> = (0..n * 2).map(|i| v[i] + d[i] + o[i]).collect();
    let stems = StemInput {
        sample_rate: SR,
        vocals: v,
        drums: d,
        bass: vec![0.0; n * 2],
    };
    TrackData::new_with_stems(
        TrackId(id),
        DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples: mix,
        },
        SR,
        Beatgrid::constant(120.0, 0.0, 1, seconds),
        &[],
        0.0,
        Some(&stems),
    )
}

/// Amplitude of the `freq` component of the left channel (Goertzel,
/// Hann-windowed).
pub fn tone_level(interleaved: &[f32], freq: f64) -> f64 {
    let l: Vec<f64> = interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .map(|f| f[0] as f64)
        .collect();
    let n = l.len();
    let w = std::f64::consts::TAU * freq / SR as f64;
    let (mut re, mut im, mut wsum) = (0.0, 0.0, 0.0);
    for (i, x) in l.iter().enumerate() {
        let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
        re += x * win * (w * i as f64).cos();
        im += x * win * (w * i as f64).sin();
        wsum += win;
    }
    2.0 * (re * re + im * im).sqrt() / wsum
}

/// Frequency of the strongest spectral component (left channel) within
/// `lo..=hi` Hz; robust against the phase jumps of granular pitch shifting,
/// unlike zero-crossing counting.
pub fn peak_freq(interleaved: &[f32], lo: f64, hi: f64) -> f64 {
    let scan = |from: f64, to: f64, step: f64| {
        let mut best = (from, -1.0);
        let mut f = from;
        while f <= to {
            let p = tone_level(interleaved, f);
            if p > best.1 {
                best = (f, p);
            }
            f += step;
        }
        best.0
    };
    let coarse = scan(lo, hi, 4.0);
    scan(coarse - 4.0, coarse + 4.0, 0.5)
}
