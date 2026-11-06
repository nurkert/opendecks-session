//! Cross-validation against rekordcrate, an independent parser (dev-only).
//!
//! Checks that rekordcrate reads our `export.pdb` and ANLZ files without
//! errors, finds the same data, and that its own re-serialization of every
//! page / file is byte-identical to ours (so no field is out of place).

use binrw::{BinRead, BinWrite, Endian};
use fallible_iterator::FallibleIterator;
use od_core::{
    Beatgrid, Cue, CueColor, Mode, MusicalKey, Phrase, PhraseKind, PhraseMood, TrackTags, Waveform,
    WaveformColumn,
};
use od_format_pioneer::anlz::{AnlzFile, AnlzInput, anlz_dat_path_for, build_anlz};
use od_format_pioneer::pdb::{ExportDatabase, ExportHistoryPlaylist, ExportPlaylist, ExportTrack};
use rekordcrate::anlz::{ANLZ, Content};
use rekordcrate::pdb::io::Database;
use rekordcrate::pdb::*;
use std::io::Cursor;

fn database(n: u32) -> ExportDatabase {
    let tracks = (1..=n)
        .map(|i| {
            let path = format!("/Contents/Artist {}/Album/Track {i} — ünï.mp3", i % 13);
            ExportTrack {
                id: i,
                device_path: path.clone(),
                tags: TrackTags {
                    title: if i % 2 == 0 {
                        format!("Tïtle {i}")
                    } else {
                        format!("Title {i}")
                    },
                    artist: format!("Artist {}", i % 13),
                    album: format!("Albüm {}", i % 5),
                    genre: if i % 3 == 0 {
                        "House".into()
                    } else {
                        String::new()
                    },
                    label: "Label".into(),
                    comment: format!("comment {i}"),
                    year: Some(2020),
                    rating: (i % 6) as u8,
                    color: (i % 4 == 0).then(|| od_format_pioneer::TRACK_COLORS[2].1),
                    ..Default::default()
                },
                duration: 200.0,
                sample_rate: 44100,
                bit_rate_kbps: 320,
                bits_per_sample: 16,
                file_size: 8_000_000,
                format: od_core::AudioFormat::Mp3,
                bpm: Some(126.5),
                key: (i % 2 == 0).then(|| MusicalKey::new(9, Mode::Minor)),
                artwork_path: (i % 3 == 0).then(|| "/PIONEER/Artwork/00001/a1.jpg".to_string()),
                anlz_path: anlz_dat_path_for(&path),
                date_added: "2026-11-01".into(),
                play_count: 0,
            }
        })
        .collect();
    let all: Vec<u32> = (1..=n).collect();
    ExportDatabase {
        tracks,
        playlists: vec![
            ExportPlaylist {
                id: 1,
                parent_id: 0,
                name: "Folder".into(),
                is_folder: true,
                sort_order: 0,
                track_ids: vec![],
            },
            ExportPlaylist {
                id: 2,
                parent_id: 1,
                name: "Nësted".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: all.clone(),
            },
            ExportPlaylist {
                id: 3,
                parent_id: 0,
                name: "Empty".into(),
                is_folder: false,
                sort_order: 1,
                track_ids: vec![],
            },
        ],
        history: vec![ExportHistoryPlaylist {
            id: 1,
            name: "HISTORY 001".into(),
            track_ids: vec![3, 1],
        }],
        export_date: "2026-11-06".into(),
    }
}

fn count<RowT: RowVariant>(db: &mut Database<Cursor<&[u8]>>) -> usize {
    db.iter_rows::<RowT>().unwrap().count().unwrap()
}

fn rows<RowT: RowVariant + Clone>(db: &mut Database<Cursor<&[u8]>>) -> Vec<RowT> {
    db.iter_rows::<RowT>()
        .unwrap()
        .map(|r| Ok(r.clone()))
        .collect()
        .unwrap()
}

#[test]
fn rekordcrate_reads_our_pdb() {
    let n = 600;
    let ours = database(n);
    let bytes = ours.to_pdb_bytes();
    let mut db =
        Database::open_non_persistent(Cursor::new(bytes.as_slice()), DatabaseType::Plain).unwrap();

    assert_eq!(count::<Track>(&mut db), n as usize);
    assert_eq!(count::<Artist>(&mut db), 13);
    // Albums are keyed by (name, artist): all 5 × 13 combinations occur.
    assert_eq!(count::<Album>(&mut db), 65);
    assert_eq!(count::<Genre>(&mut db), 1);
    assert_eq!(count::<Label>(&mut db), 1);
    assert_eq!(count::<Key>(&mut db), 1);
    assert_eq!(count::<Artwork>(&mut db), 1);
    assert_eq!(count::<Color>(&mut db), 8);
    assert_eq!(count::<ColumnEntry>(&mut db), 27);
    assert_eq!(count::<Menu>(&mut db), 22);
    assert_eq!(count::<History>(&mut db), 1);
    assert_eq!(count::<PlaylistTreeNode>(&mut db), 3);
    assert_eq!(count::<PlaylistEntry>(&mut db), n as usize);
    assert_eq!(count::<HistoryPlaylist>(&mut db), 1);
    assert_eq!(count::<HistoryEntry>(&mut db), 2);

    let tracks: Vec<Track> = rows(&mut db);
    let artists: Vec<Artist> = rows(&mut db);
    for t in &tracks {
        let o = &ours.tracks[t.id.0 as usize - 1];
        assert_eq!(
            t.offsets.inner.title.clone().into_string().unwrap(),
            o.tags.title
        );
        assert_eq!(
            t.offsets.inner.file_path.clone().into_string().unwrap(),
            o.device_path
        );
        assert_eq!(t.rating, o.tags.rating);
        let dbg = format!("{t:?}");
        assert!(dbg.contains("tempo: 12650"), "{dbg}");
        assert!(
            dbg.contains(&format!("analyze_path: DeviceSQLString({})", o.anlz_path)),
            "{dbg}"
        );
        assert!(dbg.contains("date_added: DeviceSQLString(2026-11-01)"));
        let artist = artists.iter().find(|a| a.id == t.artist_id).unwrap();
        assert_eq!(
            artist.offsets.inner.name.clone().into_string().unwrap(),
            o.tags.artist
        );
    }

    let tree = rekordcrate::device::get_playlists(&mut db).unwrap();
    let dbg = format!("{tree:?}");
    assert!(dbg.contains("name: \"Folder\""), "{dbg}");
    assert!(dbg.contains("name: \"Nësted\""), "{dbg}");
    assert!(dbg.contains("name: \"Empty\""), "{dbg}");
    let entries: Vec<PlaylistEntry> = rows(&mut db);
    let mut list: Vec<(u32, u32)> = entries
        .iter()
        .filter(|e| e.playlist_id.0 == 2)
        .map(|e| (e.entry_index, e.track_id.0))
        .collect();
    list.sort();
    assert_eq!(
        list.iter().map(|e| e.1).collect::<Vec<_>>(),
        (1..=n).collect::<Vec<_>>()
    );
    assert_eq!(list[0].0, 1, "entry indices start at 1");

    let history: Vec<History> = rows(&mut db);
    let dbg = format!("{:?}", history[0]);
    assert!(
        dbg.contains(&format!("num_tracks: {n}"))
            && dbg.contains("2026-11-06")
            && dbg.contains("1000"),
        "{dbg}"
    );
}

/// rekordcrate re-serializes every page exactly as we wrote it.
#[test]
fn rekordcrate_page_round_trip() {
    let bytes = database(300).to_pdb_bytes();
    let db =
        Database::open_non_persistent(Cursor::new(bytes.as_slice()), DatabaseType::Plain).unwrap();
    let header = db.get_header().clone();
    let mut checked = 0;
    for table in &header.tables {
        // rekordcrate has no row type for table 18 (sort configuration) and
        // drops its rows when writing, so it cannot round-trip that page.
        if matches!(table.page_type, PageType::Unknown(_)) {
            continue;
        }
        let mut index = table.first_page;
        loop {
            let at = index.offset(4096) as usize;
            let mut cur = Cursor::new(&bytes[..]);
            cur.set_position(at as u64);
            let page =
                Page::read_options(&mut cur, Endian::Little, (4096, DatabaseType::Plain)).unwrap();
            let mut out = Cursor::new(Vec::new());
            page.write_options(&mut out, Endian::Little, (4096,))
                .unwrap();
            let out = out.into_inner();
            assert_eq!(out.len(), 4096);
            if out != bytes[at..at + 4096] {
                let diff = (0..4096).find(|&i| out[i] != bytes[at + i]).unwrap();
                panic!("page at {at:#x} differs at byte {diff:#x}");
            }
            checked += 1;
            if index == table.last_page {
                break;
            }
            index = page.header.next_page;
        }
    }
    assert!(checked > 40);
}

fn anlz_set() -> (od_format_pioneer::anlz::AnlzSet, Vec<Cue>) {
    let grid = Beatgrid::constant(124.0, 0.05, 1, 30.0);
    let cues = vec![
        Cue {
            label: "Intro".into(),
            color: Some(CueColor::rgb(0xe6, 0x28, 0x28)),
            ..Cue::hot(0, 0.05)
        },
        Cue {
            loop_end: Some(0.05 + 4.0 * 60.0 / 124.0),
            ..Cue::hot(1, 0.05)
        },
        Cue::memory(12.0),
        Cue {
            label: "mem".into(),
            ..Cue::memory(3.0)
        },
    ];
    let wf = Waveform {
        columns_per_second: Waveform::DETAIL_RATE,
        columns: (0..4500)
            .map(|i| WaveformColumn {
                low: (i % 200) as u8,
                mid: 90,
                high: 30,
                peak: (i % 250) as u8,
            })
            .collect(),
    };
    let phrases = [
        Phrase {
            start_beat: 0,
            end_beat: 16,
            kind: PhraseKind::Intro,
        },
        Phrase {
            start_beat: 16,
            end_beat: 48,
            kind: PhraseKind::Verse,
        },
        Phrase {
            start_beat: 48,
            end_beat: 60,
            kind: PhraseKind::Outro,
        },
    ];
    let set = build_anlz(&AnlzInput {
        device_path: "/Contents/A/B/ünï.mp3",
        duration: 30.0,
        beatgrid: &grid,
        cues: &cues,
        waveform: &wf,
        phrases: &phrases,
        phrase_mood: PhraseMood::Mid,
        is_mp3_vbr_index: None,
    });
    (set, cues)
}

#[test]
fn rekordcrate_reads_our_anlz() {
    let (set, cues) = anlz_set();
    for (name, bytes) in [("DAT", &set.dat), ("EXT", &set.ext), ("2EX", &set.ex2)] {
        let anlz = ANLZ::read(&mut Cursor::new(bytes.as_slice()))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // Byte-identical re-serialization.
        let mut out = Cursor::new(Vec::new());
        anlz.write(&mut out).unwrap();
        assert_eq!(&out.into_inner(), bytes, "{name} re-serialization differs");
        for s in &anlz.sections {
            assert!(
                !matches!(s.content, Content::Unknown(_)) || name == "EXT",
                "{name}: unknown section {:?}",
                s.header.kind
            );
            match &s.content {
                Content::Path(p) => {
                    assert!(format!("{p:?}").contains("/Contents/A/B/ünï.mp3"), "{p:?}")
                }
                Content::BeatGrid(g) => {
                    assert_eq!(g.beats.len(), 62);
                    assert_eq!(g.beats[0].time, 50);
                    assert_eq!(g.beats[0].tempo, 12400);
                    assert_eq!(g.beats[1].beat_number, 2);
                }
                Content::ExtendedCueList(l) if !l.cues.is_empty() => {
                    for c in &l.cues {
                        // rekordbox writes 88 byte PCP2 entries plus the comment.
                        assert_eq!(c.header.total_size, 88 + c.comment.byte_len());
                        let ours = cues
                            .iter()
                            .find(|o| {
                                (o.start * 1000.0).round() as u32 == c.time
                                    && (c.hot_cue > 0) == matches!(o.kind, od_core::CueKind::Hot(_))
                                    && *c.comment == *o.label
                            })
                            .unwrap();
                        if let Some(col) = ours.color {
                            assert_eq!(c.hot_cue_color_rgb, (col.r, col.g, col.b));
                            assert_eq!(c.hot_cue_color_index, 0x2a);
                        }
                        if ours.loop_end.is_some() {
                            assert_eq!((c.loop_numerator, c.loop_denominator), (4, 1));
                        }
                    }
                }
                Content::CueList(l) => {
                    for c in &l.cues {
                        assert!(
                            cues.iter()
                                .any(|o| (o.start * 1000.0).round() as u32 == c.time)
                        );
                    }
                }
                Content::SongStructure(s) => {
                    let dbg = format!("{s:?}");
                    assert!(dbg.contains("mood: Mid"), "{dbg}");
                    assert!(dbg.contains("end_beat: 61"), "{dbg}");
                }
                _ => {}
            }
        }
    }
    // Our own reader agrees.
    let ext = AnlzFile::parse(&set.ext).unwrap();
    assert_eq!(ext.cues().len(), cues.len());
}

/// Real rekordbox ANLZ files (env `OD_PIONEER_ANLZ_SAMPLES`): our parse →
/// serialize gives the bytes rekordcrate also accepts.
#[test]
fn rekordcrate_agrees_on_samples() {
    let Ok(dir) = std::env::var("OD_PIONEER_ANLZ_SAMPLES") else {
        eprintln!("OD_PIONEER_ANLZ_SAMPLES not set, skipping");
        return;
    };
    let mut stack = vec![std::path::PathBuf::from(dir)];
    let mut n = 0;
    let mut rejected = 0;
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("DAT" | "EXT" | "2EX")
            ) {
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let ours = AnlzFile::parse(&bytes).unwrap();
            let theirs = match ANLZ::read(&mut Cursor::new(bytes.as_slice())) {
                Ok(t) => t,
                Err(e) => {
                    // rekordcrate is stricter than rekordbox in places.
                    let first = e.to_string().lines().next().unwrap_or_default().to_string();
                    eprintln!("rekordcrate cannot read {}: {first}", p.display());
                    rejected += 1;
                    continue;
                }
            };
            for s in &theirs.sections {
                if let Content::BeatGrid(g) = &s.content {
                    let og = ours.beatgrid().unwrap();
                    assert_eq!(og.beats.len(), g.beats.len());
                    for (a, b) in og.beats.iter().zip(&g.beats) {
                        assert_eq!((a.time * 1000.0).round() as u32, b.time);
                    }
                }
            }
            n += 1;
        }
    }
    eprintln!("compared {n} files, {rejected} rejected by rekordcrate");
}

/// Writes sample output for external validators (pyrekordbox, crate-digger)
/// when `OD_PIONEER_DUMP_DIR` is set.
#[test]
fn dump_for_external_validators() {
    let Ok(dir) = std::env::var("OD_PIONEER_DUMP_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let anlz = dir.join("PIONEER/USBANLZ/P000/00000000");
    std::fs::create_dir_all(&anlz).unwrap();
    std::fs::create_dir_all(dir.join("PIONEER/rekordbox")).unwrap();
    let (set, _) = anlz_set();
    std::fs::write(anlz.join("ANLZ0000.DAT"), &set.dat).unwrap();
    std::fs::write(anlz.join("ANLZ0000.EXT"), &set.ext).unwrap();
    std::fs::write(anlz.join("ANLZ0000.2EX"), &set.ex2).unwrap();
    std::fs::write(
        dir.join("PIONEER/rekordbox/export.pdb"),
        database(600).to_pdb_bytes(),
    )
    .unwrap();
}

/// Writes an empty export for a structural diff against an empty rekordbox
/// export when `OD_PIONEER_DUMP_DIR` is set.
#[test]
fn dump_empty_export() {
    let Ok(dir) = std::env::var("OD_PIONEER_DUMP_DIR") else {
        return;
    };
    let db = ExportDatabase {
        export_date: "2022-02-02".into(),
        ..Default::default()
    };
    std::fs::write(
        std::path::Path::new(&dir).join("empty.pdb"),
        db.to_pdb_bytes(),
    )
    .unwrap();
}

/// Golden test: an empty export must be byte-identical to the empty export
/// rekordbox writes (`OD_PIONEER_REFERENCE_EMPTY_PDB`, e.g. rekordcrate's
/// `data/complete_export/empty/PIONEER/rekordbox/export.pdb`, whose sync row
/// carries the date 2022-02-02).
#[test]
fn empty_export_matches_rekordbox() {
    let Ok(path) = std::env::var("OD_PIONEER_REFERENCE_EMPTY_PDB") else {
        eprintln!("OD_PIONEER_REFERENCE_EMPTY_PDB not set, skipping");
        return;
    };
    let reference = std::fs::read(path).unwrap();
    let ours = ExportDatabase {
        export_date: "2022-02-02".into(),
        ..Default::default()
    }
    .to_pdb_bytes();
    assert_eq!(ours.len(), reference.len());
    for (i, (a, b)) in ours.chunks(4096).zip(reference.chunks(4096)).enumerate() {
        if let Some(d) = (0..4096).find(|&j| a[j] != b[j]) {
            panic!(
                "page {i} differs at {d:#x}: ours {:#x}, rekordbox {:#x}",
                a[d], b[d]
            );
        }
    }
}

/// Re-exports a real rekordbox `export.pdb` (`OD_PIONEER_REFERENCE_PDB`)
/// and checks rekordcrate reads the same tracks from both.
#[test]
fn reexport_of_reference_reads_alike() {
    let Ok(path) = std::env::var("OD_PIONEER_REFERENCE_PDB") else {
        eprintln!("OD_PIONEER_REFERENCE_PDB not set, skipping");
        return;
    };
    let reference = std::fs::read(&path).unwrap();
    let parsed = ExportDatabase::parse(&reference).unwrap();
    let ours = parsed.to_pdb_bytes();
    let titles = |b: &[u8]| {
        let mut db = Database::open_non_persistent(Cursor::new(b), DatabaseType::Plain).unwrap();
        let mut v: Vec<(u32, String, String)> = rows::<Track>(&mut db)
            .iter()
            .map(|t| {
                (
                    t.id.0,
                    t.offsets.inner.title.clone().into_string().unwrap(),
                    t.offsets.inner.file_path.clone().into_string().unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(titles(&ours), titles(&reference));
    let n = |b: &[u8]| {
        let mut db = Database::open_non_persistent(Cursor::new(b), DatabaseType::Plain).unwrap();
        (
            count::<PlaylistTreeNode>(&mut db),
            count::<PlaylistEntry>(&mut db),
            count::<HistoryEntry>(&mut db),
        )
    };
    assert_eq!(n(&ours), n(&reference));
    eprintln!(
        "{} pages written for {} pages in the reference",
        ours.len() / 4096,
        reference.len() / 4096
    );
}

/// Writes the re-export of `OD_PIONEER_REFERENCE_PDB` into
/// `OD_PIONEER_DUMP_DIR/reexport.pdb` for manual structural diffs.
#[test]
fn dump_reexport() {
    let (Ok(path), Ok(dir)) = (
        std::env::var("OD_PIONEER_REFERENCE_PDB"),
        std::env::var("OD_PIONEER_DUMP_DIR"),
    ) else {
        return;
    };
    let db = ExportDatabase::parse(&std::fs::read(path).unwrap()).unwrap();
    std::fs::write(
        std::path::Path::new(&dir).join("reexport.pdb"),
        db.to_pdb_bytes(),
    )
    .unwrap();
}
