//! Compact Schroeder/Freeverb-style stereo reverb. Wet output only.

const COMB_TUNING: [usize; 6] = [1116, 1188, 1277, 1356, 1422, 1491];
const ALLPASS_TUNING: [usize; 3] = [556, 441, 341];
const STEREO_SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    filter: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            filter: 0.0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.idx];
        self.filter = out * (1.0 - damp) + self.filter * damp;
        self.buf[self.idx] = x + self.filter * feedback;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            idx: 0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let out = b - x;
        self.buf[self.idx] = x + b * 0.5;
        self.idx += 1;
        if self.idx == self.buf.len() {
            self.idx = 0;
        }
        out
    }
}

pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    /// Feedback 0.7 .. 0.98 (room size).
    pub room: f32,
    /// High-frequency damping 0..1.
    pub damp: f32,
}

impl Reverb {
    pub fn new(sample_rate: u32) -> Self {
        let scale = sample_rate as f32 / 44_100.0;
        let mk = |spread: usize| -> (Vec<Comb>, Vec<Allpass>) {
            (
                COMB_TUNING
                    .iter()
                    .map(|&t| Comb::new(((t + spread) as f32 * scale) as usize))
                    .collect(),
                ALLPASS_TUNING
                    .iter()
                    .map(|&t| Allpass::new(((t + spread) as f32 * scale) as usize))
                    .collect(),
            )
        };
        let (cl, al) = mk(0);
        let (cr, ar) = mk(STEREO_SPREAD);
        Self {
            combs: [cl, cr],
            allpasses: [al, ar],
            room: 0.84,
            damp: 0.3,
        }
    }

    /// Returns the wet signal for one input frame.
    #[inline]
    pub fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let input = (frame[0] + frame[1]) * 0.015;
        let mut out = [0.0f32; 2];
        for ch in 0..2 {
            let mut acc = 0.0;
            for c in &mut self.combs[ch] {
                acc += c.process(input, self.room, self.damp);
            }
            for a in &mut self.allpasses[ch] {
                acc = a.process(acc);
            }
            out[ch] = acc;
        }
        out
    }

    pub fn clear(&mut self) {
        for ch in 0..2 {
            for c in &mut self.combs[ch] {
                c.buf.iter_mut().for_each(|v| *v = 0.0);
                c.filter = 0.0;
            }
            for a in &mut self.allpasses[ch] {
                a.buf.iter_mut().for_each(|v| *v = 0.0);
            }
        }
    }
}
