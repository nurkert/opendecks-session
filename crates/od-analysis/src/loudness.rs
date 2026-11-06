//! Loudness measurement after ITU-R BS.1770-4 / EBU R128.
//!
//! Integrated loudness uses the K-weighting pre-filter (high shelf + RLB
//! high-pass, coefficients derived for any sample rate), 400 ms blocks with
//! 75 % overlap, an absolute gate at −70 LUFS and a relative gate 10 LU below
//! the absolute-gated level.
//!
//! Channels are weighted 1.0 (left/right/centre convention for the first two
//! channels). **Mono input is treated as dual mono** (weight 2), so a mono
//! mixdown of a centred stereo signal measures the same as the stereo
//! original: a full-scale-relative −20 dBFS 1 kHz sine reads ≈ −20 LUFS in
//! both layouts (a single *channel* of a stereo pair would read −23 LUFS).

use crate::dsp::Biquad;
use rayon::prelude::*;
use std::f64::consts::PI;

/// The two K-weighting stages for sample rate `fs`.
fn k_weighting(fs: f64) -> [Biquad; 2] {
    // Stage 1: high shelf (head acoustics).
    let f0 = 1_681.974_450_955_533;
    let g = 3.999_843_853_973_347;
    let q = 0.7071752369554196;
    let k = (PI * f0 / fs).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    // Stage 2: RLB high-pass.
    let f0 = 38.135_470_876_024_44;
    let q = 0.500_327_037_323_877_3;
    let k = (PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    let hp = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    };
    [shelf, hp]
}

/// Sum of K-weighted squared samples per 100 ms segment for one channel.
fn segment_energies(x: Vec<f32>, fs: u32) -> Vec<f64> {
    let mut y = x;
    for bq in k_weighting(fs as f64) {
        bq.process(&mut y);
    }
    let seg_len = (fs as usize / 10).max(1);
    y.chunks(seg_len)
        .filter(|c| c.len() == seg_len)
        .map(|c| c.iter().map(|&v| (v as f64) * (v as f64)).sum())
        .collect()
}

/// Integrated loudness (LUFS) of interleaved audio, or `None` when the signal
/// is shorter than one 400 ms block or entirely below the absolute gate
/// (e.g. silence).
pub fn integrated_lufs(samples: &[f32], channels: usize, sample_rate: u32) -> Option<f32> {
    if channels == 0 || sample_rate == 0 || samples.is_empty() {
        return None;
    }
    // Weighting: channels 0..2 are L/R (or L/R/C) with G = 1; further
    // channels are surround (G = 1.41). Mono counts as dual mono.
    let weights: Vec<f64> = (0..channels)
        .map(|c| match (channels, c) {
            (1, _) => 2.0,
            (_, 0..=2) => 1.0,
            _ => 1.41,
        })
        .collect();
    let per_channel: Vec<Vec<f64>> = (0..channels)
        .into_par_iter()
        .map(|c| {
            let x: Vec<f32> = samples.iter().skip(c).step_by(channels).copied().collect();
            segment_energies(x, sample_rate)
        })
        .collect();
    let seg_len = (sample_rate as usize / 10).max(1) as f64;
    let n_seg = per_channel.iter().map(Vec::len).min().unwrap_or(0);
    if n_seg < 4 {
        return None;
    }
    // Block j covers segments j..j+4 (400 ms, 100 ms step).
    let blocks: Vec<f64> = (0..=n_seg - 4)
        .map(|j| {
            per_channel
                .iter()
                .zip(&weights)
                .map(|(segs, w)| w * segs[j..j + 4].iter().sum::<f64>() / (4.0 * seg_len))
                .sum()
        })
        .collect();
    let lufs = |ms: f64| -0.691 + 10.0 * ms.log10();
    let abs_gated: Vec<f64> = blocks
        .iter()
        .copied()
        .filter(|&z| z > 0.0 && lufs(z) > -70.0)
        .collect();
    if abs_gated.is_empty() {
        return None;
    }
    let mean_abs = abs_gated.iter().sum::<f64>() / abs_gated.len() as f64;
    let rel_gate = lufs(mean_abs) - 10.0;
    let rel: Vec<f64> = abs_gated
        .into_iter()
        .filter(|&z| lufs(z) > rel_gate)
        .collect();
    if rel.is_empty() {
        return None;
    }
    Some(lufs(rel.iter().sum::<f64>() / rel.len() as f64) as f32)
}

/// Convenience wrapper for decoded audio.
pub fn integrated_lufs_audio(audio: &od_decode::DecodedAudio) -> Option<f32> {
    integrated_lufs(&audio.samples, audio.channels as usize, audio.sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(fs: u32, f: f64, amp: f64, secs: f64, channels: usize) -> Vec<f32> {
        let n = (fs as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (amp * (2.0 * PI * f * i as f64 / fs as f64).sin()) as f32;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    #[test]
    fn sine_minus_20_dbfs_stereo_is_minus_20_lufs() {
        for fs in [44_100, 48_000] {
            let x = sine(fs, 1000.0, 0.1, 5.0, 2);
            let l = integrated_lufs(&x, 2, fs).unwrap();
            assert!((l + 20.0).abs() < 0.5, "fs {fs}: {l}");
        }
    }

    #[test]
    fn ebu_reference_997hz_minus_23() {
        // EBU Tech 3341 case 1: stereo 997 Hz sine at −23 dBFS reads −23 LUFS.
        let amp = 10f64.powf(-23.0 / 20.0);
        let x = sine(48_000, 997.0, amp, 20.0, 2);
        let l = integrated_lufs(&x, 2, 48_000).unwrap();
        assert!((l + 23.0).abs() < 0.1, "{l}");
    }

    #[test]
    fn mono_counts_as_dual_mono() {
        let x = sine(44_100, 1000.0, 0.1, 5.0, 1);
        let l = integrated_lufs(&x, 1, 44_100).unwrap();
        assert!((l + 20.0).abs() < 0.5, "{l}");
    }

    #[test]
    fn single_channel_of_stereo_is_minus_3() {
        let x: Vec<f32> = sine(48_000, 997.0, 0.1, 5.0, 1)
            .into_iter()
            .flat_map(|v| [v, 0.0])
            .collect();
        let l = integrated_lufs(&x, 2, 48_000).unwrap();
        assert!((l + 23.0).abs() < 0.2, "{l}");
    }

    #[test]
    fn relative_gate_ignores_quiet_part() {
        // EBU Tech 3341 case 3 style: loud part dominates, −36 dB part gated.
        let mut x = sine(48_000, 1000.0, 10f64.powf(-36.0 / 20.0), 10.0, 2);
        x.extend(sine(48_000, 1000.0, 10f64.powf(-23.0 / 20.0), 60.0, 2));
        x.extend(sine(48_000, 1000.0, 10f64.powf(-36.0 / 20.0), 10.0, 2));
        let l = integrated_lufs(&x, 2, 48_000).unwrap();
        assert!((l + 23.0).abs() < 0.1, "{l}");
    }

    #[test]
    fn silence_and_short_input() {
        assert_eq!(integrated_lufs(&vec![0.0; 96_000], 2, 48_000), None);
        assert_eq!(integrated_lufs(&[0.1; 100], 1, 48_000), None);
        assert_eq!(integrated_lufs(&[], 2, 48_000), None);
    }
}
