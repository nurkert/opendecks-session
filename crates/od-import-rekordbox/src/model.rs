//! Source-neutral description of what is imported. Both the rekordbox
//! library reader and the USB stick reader produce a [`SrcLibrary`]; the
//! importer only works on this model.

use od_core::{Beatgrid, Cue, MusicalKey, Phrase, PhraseMood, TrackTags, Waveform};
use od_format_pioneer::anlz::AnlzFile;
use std::path::PathBuf;

/// Fields only needed to evaluate rekordbox smart playlists.
#[derive(Clone, Debug, Default)]
pub(crate) struct SmartFields {
    pub original_artist: String,
    pub key_name: String,
    pub file_name: String,
    pub stock_date: String,
    pub date_created: String,
    pub release_date: String,
    pub play_count: i64,
    pub color_id: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct SrcTrack {
    /// Id in the source database (only used to link playlist entries).
    pub src_id: String,
    pub path: PathBuf,
    pub tags: TrackTags,
    pub duration: f64,
    pub sample_rate: u32,
    pub bit_rate: Option<u32>,
    pub bits_per_sample: Option<u32>,
    pub file_size: u64,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    /// Cues from the database (`djmdCue`). `None`: take them from ANLZ.
    pub db_cues: Option<Vec<Cue>>,
    /// Analysis files in the order `.DAT`, `.EXT`, `.2EX` (any may be absent).
    pub anlz: Vec<PathBuf>,
    pub smart: SmartFields,
}

#[derive(Clone, Debug)]
pub(crate) struct SrcNode {
    pub id: String,
    pub parent: Option<String>,
    pub name: String,
    pub folder: bool,
    pub seq: i64,
    /// Source track ids in order.
    pub tracks: Vec<String>,
    /// Imported from a smart playlist (contents evaluated at import time).
    pub was_smart: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SrcLibrary {
    pub tracks: Vec<SrcTrack>,
    pub playlists: Vec<SrcNode>,
    pub history: Vec<SrcNode>,
    /// Cue count known without parsing ANLZ files (database cues).
    pub db_cue_count: usize,
    pub notes: Vec<String>,
    pub skipped: Vec<(String, String)>,
}

/// What the ANLZ files of a track contain.
#[derive(Clone, Debug, Default)]
pub(crate) struct AnlzData {
    pub grid: Option<Beatgrid>,
    pub phrases: Option<(Vec<Phrase>, PhraseMood)>,
    pub waveform: Option<Waveform>,
    pub cues: Vec<Cue>,
}

/// Reads whichever of the analysis files exist. Unreadable files are
/// ignored (logged).
pub(crate) fn load_anlz(paths: &[PathBuf]) -> AnlzData {
    let files: Vec<AnlzFile> = paths
        .iter()
        .filter_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            match AnlzFile::parse(&bytes) {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!("cannot parse analysis file {}: {e}", p.display());
                    None
                }
            }
        })
        .collect();
    let mut out = AnlzData {
        grid: files.iter().find_map(|f| f.beatgrid()).filter(|g| !g.is_empty()),
        phrases: files.iter().find_map(|f| f.phrases()).filter(|(p, _)| !p.is_empty()),
        // The later files carry the more detailed waveforms (PWV7 > PWV5 > PWV3).
        waveform: files.iter().rev().find_map(|f| f.waveform()).filter(|w| !w.columns.is_empty()),
        cues: Vec::new(),
    };
    // `.EXT` cue lists (PCO2, with colours and comments) win over `.DAT`.
    let with_ext = files.iter().find(|f| f.tag(b"PCO2").is_some());
    if let Some(f) = with_ext.or_else(|| files.iter().find(|f| f.tag(b"PCOB").is_some())) {
        out.cues = f.cues();
    }
    out
}

/// `.DAT` path plus its `.EXT` / `.2EX` siblings.
pub(crate) fn anlz_family(dat: PathBuf) -> Vec<PathBuf> {
    let ext = dat.with_extension("EXT");
    let ex2 = dat.with_extension("2EX");
    vec![dat, ext, ex2]
}
