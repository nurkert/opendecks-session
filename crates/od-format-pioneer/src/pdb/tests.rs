use super::*;
use crate::bytes::{le16, le32};
use od_core::{CueColor, Mode};

pub(crate) fn fixture(num_tracks: u32) -> ExportDatabase {
    let keys = [
        None,
        Some(MusicalKey::new(9, Mode::Minor)),
        Some(MusicalKey::new(6, Mode::Major)),
    ];
    let formats = [
        AudioFormat::Mp3,
        AudioFormat::Flac,
        AudioFormat::Wav,
        AudioFormat::Aiff,
        AudioFormat::Aac,
    ];
    let tracks = (1..=num_tracks)
        .map(|i| {
            let artist = match i % 5 {
                0 => String::new(),
                1 => "Ärtist Ünïcode 日本".to_string(),
                n => format!("Artist {n}"),
            };
            let ext = formats[i as usize % formats.len()]
                .as_str()
                .replace("aac", "m4a");
            let long = i % 17 == 0;
            ExportTrack {
                id: i,
                device_path: format!(
                    "/Contents/{}/Album {}/{:03} {}.{ext}",
                    if artist.is_empty() {
                        "UnknownArtist"
                    } else {
                        &artist
                    },
                    i % 7,
                    i,
                    if long {
                        "x".repeat(150)
                    } else {
                        format!("Title {i}")
                    }
                ),
                tags: TrackTags {
                    title: if i % 3 == 0 {
                        format!("Tïtle {i} — ✓")
                    } else {
                        format!("Title {i}")
                    },
                    artist: artist.clone(),
                    album: if i % 4 == 0 {
                        String::new()
                    } else {
                        format!("Album {}", i % 7)
                    },
                    genre: ["", "House", "Techno", "Drum & Bass"][i as usize % 4].into(),
                    label: if i % 2 == 0 {
                        "Label Ä".into()
                    } else {
                        String::new()
                    },
                    remixer: if i % 6 == 0 {
                        "Remixer".into()
                    } else {
                        String::new()
                    },
                    composer: if i % 9 == 0 {
                        "Composer".into()
                    } else {
                        String::new()
                    },
                    comment: if long {
                        "c".repeat(300)
                    } else {
                        format!("comment {i}")
                    },
                    year: (i % 3 != 0).then_some(1990 + (i % 30) as u16),
                    track_number: (i % 2 == 0).then_some(i % 12 + 1),
                    rating: (i % 6) as u8,
                    color: (i % 3 == 0).then(|| crate::TRACK_COLORS[(i % 8) as usize].1),
                },
                duration: 180.0 + i as f64,
                sample_rate: 44100,
                bit_rate_kbps: 320,
                bits_per_sample: 16,
                file_size: 5_000_000 + i as u64,
                format: formats[i as usize % formats.len()],
                bpm: (i % 4 != 1).then_some(120.0 + (i % 40) as f64 + 0.25),
                key: keys[i as usize % 3],
                artwork_path: (i % 2 == 1)
                    .then(|| format!("/PIONEER/Artwork/00001/a{}.jpg", i % 10)),
                anlz_path: crate::anlz::anlz_dat_path_for(&format!("/Contents/{i}.mp3")),
                date_added: format!("2024-0{}-1{}", 1 + i % 9, i % 10),
                play_count: i % 4,
            }
        })
        .collect();
    let all: Vec<u32> = (1..=num_tracks).collect();
    let playlists = vec![
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
            name: "Nested ✓".into(),
            is_folder: true,
            sort_order: 0,
            track_ids: vec![],
        },
        ExportPlaylist {
            id: 3,
            parent_id: 2,
            name: "Deep".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: all.iter().rev().copied().collect(),
        },
        ExportPlaylist {
            id: 4,
            parent_id: 1,
            name: "Empty".into(),
            is_folder: false,
            sort_order: 1,
            track_ids: vec![],
        },
        ExportPlaylist {
            id: 5,
            parent_id: 0,
            name: "Wïth Dupes".into(),
            is_folder: false,
            sort_order: 1,
            track_ids: vec![1, 1, 2, 1],
        },
        ExportPlaylist {
            id: 6,
            parent_id: 0,
            name: "All".into(),
            is_folder: false,
            sort_order: 2,
            track_ids: all,
        },
    ];
    let history = vec![ExportHistoryPlaylist {
        id: 1,
        name: "HISTORY 001".into(),
        track_ids: vec![2, 1, 2],
    }];
    ExportDatabase {
        tracks,
        playlists,
        history,
        export_date: "2026-11-06".into(),
    }
}

fn header_tables(b: &[u8]) -> Vec<(u32, u32, u32, u32)> {
    let n = le32(b, 8).unwrap() as usize;
    (0..n)
        .map(|t| {
            let at = 0x1c + 16 * t;
            (
                le32(b, at).unwrap(),
                le32(b, at + 4).unwrap(),
                le32(b, at + 8).unwrap(),
                le32(b, at + 12).unwrap(),
            )
        })
        .collect()
}

#[test]
fn round_trip_many_tracks() {
    let db = fixture(700);
    let bytes = db.to_pdb_bytes();
    assert_eq!(bytes.len() % 4096, 0);
    let back = ExportDatabase::parse(&bytes).unwrap();
    assert_eq!(back.export_date, "2026-11-06");
    assert_eq!(back.tracks.len(), db.tracks.len());
    for (a, b) in back.tracks.iter().zip(&db.tracks) {
        let mut b = b.clone();
        b.duration = b.duration.round();
        if b.format == AudioFormat::Alac {
            b.format = AudioFormat::Aac;
        }
        assert_eq!(a, &b);
    }
    let mut want = db.playlists.clone();
    want.sort_by_key(|p| (p.parent_id, p.sort_order, p.id));
    assert_eq!(back.playlists, want);
    assert_eq!(back.history, db.history);

    // Tables span multiple pages.
    let tables = header_tables(&bytes);
    let tracks = tables[0];
    assert_ne!(tracks.2, tracks.3);
    let entries = tables[8];
    assert_ne!(entries.2, entries.3);
}

#[test]
fn empty_database() {
    let db = ExportDatabase::default();
    let bytes = db.to_pdb_bytes();
    // Header + 20 index pages + 20 first data / empty pages.
    assert_eq!(bytes.len(), 41 * 4096);
    let back = ExportDatabase::parse(&bytes).unwrap();
    assert_eq!(back, ExportDatabase::default());
}

#[test]
fn layout_matches_rekordbox() {
    let bytes = fixture(3).to_pdb_bytes();
    // File header.
    assert_eq!(le32(&bytes, 0).unwrap(), 0);
    assert_eq!(le32(&bytes, 4).unwrap(), 4096);
    assert_eq!(le32(&bytes, 8).unwrap(), 20);
    assert_eq!(le32(&bytes, 0x10).unwrap(), 5);
    let tables = header_tables(&bytes);
    let types: Vec<u32> = tables.iter().map(|t| t.0).collect();
    assert_eq!(types, (0..20).collect::<Vec<_>>());
    let pages = bytes.len() / 4096;
    let mut max_candidate = 0;
    for (t, &(ty, cand, first, last)) in tables.iter().enumerate() {
        assert_eq!(first as usize, 2 * t + 1, "index page of table {ty}");
        let ip = &bytes[first as usize * 4096..][..4096];
        assert_eq!(ip[0x1b], 0x64);
        assert_eq!(le32(ip, 4).unwrap(), first);
        assert_eq!(le16(ip, 0x24).unwrap(), 0x03ec);
        assert_eq!(le32(ip, 0x3c).unwrap(), 0x1fff_fff8);
        assert!(ip[4096 - 20..].iter().all(|&b| b == 0));
        assert_eq!(le32(ip, 4096 - 24).unwrap(), 0x1fff_fff8);
        if first == last {
            // Empty table: candidate is the zero page after the index page.
            assert_eq!(cand, first + 1);
            assert_eq!(le32(ip, 0x0c).unwrap(), cand);
            assert_eq!(le32(ip, 0x2c).unwrap(), 0x03ff_ffff);
            assert!(
                bytes[cand as usize * 4096..][..4096]
                    .iter()
                    .all(|&b| b == 0)
            );
        } else {
            assert!(
                cand as usize >= pages,
                "candidate of a filled table lies past the end"
            );
            assert_eq!(le32(ip, 0x2c).unwrap(), first + 1);
            let dp = &bytes[last as usize * 4096..][..4096];
            assert_eq!(le32(dp, 0x0c).unwrap(), cand);
            assert_eq!(dp[0x1b], 0x24);
            max_candidate = max_candidate.max(cand);
        }
    }
    assert_eq!(le32(&bytes, 0x0c).unwrap(), max_candidate + 1);

    // Colors page as in a fresh rekordbox export.
    let colors = &bytes[14 * 4096..][..4096];
    assert_eq!(le16(colors, 0x1c).unwrap(), 3912, "free_size");
    assert_eq!(le16(colors, 0x1e).unwrap(), 124, "used_size");
    assert_eq!(
        &colors[0x18..0x20],
        &[0x08, 0x00, 0x01, 0x24, 0x48, 0x0f, 0x7c, 0x00]
    );
    assert_eq!(&colors[0x20..0x24], &[0x08, 0x00, 0x00, 0x00]);
    assert_eq!(&colors[4092..], &[0xff, 0x00, 0xff, 0x00]);
    assert_eq!(
        &colors[0x28..0x38],
        &[
            0, 0, 0, 0, 1, 1, 0, 0, 0x0b, b'P', b'i', b'n', b'k', 0, 0, 0
        ]
    );
    // Columns page.
    let columns = &bytes[34 * 4096..][..4096];
    assert_eq!(le16(columns, 0x1c).unwrap(), 3270);
    assert_eq!(le16(columns, 0x1e).unwrap(), 724);
    assert_eq!(
        &columns[4096 - 0x28..4096 - 0x24],
        &[0xff, 0x07, 0xff, 0x07]
    );
    // Menu and sort pages.
    assert_eq!(le16(&bytes[36 * 4096..], 0x1e).unwrap(), 176);
    assert_eq!(le16(&bytes[38 * 4096..], 0x1e).unwrap(), 136);
    // Sync info row.
    let sync = &bytes[40 * 4096..][..4096];
    assert_eq!(le16(sync, 0x1e).unwrap(), 40);
    assert_eq!(&sync[0x28..0x30], &[0x80, 0x02, 0, 0, 3, 0, 0, 0]);
}

#[test]
fn track_row_layout() {
    let db = fixture(2);
    let bytes = db.to_pdb_bytes();
    let page = &bytes[2 * 4096..][..4096];
    let row0 = &page[0x28..];
    assert_eq!(le16(row0, 0).unwrap(), 0x24);
    assert_eq!(le16(row0, 2).unwrap(), 0);
    assert_eq!(le32(row0, 4).unwrap(), 0x000c_0700);
    assert_eq!(le32(row0, 0x48).unwrap(), 1);
    assert_eq!(le16(row0, 0x56).unwrap(), 0x29);
    assert_eq!(le16(row0, 0x5c).unwrap(), 3);
    assert_eq!(
        le16(row0, 0x5e).unwrap(),
        0x88,
        "first string follows the offsets"
    );
    // Every UTF-16 string starts on a 4 byte boundary.
    for i in 0..21 {
        let ofs = le16(row0, 0x5e + 2 * i).unwrap() as usize;
        if row0[ofs] == 0x90 {
            assert_eq!(ofs % 4, 0);
        }
    }
    let ofs1 = le16(page, 4096 - 8).unwrap() as usize;
    assert_eq!(ofs1 % 4, 0);
    assert_eq!(
        le16(&page[0x28 + ofs1..], 2).unwrap(),
        0x20,
        "index_shift of row 1"
    );
    // Two rows, last one flagged as the last transaction.
    assert_eq!(&page[4092..], &[0x03, 0x00, 0x02, 0x00]);
    assert_eq!(&page[0x20..0x24], &[0x01, 0x00, 0x01, 0x00]);
}

#[test]
fn page_free_space_accounting() {
    let bytes = fixture(700).to_pdb_bytes();
    for p in bytes.chunks(4096).skip(1) {
        if p[0x1b] != 0x24 {
            continue;
        }
        let counts = u32::from_le_bytes([p[0x18], p[0x19], p[0x1a], 0]);
        let n = (counts & 0x1fff) as usize;
        assert_eq!(n, (counts >> 13) as usize);
        let free = le16(p, 0x1c).unwrap() as usize;
        let used = le16(p, 0x1e).unwrap() as usize;
        assert_eq!(free + used + n.div_ceil(16) * 4 + 2 * n, 4096 - 0x28);
        // Rows never overlap the offsets that are in use.
        let lowest_index = 4096 - (n.div_ceil(16) - 1) * 0x24 - 6 - 2 * ((n - 1) % 16);
        assert!(0x28 + used <= lowest_index);
    }
}

#[test]
fn key_and_color_rows() {
    let db = fixture(6);
    let back = ExportDatabase::parse(&db.to_pdb_bytes()).unwrap();
    assert_eq!(back.tracks[0].key, Some(MusicalKey::new(9, Mode::Minor)));
    assert_eq!(back.tracks[2].tags.color, Some(crate::TRACK_COLORS[3].1));
    let mut t = db.tracks[0].clone();
    t.tags.color = Some(CueColor::rgb(250, 0, 0));
    let db2 = ExportDatabase {
        tracks: vec![t],
        ..Default::default()
    };
    let back = ExportDatabase::parse(&db2.to_pdb_bytes()).unwrap();
    assert_eq!(back.tracks[0].tags.color, Some(crate::TRACK_COLORS[1].1));
}

#[test]
fn rejects_garbage() {
    assert!(ExportDatabase::parse(&[]).is_err());
    assert!(ExportDatabase::parse(&[0; 64]).is_err());
}

/// A real export: set `OD_PIONEER_REFERENCE_PDB` to an `export.pdb` written
/// by rekordbox.
#[test]
fn real_export() {
    let Ok(path) = std::env::var("OD_PIONEER_REFERENCE_PDB") else {
        eprintln!("OD_PIONEER_REFERENCE_PDB not set, skipping");
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let db = ExportDatabase::parse(&bytes).unwrap();
    eprintln!(
        "{} tracks, {} playlists, {} history",
        db.tracks.len(),
        db.playlists.len(),
        db.history.len()
    );
    for t in &db.tracks {
        assert!(t.id > 0);
        assert!(t.device_path.starts_with('/'), "{t:?}");
        assert!(t.anlz_path.starts_with("/PIONEER/USBANLZ/"), "{t:?}");
        assert_eq!(
            t.anlz_path.rsplit_once('/').unwrap().0,
            crate::anlz::anlz_dir_for(&t.device_path),
            "USBANLZ hash of {}",
            t.device_path
        );
    }
    // Re-export and parse again: the data survives.
    let again = ExportDatabase::parse(&db.to_pdb_bytes()).unwrap();
    assert_eq!(again.tracks.len(), db.tracks.len());
    for (a, b) in again.tracks.iter().zip(&db.tracks) {
        assert_eq!(a.tags.title, b.tags.title);
        assert_eq!(a.tags.artist, b.tags.artist);
        assert_eq!(a.device_path, b.device_path);
    }
}

#[test]
fn corrupted_input_never_panics() {
    let bytes = fixture(40).to_pdb_bytes();
    let mut seed = 0x8765_4321u32;
    for _ in 0..3000 {
        let mut b = bytes.clone();
        for _ in 0..8 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            // Concentrate on headers, row data and row indices.
            let page = (seed as usize >> 8) % (b.len() / 4096);
            let at = match seed % 3 {
                0 => (seed as usize >> 3) % 0x40,
                1 => 0x28 + (seed as usize >> 3) % 0x400,
                _ => 4096 - 1 - (seed as usize >> 3) % 0x80,
            };
            b[page * 4096 + at] = (seed >> 24) as u8;
        }
        let _ = ExportDatabase::parse(&b);
    }
}
