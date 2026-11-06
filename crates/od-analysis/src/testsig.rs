//! Deterministic synthetic test signals (test builds only).

use std::f64::consts::PI;

/// Tiny deterministic noise source (LCG).
pub struct Noise(u64);

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
    }
    pub fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }
}

/// Synthetic kick: 150 → 45 Hz pitch sweep with exponential decay.
pub fn kick(fs: u32, amp: f64) -> Vec<f32> {
    let n = (fs as f64 * 0.25) as usize;
    let mut phase = 0.0;
    (0..n)
        .map(|i| {
            let t = i as f64 / fs as f64;
            let f = 45.0 + 105.0 * (-t / 0.03).exp();
            phase += 2.0 * PI * f / fs as f64;
            (amp * phase.sin() * (-t / 0.08).exp()) as f32
        })
        .collect()
}

/// Short noise burst (click / hat / snare depending on length).
pub fn burst(fs: u32, amp: f64, decay: f64, seed: u64) -> Vec<f32> {
    let n = (fs as f64 * decay * 6.0) as usize;
    let mut noise = Noise::new(seed);
    let mut prev = 0.0;
    (0..n)
        .map(|i| {
            let t = i as f64 / fs as f64;
            let w = noise.next();
            let hp = w - prev; // crude high-pass for brightness
            prev = w;
            (amp * hp * 0.7 * (-t / decay).exp()) as f32
        })
        .collect()
}

pub fn add(buf: &mut [f32], at: usize, sound: &[f32]) {
    for (i, &s) in sound.iter().enumerate() {
        if let Some(b) = buf.get_mut(at + i) {
            *b += s;
        }
    }
}

fn beat_times(fs: u32, bpm: f64, offset: f64, secs: f64) -> Vec<(usize, f64)> {
    let period = 60.0 / bpm;
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let t = offset + i as f64 * period;
        if t > secs - 0.3 {
            break;
        }
        let idx = (t * fs as f64).round() as usize;
        out.push((idx, idx as f64 / fs as f64));
        i += 1;
    }
    out
}

/// Click on every beat. Returns the signal and the exact onset times.
pub fn click_track(fs: u32, bpm: f64, offset: f64, secs: f64) -> (Vec<f32>, Vec<f64>) {
    let mut x = vec![0f32; (fs as f64 * secs) as usize];
    let click = burst(fs, 0.9, 0.004, 7);
    let beats = beat_times(fs, bpm, offset, secs);
    for &(i, _) in &beats {
        add(&mut x, i, &click);
    }
    (x, beats.into_iter().map(|b| b.1).collect())
}

/// Kick track. With `four_on_floor` a kick on every beat plus quiet
/// off-beat hats; otherwise kicks on even beats and equally loud snares on
/// odd beats (drum & bass style). Returns all beat onset times.
pub fn kick_track(
    fs: u32,
    bpm: f64,
    offset: f64,
    secs: f64,
    four_on_floor: bool,
) -> (Vec<f32>, Vec<f64>) {
    let mut x = vec![0f32; (fs as f64 * secs) as usize];
    let k = kick(fs, 0.8);
    let hat = burst(fs, 0.15, 0.01, 3);
    let snare = burst(fs, 0.6, 0.05, 5);
    let half = ((60.0 / bpm / 2.0) * fs as f64).round() as usize;
    let beats = beat_times(fs, bpm, offset, secs);
    for (n, &(i, _)) in beats.iter().enumerate() {
        if four_on_floor || n % 2 == 0 {
            add(&mut x, i, &k);
        } else {
            add(&mut x, i, &snare);
        }
        if four_on_floor {
            add(&mut x, i + half, &hat);
        }
    }
    (x, beats.into_iter().map(|b| b.1).collect())
}

/// Kick track whose kicks on beats `first_down + 4k` are twice as loud and
/// carry a bass note change. Returns all beat times.
pub fn accented_track(
    fs: u32,
    bpm: f64,
    offset: f64,
    secs: f64,
    first_down: usize,
) -> (Vec<f32>, Vec<f64>) {
    let mut x = vec![0f32; (fs as f64 * secs) as usize];
    let loud = kick(fs, 0.9);
    let soft = kick(fs, 0.45);
    let beats = beat_times(fs, bpm, offset, secs);
    for (n, &(i, _)) in beats.iter().enumerate() {
        let down = n >= first_down && (n - first_down).is_multiple_of(4);
        add(&mut x, i, if down { &loud } else { &soft });
    }
    (x, beats.into_iter().map(|b| b.1).collect())
}

/// House-like loop: four-on-the-floor kick, off-beat open hat, 16th closed
/// hats and an off-beat bass. Returns kick times.
pub fn house_track(fs: u32, bpm: f64, offset: f64, secs: f64) -> (Vec<f32>, Vec<f64>) {
    let mut x = vec![0f32; (fs as f64 * secs) as usize];
    let k = kick(fs, 0.8);
    let open = burst(fs, 0.25, 0.04, 11);
    let closed = burst(fs, 0.08, 0.008, 13);
    let period = 60.0 / bpm;
    let beats = beat_times(fs, bpm, offset, secs);
    let bass_len = (period * 0.4 * fs as f64) as usize;
    for (n, &(i, t)) in beats.iter().enumerate() {
        add(&mut x, i, &k);
        let off = ((t + period / 2.0) * fs as f64).round() as usize;
        add(&mut x, off, &open);
        for s in [1, 3] {
            let at = ((t + period * s as f64 / 4.0) * fs as f64).round() as usize;
            add(&mut x, at, &closed);
        }
        let f = if (n / 8) % 2 == 0 { 55.0 } else { 49.0 };
        let bass: Vec<f32> = (0..bass_len)
            .map(|j| {
                let tt = j as f64 / fs as f64;
                let env = (1.0 - j as f64 / bass_len as f64).min(tt / 0.005).min(1.0);
                (0.3 * env * (2.0 * PI * f * tt).sin()) as f32
            })
            .collect();
        add(&mut x, off, &bass);
    }
    (x, beats.into_iter().map(|b| b.1).collect())
}
