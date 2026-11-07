//! One-time migration from rekordbox into the OpenDecks Session library
//! (SPEC §7).
//!
//! Two sources are supported:
//!
//! * [`Source::RekordboxLibrary`]: the local rekordbox 6/7 library, i.e. the
//!   directory holding `master.db` (`~/Library/Pioneer/rekordbox` on macOS,
//!   see [`default_rekordbox_dir`]; on Linux a copied rekordbox folder). The
//!   SQLCipher-encrypted `master.db` (and `-wal`/`-shm`) is copied to a
//!   temporary directory, decrypted there by [`sqlcipher`] and opened
//!   read-only. ANLZ files are read from `<dir>/share/PIONEER/USBANLZ/…`.
//! * [`Source::RekordboxStick`]: a USB stick exported by rekordbox
//!   (`/PIONEER/rekordbox/export.pdb` + ANLZ), via `od-format-pioneer`.
//!
//! Nothing in the rekordbox directory or on the stick is ever written.
//!
//! # What is imported
//!
//! Tracks (path and tags: title, artist, album, genre, label, remixer,
//! composer, comment, year, track number, rating, colour), beatgrids (ANLZ
//! `PQTZ`), keys, hot cues / memory cues / loops with colours and comments
//! (`djmdCue`, or ANLZ `PCO2`/`PCOB` on sticks), phrases (`PSSI`), the
//! detail waveform, playlists and folders in their order, smart playlists
//! (as static playlists with their contents at import time) and optionally
//! the history.
//!
//! # Priority and re-running
//!
//! * Grids, keys and cues are stored with [`DataSource::Imported`]: later
//!   analysis never replaces them, and manual edits already in the library
//!   win (locked or manual grids, manual keys and manual hot cues are kept).
//! * Tags of tracks that are already in the library are not touched.
//! * Tracks are matched by path, then by content hash, so re-running the
//!   import does not duplicate them.
//! * Playlists are created under one new top-level folder
//!   `rekordbox import <YYYY-MM-DD>`. If a top-level folder whose name starts
//!   with `rekordbox import` already holds exactly the same tree (same
//!   names, order and tracks), no new folder is created and the report says
//!   so. A changed source creates a new folder (`… (2)` on the same day);
//!   older import folders are left for the user to delete.
//! * Tracks whose file is missing are still added (flagged missing, with an
//!   empty content hash) so that playlists, cues and grids survive; they are
//!   listed in [`ImportReport::missing_files`].

pub mod itunes;
mod masterdb;
mod model;
pub mod settings;
mod smart;
pub mod sqlcipher;
mod stick;
pub mod xml;

use model::{AnlzData, SrcLibrary, SrcNode, SrcTrack, load_anlz};
use od_core::{Beatgrid, ContentHash, DataSource, TrackAnalysis, TrackId};
use od_library::{Library, NewTrack, NodeId, NodeKind};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not found: {0}")]
    NotFound(PathBuf),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cannot decrypt master.db: {0}")]
    Cipher(#[from] sqlcipher::CipherError),
    #[error("rekordbox database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("cannot read export.pdb: {0}")]
    Pioneer(#[from] od_format_pioneer::Error),
    #[error("library error: {0}")]
    Library(#[from] od_library::LibraryError),
    #[error("{0}")]
    Invalid(String),
    #[error("import cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, ImportError>;

/// Options of an import.
#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// Also import the play history (as playlists in a "rekordbox History"
    /// folder inside the import folder).
    pub include_history: bool,
    /// Stick import only: copy audio files that are not yet in the library
    /// into this folder (`<dest>/<Artist>/<file>`) and reference the copies
    /// instead of the files on the stick.
    pub copy_audio_to: Option<PathBuf>,
    /// List tracks that lack a rekordbox beatgrid or key in
    /// [`ImportReport::needs_analysis`], so that the caller can queue them for
    /// analysis.
    pub analyse_missing: bool,
}

/// Where to import from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The folder containing `master.db`.
    RekordboxLibrary { dir: PathBuf },
    /// The mount point of a stick exported by rekordbox.
    RekordboxStick { root: PathBuf },
}

/// The rekordbox library folder of this user, if one exists (macOS only;
/// rekordbox does not run on Linux).
pub fn default_rekordbox_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let dir = home.join("Library/Pioneer/rekordbox");
    dir.join("master.db").is_file().then_some(dir)
}

/// Counts shown by the import wizard before importing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPreview {
    pub tracks: usize,
    /// Playlists including smart playlists.
    pub playlists: usize,
    pub smart_playlists: usize,
    pub folders: usize,
    pub history_playlists: usize,
    pub cues: usize,
    pub missing_files: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportPhase {
    Reading,
    Tracks,
    Playlists,
    Done,
}

/// Progress callback payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportProgress {
    pub phase: ImportPhase,
    pub done: usize,
    pub total: usize,
    /// File name of the track being imported, or playlist name.
    pub item: String,
}

/// Result of an import.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub tracks_added: usize,
    pub tracks_existing: usize,
    /// Playlists created (including smart playlists and history lists).
    pub playlists: usize,
    pub folders: usize,
    /// Cues stored as imported.
    pub cues: usize,
    /// Tracks whose beatgrid is now the imported one.
    pub grids: usize,
    /// Tracks whose key is now the imported one.
    pub keys: usize,
    pub missing_files: Vec<PathBuf>,
    /// Items not imported, with the reason.
    pub skipped: Vec<(String, String)>,
    pub notes: Vec<String>,
    /// The top-level folder holding the imported playlists (new or an
    /// identical earlier one).
    pub root_folder: Option<NodeId>,
    /// With [`ImportOptions::analyse_missing`]: tracks without an imported
    /// beatgrid or key whose files exist.
    pub needs_analysis: Vec<TrackId>,
}

fn read_source(source: &Source, with_smart: bool) -> Result<SrcLibrary> {
    match source {
        Source::RekordboxLibrary { dir } => {
            let copy = masterdb::open_copy(dir)?;
            masterdb::read(&copy.conn, dir, with_smart)
        }
        Source::RekordboxStick { root } => stick::read(root),
    }
}

/// The cover art a source keeps per audio file, read without importing:
/// (audio path, cover bytes, media type). Used to fill in covers of tracks
/// imported before covers were taken over.
pub fn source_covers(source: &Source) -> Result<Vec<(PathBuf, Vec<u8>, String)>> {
    let src = read_source(source, false)?;
    Ok(src
        .tracks
        .iter()
        .filter_map(|t| source_artwork(t).map(|(data, media)| (t.path.clone(), data, media)))
        .collect())
}

/// Counts what an import would bring in. Reads the database only (and the
/// ANLZ cue lists of a stick).
pub fn preview(source: &Source) -> Result<ImportPreview> {
    let src = read_source(source, true)?;
    let cues = match source {
        Source::RekordboxLibrary { .. } => src.db_cue_count,
        Source::RekordboxStick { .. } => src
            .tracks
            .iter()
            .map(|t| load_anlz(&t.anlz).cues.len())
            .sum(),
    };
    Ok(ImportPreview {
        tracks: src.tracks.len(),
        playlists: src.playlists.iter().filter(|p| !p.folder).count(),
        smart_playlists: src.playlists.iter().filter(|p| p.was_smart).count(),
        folders: src.playlists.iter().filter(|p| p.folder).count(),
        history_playlists: src.history.iter().filter(|p| !p.folder).count(),
        cues,
        missing_files: src.tracks.iter().filter(|t| !t.path.is_file()).count(),
    })
}

/// Imports `source` into `lib`. `progress` is called regularly; returning
/// `false` cancels the import (tracks imported so far stay in the library,
/// no playlists are created).
pub fn import(
    source: &Source,
    lib: &mut Library,
    opts: &ImportOptions,
    mut progress: impl FnMut(ImportProgress) -> bool,
) -> Result<ImportReport> {
    let mut tick = |phase, done, total, item: &str| -> Result<()> {
        if progress(ImportProgress {
            phase,
            done,
            total,
            item: item.to_string(),
        }) {
            Ok(())
        } else {
            Err(ImportError::Cancelled)
        }
    };
    tick(ImportPhase::Reading, 0, 0, "")?;
    let src = read_source(source, true)?;
    let is_stick = matches!(source, Source::RekordboxStick { .. });
    let mut report = ImportReport {
        skipped: src.skipped.clone(),
        notes: src.notes.clone(),
        ..Default::default()
    };
    if lib.path().is_some() {
        lib.backup_now("before-rekordbox-import")?;
    }

    // Tracks.
    let total = src.tracks.len();
    let mut ids: HashMap<String, TrackId> = HashMap::new();
    let mut fallback_grids = 0usize;
    for (i, t) in src.tracks.iter().enumerate() {
        let name = t
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        tick(ImportPhase::Tracks, i, total, &name)?;
        match import_track(lib, t, is_stick, opts, &mut report, &mut fallback_grids) {
            Ok(Some(id)) => {
                ids.insert(t.src_id.clone(), id);
            }
            Ok(None) => {}
            Err(e) => report
                .skipped
                .push((t.path.display().to_string(), e.to_string())),
        }
    }
    tick(ImportPhase::Tracks, total, total, "")?;
    if fallback_grids > 0 {
        report.notes.push(format!(
            "{fallback_grids} track(s) had no rekordbox beatgrid; their BPM was stored as a provisional grid \
             that analysis will replace"
        ));
    }
    if is_stick && !report.missing_files.is_empty() {
        report
            .notes
            .push("some files listed in export.pdb are not on the stick".into());
    }

    // Playlists.
    let date: String = lib.now()?.chars().take(10).collect();
    let mut plan = build_tree(&src.playlists, &ids);
    if opts.include_history && !src.history.is_empty() {
        let history = build_tree(&src.history, &ids);
        plan.push(Plan::Folder {
            name: "rekordbox History".into(),
            children: history,
        });
    }
    let smart = src.playlists.iter().filter(|p| p.was_smart).count();
    if smart > 0 {
        report.notes.push(format!(
            "{smart} smart playlist(s) were imported as normal playlists with their tracks at import time"
        ));
    }
    if !plan.is_empty() {
        let wanted = canonical(&plan);
        let mut existing_names = HashSet::new();
        let mut identical = None;
        for n in lib.children(None)? {
            if n.kind == NodeKind::Folder && n.name.starts_with("rekordbox import") {
                if identical.is_none() && canonical(&existing_tree(lib, n.id)?) == wanted {
                    identical = Some(n.id);
                }
                existing_names.insert(n.name);
            }
        }
        if let Some(id) = identical {
            report.root_folder = Some(id);
            report.notes.push(format!(
                "an identical import folder (\"{}\") already exists; no playlists were created",
                lib.node(id)?.name
            ));
        } else {
            let base = format!("rekordbox import {date}");
            let mut name = base.clone();
            let mut n = 2;
            while existing_names.contains(&name) {
                name = format!("{base} ({n})");
                n += 1;
            }
            let total = count_nodes(&plan);
            let root = lib.create_node(None, NodeKind::Folder, &name)?;
            report.folders += 1;
            report.root_folder = Some(root);
            let mut done = 0;
            create_tree(lib, root, &plan, &mut report, &mut done, total, &mut tick)?;
        }
    }
    tick(ImportPhase::Done, total, total, "")?;
    Ok(report)
}

/// Copies a stick file into `dest/<Artist>/<file>` (reusing an existing copy
/// of the same size).
fn copy_into(t: &SrcTrack, dest: &Path) -> Result<PathBuf> {
    use od_library::relocate::sanitize;
    let artist = sanitize(if t.tags.artist.is_empty() {
        "Unknown Artist"
    } else {
        &t.tags.artist
    });
    let name = t
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "track".into());
    let size = std::fs::metadata(&t.path)?.len();
    let stem = Path::new(&name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = Path::new(&name)
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut n = 1;
    let mut target = dest.join(&artist).join(sanitize(&name));
    loop {
        match std::fs::metadata(&target) {
            Ok(m)
                if m.len() == size
                    && od_decode::content_hash(&target)? == od_decode::content_hash(&t.path)? =>
            {
                return Ok(target);
            }
            Ok(_) => {
                target = dest
                    .join(&artist)
                    .join(sanitize(&format!("{stem} ({n}).{ext}")));
                n += 1;
            }
            Err(_) => break,
        }
    }
    std::fs::create_dir_all(target.parent().unwrap_or(dest))?;
    let tmp = target.with_extension(format!("{ext}.part"));
    std::fs::copy(&t.path, &tmp)?;
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

fn merge_tags(rb: &od_core::TrackTags, file: od_core::TrackTags) -> od_core::TrackTags {
    let pick = |a: &String, b: String| if a.trim().is_empty() { b } else { a.clone() };
    od_core::TrackTags {
        title: pick(&rb.title, file.title),
        artist: pick(&rb.artist, file.artist),
        album: pick(&rb.album, file.album),
        genre: pick(&rb.genre, file.genre),
        label: pick(&rb.label, file.label),
        remixer: pick(&rb.remixer, file.remixer),
        composer: pick(&rb.composer, file.composer),
        comment: pick(&rb.comment, file.comment),
        year: rb.year.or(file.year),
        track_number: rb.track_number.or(file.track_number),
        rating: rb.rating.min(5),
        color: rb.color,
    }
}

/// Adds (or finds) one track and stores its analysis and cues.
fn import_track(
    lib: &mut Library,
    t: &SrcTrack,
    is_stick: bool,
    opts: &ImportOptions,
    report: &mut ImportReport,
    fallback_grids: &mut usize,
) -> Result<Option<TrackId>> {
    let exists = t.path.is_file();
    if od_core::AudioFormat::from_path(&t.path).is_none() {
        report
            .skipped
            .push((t.path.display().to_string(), "unsupported file type".into()));
        return Ok(None);
    }
    // Find an existing library track: by path, then by content hash.
    let mut existing = lib.track_by_path(&t.path)?.map(|r| r.id);
    let mut hash = None;
    if existing.is_none() && exists {
        let h = od_decode::content_hash(&t.path)?;
        let matches = lib.tracks_by_hash(&h)?;
        existing = matches
            .iter()
            .find(|r| !r.missing)
            .or(matches.first())
            .map(|r| r.id);
        hash = Some(h);
    }
    let id = match existing {
        Some(id) => {
            report.tracks_existing += 1;
            id
        }
        None => {
            let mut path = t.path.clone();
            if is_stick && exists {
                if let Some(dest) = &opts.copy_audio_to {
                    path = copy_into(t, dest)?;
                    if let Some(r) = lib.track_by_path(&path)? {
                        report.tracks_existing += 1;
                        return finish_track(lib, r.id, t, true, opts, report, fallback_grids)
                            .map(Some);
                    }
                }
            }
            let probe = if exists {
                od_decode::probe(&path).ok()
            } else {
                None
            };
            let (tags, artwork) = match &probe {
                Some(p) => (merge_tags(&t.tags, p.tags.clone()), p.artwork.clone()),
                None => (t.tags.clone(), None),
            };
            let new = NewTrack {
                path: path.clone(),
                content_hash: hash.unwrap_or(ContentHash(String::new())),
                file_size: if exists {
                    std::fs::metadata(&path)?.len()
                } else {
                    t.file_size
                },
                format: od_core::AudioFormat::from_path(&path),
                duration: probe
                    .as_ref()
                    .and_then(|p| p.duration)
                    .filter(|d| *d > 0.0)
                    .unwrap_or(t.duration),
                sample_rate: probe
                    .as_ref()
                    .map(|p| p.sample_rate)
                    .unwrap_or(t.sample_rate),
                channels: probe.as_ref().map(|p| p.channels).unwrap_or(2),
                bit_rate: probe.as_ref().and_then(|p| p.bit_rate_kbps).or(t.bit_rate),
                bits_per_sample: probe
                    .as_ref()
                    .and_then(|p| p.bits_per_sample)
                    .or(t.bits_per_sample),
                tags,
                artwork,
            };
            let id = lib.add_track(&new)?;
            if !exists {
                lib.set_missing(id, true)?;
                report.missing_files.push(t.path.clone());
            }
            report.tracks_added += 1;
            id
        }
    };
    finish_track(lib, id, t, exists, opts, report, fallback_grids).map(Some)
}

/// The source's own cover file for a track, preferring the larger `_m`
/// variant rekordbox keeps next to the thumbnail.
fn source_artwork(t: &SrcTrack) -> Option<(Vec<u8>, String)> {
    let path = t.artwork.as_ref()?;
    let larger = path.file_stem().zip(path.extension()).map(|(s, e)| {
        path.with_file_name(format!("{}_m.{}", s.to_string_lossy(), e.to_string_lossy()))
    });
    let path = larger
        .filter(|p| p.is_file())
        .unwrap_or_else(|| path.clone());
    let data = std::fs::read(&path).ok()?;
    let media_type = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        _ => "image/jpeg",
    };
    Some((data, media_type.to_string()))
}

fn finish_track(
    lib: &mut Library,
    id: TrackId,
    t: &SrcTrack,
    file_exists: bool,
    opts: &ImportOptions,
    report: &mut ImportReport,
    fallback_grids: &mut usize,
) -> Result<TrackId> {
    let anlz: AnlzData = load_anlz(&t.anlz);
    if let Some((data, media_type)) = source_artwork(t) {
        if lib.artwork(id)?.is_none() {
            lib.set_artwork_if_missing(id, &data, &media_type)?;
        }
    }
    let before = lib.beatgrid_with_source(id)?;
    let grid_will_change = anlz.grid.is_some()
        && before
            .as_ref()
            .is_none_or(|(g, s)| !g.locked && *s <= DataSource::Imported);
    let mut a: TrackAnalysis = lib.analysis(id)?;
    a.beatgrid = anlz.grid.clone().unwrap_or_default();
    a.key = t.key;
    if a.waveform.columns.is_empty() {
        if let Some(w) = anlz.waveform {
            a.waveform = w;
        }
    }
    if let Some((p, m)) = anlz.phrases {
        // Phrases are beat indices of the rekordbox grid: only take them if
        // that grid is the one the track ends up with.
        if grid_will_change {
            a.phrases = p;
            a.phrase_mood = m;
        }
    }
    lib.store_analysis(id, &a, DataSource::Imported)?;
    let duration = lib.track(id)?.duration.max(t.duration);
    if anlz.grid.is_none() && before.is_none() {
        if let Some(bpm) = t.bpm.filter(|b| *b > 0.0 && duration > 0.0) {
            let mut b = lib.analysis(id)?;
            b.beatgrid = Beatgrid::constant(bpm, 0.0, 1, duration);
            b.key = None;
            lib.store_analysis(id, &b, DataSource::Analysis)?;
            *fallback_grids += 1;
        }
    }
    let grid_src = lib.beatgrid_with_source(id)?.map(|(_, s)| s);
    let key_src = lib.key_with_source(id)?.map(|(_, s)| s);
    if anlz.grid.is_some() && grid_src == Some(DataSource::Imported) {
        report.grids += 1;
    }
    if t.key.is_some() && key_src == Some(DataSource::Imported) {
        report.keys += 1;
    }
    let cues = t.db_cues.clone().unwrap_or(anlz.cues);
    if !cues.is_empty() {
        lib.store_imported_cues(id, &cues)?;
        report.cues += lib
            .cues(id)?
            .iter()
            .filter(|c| c.source == DataSource::Imported)
            .count();
    }
    if opts.analyse_missing
        && file_exists
        && (grid_src.is_none_or(|s| s == DataSource::Analysis) || key_src.is_none())
    {
        report.needs_analysis.push(id);
    }
    Ok(id)
}

/// The playlist tree to create.
#[derive(Clone, Debug, PartialEq)]
enum Plan {
    Folder { name: String, children: Vec<Plan> },
    Playlist { name: String, tracks: Vec<TrackId> },
}

fn build_tree(nodes: &[SrcNode], ids: &HashMap<String, TrackId>) -> Vec<Plan> {
    let known: HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let mut children: HashMap<Option<&str>, Vec<&SrcNode>> = HashMap::new();
    for n in nodes {
        // Nodes whose parent is missing (deleted) go to the top level.
        let parent = n
            .parent
            .as_deref()
            .filter(|p| known.contains(p) && *p != n.id);
        children.entry(parent).or_default().push(n);
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| a.seq.cmp(&b.seq).then_with(|| a.name.cmp(&b.name)));
    }
    fn walk<'a>(
        parent: Option<&'a str>,
        children: &HashMap<Option<&'a str>, Vec<&'a SrcNode>>,
        ids: &HashMap<String, TrackId>,
        seen: &mut HashSet<&'a str>,
    ) -> Vec<Plan> {
        let mut out = Vec::new();
        for n in children.get(&parent).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !seen.insert(n.id.as_str()) {
                continue;
            }
            out.push(if n.folder {
                Plan::Folder {
                    name: n.name.clone(),
                    children: walk(Some(n.id.as_str()), children, ids, seen),
                }
            } else {
                Plan::Playlist {
                    name: n.name.clone(),
                    tracks: n
                        .tracks
                        .iter()
                        .filter_map(|t| ids.get(t).copied())
                        .collect(),
                }
            });
        }
        out
    }
    walk(None, &children, ids, &mut HashSet::new())
}

fn canonical(plan: &[Plan]) -> String {
    fn go(p: &[Plan], out: &mut String) {
        for n in p {
            match n {
                Plan::Folder { name, children } => {
                    out.push_str(&format!("F{}:{name}[", name.len()));
                    go(children, out);
                    out.push(']');
                }
                Plan::Playlist { name, tracks } => {
                    out.push_str(&format!("P{}:{name}(", name.len()));
                    for t in tracks {
                        out.push_str(&format!("{},", t.0));
                    }
                    out.push(')');
                }
            }
        }
    }
    let mut s = String::new();
    go(plan, &mut s);
    s
}

fn existing_tree(lib: &Library, folder: NodeId) -> Result<Vec<Plan>> {
    let mut out = Vec::new();
    for n in lib.children(Some(folder))? {
        out.push(match n.kind {
            NodeKind::Folder => Plan::Folder {
                name: n.name,
                children: existing_tree(lib, n.id)?,
            },
            _ => Plan::Playlist {
                name: n.name,
                tracks: lib.entries(n.id)?.into_iter().map(|e| e.track).collect(),
            },
        });
    }
    Ok(out)
}

fn count_nodes(plan: &[Plan]) -> usize {
    plan.iter()
        .map(|p| match p {
            Plan::Folder { children, .. } => 1 + count_nodes(children),
            Plan::Playlist { .. } => 1,
        })
        .sum()
}

fn create_tree(
    lib: &mut Library,
    parent: NodeId,
    plan: &[Plan],
    report: &mut ImportReport,
    done: &mut usize,
    total: usize,
    tick: &mut impl FnMut(ImportPhase, usize, usize, &str) -> Result<()>,
) -> Result<()> {
    for p in plan {
        match p {
            Plan::Folder { name, children } => {
                tick(ImportPhase::Playlists, *done, total, name)?;
                let id = lib.create_node(Some(parent), NodeKind::Folder, name)?;
                report.folders += 1;
                *done += 1;
                create_tree(lib, id, children, report, done, total, tick)?;
            }
            Plan::Playlist { name, tracks } => {
                tick(ImportPhase::Playlists, *done, total, name)?;
                let id = lib.create_node(Some(parent), NodeKind::Playlist, name)?;
                if !tracks.is_empty() {
                    lib.add_to_node(id, tracks, None)?;
                }
                report.playlists += 1;
                *done += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
