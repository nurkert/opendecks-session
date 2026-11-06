//! The rekordbox XML library format (the documented exchange format many DJ
//! applications read and write): export the OpenDecks library to it and
//! import one from it. Tracks, tags, key, beatgrid (TEMPO), hot / memory
//! cues and loops (POSITION_MARK), playlists and folders.

use od_core::{Beat, Beatgrid, Cue, CueColor, CueKind, KeyNotation, MusicalKey, TrackId, TrackTags};
use od_library::{Library, NewTrack, NodeId, NodeKind};
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::{Reader, Writer};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum XmlError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml: {0}")]
    Xml(String),
    #[error(transparent)]
    Library(#[from] od_library::LibraryError),
}

type Result<T> = std::result::Result<T, XmlError>;

fn xml_err(e: impl std::fmt::Display) -> XmlError {
    XmlError::Xml(e.to_string())
}

/// Stars 0..=5 ↔ the XML's 0..=255 scale.
pub fn rating_to_xml(stars: u8) -> u8 {
    [0, 51, 102, 153, 204, 255][stars.min(5) as usize]
}

pub fn rating_from_xml(v: u32) -> u8 {
    match v {
        0..=25 => 0,
        26..=76 => 1,
        77..=127 => 2,
        128..=178 => 3,
        179..=229 => 4,
        _ => 5,
    }
}

/// `file://localhost/` URL for a path (percent-encoding what URLs need).
pub fn location(path: &Path) -> String {
    let mut out = String::from("file://localhost");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn path_from_location(loc: &str) -> PathBuf {
    let rest = loc.strip_prefix("file://localhost").or_else(|| loc.strip_prefix("file://")).unwrap_or(loc);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

/// Tempo markers of a grid: a new marker wherever the tempo changes.
fn tempo_markers(grid: &Beatgrid) -> Vec<&Beat> {
    let mut out: Vec<&Beat> = Vec::new();
    for b in &grid.beats {
        if out.last().is_none_or(|l| (l.bpm - b.bpm).abs() > 0.005) {
            out.push(b);
        }
    }
    out
}

/// Writes the whole library as rekordbox XML.
pub fn export_xml(lib: &Library, path: &Path) -> Result<usize> {
    let tracks = lib.tracks(od_library::SortKey::DateAdded, false)?;
    let mut w = Writer::new_with_indent(std::io::BufWriter::new(std::fs::File::create(path)?), b' ', 2);
    let ev = |w: &mut Writer<_>, e: Event| w.write_event(e).map_err(xml_err);
    ev(&mut w, Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
    let mut root = BytesStart::new("DJ_PLAYLISTS");
    root.push_attribute(("Version", "1.0.0"));
    ev(&mut w, Event::Start(root))?;
    let mut product = BytesStart::new("PRODUCT");
    product.push_attribute(("Name", "OpenDecks Session"));
    product.push_attribute(("Version", env!("CARGO_PKG_VERSION")));
    product.push_attribute(("Company", "OpenDecks"));
    ev(&mut w, Event::Empty(product))?;
    let mut coll = BytesStart::new("COLLECTION");
    coll.push_attribute(("Entries", tracks.len().to_string().as_str()));
    ev(&mut w, Event::Start(coll))?;
    for t in &tracks {
        let mut e = BytesStart::new("TRACK");
        let n = |v: f64| format!("{v:.2}");
        let attrs: Vec<(&str, String)> = vec![
            ("TrackID", t.id.0.to_string()),
            ("Name", t.tags.title.clone()),
            ("Artist", t.tags.artist.clone()),
            ("Composer", t.tags.composer.clone()),
            ("Album", t.tags.album.clone()),
            ("Genre", t.tags.genre.clone()),
            ("Size", t.file_size.to_string()),
            ("TotalTime", (t.duration.round() as i64).to_string()),
            ("TrackNumber", t.tags.track_number.unwrap_or(0).to_string()),
            ("Year", t.tags.year.map(|y| y.to_string()).unwrap_or_default()),
            ("AverageBpm", t.bpm.map(n).unwrap_or_else(|| "0.00".into())),
            ("DateAdded", t.date_added.get(..10).unwrap_or("").to_string()),
            // The library stores kbit/s, as the XML does.
            ("BitRate", t.bit_rate.map(|b| b.to_string()).unwrap_or_default()),
            ("SampleRate", t.sample_rate.to_string()),
            ("Comments", t.tags.comment.clone()),
            ("PlayCount", t.play_count.to_string()),
            ("Rating", rating_to_xml(t.tags.rating).to_string()),
            ("Location", location(&t.path)),
            ("Remixer", t.tags.remixer.clone()),
            ("Tonality", t.key.map(|k| k.format(KeyNotation::Musical)).unwrap_or_default()),
            ("Label", t.tags.label.clone()),
        ];
        for (k, v) in &attrs {
            e.push_attribute((*k, v.as_str()));
        }
        ev(&mut w, Event::Start(e))?;
        if let Some((grid, _)) = lib.beatgrid_with_source(t.id)? {
            for b in tempo_markers(&grid) {
                let mut tm = BytesStart::new("TEMPO");
                tm.push_attribute(("Inizio", format!("{:.3}", b.time).as_str()));
                tm.push_attribute(("Bpm", n(b.bpm).as_str()));
                tm.push_attribute(("Metro", "4/4"));
                tm.push_attribute(("Battito", b.bar_position.to_string().as_str()));
                ev(&mut w, Event::Empty(tm))?;
            }
        }
        for c in lib.cues(t.id)? {
            let mut pm = BytesStart::new("POSITION_MARK");
            pm.push_attribute(("Name", c.label.as_str()));
            pm.push_attribute(("Type", if c.loop_end.is_some() { "4" } else { "0" }));
            pm.push_attribute(("Start", format!("{:.3}", c.start).as_str()));
            if let Some(end) = c.loop_end {
                pm.push_attribute(("End", format!("{end:.3}").as_str()));
            }
            let num = match c.kind {
                CueKind::Hot(s) => s as i32,
                CueKind::Memory => -1,
            };
            pm.push_attribute(("Num", num.to_string().as_str()));
            if let (CueKind::Hot(_), Some(col)) = (c.kind, c.color) {
                pm.push_attribute(("Red", col.r.to_string().as_str()));
                pm.push_attribute(("Green", col.g.to_string().as_str()));
                pm.push_attribute(("Blue", col.b.to_string().as_str()));
            }
            ev(&mut w, Event::Empty(pm))?;
        }
        ev(&mut w, Event::End(BytesEnd::new("TRACK")))?;
    }
    ev(&mut w, Event::End(BytesEnd::new("COLLECTION")))?;
    ev(&mut w, Event::Start(BytesStart::new("PLAYLISTS")))?;
    write_nodes(lib, &mut w, None, "ROOT")?;
    ev(&mut w, Event::End(BytesEnd::new("PLAYLISTS")))?;
    ev(&mut w, Event::End(BytesEnd::new("DJ_PLAYLISTS")))?;
    w.into_inner().flush()?;
    Ok(tracks.len())
}

fn write_nodes<W: Write>(lib: &Library, w: &mut Writer<W>, parent: Option<NodeId>, name: &str) -> Result<()> {
    let children = lib.children(parent)?;
    let mut f = BytesStart::new("NODE");
    f.push_attribute(("Type", "0"));
    f.push_attribute(("Name", name));
    f.push_attribute(("Count", children.len().to_string().as_str()));
    w.write_event(Event::Start(f)).map_err(xml_err)?;
    for n in children {
        match n.kind {
            NodeKind::Folder => write_nodes(lib, w, Some(n.id), &n.name)?,
            _ => {
                // Playlists, sets and smart lists (as their current tracks).
                let tracks = lib.node_tracks(n.id)?;
                let mut p = BytesStart::new("NODE");
                p.push_attribute(("Name", n.name.as_str()));
                p.push_attribute(("Type", "1"));
                p.push_attribute(("KeyType", "0"));
                p.push_attribute(("Entries", tracks.len().to_string().as_str()));
                w.write_event(Event::Start(p)).map_err(xml_err)?;
                for t in tracks {
                    let mut e = BytesStart::new("TRACK");
                    e.push_attribute(("Key", t.id.0.to_string().as_str()));
                    w.write_event(Event::Empty(e)).map_err(xml_err)?;
                }
                w.write_event(Event::End(BytesEnd::new("NODE"))).map_err(xml_err)?;
            }
        }
    }
    w.write_event(Event::End(BytesEnd::new("NODE"))).map_err(xml_err)?;
    Ok(())
}

/// What an XML import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XmlImportReport {
    pub tracks: usize,
    pub playlists: usize,
    pub cues: usize,
    /// Tracks skipped because their file is missing.
    pub missing: Vec<PathBuf>,
}

#[derive(Default)]
struct XmlTrack {
    attrs: HashMap<String, String>,
    tempos: Vec<HashMap<String, String>>,
    marks: Vec<HashMap<String, String>>,
}

fn attrs_of(e: &BytesStart) -> Result<HashMap<String, String>> {
    let mut m = HashMap::new();
    for a in e.attributes() {
        let a = a.map_err(xml_err)?;
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let value = a.unescape_value().map_err(xml_err)?.into_owned();
        m.insert(key, value);
    }
    Ok(m)
}

/// Imports a rekordbox XML file into `lib` (new tracks are added; tracks
/// whose file already is in the library get the XML's cues, grid and tags).
pub fn import_xml(lib: &mut Library, path: &Path) -> Result<XmlImportReport> {
    let text = std::fs::read_to_string(path)?;
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);
    let mut tracks: Vec<XmlTrack> = Vec::new();
    // Playlist tree: (depth, name, is_folder, track keys)
    let mut nodes: Vec<(usize, String, bool, Vec<String>)> = Vec::new();
    let mut in_track = false;
    let mut in_playlists = false;
    let mut depth = 0usize;
    loop {
        let event = reader.read_event().map_err(xml_err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let name = e.name();
                match name.as_ref() {
                    b"TRACK" if !in_playlists => {
                        tracks.push(XmlTrack { attrs: attrs_of(e)?, ..Default::default() });
                        in_track = !empty;
                    }
                    b"TEMPO" if in_track => tracks.last_mut().unwrap().tempos.push(attrs_of(e)?),
                    b"POSITION_MARK" if in_track => tracks.last_mut().unwrap().marks.push(attrs_of(e)?),
                    b"PLAYLISTS" => in_playlists = true,
                    b"NODE" if in_playlists => {
                        let a = attrs_of(e)?;
                        let folder = a.get("Type").map(String::as_str) == Some("0");
                        nodes.push((depth, a.get("Name").cloned().unwrap_or_default(), folder, Vec::new()));
                        if !empty {
                            depth += 1;
                        }
                    }
                    b"TRACK" if in_playlists => {
                        if let Some(n) = nodes.last_mut() {
                            n.3.push(attrs_of(e)?.get("Key").cloned().unwrap_or_default());
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => match e.name().as_ref() {
                b"TRACK" if !in_playlists => in_track = false,
                b"NODE" if in_playlists => depth = depth.saturating_sub(1),
                b"PLAYLISTS" => in_playlists = false,
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    let mut report = XmlImportReport::default();
    let mut ids: HashMap<String, TrackId> = HashMap::new();
    for t in &tracks {
        let a = &t.attrs;
        let get = |k: &str| a.get(k).cloned().unwrap_or_default();
        let file = path_from_location(&get("Location"));
        if !file.is_file() {
            report.missing.push(file);
            continue;
        }
        let tags = TrackTags {
            title: get("Name"),
            artist: get("Artist"),
            album: get("Album"),
            genre: get("Genre"),
            label: get("Label"),
            remixer: get("Remixer"),
            composer: get("Composer"),
            comment: get("Comments"),
            year: get("Year").parse().ok().filter(|y| *y > 0),
            track_number: get("TrackNumber").parse().ok().filter(|n| *n > 0),
            rating: rating_from_xml(get("Rating").parse().unwrap_or(0)),
            color: None,
        };
        let id = lib.add_track(&NewTrack {
            content_hash: od_decode::content_hash(&file)?,
            file_size: std::fs::metadata(&file)?.len(),
            path: file.clone(),
            format: None,
            duration: get("TotalTime").parse().unwrap_or(0.0),
            sample_rate: get("SampleRate").parse().unwrap_or(0),
            channels: 2,
            bit_rate: get("BitRate").parse::<u32>().ok(),
            bits_per_sample: None,
            tags: tags.clone(),
            artwork: None,
        })?;
        lib.set_tags(id, tags)?;
        if let Some(k) = MusicalKey::parse(&get("Tonality")) {
            lib.set_key(id, Some(k))?;
        }
        let duration: f64 = get("TotalTime").parse().unwrap_or(0.0);
        if let Some(grid) = grid_from_tempos(&t.tempos, duration) {
            lib.set_beatgrid(id, grid)?;
        }
        let cues: Vec<Cue> = t.marks.iter().filter_map(cue_from_mark).collect();
        if !cues.is_empty() {
            report.cues += cues.len();
            lib.set_cues(id, cues)?;
        }
        ids.insert(get("TrackID"), id);
        report.tracks += 1;
    }

    // Playlists: rebuild the folder tree under an "XML import" folder.
    let top = lib.create_node(None, NodeKind::Folder, "XML import")?;
    let mut stack: Vec<NodeId> = vec![top];
    for (d, name, folder, keys) in nodes.into_iter().skip(1) {
        // `d` counts from the ROOT node's children (depth 1).
        stack.truncate(d.max(1));
        let parent = *stack.last().unwrap_or(&top);
        if folder {
            let f = lib.create_node(Some(parent), NodeKind::Folder, &name)?;
            stack.push(f);
        } else {
            let p = lib.create_node(Some(parent), NodeKind::Playlist, &name)?;
            let members: Vec<TrackId> = keys.iter().filter_map(|k| ids.get(k).copied()).collect();
            lib.add_to_node(p, &members, None)?;
            report.playlists += 1;
        }
    }
    Ok(report)
}

fn grid_from_tempos(tempos: &[HashMap<String, String>], duration: f64) -> Option<Beatgrid> {
    let num = |m: &HashMap<String, String>, k: &str| m.get(k).and_then(|v| v.parse::<f64>().ok());
    let marks: Vec<(f64, f64, u8)> = tempos
        .iter()
        .filter_map(|m| Some((num(m, "Inizio")?, num(m, "Bpm")?, num(m, "Battito").unwrap_or(1.0) as u8)))
        .filter(|(_, bpm, _)| *bpm > 0.0)
        .collect();
    let (first_t, first_bpm, first_pos) = *marks.first()?;
    if marks.len() == 1 || duration <= 0.0 {
        return Some(Beatgrid::constant(first_bpm, first_t, first_pos.clamp(1, 4), duration.max(first_t + 1.0)));
    }
    // Variable tempo: beats from each marker to the next.
    let mut beats = Vec::new();
    for (i, &(t0, bpm, pos)) in marks.iter().enumerate() {
        let end = marks.get(i + 1).map(|m| m.0).unwrap_or(duration);
        let (mut t, mut p) = (t0, pos.clamp(1, 4));
        while t < end - 1e-6 {
            beats.push(Beat { time: t, bpm, bar_position: p });
            t += 60.0 / bpm;
            p = p % 4 + 1;
        }
    }
    Some(Beatgrid { beats, locked: false })
}

fn cue_from_mark(m: &HashMap<String, String>) -> Option<Cue> {
    let num: i32 = m.get("Num")?.parse().ok()?;
    let start: f64 = m.get("Start")?.parse().ok()?;
    let kind = if num >= 0 { CueKind::Hot(num as u8) } else { CueKind::Memory };
    let mut c = Cue::hot(0, start);
    c.kind = kind;
    c.label = m.get("Name").cloned().unwrap_or_default();
    if m.get("Type").map(String::as_str) == Some("4") {
        c.loop_end = m.get("End").and_then(|v| v.parse().ok());
    }
    let channel = |k: &str| m.get(k).and_then(|v| v.parse::<u8>().ok());
    if let (Some(r), Some(g), Some(b)) = (channel("Red"), channel("Green"), channel("Blue")) {
        c.color = Some(CueColor { r, g, b });
    }
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_round_trip() {
        for s in 0..=5u8 {
            assert_eq!(rating_from_xml(rating_to_xml(s) as u32), s);
        }
        let p = Path::new("/Users/dj/Music/Café & Bar/01 Track #1.mp3");
        assert_eq!(path_from_location(&location(p)), p);
        assert!(location(p).starts_with("file://localhost/Users/dj/Music/Caf%C3%A9%20%26%20Bar/"));
    }

    #[test]
    fn export_then_import_keeps_tracks_cues_grid_and_playlists() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("Summa.mp3");
        std::fs::write(&audio, b"not really audio, but a file").unwrap();
        let mut src = Library::open(&dir.path().join("a.sqlite")).unwrap();
        let tags = TrackTags { title: "Summa".into(), artist: "Frink".into(), comment: "opener".into(), rating: 4, ..Default::default() };
        let id = src
            .add_track(&NewTrack {
                content_hash: od_decode::content_hash(&audio).unwrap(),
                file_size: 28,
                path: audio.clone(),
                format: None,
                duration: 120.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: Some(320),
                bits_per_sample: None,
                tags,
                artwork: None,
            })
            .unwrap();
        src.set_key(id, MusicalKey::parse("8A")).unwrap();
        src.set_beatgrid(id, Beatgrid::constant(123.0, 0.25, 1, 120.0)).unwrap();
        let mut a = Cue::hot(0, 16.0);
        a.label = "Drop".into();
        a.color = Some(CueColor { r: 240, g: 30, b: 30 });
        let mut lp = Cue::hot(0, 32.0);
        lp.kind = CueKind::Memory;
        lp.loop_end = Some(40.0);
        src.set_cues(id, vec![a, lp]).unwrap();
        let folder = src.create_node(None, NodeKind::Folder, "Gigs").unwrap();
        let pl = src.create_node(Some(folder), NodeKind::Playlist, "Friday").unwrap();
        src.add_to_node(pl, &[id], None).unwrap();

        let xml = dir.path().join("lib.xml");
        assert_eq!(export_xml(&src, &xml).unwrap(), 1);

        let mut dst = Library::open(&dir.path().join("b.sqlite")).unwrap();
        let r = import_xml(&mut dst, &xml).unwrap();
        assert_eq!((r.tracks, r.playlists, r.cues), (1, 1, 2));
        let t = &dst.tracks(od_library::SortKey::DateAdded, false).unwrap()[0];
        assert_eq!((t.tags.title.as_str(), t.tags.comment.as_str(), t.tags.rating), ("Summa", "opener", 4));
        assert_eq!(t.key, MusicalKey::parse("8A"));
        let (g, _) = dst.beatgrid_with_source(t.id).unwrap().unwrap();
        assert!((g.first_bpm().unwrap() - 123.0).abs() < 1e-6);
        let cues = dst.cues(t.id).unwrap();
        let hot = cues.iter().find(|c| c.kind == CueKind::Hot(0)).unwrap();
        assert_eq!((hot.start, hot.label.as_str(), hot.color), (16.0, "Drop", Some(CueColor { r: 240, g: 30, b: 30 })));
        assert!(cues.iter().any(|c| c.kind == CueKind::Memory && c.loop_end == Some(40.0)));
        let gigs = dst.children(None).unwrap().into_iter().find(|n| n.name == "XML import").unwrap();
        let sub = dst.children(Some(gigs.id)).unwrap();
        assert_eq!(sub[0].name, "Gigs");
        let lists = dst.children(Some(sub[0].id)).unwrap();
        assert_eq!(lists[0].name, "Friday");
        assert_eq!(dst.node_tracks(lists[0].id).unwrap().len(), 1);
    }
}
