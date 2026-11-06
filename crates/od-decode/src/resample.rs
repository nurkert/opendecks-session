//! Offline windowed-sinc resampler for mono or interleaved audio.
//!
//! Used for analysis and file conversion, not in the real-time path.

use std::f64::consts::PI;

const HALF_TAPS: i64 = 16;

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

fn blackman(n: f64, half: f64) -> f64 {
    // n in [-half, half]
    let t = (n + half) / (2.0 * half);
    0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos()
}

/// Resamples interleaved `input` with `channels` channels from `from` Hz to
/// `to` Hz. Applies a low-pass at the lower of the two Nyquist frequencies.
pub fn resample(input: &[f32], channels: usize, from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || channels == 0 {
        return input.to_vec();
    }
    let frames_in = input.len() / channels;
    let ratio = to as f64 / from as f64;
    let frames_out = (frames_in as f64 * ratio).round() as usize;
    let cutoff = ratio.min(1.0) * 0.95;
    let half = HALF_TAPS as f64 / cutoff.min(1.0);
    let mut out = vec![0f32; frames_out * channels];
    for o in 0..frames_out {
        let center = o as f64 / ratio;
        let lo = (center - half).ceil() as i64;
        let hi = (center + half).floor() as i64;
        let mut acc = vec![0f64; channels];
        let mut norm = 0.0;
        for i in lo..=hi {
            if i < 0 || i as usize >= frames_in {
                continue;
            }
            let d = i as f64 - center;
            let w = cutoff * sinc(cutoff * d) * blackman(d, half);
            norm += w;
            let base = i as usize * channels;
            for c in 0..channels {
                acc[c] += input[base + c] as f64 * w;
            }
        }
        let norm = if norm.abs() < 1e-12 { 1.0 } else { norm / cutoff.min(1.0) * cutoff.min(1.0) };
        for c in 0..channels {
            out[o * channels + c] = (acc[c] / norm) as f32;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, freq: f64, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32).collect()
    }

    #[test]
    fn preserves_tone_amplitude_and_length() {
        let x = tone(44_100, 1000.0, 44_100);
        let y = resample(&x, 1, 44_100, 22_050);
        assert_eq!(y.len(), 22_050);
        let peak = y[1000..21000].iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert!((peak - 1.0).abs() < 0.02, "peak {peak}");
    }

    #[test]
    fn removes_content_above_nyquist() {
        let x = tone(44_100, 15_000.0, 44_100);
        let y = resample(&x, 1, 44_100, 22_050);
        let peak = y[1000..21000].iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert!(peak < 0.05, "aliased peak {peak}");
    }

    #[test]
    fn upsampling_interleaved() {
        let x: Vec<f32> = tone(22_050, 500.0, 2205).into_iter().flat_map(|v| [v, -v]).collect();
        let y = resample(&x, 2, 22_050, 44_100);
        assert_eq!(y.len(), 4410 * 2);
        assert!((y[2000] + y[2001]).abs() < 1e-4);
    }
}
