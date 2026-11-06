use super::*;
use od_core::CueColor;

pub(crate) fn fixture_waveform(seconds: f64) -> Waveform {
    let n = (seconds * Waveform::DETAIL_RATE) as usize;
    Waveform {
        columns_per_second: Waveform::DETAIL_RATE,
        columns: (0..n)
            .map(|i| {
                let v = (i * 7 % 256) as u8;
                WaveformColumn {
                    low: v,
                    mid: v / 2,
                    high: v / 4,
                    peak: v,
                }
            })
            .collect(),
    }
}

pub(crate) fn fixture_cues() -> Vec<Cue> {
    vec![
        Cue {
            label: "Drop".into(),
            color: Some(HOT_CUE_PALETTE_FIRST),
            ..Cue::hot(0, 1.5)
        },
        Cue {
            loop_end: Some(9.25),
            ..Cue::hot(2, 5.5)
        },
        Cue {
            label: "Ünïcödé ✓".into(),
            ..Cue::memory(30.25)
        },
        Cue {
            loop_end: Some(12.0),
            color: Some(crate::TRACK_COLORS[1].1),
            ..Cue::memory(10.0)
        },
    ]
}

const HOT_CUE_PALETTE_FIRST: CueColor = crate::HOT_CUE_PALETTE[21];

fn build_fixture() -> (AnlzSet, Beatgrid, Vec<Cue>, Vec<Phrase>) {
    let grid = Beatgrid::constant(128.0, 0.12, 1, 60.0);
    let cues = fixture_cues();
    let phrases = vec![
        Phrase {
            start_beat: 0,
            end_beat: 32,
            kind: PhraseKind::Intro,
        },
        Phrase {
            start_beat: 32,
            end_beat: 64,
            kind: PhraseKind::Chorus,
        },
        Phrase {
            start_beat: 64,
            end_beat: 96,
            kind: PhraseKind::Outro,
        },
    ];
    let wf = fixture_waveform(60.0);
    let vbr: Vec<u32> = (0..401).map(|i| i * 1000).collect();
    let set = build_anlz(&AnlzInput {
        device_path: "/Contents/Ärtist/Album/track.mp3",
        duration: 60.0,
        beatgrid: &grid,
        cues: &cues,
        waveform: &wf,
        phrases: &phrases,
        phrase_mood: PhraseMood::High,
        is_mp3_vbr_index: Some(&vbr),
    });
    (set, grid, cues, phrases)
}

fn tags(f: &AnlzFile) -> Vec<String> {
    f.tags.iter().map(Tag::fourcc_str).collect()
}

#[test]
fn files_have_rekordbox_tag_order() {
    let (set, ..) = build_fixture();
    let dat = AnlzFile::parse(&set.dat).unwrap();
    let ext = AnlzFile::parse(&set.ext).unwrap();
    let ex2 = AnlzFile::parse(&set.ex2).unwrap();
    assert_eq!(
        tags(&dat),
        ["PPTH", "PVBR", "PQTZ", "PWAV", "PWV2", "PCOB", "PCOB"]
    );
    assert_eq!(
        tags(&ext),
        [
            "PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PQT2", "PWV5", "PWV4", "PSSI"
        ]
    );
    assert_eq!(tags(&ex2), ["PPTH", "PWV7", "PWV6", "PWVC"]);
    for f in [&dat, &ext, &ex2] {
        assert!(
            !f.tags
                .iter()
                .any(|t| matches!(t, Tag::Unknown { fourcc, .. } if fourcc != b"PQT2"))
        );
    }
    // File header as rekordbox writes it.
    assert_eq!(&set.dat[..4], b"PMAI");
    assert_eq!(&set.dat[4..8], &[0, 0, 0, 0x1c]);
    assert_eq!(
        u32::from_be_bytes(set.dat[8..12].try_into().unwrap()) as usize,
        set.dat.len()
    );
    assert_eq!(&set.dat[12..28], &DEFAULT_HEADER_TAIL);
}

#[test]
fn round_trip_every_tag() {
    let (set, grid, cues, phrases) = build_fixture();
    for bytes in [&set.dat, &set.ext, &set.ex2] {
        let f = AnlzFile::parse(bytes).unwrap();
        assert_eq!(&f.to_bytes(), bytes, "re-serialization must be identical");
        assert_eq!(
            f.path().as_deref(),
            Some("/Contents/Ärtist/Album/track.mp3")
        );
    }
    let dat = AnlzFile::parse(&set.dat).unwrap();
    let ext = AnlzFile::parse(&set.ext).unwrap();
    let ex2 = AnlzFile::parse(&set.ex2).unwrap();

    // PQTZ
    let g = dat.beatgrid().unwrap();
    assert_eq!(g.beats.len(), grid.beats.len());
    for (a, b) in g.beats.iter().zip(&grid.beats) {
        assert!((a.time - b.time).abs() <= 0.0005 + 1e-9);
        assert_eq!(a.bpm, 128.0);
        assert_eq!(a.bar_position, b.bar_position);
    }

    // PCO2 (EXT) carries everything, PCOB (DAT) positions only.
    let ext_cues = ext.cues();
    assert_eq!(ext_cues.len(), cues.len());
    let find = |list: &[Cue], kind: CueKind, start: f64| {
        list.iter()
            .find(|c| c.kind == kind && (c.start - start).abs() < 1e-9)
            .cloned()
            .unwrap()
    };
    for c in &cues {
        let got = find(&ext_cues, c.kind, c.start);
        assert_eq!(got.loop_end, c.loop_end);
        assert_eq!(got.label, c.label);
        assert_eq!(got.color, c.color, "{c:?}");
        let got = find(&dat.cues(), c.kind, c.start);
        assert_eq!(got.loop_end, c.loop_end);
    }
    // Memory cues are chained in time order in PCOB.
    let Some(Tag::CueList {
        cues: mem,
        memory_count,
        ..
    }) = dat.tags.iter().find(|t| {
        matches!(
            t,
            Tag::CueList {
                list_type: CueListType::Memory,
                ..
            }
        )
    })
    else {
        panic!()
    };
    assert_eq!(*memory_count, 1);
    assert_eq!((mem[0].order_first, mem[0].order_last), (0xffff, 1));
    assert_eq!((mem[1].order_first, mem[1].order_last), (0, 0xffff));
    // A 4 beat loop at 128 BPM is quantized.
    let Some(Tag::ExtCueList { cues: hot, .. }) = ext.tag(b"PCO2") else {
        panic!()
    };
    let lp = hot.iter().find(|c| c.cue_type == 2).unwrap();
    assert_eq!((lp.loop_numerator, lp.loop_denominator), (8, 1));
    assert_eq!(hot[0].comment(), "Drop");
    assert_eq!(hot[0].hot_color.unwrap()[0], 22);

    // PSSI
    let (p, mood) = ext.phrases().unwrap();
    assert_eq!(mood, PhraseMood::High);
    assert_eq!(p, phrases);
    let Some(Tag::SongStructure(s)) = ext.tag(b"PSSI") else {
        panic!()
    };
    assert!(s.masked);
    assert_eq!(s.entries[0].k1(), 1);
    // The masked mood bytes must not look like a valid mood.
    let at = set.ext.windows(4).position(|w| w == b"PSSI").unwrap();
    let raw_mood = u16::from_be_bytes([set.ext[at + 18], set.ext[at + 19]]);
    assert!(raw_mood > 20);

    // PVBR
    let Some(Tag::Vbr { entries, .. }) = dat.tag(b"PVBR") else {
        panic!()
    };
    assert_eq!(entries.len(), 401);
    assert_eq!(entries[400], 400_000);

    // Waveforms: sizes and a decoded detail close to the input.
    let size = |f: &AnlzFile, cc: &[u8; 4]| match f.tag(cc).unwrap() {
        Tag::WavePreview { data, .. }
        | Tag::WaveTiny { data, .. }
        | Tag::WaveDetail { data, .. } => data.len(),
        Tag::WaveColorPreview { data, .. } => data.len(),
        Tag::WaveColorDetail { data, .. } => data.len(),
        Tag::Wave3BandPreview { data } => data.len(),
        Tag::Wave3BandDetail { data, .. } => data.len(),
        _ => unreachable!(),
    };
    assert_eq!(size(&dat, b"PWAV"), 400);
    assert_eq!(size(&dat, b"PWV2"), 100);
    assert_eq!(size(&ext, b"PWV3"), 9000);
    assert_eq!(size(&ext, b"PWV5"), 9000);
    assert_eq!(size(&ext, b"PWV4"), 1200);
    assert_eq!(size(&ex2, b"PWV6"), 1200);
    assert_eq!(size(&ex2, b"PWV7"), 9000);
    let w = ex2.waveform().unwrap();
    let src = fixture_waveform(60.0);
    assert_eq!(w.columns.len(), src.columns.len());
    for (a, b) in w.columns.iter().zip(&src.columns) {
        assert!((a.low as i32 - b.low as i32).abs() <= 2);
    }
    assert!(ext.waveform().is_some());
    assert!(matches!(ex2.tag(b"PWVC"), Some(Tag::VocalConfig { .. })));
}

#[test]
fn unknown_tags_survive() {
    let mut f = AnlzFile::new(vec![
        Tag::Path("/x.mp3".into()),
        Tag::Unknown {
            fourcc: *b"PXYZ",
            header: vec![1, 2, 3],
            body: vec![9; 5],
        },
    ]);
    f.header_tail = vec![0xaa; 16];
    let b = f.to_bytes();
    assert_eq!(AnlzFile::parse(&b).unwrap(), f);
}

#[test]
fn truncated_cue_entries_parse() {
    // A PCP2 entry that ends right after the loop fraction.
    let mut e = Vec::new();
    e.extend_from_slice(b"PCP2");
    e.extend_from_slice(&0x10u32.to_be_bytes());
    e.extend_from_slice(&0x28u32.to_be_bytes());
    e.extend_from_slice(&1u32.to_be_bytes());
    e.extend_from_slice(&[1, 0, 3, 0xe8]);
    e.extend_from_slice(&1234u32.to_be_bytes());
    e.extend_from_slice(&NO_LOOP.to_be_bytes());
    e.extend_from_slice(&[0; 12]);
    let f = AnlzFile::new(vec![Tag::Unknown {
        fourcc: *b"PCO2",
        header: vec![0, 0, 0, 1, 0, 1, 0, 0],
        body: e,
    }]);
    let parsed = AnlzFile::parse(&f.to_bytes()).unwrap();
    assert!(matches!(parsed.tags[0], Tag::ExtCueList { .. }));
    let c = parsed.cues();
    assert_eq!(c[0].kind, CueKind::Hot(0));
    assert_eq!(c[0].start, 1.234);
    assert_eq!(parsed.to_bytes(), f.to_bytes());
}

#[test]
fn rejects_garbage() {
    assert!(AnlzFile::parse(b"").is_err());
    assert!(AnlzFile::parse(b"XXXX\0\0\0\x1c\0\0\0\x1c").is_err());
    let mut b = build_fixture().0.dat;
    b.truncate(b.len() - 3);
    assert!(AnlzFile::parse(&b).is_err());
}

#[test]
fn empty_track_builds() {
    let set = build_anlz(&AnlzInput {
        device_path: "/Contents/a.wav",
        duration: 0.0,
        beatgrid: &Beatgrid::default(),
        cues: &[],
        waveform: &Waveform::default(),
        phrases: &[],
        phrase_mood: PhraseMood::Mid,
        is_mp3_vbr_index: None,
    });
    let ext = AnlzFile::parse(&set.ext).unwrap();
    assert!(ext.phrases().is_none());
    assert!(ext.cues().is_empty());
    assert_eq!(
        AnlzFile::parse(&set.dat)
            .unwrap()
            .beatgrid()
            .unwrap()
            .beats
            .len(),
        0
    );
}

#[test]
fn resamples_other_waveform_rates() {
    let src = Waveform {
        columns_per_second: 50.0,
        columns: vec![
            WaveformColumn {
                low: 255,
                mid: 0,
                high: 0,
                peak: 255
            };
            500
        ],
    };
    let set = build_anlz(&AnlzInput {
        device_path: "/a.flac",
        duration: 10.0,
        beatgrid: &Beatgrid::default(),
        cues: &[],
        waveform: &src,
        phrases: &[],
        phrase_mood: PhraseMood::Mid,
        is_mp3_vbr_index: None,
    });
    let w = AnlzFile::parse(&set.ex2).unwrap().waveform().unwrap();
    assert_eq!(w.columns.len(), 1500);
    assert!(w.columns.iter().all(|c| c.low == 255));
}

/// Real rekordbox files: set `OD_PIONEER_ANLZ_SAMPLES` to a directory that
/// contains `ANLZ*.DAT/.EXT/.2EX` files (searched recursively).
#[test]
fn real_samples() {
    let Ok(dir) = std::env::var("OD_PIONEER_ANLZ_SAMPLES") else {
        eprintln!("OD_PIONEER_ANLZ_SAMPLES not set, skipping");
        return;
    };
    let mut files = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().unwrap().to_string_lossy().starts_with("ANLZ")
                && matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("DAT" | "EXT" | "2EX")
                )
            {
                files.push(p);
            }
        }
    }
    assert!(!files.is_empty(), "no samples found");
    let mut grids = 0;
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        let f = AnlzFile::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        assert_eq!(f.to_bytes(), bytes, "{} does not round trip", p.display());
        assert!(f.path().is_some(), "{}", p.display());
        let unknown: Vec<String> = f
            .tags
            .iter()
            .filter(|t| matches!(t, Tag::Unknown { .. }))
            .map(Tag::fourcc_str)
            .collect();
        assert!(
            unknown.iter().all(|t| t == "PQT2"),
            "{}: unparsed tags {unknown:?}",
            p.display()
        );
        if let Some(g) = f.beatgrid().filter(|g| !g.is_empty()) {
            grids += 1;
            for w in g.beats.windows(2) {
                assert!(w[1].time > w[0].time);
                assert_eq!(w[1].bar_position, w[0].bar_position % 4 + 1);
            }
            assert!(g.beats.iter().all(|b| (40.0..=250.0).contains(&b.bpm)));
        }
        if let Some((phrases, _)) = f.phrases() {
            assert!(!phrases.is_empty());
            for w in phrases.windows(2) {
                assert!(w[0].start_beat < w[1].start_beat);
                assert_eq!(w[0].end_beat, w[1].start_beat);
            }
        }
        for c in f.cues() {
            assert!(c.start >= 0.0 && c.start < 7200.0);
        }
        // Rebuilding from the decoded data gives a file that parses to the
        // same grid and phrases.
        if let (Some(g), Some(path)) = (f.beatgrid().filter(|g| !g.is_empty()), f.path()) {
            let dur = g.beats.last().unwrap().time + 1.0;
            let set = build_anlz(&AnlzInput {
                device_path: &path,
                duration: dur,
                beatgrid: &g,
                cues: &f.cues(),
                waveform: &Waveform::default(),
                phrases: &[],
                phrase_mood: PhraseMood::Mid,
                is_mp3_vbr_index: None,
            });
            assert_eq!(AnlzFile::parse(&set.dat).unwrap().beatgrid().unwrap(), g);
        }
    }
    eprintln!("checked {} files, {grids} beat grids", files.len());
    assert!(grids > 0);
}

#[test]
fn corrupted_input_never_panics() {
    let (set, ..) = build_fixture();
    for bytes in [&set.dat, &set.ext, &set.ex2] {
        let mut seed = 0x1234_5678u32;
        for _ in 0..2000 {
            let mut b = bytes.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let i = (seed as usize >> 4) % b.len().min(4000);
                b[i] = (seed >> 24) as u8;
            }
            if let Ok(f) = AnlzFile::parse(&b) {
                let _ = (
                    f.cues(),
                    f.beatgrid(),
                    f.phrases(),
                    f.waveform(),
                    f.to_bytes(),
                );
            }
        }
    }
}
