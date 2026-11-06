//! Fast load-time resampler for interleaved stereo (polyphase windowed sinc,
//! Kaiser window, linear interpolation between 1024 filter phases, split
//! across CPU cores). Used off the real-time thread when a track's rate
//! differs from the engine rate (SPEC §13: track load ≤ 150 ms).

use std::f64::consts::PI;

const HALF_TAPS: usize = 16;
const PHASES: usize = 1024;
const KAISER_BETA: f64 = 8.6;

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let q = x * x / 4.0;
    for k in 1..50 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

struct Table {
    /// Taps per phase (2 × half width in input samples).
    taps: usize,
    half: usize,
    /// `(PHASES + 1) × taps` weights; phase p holds the kernel for a
    /// fractional offset p / PHASES.
    w: Vec<f32>,
}

fn table(cutoff: f64) -> Table {
    let half = (HALF_TAPS as f64 / cutoff).ceil() as usize;
    let taps = 2 * half;
    let mut w = vec![0f32; (PHASES + 1) * taps];
    let i0b = bessel_i0(KAISER_BETA);
    for p in 0..=PHASES {
        let frac = p as f64 / PHASES as f64;
        let row = &mut w[p * taps..(p + 1) * taps];
        let mut sum = 0.0;
        let mut tmp = [0f64; 512];
        for (k, t) in tmp.iter_mut().enumerate().take(taps) {
            // Tap k reads input sample i0 - half + 1 + k; distance from centre:
            let d = (k as f64 - (half as f64 - 1.0)) - frac;
            let x = d / half as f64;
            let win = if x.abs() >= 1.0 {
                0.0
            } else {
                bessel_i0(KAISER_BETA * (1.0 - x * x).sqrt()) / i0b
            };
            let s = if d.abs() < 1e-12 {
                1.0
            } else {
                (PI * cutoff * d).sin() / (PI * cutoff * d)
            };
            *t = s * win;
            sum += *t;
        }
        for k in 0..taps {
            row[k] = (tmp[k] / sum) as f32;
        }
    }
    Table { taps, half, w }
}

fn render(input: &[f32], frames_in: usize, ratio: f64, t: &Table, out: &mut [f32], first: usize) {
    let taps = t.taps;
    let mut acc_w = [0f32; 512];
    for (j, o) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let center = (first + j) as f64 / ratio;
        let i0 = center.floor() as isize;
        let fpos = (center - i0 as f64) * PHASES as f64;
        let p = (fpos as usize).min(PHASES - 1);
        let fr = (fpos - p as f64) as f32;
        let r0 = &t.w[p * taps..(p + 1) * taps];
        let r1 = &t.w[(p + 1) * taps..(p + 2) * taps];
        for k in 0..taps {
            acc_w[k] = r0[k] + (r1[k] - r0[k]) * fr;
        }
        let start = i0 - t.half as isize + 1;
        let (mut l, mut r) = (0f32, 0f32);
        if start >= 0 && (start as usize + taps) <= frames_in {
            let s = &input[start as usize * 2..(start as usize + taps) * 2];
            for k in 0..taps {
                l += s[k * 2] * acc_w[k];
                r += s[k * 2 + 1] * acc_w[k];
            }
        } else {
            for k in 0..taps {
                let i = start + k as isize;
                if i >= 0 && (i as usize) < frames_in {
                    l += input[i as usize * 2] * acc_w[k];
                    r += input[i as usize * 2 + 1] * acc_w[k];
                }
            }
        }
        o[0] = l;
        o[1] = r;
    }
}

/// Resamples interleaved stereo from `from` Hz to `to` Hz.
pub fn resample_stereo(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.len() < 2 || from == 0 || to == 0 {
        return input.to_vec();
    }
    let frames_in = input.len() / 2;
    let ratio = to as f64 / from as f64;
    let frames_out = (frames_in as f64 * ratio).round() as usize;
    let cutoff = ratio.min(1.0) * 0.96;
    let t = table(cutoff);
    let mut out = vec![0f32; frames_out * 2];
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, 16);
    let chunk = frames_out.div_ceil(threads).max(4096);
    std::thread::scope(|s| {
        for (ci, part) in out.chunks_mut(chunk * 2).enumerate() {
            let t = &t;
            s.spawn(move || render(input, frames_in, ratio, t, part, ci * chunk));
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, freq: f64, secs: f64) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32;
                [v, -v]
            })
            .collect()
    }

    #[test]
    fn upsample_preserves_tone() {
        let x = tone(44_100, 1000.0, 1.0);
        let y = resample_stereo(&x, 44_100, 48_000);
        assert_eq!(y.len(), 48_000 * 2);
        // Compare with the ideal 1 kHz sine at 48 kHz in the steady state.
        let mut err = 0f32;
        for i in 1000..47_000 {
            let ideal = (2.0 * PI * 1000.0 * i as f64 / 48_000.0).sin() as f32;
            err = err.max((y[i * 2] - ideal).abs());
            assert!((y[i * 2] + y[i * 2 + 1]).abs() < 1e-6);
        }
        assert!(err < 2e-4, "max error {err}");
    }

    #[test]
    fn downsample_rejects_aliases() {
        // 30 kHz in a 96 kHz file would alias to 14.1 kHz at 44.1 kHz.
        let x = tone(96_000, 30_000.0, 1.0);
        let y = resample_stereo(&x, 96_000, 44_100);
        let peak = y[2000..80_000].iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert!(peak < 1e-3, "aliased peak {peak}");
    }
}
