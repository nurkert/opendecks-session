//! Import of an iTunes / Music library export (`Library.xml`, File ▸ Library
//! ▸ Export Library… in Music): tracks with their tags and the playlist
//! folders. The file is an XML property list (Apple's documented plist
//! format: `<dict>` of `<key>`/value pairs, `<array>`, `<string>`,
//! `<integer>`, `<real>`, `<true/>`, `<false/>`, `<date>`, `<data>`).
//! Tracks whose file is missing are reported, not added.

use crate::xml::{XmlError, path_from_location};
use od_core::{TrackId, TrackTags};
use od_library::{Library, NewTrack, NodeId, NodeKind};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, XmlError>;

/// A property list value.
#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Dict(Vec<(String, Plist)>),
    Array(Vec<Plist>),
    Str(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    /// `<date>` and `<data>` as their text.
    Other(String),
}

impl Plist {
    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Plist::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Plist::Int(i) => Some(*i),
            Plist::Real(r) => Some(*r as i64),
            Plist::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Plist::Bool(true)))
    }
}

fn err(e: impl std::fmt::Display) -> XmlError {
    XmlError::Xml(e.to_string())
}

/// Parses an XML property list.
pub fn parse_plist(text: &str) -> Result<Plist> {
    enum Frame {
        Dict(Vec<(String, Plist)>, Option<String>),
        Array(Vec<Plist>),
    }
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Plist> = None;
    let mut scalar: Option<(String, String)> = None;
    let push = |stack: &mut Vec<Frame>, root: &mut Option<Plist>, v: Plist| -> Result<()> {
        match stack.last_mut() {
            Some(Frame::Dict(items, key)) => {
                let k = key.take().ok_or_else(|| err("plist value without a key"))?;
                items.push((k, v));
            }
            Some(Frame::Array(items)) => items.push(v),
            None => *root = Some(v),
        }
        Ok(())
    };
    loop {
        match reader.read_event().map_err(err)? {
            Event::Start(e) => match e.name().as_ref() {
                b"dict" => stack.push(Frame::Dict(Vec::new(), None)),
                b"array" => stack.push(Frame::Array(Vec::new())),
                b"plist" => {}
                other => {
                    scalar = Some((String::from_utf8_lossy(other).into_owned(), String::new()))
                }
            },
            Event::Empty(e) => match e.name().as_ref() {
                b"true" => push(&mut stack, &mut root, Plist::Bool(true))?,
                b"false" => push(&mut stack, &mut root, Plist::Bool(false))?,
                b"dict" => push(&mut stack, &mut root, Plist::Dict(Vec::new()))?,
                b"array" => push(&mut stack, &mut root, Plist::Array(Vec::new()))?,
                b"string" => push(&mut stack, &mut root, Plist::Str(String::new()))?,
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, s)) = scalar.as_mut() {
                    s.push_str(&t.unescape().map_err(err)?);
                }
            }
            Event::CData(t) => {
                if let Some((_, s)) = scalar.as_mut() {
                    s.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::End(e) => match e.name().as_ref() {
                b"dict" => match stack.pop() {
                    Some(Frame::Dict(items, _)) => push(&mut stack, &mut root, Plist::Dict(items))?,
                    _ => return Err(err("unbalanced </dict>")),
                },
                b"array" => match stack.pop() {
                    Some(Frame::Array(items)) => push(&mut stack, &mut root, Plist::Array(items))?,
                    _ => return Err(err("unbalanced </array>")),
                },
                b"plist" => {}
                _ => {
                    let Some((tag, text)) = scalar.take() else {
                        continue;
                    };
                    if tag == "key" {
                        if let Some(Frame::Dict(_, key)) = stack.last_mut() {
                            *key = Some(text);
                        }
                        continue;
                    }
                    let v = match tag.as_str() {
                        "string" => Plist::Str(text),
                        "integer" => Plist::Int(text.trim().parse().map_err(err)?),
                        "real" => Plist::Real(text.trim().parse().map_err(err)?),
                        _ => Plist::Other(text),
                    };
                    push(&mut stack, &mut root, v)?;
                }
            },
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| err("empty property list"))
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItunesImportReport {
    /// Tracks added or found in the library.
    pub tracks: usize,
    pub playlists: usize,
    /// Tracks whose file is missing.
    pub missing: Vec<PathBuf>,
}

/// Name of the folder the playlists are placed in.
pub const IMPORT_FOLDER: &str = "Music library import";

/// Stars from the 0..100 rating of the export (20 per star).
pub fn stars(rating: i64) -> u8 {
    ((rating.clamp(0, 100) + 10) / 20) as u8
}

/// Imports an iTunes / Music `Library.xml` into `lib`.
pub fn import_itunes(lib: &mut Library, path: &Path) -> Result<ItunesImportReport> {
    let root = parse_plist(&std::fs::read_to_string(path)?)?;
    let mut report = ItunesImportReport::default();
    let mut ids: HashMap<i64, TrackId> = HashMap::new();
    if let Some(Plist::Dict(tracks)) = root.get("Tracks") {
        for (_, t) in tracks {
            let Some(loc) = t.str("Location") else {
                continue;
            };
            if t.str("Kind").is_some_and(|k| k.contains("video"))
                || t.flag("Movie")
                || t.flag("Podcast")
            {
                continue;
            }
            let file = path_from_location(loc);
            if !file.is_file() {
                report.missing.push(file);
                continue;
            }
            let s = |k: &str| t.str(k).unwrap_or_default().to_string();
            let tags = TrackTags {
                title: s("Name"),
                artist: s("Artist"),
                album: s("Album"),
                genre: s("Genre"),
                composer: s("Composer"),
                comment: s("Comments"),
                year: t.int("Year").map(|y| y as u16).filter(|y| *y > 0),
                track_number: t.int("Track Number").map(|n| n as u32).filter(|n| *n > 0),
                rating: stars(t.int("Rating").unwrap_or(0)),
                ..Default::default()
            };
            let id = match lib.track_by_path(&file)? {
                Some(known) => known.id,
                None => lib.add_track(&NewTrack {
                    content_hash: od_decode::content_hash(&file)?,
                    file_size: std::fs::metadata(&file)?.len(),
                    path: file.clone(),
                    format: None,
                    duration: t.int("Total Time").unwrap_or(0) as f64 / 1000.0,
                    sample_rate: t.int("Sample Rate").unwrap_or(0) as u32,
                    channels: 2,
                    bit_rate: t.int("Bit Rate").map(|b| b as u32),
                    bits_per_sample: None,
                    tags: tags.clone(),
                    artwork: None,
                })?,
            };
            lib.set_tags(id, tags)?;
            if let Some(id_in_file) = t.int("Track ID") {
                ids.insert(id_in_file, id);
            }
            report.tracks += 1;
        }
    }
    // Playlists: the folder tree by persistent ids, under one folder. The
    // library itself ("Master") and the built-in smart lists are skipped.
    let mut by_pid: HashMap<String, NodeId> = HashMap::new();
    let lists: Vec<&Plist> = match root.get("Playlists") {
        Some(Plist::Array(v)) => v
            .iter()
            .filter(|p| {
                !p.flag("Master")
                    && p.get("Distinguished Kind").is_none()
                    && !matches!(p.get("Visible"), Some(Plist::Bool(false)))
            })
            .collect(),
        _ => Vec::new(),
    };
    if lists.is_empty() {
        return Ok(report);
    }
    let top = lib.create_node(None, NodeKind::Folder, IMPORT_FOLDER)?;
    // Parents may come after their children: place folders in passes.
    let mut pending: Vec<&Plist> = lists;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for p in pending {
            let parent = match p.str("Parent Persistent ID") {
                None => Some(top),
                Some(pid) => by_pid.get(pid).copied(),
            };
            let Some(parent) = parent else {
                rest.push(p);
                continue;
            };
            let name = p.str("Name").unwrap_or("Playlist");
            let node = if p.flag("Folder") {
                lib.create_node(Some(parent), NodeKind::Folder, name)?
            } else {
                let n = lib.create_node(Some(parent), NodeKind::Playlist, name)?;
                let members: Vec<TrackId> = match p.get("Playlist Items") {
                    Some(Plist::Array(items)) => items
                        .iter()
                        .filter_map(|i| i.int("Track ID"))
                        .filter_map(|i| ids.get(&i).copied())
                        .collect(),
                    _ => Vec::new(),
                };
                lib.add_to_node(n, &members, None)?;
                report.playlists += 1;
                n
            };
            if let Some(pid) = p.str("Playlist Persistent ID") {
                by_pid.insert(pid.to_string(), node);
            }
        }
        if rest.len() == before {
            // Their parent folder is not in the file: place them at the top.
            for p in rest {
                lib.create_node(
                    Some(top),
                    NodeKind::Playlist,
                    p.str("Name").unwrap_or("Playlist"),
                )?;
                report.playlists += 1;
            }
            break;
        }
        pending = rest;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_xml(a: &Path, b: &Path, missing: &Path) -> String {
        let loc = |p: &Path| crate::xml::location(p).replace("file://localhost", "file://");
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Major Version</key><integer>1</integer>
	<key>Application Version</key><string>1.4</string>
	<key>Tracks</key>
	<dict>
		<key>101</key>
		<dict>
			<key>Track ID</key><integer>101</integer>
			<key>Name</key><string>Sunrise &amp; Co</string>
			<key>Artist</key><string>The Band</string>
			<key>Album</key><string>Morning</string>
			<key>Genre</key><string>House</string>
			<key>Year</key><integer>2019</integer>
			<key>Rating</key><integer>80</integer>
			<key>Total Time</key><integer>2000</integer>
			<key>Location</key><string>{}</string>
		</dict>
		<key>102</key>
		<dict>
			<key>Track ID</key><integer>102</integer>
			<key>Name</key><string>Second</string>
			<key>Compilation</key><true/>
			<key>Location</key><string>{}</string>
		</dict>
		<key>103</key>
		<dict>
			<key>Track ID</key><integer>103</integer>
			<key>Name</key><string>Gone</string>
			<key>Location</key><string>{}</string>
		</dict>
	</dict>
	<key>Playlists</key>
	<array>
		<dict>
			<key>Name</key><string>Library</string>
			<key>Master</key><true/>
			<key>Playlist Items</key><array><dict><key>Track ID</key><integer>101</integer></dict></array>
		</dict>
		<dict>
			<key>Name</key><string>Music</string>
			<key>Distinguished Kind</key><integer>4</integer>
		</dict>
		<dict>
			<key>Name</key><string>Warm-up</string>
			<key>Playlist Persistent ID</key><string>AAA2</string>
			<key>Parent Persistent ID</key><string>AAA1</string>
			<key>Playlist Items</key>
			<array>
				<dict><key>Track ID</key><integer>102</integer></dict>
				<dict><key>Track ID</key><integer>101</integer></dict>
				<dict><key>Track ID</key><integer>103</integer></dict>
			</array>
		</dict>
		<dict>
			<key>Name</key><string>Gigs</string>
			<key>Playlist Persistent ID</key><string>AAA1</string>
			<key>Folder</key><true/>
		</dict>
		<dict>
			<key>Name</key><string>Empty</string>
			<key>Playlist Persistent ID</key><string>AAA3</string>
			<key>Playlist Items</key><array/>
		</dict>
	</array>
</dict>
</plist>"#,
            loc(a),
            loc(b),
            loc(missing)
        )
    }

    #[test]
    fn plist_values() {
        let p = parse_plist("<plist><dict><key>a</key><integer>-3</integer><key>b</key><real>1.5</real><key>c</key><false/><key>d</key><array><string>x</string><string/></array><key>e</key><date>2020-01-01T00:00:00Z</date></dict></plist>").unwrap();
        assert_eq!(p.int("a"), Some(-3));
        assert_eq!(p.get("b"), Some(&Plist::Real(1.5)));
        assert!(!p.flag("c"));
        assert_eq!(
            p.get("d"),
            Some(&Plist::Array(vec![
                Plist::Str("x".into()),
                Plist::Str(String::new())
            ]))
        );
        assert_eq!(
            p.get("e"),
            Some(&Plist::Other("2020-01-01T00:00:00Z".into()))
        );
        assert!(parse_plist("<plist><dict><key>a</key></array></plist>").is_err());
        assert_eq!((stars(0), stars(20), stars(80), stars(100)), (0, 1, 4, 5));
    }

    #[test]
    fn imports_tracks_and_playlist_folders() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("Music/Sun rise.mp3");
        let b = dir.path().join("Music/second.mp3");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::write(&a, vec![1u8; 2048]).unwrap();
        std::fs::write(&b, vec![2u8; 2048]).unwrap();
        let xml = dir.path().join("Library.xml");
        std::fs::write(
            &xml,
            library_xml(&a, &b, &dir.path().join("Music/gone.mp3")),
        )
        .unwrap();
        let mut lib = Library::open(&dir.path().join("lib.sqlite")).unwrap();
        let r = import_itunes(&mut lib, &xml).unwrap();
        assert_eq!((r.tracks, r.playlists, r.missing.len()), (2, 2, 1));
        let t = lib.track_by_path(&a).unwrap().expect("track added");
        assert_eq!(
            (
                t.tags.title.as_str(),
                t.tags.artist.as_str(),
                t.tags.genre.as_str(),
                t.tags.year,
                t.tags.rating
            ),
            ("Sunrise & Co", "The Band", "House", Some(2019), 4)
        );
        let nodes = lib.all_nodes().unwrap();
        let top = nodes.iter().find(|n| n.name == IMPORT_FOLDER).unwrap();
        let gigs = nodes.iter().find(|n| n.name == "Gigs").unwrap();
        assert_eq!((gigs.kind, gigs.parent), (NodeKind::Folder, Some(top.id)));
        let warm = nodes.iter().find(|n| n.name == "Warm-up").unwrap();
        assert_eq!(warm.parent, Some(gigs.id), "the folder tree is kept");
        let titles: Vec<String> = lib
            .node_tracks(warm.id)
            .unwrap()
            .into_iter()
            .map(|t| t.tags.title)
            .collect();
        assert_eq!(
            titles,
            ["Second", "Sunrise & Co"],
            "playlist order, missing file left out"
        );
        assert!(
            !nodes
                .iter()
                .any(|n| n.name == "Library" || n.name == "Music"),
            "built-in lists skipped"
        );
        // A second import finds the tracks again.
        let r2 = import_itunes(&mut lib, &xml).unwrap();
        assert_eq!(r2.tracks, 2);
        assert_eq!(lib.track_count().unwrap(), 2);
    }
}
