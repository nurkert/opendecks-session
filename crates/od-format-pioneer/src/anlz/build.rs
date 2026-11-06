//! Generation of the `.DAT`, `.EXT` and `.2EX` files for one track.
//!
//! Tag order and fixed header values follow files written by rekordbox
//! (documented by Deep Symmetry and observed in the rekordbox demo export
//! shipped as test data with rekordcrate):
//!
//! * `.DAT`: `PPTH PVBR PQTZ PWAV PWV2 PCOB(hot) PCOB(memory)`
//! * `.EXT`: `PPTH PWV3 PCOB PCOB PCO2(hot) PCO2(memory) PQT2 PWV5 PWV4 PSSI`
//!   (rekordbox leaves the `PCOB` lists of the `.EXT` empty)
//! * `.2EX`: `PPTH PWV7 PWV6 PWVC`
//!
//! Waveform scaling: `od_core` waveform values are 0..=255 peaks. Pioneer
//! heights are 5 bit (`PWAV`, `PWV3`, `PWV5`), 4 bit (`PWV2`) or roughly
//! 7 bit (`PWV4`, `PWV6`, `PWV7`). The exact curves rekordbox uses are not
//! documented; the mapping here is linear.

use super::*;

/// Everything needed to write the analysis files of a track.
#[derive(Clone, Copy, Debug)]
pub struct AnlzInput<'a> {
    /// USB-relative audio path, e.g. `/Contents/Artist/Album/file.mp3`.
    pub device_path: &'a str,
    /// Track length in seconds.
    pub duration: f64,
    pub beatgrid: &'a Beatgrid,
    pub cues: &'a [Cue],
    pub waveform: &'a Waveform,
    pub phrases: &'a [Phrase],
    pub phrase_mood: PhraseMood,
    /// MP3 seek index for `PVBR`: 400 byte offsets of the audio frames at
    /// 400 equally spaced times, optionally followed by a 401st value, the
    /// total number of samples (frames × 1152). `None` writes zeros.
    pub is_mp3_vbr_index: Option<&'a [u32]>,
}

/// The three analysis files of a track.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnlzSet {
    pub dat: Vec<u8>,
    pub ext: Vec<u8>,
    pub ex2: Vec<u8>,
}

/// Builds the `.DAT`, `.EXT` and `.2EX` files for a track.
pub fn build_anlz(input: &AnlzInput) -> AnlzSet {
    let path = Tag::Path(input.device_path.to_string());
    let wf = Waves::new(input.waveform, input.duration);

    let mut hot: Vec<&Cue> = input
        .cues
        .iter()
        .filter(|c| matches!(c.kind, CueKind::Hot(_)))
        .collect();
    let mut mem: Vec<&Cue> = input
        .cues
        .iter()
        .filter(|c| c.kind == CueKind::Memory)
        .collect();
    hot.sort_by_key(|c| match c.kind {
        CueKind::Hot(n) => n,
        CueKind::Memory => 0,
    });
    mem.sort_by(|a, b| a.start.total_cmp(&b.start));
    let bpm_at = |t: f64| {
        input
            .beatgrid
            .beat_index_at(t)
            .or((!input.beatgrid.is_empty()).then_some(0))
            .map(|i| input.beatgrid.beats[i].bpm)
    };

    let dat = AnlzFile::new(vec![
        path.clone(),
        vbr_tag(input.is_mp3_vbr_index),
        beatgrid_tag(input.beatgrid),
        Tag::WavePreview {
            unknown: 0x0001_0000,
            data: wf.pwav(),
        },
        Tag::WaveTiny {
            unknown: 0x0001_0000,
            data: wf.pwv2(),
        },
        pcob(CueListType::Hot, &hot),
        pcob(CueListType::Memory, &mem),
    ]);

    let mut ext_tags = vec![
        path.clone(),
        Tag::WaveDetail {
            unknown: 0x0096_0000,
            data: wf.pwv3(),
        },
        pcob(CueListType::Hot, &[]),
        pcob(CueListType::Memory, &[]),
        pco2(CueListType::Hot, &hot, &bpm_at),
        pco2(CueListType::Memory, &mem, &bpm_at),
        empty_pqt2(),
        Tag::WaveColorDetail {
            unknown: 0x0096_0305,
            data: wf.pwv5(),
        },
        Tag::WaveColorPreview {
            unknown: 0,
            data: wf.pwv4(),
        },
    ];
    if !input.phrases.is_empty() {
        ext_tags.push(Tag::SongStructure(song_structure(
            input.phrases,
            input.phrase_mood,
        )));
    }
    let ext = AnlzFile::new(ext_tags);

    let ex2 = AnlzFile::new(vec![
        path,
        Tag::Wave3BandDetail {
            unknown: 0x0096_0000,
            data: wf.pwv7(),
        },
        Tag::Wave3BandPreview { data: wf.pwv6() },
        // Thresholds as seen in rekordbox files; their derivation is unknown.
        Tag::VocalConfig {
            reserved: 0,
            low: 0x50,
            mid: 0x59,
            high: 0x66,
        },
    ]);

    AnlzSet {
        dat: dat.to_bytes(),
        ext: ext.to_bytes(),
        ex2: ex2.to_bytes(),
    }
}

fn ms(seconds: f64) -> u32 {
    (seconds.max(0.0) * 1000.0)
        .round()
        .min(u32::MAX as f64 - 1.0) as u32
}

fn vbr_tag(index: Option<&[u32]>) -> Tag {
    let mut entries = vec![0u32; 401];
    if let Some(ix) = index {
        for (d, s) in entries.iter_mut().zip(ix.iter()) {
            *d = *s;
        }
    }
    Tag::Vbr {
        unknown: 0,
        entries,
    }
}

fn beatgrid_tag(grid: &Beatgrid) -> Tag {
    Tag::BeatGrid {
        unknown1: 0,
        unknown2: 0x0008_0000,
        beats: grid
            .beats
            .iter()
            .map(|b| PqtzBeat {
                beat_number: b.bar_position.clamp(1, 4) as u16,
                tempo: (b.bpm * 100.0).round().clamp(0.0, u16::MAX as f64) as u16,
                time: ms(b.time),
            })
            .collect(),
    }
}

/// `PQT2` as rekordbox writes it without an extended grid.
fn empty_pqt2() -> Tag {
    let mut header = vec![0u8; 44];
    header[4..8].copy_from_slice(&[0x01, 0x00, 0x00, 0x02]);
    Tag::Unknown {
        fourcc: *b"PQT2",
        header,
        body: Vec::new(),
    }
}

fn hot_number(c: &Cue) -> u32 {
    match c.kind {
        CueKind::Hot(n) => n as u32 + 1,
        CueKind::Memory => 0,
    }
}

fn cue_type_and_loop(c: &Cue) -> (u8, u32) {
    match c.loop_end {
        Some(end) if end > c.start => (2, ms(end)),
        _ => (1, NO_LOOP),
    }
}

fn pcob(list: CueListType, cues: &[&Cue]) -> Tag {
    let n = cues.len();
    let memory_count = if list == CueListType::Memory && n > 0 {
        n as u32 - 1
    } else {
        0xffff_ffff
    };
    let cues = cues
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (cue_type, loop_time) = cue_type_and_loop(c);
            // Memory cues are chained (previous / next index); hot cues are not.
            let (order_first, order_last) = if list == CueListType::Hot {
                (0xffff, 0xffff)
            } else {
                (
                    if i == 0 { 0xffff } else { (i - 1) as u16 },
                    if i + 1 == n { 0xffff } else { (i + 1) as u16 },
                )
            };
            Pcpt {
                len_header: 0x1c,
                hot_cue: hot_number(c),
                status: 0,
                unknown1: 0x0001_0000,
                order_first,
                order_last,
                cue_type,
                unknown2: [0x00, 0x03, 0xe8],
                time: ms(c.start),
                loop_time,
                tail: vec![0; 16],
            }
        })
        .collect();
    Tag::CueList {
        list_type: list,
        unknown: 0,
        memory_count,
        cues,
    }
}

fn pco2(list: CueListType, cues: &[&Cue], bpm_at: &dyn Fn(f64) -> Option<f64>) -> Tag {
    let cues = cues
        .iter()
        .map(|c| {
            let (cue_type, loop_time) = cue_type_and_loop(c);
            let (loop_numerator, loop_denominator) = match (cue_type, bpm_at(c.start)) {
                (2, Some(bpm)) if bpm > 0.0 => {
                    loop_fraction((c.loop_end.unwrap() - c.start) * bpm / 60.0)
                }
                _ => (0, 0),
            };
            let comment_raw = if c.label.is_empty() {
                Vec::new()
            } else {
                utf16be_nul(&c.label)
            };
            let is_hot = matches!(c.kind, CueKind::Hot(_));
            let hot_color = match (is_hot, c.color) {
                (true, Some(col)) => {
                    let code = palette::hot_cue_code(col);
                    let p = palette::hot_cue_color(code).unwrap();
                    [code, p.r, p.g, p.b]
                }
                _ => [0; 4],
            };
            let color_id = match (is_hot, c.color) {
                (false, Some(col)) => palette::track_color_id(col),
                _ => 0,
            };
            Pcp2 {
                len_header: 0x10,
                hot_cue: hot_number(c),
                cue_type,
                unknown1: [0x00, 0x03, 0xe8],
                time: ms(c.start),
                loop_time,
                color_id,
                unknown2: [0x01, 0, 0, 0, 0, 0, 0],
                loop_numerator,
                loop_denominator,
                comment_raw: Some(comment_raw),
                hot_color: Some(hot_color),
                trailing: vec![0; 40],
            }
        })
        .collect();
    Tag::ExtCueList {
        list_type: list,
        unknown: 0,
        cues,
    }
}

/// Quantized loop length as a power-of-two fraction of beats, or `(0, 0)`.
fn loop_fraction(beats: f64) -> (u16, u16) {
    if beats <= 0.0 {
        return (0, 0);
    }
    let l = beats.log2();
    let r = l.round();
    if (l - r).abs() > 0.01 || !(-5.0..=9.0).contains(&r) {
        return (0, 0);
    }
    if r >= 0.0 {
        (2u16.pow(r as u32), 1)
    } else {
        (1, 2u16.pow((-r) as u32))
    }
}

fn song_structure(phrases: &[Phrase], mood: PhraseMood) -> SongStructure {
    let mut sorted: Vec<&Phrase> = phrases.iter().collect();
    sorted.sort_by_key(|p| p.start_beat);
    let entries = sorted
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut rest = [0u8; 18];
            if mood == PhraseMood::High {
                // k1 = 1 selects the "1" variant of intro, chorus and outro;
                // k2 = k3 = 0 selects "Up 1".
                if matches!(
                    p.kind,
                    PhraseKind::Intro | PhraseKind::Chorus | PhraseKind::Outro
                ) {
                    rest[1] = 1;
                }
            }
            PssiEntry {
                index: i as u16 + 1,
                beat: (p.start_beat + 1).min(u16::MAX as u32) as u16,
                kind: phrase_id(mood, p.kind),
                rest,
            }
        })
        .collect();
    let end = sorted.iter().map(|p| p.end_beat).max().unwrap_or(0);
    SongStructure {
        len_entry_bytes: 24,
        masked: true,
        mood: match mood {
            PhraseMood::High => 1,
            PhraseMood::Mid => 2,
            PhraseMood::Low => 3,
        },
        unknown1: [0; 6],
        end_beat: (end + 1).min(u16::MAX as u32) as u16,
        unknown2: 0x0100,
        bank: 0,
        unknown3: 0,
        entries,
    }
}

/// Waveform resampled to the Pioneer resolutions.
struct Waves {
    detail: Vec<WaveformColumn>,
}

impl Waves {
    fn new(w: &Waveform, duration: f64) -> Self {
        let n = (duration.max(0.0) * Waveform::DETAIL_RATE).round() as usize;
        let detail = if w.columns.is_empty() || w.columns_per_second <= 0.0 {
            vec![WaveformColumn::default(); n]
        } else if (w.columns_per_second - Waveform::DETAIL_RATE).abs() < 1e-9 {
            let mut d = w.columns.clone();
            d.resize(n, WaveformColumn::default());
            d
        } else {
            // Maximum over the source columns covered by each detail column.
            let ratio = w.columns_per_second / Waveform::DETAIL_RATE;
            (0..n)
                .map(|i| {
                    let a = ((i as f64) * ratio).floor() as usize;
                    let b = (((i + 1) as f64) * ratio).ceil() as usize;
                    let b = b.max(a + 1).min(w.columns.len());
                    w.columns
                        .get(a..b)
                        .unwrap_or(&[])
                        .iter()
                        .fold(WaveformColumn::default(), max_col)
                })
                .collect()
        };
        Self { detail }
    }

    fn overview(&self, width: usize) -> Vec<WaveformColumn> {
        if self.detail.is_empty() {
            return vec![WaveformColumn::default(); width];
        }
        Waveform {
            columns_per_second: Waveform::DETAIL_RATE,
            columns: self.detail.clone(),
        }
        .overview(width)
    }

    /// 5 bit height, 3 bit whiteness (share of high frequencies).
    fn mono(c: &WaveformColumn) -> u8 {
        let height = scale(c.peak, 31);
        let white = if c.peak == 0 {
            0
        } else {
            (c.high as u32 * 7 / c.peak.max(1) as u32).min(7) as u8
        };
        height | (white << 5)
    }

    fn pwav(&self) -> Vec<u8> {
        self.overview(400).iter().map(Self::mono).collect()
    }

    fn pwv2(&self) -> Vec<u8> {
        self.overview(100)
            .iter()
            .map(|c| scale(c.peak, 15))
            .collect()
    }

    fn pwv3(&self) -> Vec<u8> {
        self.detail.iter().map(Self::mono).collect()
    }

    /// 3 bit red (low), green (mid), blue (high), 5 bit height, 2 unused bits.
    fn pwv5(&self) -> Vec<u16> {
        self.detail
            .iter()
            .map(|c| {
                let r = scale(c.low, 7) as u16;
                let g = scale(c.mid, 7) as u16;
                let b = scale(c.high, 7) as u16;
                let h = scale(c.peak, 31) as u16;
                (r << 13) | (g << 10) | (b << 7) | (h << 2)
            })
            .collect()
    }

    /// Bytes 0 and 1 drive the "whiteness" of the blue preview, byte 2 is the
    /// energy below ~10 kHz, bytes 3..=5 the low, mid and high bands.
    fn pwv4(&self) -> Vec<[u8; 6]> {
        self.overview(1200)
            .iter()
            .map(|c| {
                let white = scale(c.high, 127);
                [
                    white,
                    scale(c.peak, 127),
                    scale(c.low.max(c.mid), 127),
                    scale(c.low, 127),
                    scale(c.mid, 127),
                    scale(c.high, 127),
                ]
            })
            .collect()
    }

    fn pwv6(&self) -> Vec<[u8; 3]> {
        self.overview(1200).iter().map(band3).collect()
    }

    fn pwv7(&self) -> Vec<[u8; 3]> {
        self.detail.iter().map(band3).collect()
    }
}

/// Byte order low, mid, high as modelled by rekordcrate. The Deep Symmetry
/// text describes the order as mid, high, low; real files (third byte
/// smallest on typical dance music) support rekordcrate's reading. To be
/// confirmed on a CDJ-3000.
fn band3(c: &WaveformColumn) -> [u8; 3] {
    [scale(c.low, 127), scale(c.mid, 127), scale(c.high, 127)]
}

fn scale(v: u8, max: u8) -> u8 {
    ((v as u32 * max as u32 + 127) / 255) as u8
}

fn max_col(a: WaveformColumn, c: &WaveformColumn) -> WaveformColumn {
    WaveformColumn {
        low: a.low.max(c.low),
        mid: a.mid.max(c.mid),
        high: a.high.max(c.high),
        peak: a.peak.max(c.peak),
    }
}
