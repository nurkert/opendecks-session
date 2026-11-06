//! Three-band colour waveform.
//!
//! The mono signal is split with 4th-order (Linkwitz–Riley style, two
//! cascaded Butterworth biquads) crossovers at [`LOW_CROSSOVER`] and
//! [`HIGH_CROSSOVER`] Hz. Each column stores the absolute peak of every band
//! and of the full-band signal over its time span, scaled so that full scale
//! (1.0) maps to 255.

use crate::dsp::{Biquad, filtered};
use od_core::{Waveform, WaveformColumn};
use std::f64::consts::FRAC_1_SQRT_2;

/// Low/mid crossover in Hz.
pub const LOW_CROSSOVER: f64 = 200.0;
/// Mid/high crossover in Hz.
pub const HIGH_CROSSOVER: f64 = 2500.0;

/// Computes the detail waveform at [`Waveform::DETAIL_RATE`] columns per
/// second from mono samples.
pub fn compute_waveform(samples: &[f32], sample_rate: u32) -> Waveform {
    compute_waveform_at(samples, sample_rate, Waveform::DETAIL_RATE)
}

/// Computes a waveform with an arbitrary column rate.
pub fn compute_waveform_at(samples: &[f32], sample_rate: u32, columns_per_second: f64) -> Waveform {
    if samples.is_empty() || sample_rate == 0 || columns_per_second <= 0.0 {
        return Waveform {
            columns_per_second,
            columns: Vec::new(),
        };
    }
    let fs = sample_rate as f64;
    let q = FRAC_1_SQRT_2;
    let nyq = fs * 0.45;
    let lo_f = LOW_CROSSOVER.min(nyq);
    let hi_f = HIGH_CROSSOVER.min(nyq);
    let lp_low = Biquad::lowpass(fs, lo_f, q);
    let hp_low = Biquad::highpass(fs, lo_f, q);
    let lp_high = Biquad::lowpass(fs, hi_f, q);
    let hp_high = Biquad::highpass(fs, hi_f, q);

    let n_cols = ((samples.len() as f64 / fs) * columns_per_second).ceil() as usize;
    let bounds: Vec<usize> = (0..=n_cols)
        .map(|i| ((i as f64 * fs / columns_per_second).round() as usize).min(samples.len()))
        .collect();
    let peaks = |x: &[f32]| -> Vec<u8> {
        bounds
            .windows(2)
            .map(|w| {
                let p = x[w[0]..w[1]].iter().fold(0f32, |a, &b| a.max(b.abs()));
                (p * 255.0).round().clamp(0.0, 255.0) as u8
            })
            .collect()
    };
    let ((low, mid), (high, peak)) = rayon::join(
        || {
            rayon::join(
                || peaks(&filtered(samples, &[lp_low, lp_low])),
                || peaks(&filtered(samples, &[hp_low, hp_low, lp_high, lp_high])),
            )
        },
        || {
            rayon::join(
                || peaks(&filtered(samples, &[hp_high, hp_high])),
                || peaks(samples),
            )
        },
    );
    let columns = (0..n_cols)
        .map(|i| WaveformColumn {
            low: low[i],
            mid: mid[i],
            high: high[i],
            peak: peak[i],
        })
        .collect();
    Waveform {
        columns_per_second,
        columns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn sine(fs: u32, f: f64, amp: f64, secs: f64) -> Vec<f32> {
        (0..(fs as f64 * secs) as usize)
            .map(|i| (amp * (2.0 * PI * f * i as f64 / fs as f64).sin()) as f32)
            .collect()
    }

    fn mean(cols: &[WaveformColumn], f: impl Fn(&WaveformColumn) -> u8) -> f64 {
        cols.iter().map(|c| f(c) as f64).sum::<f64>() / cols.len() as f64
    }

    #[test]
    fn column_count_and_rate() {
        let w = compute_waveform(&sine(44_100, 440.0, 0.5, 2.0), 44_100);
        assert_eq!(w.columns_per_second, Waveform::DETAIL_RATE);
        assert_eq!(w.columns.len(), 300);
    }

    #[test]
    fn low_sine_lands_in_low_band() {
        let w = compute_waveform(&sine(44_100, 50.0, 0.8, 2.0), 44_100);
        let c = &w.columns[50..];
        assert!(mean(c, |c| c.low) > 180.0);
        assert!(mean(c, |c| c.mid) < 40.0);
        assert!(mean(c, |c| c.high) < 5.0);
        let max_peak = c.iter().map(|c| c.peak).max().unwrap_or(0);
        assert!((max_peak as i32 - 204).abs() <= 2, "{max_peak}");
    }

    #[test]
    fn high_sine_lands_in_high_band() {
        let w = compute_waveform(&sine(48_000, 8000.0, 0.8, 2.0), 48_000);
        let c = &w.columns[50..];
        assert!(mean(c, |c| c.high) > 180.0);
        assert!(mean(c, |c| c.mid) < 30.0);
        assert!(mean(c, |c| c.low) < 5.0);
    }

    #[test]
    fn mid_sine_lands_in_mid_band() {
        let w = compute_waveform(&sine(44_100, 700.0, 0.8, 2.0), 44_100);
        let c = &w.columns[50..];
        assert!(mean(c, |c| c.mid) > 150.0);
        assert!(mean(c, |c| c.low) < 40.0);
        assert!(mean(c, |c| c.high) < 40.0);
    }

    #[test]
    fn empty_input() {
        assert!(compute_waveform(&[], 44_100).columns.is_empty());
    }
}
