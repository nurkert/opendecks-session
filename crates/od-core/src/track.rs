use crate::{Beatgrid, CueColor, MusicalKey, Phrase, PhraseMood, Waveform};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Where a piece of track data came from. Higher-priority sources are never
/// overwritten by lower ones automatically (manual > imported > analysis).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    Analysis = 0,
    Imported = 1,
    Manual = 2,
}

impl DataSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DataSource::Analysis => "analysis",
            DataSource::Imported => "imported",
            DataSource::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "analysis" => DataSource::Analysis,
            "imported" => DataSource::Imported,
            "manual" => DataSource::Manual,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Mp3,
    Aac,
    Alac,
    Flac,
    Wav,
    Aiff,
    Ogg,
    Opus,
}

impl AudioFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            "mp3" => Self::Mp3,
            "m4a" | "aac" | "mp4" => Self::Aac,
            "alac" => Self::Alac,
            "flac" => Self::Flac,
            "wav" | "wave" => Self::Wav,
            "aif" | "aiff" => Self::Aiff,
            "ogg" | "oga" => Self::Ogg,
            "opus" => Self::Opus,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Aac => "aac",
            Self::Alac => "alac",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "mp3" => Self::Mp3,
            "aac" => Self::Aac,
            "alac" => Self::Alac,
            "flac" => Self::Flac,
            "wav" => Self::Wav,
            "aiff" => Self::Aiff,
            "ogg" => Self::Ogg,
            "opus" => Self::Opus,
            _ => return None,
        })
    }

    /// Whether Pioneer players can play files of this format.
    pub fn pioneer_playable(self) -> bool {
        !matches!(self, Self::Ogg | Self::Opus)
    }

    pub fn is_supported_extension(path: &Path) -> bool {
        Self::from_path(path).is_some()
    }
}

/// Descriptive metadata of a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackTags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub remixer: String,
    pub composer: String,
    pub comment: String,
    pub year: Option<u16>,
    pub track_number: Option<u32>,
    /// 0..=5 stars.
    pub rating: u8,
    pub color: Option<CueColor>,
}

/// Result of analysing a track.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackAnalysis {
    pub analysis_version: u32,
    pub beatgrid: Beatgrid,
    pub key: Option<MusicalKey>,
    pub waveform: Waveform,
    pub phrases: Vec<Phrase>,
    pub phrase_mood: PhraseMood,
    /// Integrated loudness in LUFS.
    pub loudness_lufs: Option<f32>,
    /// Peak sample amplitude, linear.
    pub peak: f32,
    /// Normalised 0..=1 energy over the track in 1-second steps.
    pub energy: Vec<f32>,
}
