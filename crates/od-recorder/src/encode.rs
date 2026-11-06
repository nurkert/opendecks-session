//! Encoding a finished WAV recording to FLAC (lossless, native) or MP3
//! (LAME, 320 kbit/s). Both stream frame by frame, so hour-long mixes never
//! sit in memory.

use crate::{RecordError, Result};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

fn other(e: impl std::fmt::Display) -> RecordError {
    RecordError::Encode(e.to_string())
}

/// `fLaC` plus the STREAMINFO block of `stream` (written before and after
/// the frames; same length both times).
fn flac_header(stream: &flacenc::component::Stream) -> Result<Vec<u8>> {
    use flacenc::component::BitRepr;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(other)?;
    Ok(sink.as_slice().to_vec())
}

/// Encodes `wav` to `flac` without loading the whole file.
pub fn wav_to_flac(wav: &Path, flac: &Path) -> Result<()> {
    use flacenc::component::{BitRepr, Stream};
    use flacenc::error::Verify;
    use flacenc::source::{Context, Fill, FrameBuf};

    let mut reader = hound::WavReader::open(wav)?;
    let spec = reader.spec();
    let (channels, bits, rate) = (spec.channels as usize, spec.bits_per_sample as usize, spec.sample_rate as usize);
    let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, e)| other(e))?;
    let block = config.block_size;
    let mut stream = Stream::new(rate, channels, bits).map_err(other)?;
    stream.stream_info_mut().set_block_sizes(block, block).map_err(other)?;
    let header_len = flac_header(&stream)?.len();

    let mut out = BufWriter::new(File::create(flac)?);
    out.write_all(&vec![0u8; header_len])?;
    let mut buf = (FrameBuf::with_size(channels, block).map_err(other)?, Context::new(bits, channels));
    let mut samples = reader.samples::<i32>();
    let mut chunk: Vec<i32> = Vec::with_capacity(block * channels);
    // Frame numbers count from 0 (the context's counter is already advanced
    // after filling, which strict decoders reject).
    let mut frame_number = 0usize;
    loop {
        chunk.clear();
        for s in samples.by_ref().take(block * channels) {
            chunk.push(s?);
        }
        if chunk.is_empty() {
            break;
        }
        if chunk.len() < block * channels {
            // The last frame carries exactly the remaining samples.
            buf.0 = FrameBuf::with_size(channels, chunk.len() / channels).map_err(other)?;
        }
        buf.fill_interleaved(&chunk).map_err(other)?;
        let frame = flacenc::encode_fixed_size_frame(&config, &buf.0, frame_number, stream.stream_info()).map_err(other)?;
        frame_number += 1;
        stream.stream_info_mut().update_frame_info(&frame);
        let mut sink = flacenc::bitsink::ByteSink::new();
        frame.write(&mut sink).map_err(other)?;
        out.write_all(sink.as_slice())?;
    }
    // A fixed-block-size stream: STREAMINFO's minimum excludes the shorter
    // last block (FLAC format spec), otherwise decoders read the frame
    // numbers as sample numbers.
    if frame_number > 1 {
        stream.stream_info_mut().set_block_sizes(block, block).map_err(other)?;
    }
    stream.stream_info_mut().set_md5_digest(&buf.1.md5_digest());
    stream.stream_info_mut().set_total_samples(buf.1.total_samples());
    // Rewrite the header with the final sizes, sample count and MD5.
    let header = flac_header(&stream)?;
    debug_assert_eq!(header.len(), header_len);
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&header)?;
    out.flush()?;
    Ok(())
}

/// Encodes `wav` to a 320 kbit/s MP3.
pub fn wav_to_mp3(wav: &Path, mp3: &Path) -> Result<()> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm, Quality};

    let mut reader = hound::WavReader::open(wav)?;
    let spec = reader.spec();
    let mut b = Builder::new().ok_or_else(|| other("LAME could not start"))?;
    b.set_num_channels(spec.channels as u8).map_err(other)?;
    b.set_sample_rate(spec.sample_rate).map_err(other)?;
    b.set_brate(Bitrate::Kbps320).map_err(other)?;
    b.set_quality(Quality::Best).map_err(other)?;
    let mut enc = b.build().map_err(other)?;
    let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
    let mut out = BufWriter::new(File::create(mp3)?);
    let mut samples = reader.samples::<i32>();
    let mut pcm: Vec<f32> = Vec::with_capacity(8192);
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        pcm.clear();
        for s in samples.by_ref().take(8192) {
            pcm.push(s? as f32 * scale);
        }
        if pcm.is_empty() {
            break;
        }
        bytes.clear();
        bytes.reserve(mp3lame_encoder::max_required_buffer_size(pcm.len() / spec.channels as usize));
        enc.encode_to_vec(InterleavedPcm(pcm.as_slice()), &mut bytes).map_err(other)?;
        out.write_all(&bytes)?;
    }
    bytes.clear();
    bytes.reserve(7200);
    enc.flush_to_vec::<FlushNoGap>(&mut bytes).map_err(other)?;
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(())
}

/// Automatic start / stop from the master level (SPEC §11): starts after
/// `start_after` seconds above the threshold, stops after `stop_after`
/// seconds below it.
#[derive(Clone, Debug)]
pub struct SilenceDetector {
    pub threshold_db: f32,
    pub start_after: f32,
    pub stop_after: f32,
    loud: f32,
    quiet: f32,
}

/// What the detector asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoAction {
    Start,
    Stop,
}

impl SilenceDetector {
    pub fn new(threshold_db: f32, start_after: f32, stop_after: f32) -> Self {
        Self { threshold_db, start_after, stop_after, loud: 0.0, quiet: 0.0 }
    }

    /// Feeds `dt` seconds at `level_db`; `recording` says whether a
    /// recording runs. Returns an action when one is due.
    pub fn update(&mut self, level_db: f32, dt: f32, recording: bool) -> Option<AutoAction> {
        if level_db >= self.threshold_db {
            self.loud += dt;
            self.quiet = 0.0;
        } else {
            self.quiet += dt;
            self.loud = 0.0;
        }
        match recording {
            false if self.loud >= self.start_after => {
                self.loud = 0.0;
                Some(AutoAction::Start)
            }
            true if self.quiet >= self.stop_after => {
                self.quiet = 0.0;
                Some(AutoAction::Stop)
            }
            _ => None,
        }
    }
}
