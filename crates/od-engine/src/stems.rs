//! Stem (part) playback: per-deck levels for vocals, drums, bass and the
//! instrumental remainder of a track.
//!
//! Separation itself happens offline (crate `od-stems`); the engine only
//! receives the separated vocals, drums and bass next to the full mix. The
//! fourth part, "inst" (everything else), is implicit:
//! `inst = mix − vocals − drums − bass`. Mixing a frame with part levels
//! `l_v, l_d, l_b, l_i` is
//!
//! ```text
//! out = mix − Σ_{s∈v,d,b} (1 − l_s)·s − (1 − l_i)·(mix − v − d − b)
//!     = l_i·mix + (l_v − l_i)·v + (l_d − l_i)·d + (l_b − l_i)·b
//! ```
//!
//! which is linear in the input, so the per-deck resampler and the keylock
//! feed simply interpolate / feed the combined signal. With all levels at 1
//! the result is the mix itself and the stem path is skipped entirely.
//!
//! Memory: stems are kept as `i16` with a per-stem scale (peak-normalised),
//! i.e. 12 bytes per frame for the three stems next to the 8 bytes of the
//! `f32` mix. `f32` stems would cost 24 bytes per frame (a 6-minute track at
//! 48 kHz: 415 MB instead of 207 MB per deck), while 16-bit quantisation
//! noise (≈ −98 dBFS per stem, ≈ −93 dBFS summed over three) is far below
//! the separation artefacts themselves.

/// One part of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stem {
    Vocals,
    Drums,
    Bass,
    /// Instrumental remainder: mix − vocals − drums − bass.
    Inst,
}

impl Stem {
    pub const ALL: [Stem; 4] = [Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Inst];

    /// Index into [`crate::DeckState::stem_levels`].
    pub const fn index(self) -> usize {
        match self {
            Stem::Vocals => 0,
            Stem::Drums => 1,
            Stem::Bass => 2,
            Stem::Inst => 3,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Stem::Vocals => "vocals",
            Stem::Drums => "drums",
            Stem::Bass => "bass",
            Stem::Inst => "inst",
        }
    }
}

/// Separated stems as produced by the separator: interleaved stereo `f32`
/// at `sample_rate` (normally the track's own rate, aligned with the mix).
#[derive(Clone, Debug, Default)]
pub struct StemInput {
    pub sample_rate: u32,
    pub vocals: Vec<f32>,
    pub drums: Vec<f32>,
    pub bass: Vec<f32>,
}

/// Stems at the engine rate, ready for playback.
#[derive(Debug)]
pub struct StemData {
    frames: usize,
    /// Dequantisation factor per stem (vocals, drums, bass).
    scale: [f32; 3],
    /// Per frame: `[vL, vR, dL, dR, bL, bR]`.
    data: Box<[i16]>,
}

impl StemData {
    /// Resamples `input` to `engine_rate` (slow; off the real-time thread)
    /// and quantises it to 16 bit.
    pub fn new(input: &StemInput, engine_rate: u32) -> Self {
        fn rs(v: &[f32], from: u32, to: u32) -> std::borrow::Cow<'_, [f32]> {
            if from != to && from > 0 {
                crate::dsp::resample::resample_stereo(v, from, to).into()
            } else {
                v.into()
            }
        }
        // The three parts resample in parallel (this runs while a deck waits).
        let (from, to) = (input.sample_rate, engine_rate);
        let parts = std::thread::scope(|s| {
            let d = s.spawn(move || rs(&input.drums, from, to));
            let b = s.spawn(move || rs(&input.bass, from, to));
            let v = rs(&input.vocals, from, to);
            [
                v,
                d.join().expect("resampler thread"),
                b.join().expect("resampler thread"),
            ]
        });
        let frames = parts.iter().map(|p| p.len() / 2).max().unwrap_or(0);
        let mut scale = [0f32; 3];
        let mut data = vec![0i16; frames * 6].into_boxed_slice();
        for (s, p) in parts.iter().enumerate() {
            let peak = p.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-9);
            scale[s] = peak / 32767.0;
            let inv = 32767.0 / peak;
            for (f, pair) in p.chunks_exact(2).enumerate() {
                data[f * 6 + s * 2] = (pair[0] * inv).round().clamp(-32767.0, 32767.0) as i16;
                data[f * 6 + s * 2 + 1] = (pair[1] * inv).round().clamp(-32767.0, 32767.0) as i16;
            }
        }
        Self {
            frames,
            scale,
            data,
        }
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Heap memory held by the stems.
    pub fn memory_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<i16>()
    }

    /// One stem as interleaved stereo `f32` (for tests and inspection).
    pub fn stem(&self, stem: Stem) -> Vec<f32> {
        let s = match stem {
            Stem::Vocals => 0,
            Stem::Drums => 1,
            Stem::Bass => 2,
            Stem::Inst => return Vec::new(),
        };
        let k = self.scale[s];
        self.data
            .chunks_exact(6)
            .flat_map(|f| [f[s * 2] as f32 * k, f[s * 2 + 1] as f32 * k])
            .collect()
    }

    /// Where the vocals sing (SPEC §10.4 vocal position): spans in seconds
    /// where the vocal stem's RMS (¼-second windows) is above −16 dB of its
    /// loudest window and above −46 dBFS; gaps under 1.5 s are bridged and
    /// spans under 1 s dropped.
    pub fn vocal_regions(&self, sample_rate: u32) -> Vec<(f64, f64)> {
        let win = (sample_rate as usize / 4).max(1);
        let k = self.scale[0];
        let rms: Vec<f32> = (0..self.frames.div_ceil(win))
            .map(|w| {
                let (a, b) = (w * win, ((w + 1) * win).min(self.frames));
                let sum: f32 = (a..b)
                    .map(|i| {
                        let (l, r) = (self.data[i * 6] as f32 * k, self.data[i * 6 + 1] as f32 * k);
                        (l * l + r * r) * 0.5
                    })
                    .sum();
                (sum / (b - a).max(1) as f32).sqrt()
            })
            .collect();
        let loudest = rms.iter().copied().fold(0.0f32, f32::max);
        let gate = (loudest * 0.158).max(0.005);
        let secs = |w: usize| (w * win) as f64 / sample_rate as f64;
        let mut spans: Vec<(f64, f64)> = Vec::new();
        let mut start: Option<usize> = None;
        for (w, v) in rms.iter().enumerate() {
            match (v >= &gate, start) {
                (true, None) => start = Some(w),
                (false, Some(a)) => {
                    spans.push((secs(a), secs(w)));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(a) = start {
            spans.push((secs(a), self.frames as f64 / sample_rate as f64));
        }
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(last) if a - last.1 < 1.5 => last.1 = b,
                _ => merged.push((a, b)),
            }
        }
        merged.retain(|(a, b)| b - a >= 1.0);
        merged
    }

    /// Combined frame `i` (in range) for mix coefficients `c`.
    #[inline(always)]
    fn weighted(&self, i: usize, c: &[f32; 4]) -> (f32, f32) {
        let f = &self.data[i * 6..i * 6 + 6];
        let (a, b, d) = (
            c[1] * self.scale[0],
            c[2] * self.scale[1],
            c[3] * self.scale[2],
        );
        (
            a * f[0] as f32 + b * f[2] as f32 + d * f[4] as f32,
            a * f[1] as f32 + b * f[3] as f32 + d * f[5] as f32,
        )
    }
}

/// Full-swing ramp time of a stem level change (click-free mutes).
pub(crate) const STEM_RAMP_S: f32 = 0.02;

/// Smoothed stem levels and the derived mix coefficients.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StemRamp {
    pub level: [f32; 4],
    step: f32,
}

impl StemRamp {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            level: [1.0; 4],
            step: 1.0 / (STEM_RAMP_S * sample_rate as f32).max(1.0),
        }
    }

    pub fn settled(&self, target: &[f32; 4]) -> bool {
        self.level == *target
    }

    /// Moves one frame towards `target`.
    #[inline(always)]
    pub fn tick(&mut self, target: &[f32; 4]) {
        for k in 0..4 {
            let d = target[k] - self.level[k];
            self.level[k] = if d.abs() <= self.step {
                target[k]
            } else {
                self.level[k] + self.step.copysign(d)
            };
        }
    }

    /// `[c_mix, c_vocals, c_drums, c_bass]`.
    #[inline(always)]
    pub fn coefs(&self) -> [f32; 4] {
        let [v, d, b, i] = self.level;
        [i, v - i, d - i, b - i]
    }
}

/// Mix coefficients for part levels `[vocals, drums, bass, inst]`.
#[inline(always)]
pub(crate) fn coefs_of(level: [f32; 4]) -> [f32; 4] {
    let [v, d, b, i] = level;
    [i, v - i, d - i, b - i]
}

/// Stem FX split: coefficients of the deck without `part` and of `part`
/// alone (their sum is the deck's normal output).
#[inline(always)]
pub(crate) fn split_coefs(level: [f32; 4], part: Stem) -> ([f32; 4], [f32; 4]) {
    let k = part.index();
    let mut main = level;
    main[k] = 0.0;
    let mut alone = [0.0; 4];
    alone[k] = level[k];
    (coefs_of(main), coefs_of(alone))
}

/// Mix frame `i` with stem levels applied; zero outside the track.
#[inline(always)]
pub(crate) fn frame_at(mix: &[f32], stems: &StemData, i: isize, c: &[f32; 4]) -> (f32, f32) {
    let total = (mix.len() / 2) as isize;
    if i < 0 || i >= total {
        return (0.0, 0.0);
    }
    let k = i as usize * 2;
    let (mut l, mut r) = (c[0] * mix[k], c[0] * mix[k + 1]);
    if (i as usize) < stems.frames {
        let (sl, sr) = stems.weighted(i as usize, c);
        l += sl;
        r += sr;
    }
    (l, r)
}
