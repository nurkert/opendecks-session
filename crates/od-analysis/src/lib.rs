//! Pure-Rust track analysis for OpenDecks Session.
//!
//! [`analyze`] runs the whole pipeline on decoded audio and fills every field
//! of [`od_core::TrackAnalysis`]:
//!
//! | Field | Module |
//! |---|---|
//! | `beatgrid` (constant tempo, downbeats) | [`tempo`] |
//! | `key` | [`key`] |
//! | `waveform` (3-band, 150 columns/s) | [`waveform`] |
//! | `phrases`, `phrase_mood` | [`phrase`] |
//! | `loudness_lufs` (EBU R128 integrated) | [`loudness`] |
//! | `peak`, `energy` | this module |
//!
//! Tempo, key and phrase analysis run on a mono mixdown decimated to
//! ≈ 11 kHz (≈ 8 kHz in lite mode); waveform and loudness use the full
//! rate. Independent stages run in parallel with rayon. All results are
//! deterministic for identical input.

mod dsp;
pub mod key;
pub mod loudness;
pub mod phrase;
pub mod tempo;
pub mod waveform;

#[cfg(test)]
mod testsig;

use od_core::TrackAnalysis;
use od_decode::{DecodeError, DecodedAudio, Layout};
use std::path::Path;

/// Version of the analysis algorithms. Bump whenever results change so
/// stored analyses can be refreshed.
pub const ANALYSIS_VERSION: u32 = 1;

/// Analysis settings.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalysisOptions {
    /// Lowest BPM reported; tempos are folded by octaves into
    /// `[min_bpm, max_bpm]`.
    pub min_bpm: f64,
    /// Highest BPM reported.
    pub max_bpm: f64,
    /// Whether to detect phrases (and the phrase mood).
    pub detect_phrases: bool,
    /// Faster, slightly less precise settings (e.g. Raspberry Pi).
    pub lite: bool,
}

impl Default for AnalysisOptions {
    /// 87–175 BPM (the usual DJ fold: hip-hop doubles, drum & bass stays at
    /// 170+), phrases on, full quality.
    fn default() -> Self {
        Self {
            min_bpm: 87.0,
            max_bpm: 175.0,
            detect_phrases: true,
            lite: false,
        }
    }
}

/// Analyses decoded audio (mono or interleaved multichannel).
pub fn analyze(audio: &DecodedAudio, opts: &AnalysisOptions) -> TrackAnalysis {
    let channels = audio.channels as usize;
    let mut result = TrackAnalysis {
        analysis_version: ANALYSIS_VERSION,
        ..TrackAnalysis::default()
    };
    if channels == 0 || audio.sample_rate == 0 || audio.samples.len() < channels {
        return result;
    }
    let sr = audio.sample_rate;
    let mono = dsp::mixdown(&audio.samples, channels);
    let duration = mono.len() as f64 / sr as f64;

    let (loudness, (wave, (energy, ((grid, key), (phrases, mood))))) = rayon::join(
        || loudness::integrated_lufs(&audio.samples, channels, sr),
        || {
            rayon::join(
                || waveform::compute_waveform(&mono, sr),
                || {
                    rayon::join(
                        || energy_curve(&mono, sr),
                        || {
                            let factor =
                                dsp::decimation_factor(sr, tempo::analysis_rate_target(opts));
                            let x = dsp::decimate(&mono, factor);
                            let fs = sr as f64 / factor as f64;
                            let (grid, key) = rayon::join(
                                || tempo::detect_beatgrid_at(&x, fs, duration, opts),
                                || key::detect_key_at(&x, fs, opts.lite),
                            );
                            let phrases = if opts.detect_phrases && !grid.is_empty() {
                                phrase::detect_phrases(&x, fs.round() as u32, &grid)
                            } else {
                                (Vec::new(), Default::default())
                            };
                            ((grid, key), phrases)
                        },
                    )
                },
            )
        },
    );
    result.peak = dsp::peak_abs(&audio.samples);
    result.loudness_lufs = loudness;
    result.waveform = wave;
    result.energy = energy;
    result.beatgrid = grid;
    result.key = key;
    result.phrases = phrases;
    result.phrase_mood = mood;
    result
}

/// Decodes and analyses a file. Decodes to stereo so that loudness is
/// measured per channel as BS.1770 requires.
pub fn analyze_file(path: &Path, opts: &AnalysisOptions) -> Result<TrackAnalysis, DecodeError> {
    let audio = od_decode::decode(path, Layout::Stereo)?;
    Ok(analyze(&audio, opts))
}

/// RMS per second (last partial second included), normalised so that the
/// loudest second is 1.0. All zeros for silence.
pub fn energy_curve(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    if sample_rate == 0 || mono.is_empty() {
        return Vec::new();
    }
    let rms: Vec<f64> = mono
        .chunks(sample_rate as usize)
        .map(|c| (c.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / c.len() as f64).sqrt())
        .collect();
    let max = rms.iter().cloned().fold(0.0, f64::max);
    if max <= 1e-9 {
        return vec![0.0; rms.len()];
    }
    rms.iter().map(|&r| (r / max) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsig::*;
    use std::time::Instant;

    fn stereo(mono: &[f32], sr: u32) -> DecodedAudio {
        DecodedAudio {
            sample_rate: sr,
            channels: 2,
            samples: mono.iter().flat_map(|&v| [v, v]).collect(),
        }
    }

    #[test]
    fn full_pipeline_on_house_loop() {
        let (x, kicks) = house_track(44_100, 124.0, 0.18, 90.0);
        let a = analyze(&stereo(&x, 44_100), &AnalysisOptions::default());
        assert_eq!(a.analysis_version, ANALYSIS_VERSION);
        assert!((a.beatgrid.first_bpm().unwrap() - 124.0).abs() < 0.05);
        let i = a.beatgrid.beat_index_at(kicks[10] + 0.05).unwrap();
        assert!((a.beatgrid.beats[i].time - kicks[10]).abs() < 0.005);
        assert_eq!(a.waveform.columns.len(), 13_500);
        assert_eq!(a.energy.len(), 90);
        assert!(a.energy.iter().all(|&e| (0.0..=1.0).contains(&e)));
        assert!(a.peak > 0.5 && a.peak <= 2.0);
        assert!(a.loudness_lufs.is_some());
        assert!(!a.phrases.is_empty());
    }

    #[test]
    fn deterministic() {
        let (x, _) = house_track(44_100, 128.0, 0.0, 40.0);
        let audio = stereo(&x, 44_100);
        let a = analyze(&audio, &AnalysisOptions::default());
        let b = analyze(&audio, &AnalysisOptions::default());
        assert_eq!(a, b);
    }

    #[test]
    fn silence_does_not_panic() {
        let audio = DecodedAudio {
            sample_rate: 44_100,
            channels: 2,
            samples: vec![0.0; 44_100 * 2 * 20],
        };
        let a = analyze(&audio, &AnalysisOptions::default());
        assert!(a.beatgrid.is_empty());
        assert_eq!(a.key, None);
        assert_eq!(a.loudness_lufs, None);
        assert_eq!(a.peak, 0.0);
        assert!(a.phrases.is_empty());
        assert_eq!(a.energy, vec![0.0; 20]);
        assert_eq!(a.waveform.columns.len(), 3000);
        let empty = analyze(&DecodedAudio::default(), &AnalysisOptions::default());
        assert_eq!(empty.analysis_version, ANALYSIS_VERSION);
        let tiny = DecodedAudio {
            sample_rate: 44_100,
            channels: 1,
            samples: vec![0.5; 10],
        };
        let _ = analyze(
            &tiny,
            &AnalysisOptions {
                lite: true,
                ..Default::default()
            },
        );
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(
            analyze_file(
                Path::new("/nonexistent/od-analysis.wav"),
                &AnalysisOptions::default()
            )
            .is_err()
        );
    }

    /// Synthetic six-minute track for timing.
    fn six_minutes() -> DecodedAudio {
        let (x, _) = house_track(44_100, 126.0, 0.1, 360.0);
        let mut noise = Noise::new(1);
        let x: Vec<f32> = x
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let t = i as f64 / 44_100.0;
                v + (0.05 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() + 0.01 * noise.next())
                    as f32
            })
            .collect();
        stereo(&x, 44_100)
    }

    /// Analyses the files listed (newline-separated) in `OD_ANALYSIS_FILES`
    /// and prints a summary per file. For manual checks against real music.
    #[test]
    #[ignore = "manual; set OD_ANALYSIS_FILES and run with --release -- --ignored --nocapture"]
    fn real_files_from_env() {
        let Ok(list) = std::env::var("OD_ANALYSIS_FILES") else {
            return;
        };
        for line in list.lines().filter(|l| !l.trim().is_empty()) {
            let path = Path::new(line.trim());
            let t = Instant::now();
            match analyze_file(path, &AnalysisOptions::default()) {
                Ok(a) => {
                    let first = a
                        .beatgrid
                        .beats
                        .iter()
                        .find(|b| b.bar_position == 1)
                        .map(|b| b.time);
                    let kinds: Vec<String> = a
                        .phrases
                        .iter()
                        .map(|p| format!("{:?}{}", p.kind, (p.end_beat - p.start_beat) / 4))
                        .collect();
                    println!(
                        "{:.2}s | bpm {:?} | first downbeat {:?} | key {:?} | {:?} LUFS | peak {:.2} | mood {:?} | {}\n    {}",
                        t.elapsed().as_secs_f64(),
                        a.beatgrid.first_bpm(),
                        first.map(|f| (f * 1000.0).round() / 1000.0),
                        a.key.map(|k| k.format(od_core::KeyNotation::Musical)),
                        a.loudness_lufs.map(|l| (l * 10.0).round() / 10.0),
                        a.peak,
                        a.phrase_mood,
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default(),
                        kinds.join(" "),
                    );
                }
                Err(e) => println!("{}: {e}", path.display()),
            }
        }
    }

    #[test]
    #[ignore = "timing; run with --release -- --ignored --nocapture"]
    fn timing_six_minutes() {
        let audio = six_minutes();
        for lite in [false, true] {
            let opts = AnalysisOptions {
                lite,
                ..Default::default()
            };
            let _ = analyze(&audio, &opts); // warm-up (thread pool, FFT plans)
            let t = Instant::now();
            let a = analyze(&audio, &opts);
            let el = t.elapsed();
            println!(
                "lite={lite}: 6 min analysed in {:.3} s (bpm {:?}, key {:?}, lufs {:?}, phrases {})",
                el.as_secs_f64(),
                a.beatgrid.first_bpm(),
                a.key.map(|k| k.to_string()),
                a.loudness_lufs,
                a.phrases.len()
            );
            assert!((a.beatgrid.first_bpm().unwrap() - 126.0).abs() < 0.05);
        }
    }
}
