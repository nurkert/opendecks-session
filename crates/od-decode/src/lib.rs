//! Audio decoding, probing and tag reading.
//!
//! All decoders go through symphonia. Decoded audio is returned as `f32`
//! samples; callers choose between a mono mixdown (analysis) and interleaved
//! stereo (playback).

mod resample;

pub use resample::resample;

use od_core::{AudioFormat, ContentHash, TrackTags};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    #[error("unsupported or corrupt audio: {0}")]
    Unsupported(String),
    #[error("file contains no audio track")]
    NoAudioTrack,
}

impl From<SymError> for DecodeError {
    fn from(e: SymError) -> Self {
        match e {
            SymError::IoError(e) => DecodeError::Io(e),
            other => DecodeError::Unsupported(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, DecodeError>;

/// Facts about a file obtained without decoding all audio.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeInfo {
    pub format: Option<AudioFormat>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Duration in seconds, if the container states it.
    pub duration: Option<f64>,
    pub bit_rate_kbps: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub tags: TrackTags,
    /// Embedded cover art (raw image bytes and media type).
    pub artwork: Option<(Vec<u8>, String)>,
}

/// Decoded PCM audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples.
    pub samples: Vec<f32>,
}

impl DecodedAudio {
    pub fn frames(&self) -> usize {
        if self.channels == 0 { 0 } else { self.samples.len() / self.channels as usize }
    }

    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / self.sample_rate as f64 }
    }
}

/// Which channel layout to decode into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Mono,
    Stereo,
}

fn open(path: &Path) -> Result<(Box<dyn FormatReader>, Option<MetadataRevision>)> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions { enable_gapless: true, ..Default::default() },
        &MetadataOptions::default(),
    )?;
    // Metadata found before the container (ID3v2) lives in the probe result.
    let mut probed_meta = probed.metadata;
    let early = probed_meta.get().and_then(|m| m.current().cloned());
    Ok((probed.format, early))
}

fn apply_revision(rev: &MetadataRevision, tags: &mut TrackTags, artwork: &mut Option<(Vec<u8>, String)>) {
    for tag in rev.tags() {
        let text = match &tag.value {
            Value::String(s) => s.trim().to_string(),
            other => other.to_string(),
        };
        let Some(key) = tag.std_key else { continue };
        match key {
            StandardTagKey::TrackTitle => tags.title = text,
            StandardTagKey::Artist => tags.artist = text,
            StandardTagKey::Album => tags.album = text,
            StandardTagKey::Genre => tags.genre = text,
            StandardTagKey::Label => tags.label = text,
            StandardTagKey::Remixer => tags.remixer = text,
            StandardTagKey::Composer => tags.composer = text,
            StandardTagKey::Comment => tags.comment = text,
            StandardTagKey::Date | StandardTagKey::ReleaseDate | StandardTagKey::OriginalDate => {
                if tags.year.is_none() {
                    tags.year = text.get(..4).and_then(|y| y.parse().ok());
                }
            }
            StandardTagKey::TrackNumber => {
                tags.track_number = text.split('/').next().and_then(|n| n.trim().parse().ok());
            }
            _ => {}
        }
    }
    if artwork.is_none() {
        if let Some(visual) = rev.visuals().first() {
            *artwork = Some((visual.data.to_vec(), visual.media_type.clone()));
        }
    }
}

/// Reads stream parameters and tags without decoding the audio.
pub fn probe(path: &Path) -> Result<ProbeInfo> {
    let (mut format, early) = open(path)?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::NoAudioTrack)?;
    let p = &track.codec_params;
    let sample_rate = p.sample_rate.unwrap_or(44_100);
    let channels = p.channels.map(|c| c.count() as u16).unwrap_or(2);
    let duration = match (p.n_frames, p.time_base) {
        (Some(n), Some(tb)) => {
            let t = tb.calc_time(n);
            Some(t.seconds as f64 + t.frac)
        }
        (Some(n), None) => Some(n as f64 / sample_rate as f64),
        _ => None,
    };
    let bits_per_sample = p.bits_per_sample;
    let mut tags = TrackTags::default();
    let mut artwork = None;
    if let Some(rev) = early {
        apply_revision(&rev, &mut tags, &mut artwork);
    }
    if let Some(rev) = format.metadata().skip_to_latest() {
        apply_revision(rev, &mut tags, &mut artwork);
    }
    if tags.title.is_empty() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        // Common file naming without tags: "Artist - Title".
        match stem.split_once(" - ") {
            Some((artist, title)) if tags.artist.is_empty() && !artist.trim().is_empty() && !title.trim().is_empty() => {
                tags.artist = artist.trim().to_string();
                tags.title = title.trim().to_string();
            }
            _ => tags.title = stem,
        }
    }
    let file_len = std::fs::metadata(path)?.len();
    let bit_rate_kbps = duration
        .filter(|d| *d > 0.0)
        .map(|d| ((file_len as f64 * 8.0) / d / 1000.0).round() as u32);
    Ok(ProbeInfo {
        format: AudioFormat::from_path(path),
        sample_rate,
        channels,
        duration,
        bit_rate_kbps,
        bits_per_sample,
        tags,
        artwork,
    })
}

/// Decodes a whole file into the requested layout at its native sample rate.
pub fn decode(path: &Path, layout: Layout) -> Result<DecodedAudio> {
    decode_with(path, layout, |_| true)
}

/// Like [`decode`], but calls `progress` with the decoded duration in seconds
/// after each packet. Returning `false` cancels decoding.
pub fn decode_with(
    path: &Path,
    layout: Layout,
    mut progress: impl FnMut(f64) -> bool,
) -> Result<DecodedAudio> {
    let (mut format, _) = open(path)?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::NoAudioTrack)?;
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let out_channels: u16 = match layout {
        Layout::Mono => 1,
        Layout::Stereo => 2,
    };
    let mut samples: Vec<f32> = Vec::new();
    if let Some(n) = track.codec_params.n_frames {
        samples.reserve(n as usize * out_channels as usize);
    }
    let mut buf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue, // skip corrupt frame
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let in_ch = spec.channels.count();
        let needed = decoded.capacity() as u64;
        if buf.as_ref().is_none_or(|b| b.capacity() < decoded.capacity() * in_ch) {
            buf = Some(SampleBuffer::new(needed, spec));
        }
        let b = buf.as_mut().unwrap();
        b.copy_interleaved_ref(decoded);
        let s = b.samples();
        match (layout, in_ch) {
            (Layout::Mono, 1) | (Layout::Stereo, 2) => samples.extend_from_slice(s),
            (Layout::Mono, n) => {
                let inv = 1.0 / n as f32;
                samples.extend(s.chunks_exact(n).map(|f| f.iter().sum::<f32>() * inv));
            }
            (Layout::Stereo, 1) => samples.extend(s.iter().flat_map(|&v| [v, v])),
            (Layout::Stereo, n) => {
                // Keep front left/right, drop the rest.
                samples.extend(s.chunks_exact(n).flat_map(|f| [f[0], f[1]]));
            }
        }
        if sample_rate > 0 {
            let secs = samples.len() as f64 / out_channels as f64 / sample_rate as f64;
            if !progress(secs) {
                return Err(DecodeError::Unsupported("cancelled".into()));
            }
        }
    }
    if sample_rate == 0 {
        return Err(DecodeError::NoAudioTrack);
    }
    Ok(DecodedAudio { sample_rate, channels: out_channels, samples })
}

/// BLAKE3 hash of the file contents.
pub fn content_hash(path: &Path) -> io::Result<ContentHash> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(ContentHash(hasher.finalize().to_hex().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, channels: u16, secs: f32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (rate as f32 * secs) as usize;
        for i in 0..n {
            let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 16000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn probe_and_decode_wav() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        write_wav(&p, 44_100, 2, 1.5);
        let info = probe(&p).unwrap();
        assert_eq!(info.sample_rate, 44_100);
        assert_eq!(info.channels, 2);
        assert!((info.duration.unwrap() - 1.5).abs() < 0.01);
        assert_eq!(info.tags.title, "tone");
        let named = dir.path().join("Some Artist - Great Track (Extended Mix).wav");
        write_wav(&named, 8_000, 1, 0.1);
        let t = probe(&named).unwrap().tags;
        assert_eq!((t.artist.as_str(), t.title.as_str()), ("Some Artist", "Great Track (Extended Mix)"));
        assert_eq!(info.format, Some(AudioFormat::Wav));

        let mono = decode(&p, Layout::Mono).unwrap();
        assert_eq!(mono.channels, 1);
        assert_eq!(mono.frames(), 66_150);
        let stereo = decode(&p, Layout::Stereo).unwrap();
        assert_eq!(stereo.samples.len(), 132_300);
        let peak = mono.samples.iter().fold(0f32, |a, &b| a.max(b.abs()));
        assert!((peak - 16000.0 / 32768.0).abs() < 0.01);
    }

    #[test]
    fn mono_to_stereo_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("m.wav");
        write_wav(&p, 48_000, 1, 0.1);
        let s = decode(&p, Layout::Stereo).unwrap();
        assert_eq!(s.samples[2], s.samples[3]);
        assert_eq!(s.sample_rate, 48_000);
    }

    #[test]
    fn hash_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, 8_000, 1, 0.1);
        assert_eq!(content_hash(&p).unwrap(), content_hash(&p).unwrap());
        assert_eq!(content_hash(&p).unwrap().0.len(), 64);
    }
}
