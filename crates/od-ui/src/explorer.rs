//! Explorer: browse folders on disk (rekordbox-style) and play directly.
//!
//! Root folders are user-chosen and kept in the library meta
//! (`explorer.roots`, a JSON list; `~/Music` until the user changes it).
//! Folders are listed lazily when expanded. Selecting a folder lists its
//! audio files in the track table at once (file names, or library data for
//! files already in the collection); tags are read in the background and
//! fill in as they arrive. Loading a file adds it to the collection through
//! the same import as ADD MUSIC, queues its analysis and then loads it to a
//! deck (Perform) or the preview player (Prepare). Files are never copied.

use crate::app::{App, handler};
use crate::browser::{self, SortFields};
use crate::models::model;
use crate::workers::import_file;
use crate::{ExplorerRow, TrackRow, decks, format, player};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_core::{MusicalKey, TrackId};
use od_library::Library;
use slint::SharedString;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

const META_ROOTS: &str = "explorer.roots";

/// One audio file of the open folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ExplorerFile {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    pub duration: f64,
    /// The file is in the collection.
    pub track: Option<TrackId>,
    pub analysed: bool,
    /// Tags were read (or came from the library).
    pub tagged: bool,
}

impl ExplorerFile {
    /// A file not read yet: "Artist - Title" file names are split.
    pub fn from_name(path: PathBuf) -> Self {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) if !a.trim().is_empty() && !t.trim().is_empty() => (a.trim().to_string(), t.trim().to_string()),
            _ => (String::new(), stem),
        };
        Self { path, title, artist, analysed: true, ..Default::default() }
    }

    fn from_record(path: PathBuf, t: &od_library::TrackRecord) -> Self {
        Self {
            path,
            title: t.tags.title.clone(),
            artist: t.tags.artist.clone(),
            bpm: t.bpm,
            key: t.key,
            duration: t.duration,
            track: Some(t.id),
            analysed: t.analysis_version >= od_analysis::ANALYSIS_VERSION,
            tagged: true,
        }
    }
}

impl SortFields for ExplorerFile {
    fn title(&self) -> &str {
        &self.title
    }
    fn artist(&self) -> &str {
        &self.artist
    }
    fn genre(&self) -> &str {
        ""
    }
    fn bpm(&self) -> Option<f64> {
        self.bpm
    }
    fn key(&self) -> Option<MusicalKey> {
        self.key
    }
    fn rating(&self) -> u8 {
        0
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn date_added(&self) -> &str {
        ""
    }
}

#[derive(Default)]
pub(crate) struct ExplorerState {
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    /// Sub-folders of listed folders.
    children: HashMap<PathBuf, Vec<PathBuf>>,
    /// Folder paths of the sidebar rows, by row id.
    visible: Vec<PathBuf>,
    /// The folder shown in the track list, if any.
    pub open: Option<PathBuf>,
    pub files: Vec<ExplorerFile>,
    /// Bumped whenever another folder opens; stale tag results are dropped.
    generation: u64,
}

/// Roots from the stored meta value; `None` (never set) means `default`.
pub(crate) fn parse_roots(stored: Option<&str>, default: Option<PathBuf>) -> Vec<PathBuf> {
    match stored {
        Some(v) => serde_json::from_str::<Vec<PathBuf>>(v).unwrap_or_default(),
        None => default.into_iter().collect(),
    }
}

fn default_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Music")).filter(|p| p.is_dir())
}

/// Folders the Explorer does not show: hidden ones and rekordbox / player
/// data folders.
pub(crate) fn hidden_dir(name: &str) -> bool {
    name.starts_with('.') || ["PIONEER", "PioneerDJ", "rekordbox"].iter().any(|n| name.eq_ignore_ascii_case(n))
}

fn name_key(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Sub-folders and supported audio files of `dir`, each sorted by name.
pub(crate) fn list_dir(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    if let Ok(read) = std::fs::read_dir(dir) {
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            // Follow symlinks for the type (music folders are often linked).
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            if meta.is_dir() {
                if !hidden_dir(&name) {
                    dirs.push(p);
                }
            } else if meta.is_file() && !name.starts_with('.') && od_core::AudioFormat::is_supported_extension(&p) {
                files.push(p);
            }
        }
    }
    dirs.sort_by_key(|p| name_key(p));
    files.sort_by_key(|p| name_key(p));
    (dirs, files)
}

impl ExplorerState {
    pub fn load(lib: &Library) -> Self {
        let stored = lib.meta(META_ROOTS).ok().flatten();
        Self { roots: parse_roots(stored.as_deref(), default_root()), ..Default::default() }
    }

    fn save(&self, lib: &mut Library) {
        let _ = lib.set_meta(META_ROOTS, &serde_json::to_string(&self.roots).unwrap_or_else(|_| "[]".into()));
    }

    pub fn close(&mut self) {
        self.open = None;
        self.files.clear();
        self.generation += 1;
    }

    pub fn track_at(&self, i: usize) -> Option<TrackId> {
        self.files.get(i).and_then(|f| f.track)
    }

    pub fn selected_ids(&self, selection: &BTreeSet<usize>) -> Vec<TrackId> {
        selection.iter().filter_map(|&i| self.track_at(i)).collect()
    }

    /// Sorts the files, keeping the selection on the same files. No column
    /// keeps the folder's name order.
    pub fn sort(&mut self, column: &str, descending: bool, selection: &mut BTreeSet<usize>) {
        let selected: HashSet<PathBuf> = selection.iter().filter_map(|&i| self.files.get(i)).map(|f| f.path.clone()).collect();
        if browser::sort_key_known(column) {
            browser::sort_rows(&mut self.files, column, descending);
        } else {
            self.files.sort_by_key(|f| name_key(&f.path));
        }
        *selection = self.files.iter().enumerate().filter(|(_, f)| selected.contains(&f.path)).map(|(i, _)| i).collect();
    }

    /// Sub-folders of `dir`, listed on first use.
    fn children_of(&mut self, dir: &Path) -> &[PathBuf] {
        if !self.children.contains_key(dir) {
            let (dirs, _) = list_dir(dir);
            self.children.insert(dir.to_path_buf(), dirs);
        }
        &self.children[dir]
    }

    /// Sidebar rows for the roots and expanded folders.
    fn rows(&mut self) -> Vec<ExplorerRow> {
        let mut out = Vec::new();
        let mut paths = Vec::new();
        fn walk(st: &mut ExplorerState, dir: &Path, depth: i32, root: bool, out: &mut Vec<ExplorerRow>, paths: &mut Vec<PathBuf>) {
            let expanded = st.expanded.contains(dir);
            // Unlisted folders may have children until we look.
            let has_children = st.children.get(dir).is_none_or(|c| !c.is_empty());
            let name = if root { dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string()) } else { dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
            out.push(ExplorerRow {
                id: paths.len() as i32,
                name: name.into(),
                path: dir.display().to_string().into(),
                depth,
                root,
                has_children,
                expanded,
            });
            paths.push(dir.to_path_buf());
            if expanded {
                let kids = st.children_of(dir).to_vec();
                if let Some(r) = out.last_mut() {
                    r.has_children = !kids.is_empty();
                }
                for k in kids {
                    walk(st, &k, depth + 1, false, out, paths);
                }
            }
        }
        for r in self.roots.clone() {
            walk(self, &r, 0, true, &mut out, &mut paths);
        }
        self.visible = paths;
        out
    }

    fn path_of(&self, row: i32) -> Option<PathBuf> {
        usize::try_from(row).ok().and_then(|i| self.visible.get(i)).cloned()
    }
}

// ── Background work ─────────────────────────────────────────────────────

/// Tags of a file read in the background.
struct TagResult {
    generation: u64,
    path: PathBuf,
    title: String,
    artist: String,
    bpm: Option<f64>,
    duration: f64,
}

/// What to do once a file is in the collection.
#[derive(Clone, Copy, Debug)]
enum LoadTarget {
    /// A deck (0-based).
    Deck(usize),
    /// Double-click / browse press: free deck (Perform) or preview (Prepare).
    Free,
}

struct Imported {
    path: PathBuf,
    result: Result<TrackId, String>,
    target: LoadTarget,
}

pub(crate) struct ExplorerRuntime {
    tags_tx: Sender<TagResult>,
    tags_rx: Receiver<TagResult>,
    import_tx: Sender<Imported>,
    import_rx: Receiver<Imported>,
    /// Files being added, so a second click does not add twice.
    importing: std::cell::RefCell<HashSet<PathBuf>>,
}

impl Default for ExplorerRuntime {
    fn default() -> Self {
        let (tags_tx, tags_rx) = unbounded();
        let (import_tx, import_rx) = unbounded();
        Self { tags_tx, tags_rx, import_tx, import_rx, importing: Default::default() }
    }
}

/// BPM from the file's tags (TBPM and equivalents), if any.
fn tagged_bpm(path: &Path) -> Option<f64> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTagKey};
    use symphonia::core::probe::Hint;
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).ok()?;
    let find = |rev: &symphonia::core::meta::MetadataRevision| {
        rev.tags().iter().find(|t| t.std_key == Some(StandardTagKey::Bpm)).and_then(|t| parse_bpm(&t.value.to_string()))
    };
    let early = probed.metadata.get().and_then(|m| m.current().and_then(find));
    early.or_else(|| probed.format.metadata().current().and_then(find))
}

/// "128", "127.5", "128 BPM" → BPM within a plausible range.
pub(crate) fn parse_bpm(s: &str) -> Option<f64> {
    let num: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    num.replace(',', ".").parse::<f64>().ok().filter(|b| (20.0..=400.0).contains(b))
}

/// Reads tags of untagged files of the open folder (one thread per folder).
fn read_tags_in_background(app: &App, generation: u64, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let tx = app.explorer.tags_tx.clone();
    std::thread::Builder::new()
        .name("od-explorer-tags".into())
        .spawn(move || {
            for path in paths {
                let Ok(info) = od_decode::probe(&path) else { continue };
                let r = TagResult {
                    generation,
                    bpm: tagged_bpm(&path),
                    title: info.tags.title,
                    artist: info.tags.artist,
                    duration: info.duration.unwrap_or(0.0),
                    path,
                };
                if tx.send(r).is_err() {
                    return;
                }
            }
        })
        .ok();
}

// ── Projection ──────────────────────────────────────────────────────────

pub(crate) fn refresh_tree(app: &App) {
    let (rows, selected) = {
        let mut st = app.state.borrow_mut();
        let rows = st.explorer.rows();
        let selected = st.explorer.open.as_ref().and_then(|o| st.explorer.visible.iter().position(|p| p == o)).map(|i| i as i32).unwrap_or(-1);
        (rows, selected)
    };
    app.ui.set_explorer(model(rows));
    app.ui.set_explorer_selected(selected);
}

/// Table rows of the open folder.
pub(crate) fn file_rows(files: &[ExplorerFile], selection: &BTreeSet<usize>, current_key: Option<MusicalKey>, notation: od_core::KeyNotation, loaded: &[(TrackId, usize)]) -> Vec<TrackRow> {
    files
        .iter()
        .enumerate()
        .map(|(i, f)| TrackRow {
            id: f.track.map(|t| t.0 as i32).unwrap_or(-1 - i as i32),
            position: i as i32 + 1,
            title: f.title.clone().into(),
            artist: f.artist.clone().into(),
            bpm: f.bpm.map(|b| format!("{b:.1}")).unwrap_or_default().into(),
            key: f.key.map(|k| k.format(notation)).unwrap_or_default().into(),
            key_distance: browser::key_distance(current_key, f.key),
            time: if f.duration > 0.0 { format::time(f.duration) } else { String::new() }.into(),
            analysed: f.analysed,
            selected: selection.contains(&i),
            deck: f.track.and_then(|t| loaded.iter().find(|(id, _)| *id == t)).map(|(_, d)| *d as i32 + 1).unwrap_or(0),
            ..Default::default()
        })
        .collect()
}

/// The open folder in the track table and list header.
pub(crate) fn refresh_files(app: &App) {
    let st = app.state.borrow();
    let Some(folder) = st.explorer.open.as_ref() else { return };
    let ex = &st.explorer;
    let sel = &st.browser.selection;
    let current_key = st.player.current.and_then(|id| ex.files.iter().find(|f| f.track == Some(id))).and_then(|f| f.key);
    let loaded = {
        let d = app.decks.borrow();
        if d.source_is_stick() { Vec::new() } else { d.loaded_decks() }
    };
    let rows = file_rows(&ex.files, sel, current_key, st.config.key_notation, &loaded);
    app.browser.tracks.sync(rows, browser::same_row);
    let total: f64 = ex.files.iter().map(|f| f.duration).sum();
    let ui = &app.ui;
    ui.set_sort_column(st.browser.sort.clone().into());
    ui.set_sort_descending(st.browser.descending);
    ui.set_has_selection(!st.selected_ids().is_empty());
    ui.set_selection_count(sel.len() as i32);
    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.display().to_string());
    ui.set_node_title(name.into());
    ui.set_node_kind("explorer".into());
    ui.set_node_sub(od_i18n::trf("{} TRACKS  ·  {}", &[&ex.files.len(), &format::time(total)]).into());
    ui.set_read_only(false);
    ui.set_register_target(SharedString::new());
}

// ── Commands ────────────────────────────────────────────────────────────

/// Shows `dir`'s audio files in the track table.
fn open_folder(app: &App, dir: PathBuf) {
    let (generation, untagged) = {
        let mut st = app.state.borrow_mut();
        let (_, paths) = list_dir(&dir);
        let files: Vec<ExplorerFile> = paths
            .into_iter()
            .map(|p| match st.lib.track_by_path(&p).ok().flatten() {
                Some(t) => ExplorerFile::from_record(p, &t),
                None => ExplorerFile::from_name(p),
            })
            .collect();
        st.browser.stick = None;
        st.browser.node = None;
        st.browser.rows.clear();
        st.browser.selection.clear();
        st.browser.anchor = None;
        st.browser.flow_selected = None;
        st.browser.search.clear();
        st.browser.sort = String::new();
        st.browser.descending = false;
        st.explorer.close();
        st.explorer.open = Some(dir);
        let untagged = files.iter().filter(|f| !f.tagged).map(|f| f.path.clone()).collect();
        st.explorer.files = files;
        (st.explorer.generation, untagged)
    };
    read_tags_in_background(app, generation, untagged);
    app.ui.set_search_text(SharedString::new());
    app.ui.set_section("library".into());
    browser::refresh_tree(app);
    refresh_tree(app);
    refresh_files(app);
}

/// Loads file `i` of the open folder: adds it to the collection first if
/// needed (in the background), then loads it.
pub(crate) fn activate(app: &App, i: usize, deck: Option<usize>) {
    let target = deck.map(LoadTarget::Deck).unwrap_or(LoadTarget::Free);
    let file = app.state.borrow().explorer.files.get(i).cloned();
    let Some(file) = file else { return };
    match file.track.filter(|t| app.state.borrow().lib.track(*t).is_ok()) {
        Some(id) => load(app, id, target),
        None => start_import(app, file, target),
    }
}

/// LOAD / browse press while a folder is shown: handles files that are not
/// in the collection yet. Returns true when it took over.
pub(crate) fn load_selection(app: &App, deck: Option<usize>) -> bool {
    let sel = {
        let st = app.state.borrow();
        if st.explorer.open.is_none() {
            return false;
        }
        st.browser.selection.iter().next_back().copied()
    };
    if let Some(i) = sel {
        activate(app, i, deck);
    }
    true
}

fn start_import(app: &App, file: ExplorerFile, target: LoadTarget) {
    if !app.explorer.importing.borrow_mut().insert(file.path.clone()) {
        return;
    }
    let library = app.state.borrow().config.library_path.clone();
    let tx = app.explorer.import_tx.clone();
    app.status(format!("Adding {} to the collection…", file.title));
    let path = file.path;
    std::thread::spawn(move || {
        let result = Library::open(&library).map_err(|e| e.to_string()).and_then(|mut lib| import_file(&mut lib, &path).map_err(|e| e.to_string()));
        let _ = tx.send(Imported { path, result, target });
    });
}

/// Makes `id` the current track and loads it as `target` asks.
fn load(app: &App, id: TrackId, target: LoadTarget) {
    {
        let mut st = app.state.borrow_mut();
        if st.player.current != Some(id) {
            st.player.current = Some(id);
            st.player.cursor = 0.0;
        }
    }
    player::refresh_detail(app);
    match target {
        LoadTarget::Deck(d) => decks::load_track(app, d, id, None),
        LoadTarget::Free => decks::load_selected_into_free_deck(app),
    }
}

/// Frame tick: tag results and finished imports.
pub(crate) fn poll(app: &App) {
    let rt = &app.explorer;
    let tags: Vec<TagResult> = rt.tags_rx.try_iter().collect();
    let imported: Vec<Imported> = rt.import_rx.try_iter().collect();
    if tags.is_empty() && imported.is_empty() {
        return;
    }
    let mut changed = false;
    {
        let mut st = app.state.borrow_mut();
        let generation = st.explorer.generation;
        for t in tags.into_iter().filter(|t| t.generation == generation) {
            if let Some(f) = st.explorer.files.iter_mut().find(|f| f.path == t.path && !f.tagged) {
                f.title = t.title;
                f.artist = t.artist;
                f.bpm = t.bpm;
                f.duration = t.duration;
                f.tagged = true;
                changed = true;
            }
        }
        if changed && browser::sort_key_known(&st.browser.sort) {
            let (col, desc) = (st.browser.sort.clone(), st.browser.descending);
            let st = &mut *st;
            st.explorer.sort(&col, desc, &mut st.browser.selection);
        }
    }
    for im in imported {
        app.explorer.importing.borrow_mut().remove(&im.path);
        match im.result {
            Ok(id) => {
                let title = {
                    let mut st = app.state.borrow_mut();
                    let record = st.lib.track(id).ok();
                    if let (Some(f), Some(t)) = (st.explorer.files.iter_mut().find(|f| f.path == im.path), record.as_ref()) {
                        *f = ExplorerFile::from_record(im.path.clone(), t);
                    }
                    record.map(|t| t.tags.title).unwrap_or_default()
                };
                let _ = app.analysis.wake.send(());
                app.status(format!("Added {title} to the collection — analysing"));
                changed = true;
                load(app, id, im.target);
            }
            Err(e) => app.status(format!("Could not add {}: {e}", im.path.display())),
        }
    }
    if changed {
        crate::browser::refresh_tracks(app);
        browser::refresh_tree(app);
    }
}

/// Re-reads library data (analysis results) of files in the open folder.
pub(crate) fn refresh_library_data(app: &App) {
    let mut st = app.state.borrow_mut();
    if st.explorer.open.is_none() {
        return;
    }
    let updates: Vec<(usize, od_library::TrackRecord)> =
        st.explorer.files.iter().enumerate().filter_map(|(i, f)| f.track.and_then(|t| st.lib.track(t).ok()).map(|t| (i, t))).collect();
    for (i, t) in updates {
        let path = st.explorer.files[i].path.clone();
        st.explorer.files[i] = ExplorerFile::from_record(path, &t);
    }
}

fn add_roots(app: &App) {
    let Some(dirs) = rfd::FileDialog::new().set_title(od_i18n::tr("Add a folder to the Explorer")).pick_folders() else { return };
    {
        let mut st = app.state.borrow_mut();
        let st = &mut *st;
        for d in dirs {
            if !st.explorer.roots.contains(&d) {
                st.explorer.roots.push(d);
            }
        }
        st.explorer.save(&mut st.lib);
    }
    refresh_tree(app);
}

fn remove_root(app: &App, row: i32) {
    let closed = {
        let mut st = app.state.borrow_mut();
        let st = &mut *st;
        let Some(path) = st.explorer.path_of(row) else { return };
        if !st.explorer.roots.contains(&path) {
            return;
        }
        st.explorer.roots.retain(|r| *r != path);
        st.explorer.expanded.retain(|p| !p.starts_with(&path));
        st.explorer.save(&mut st.lib);
        let closing = st.explorer.open.as_ref().is_some_and(|o| o.starts_with(&path));
        if closing {
            st.explorer.close();
        }
        closing
    };
    if closed {
        // Back to the collection.
        app.ui.invoke_select_node(-1);
    }
    refresh_tree(app);
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.on_explorer_select(handler!(app, |a, row: i32| {
        let path = a.state.borrow().explorer.path_of(row);
        if let Some(p) = path {
            open_folder(&a, p);
        }
    }));
    ui.on_explorer_toggle(handler!(app, |a, row: i32| {
        {
            let mut st = a.state.borrow_mut();
            let Some(p) = st.explorer.path_of(row) else { return };
            if !st.explorer.expanded.remove(&p) {
                // Re-list on every expand: folders change on disk.
                st.explorer.children.remove(&p);
                st.explorer.expanded.insert(p);
            }
        }
        refresh_tree(&a);
    }));
    ui.on_explorer_add_root(handler!(app, |a| add_roots(&a)));
    ui.on_explorer_remove_root(handler!(app, |a, row: i32| remove_root(&a, row)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_default_and_persisted() {
        let music = PathBuf::from("/home/dj/Music");
        assert_eq!(parse_roots(None, Some(music.clone())), vec![music.clone()]);
        assert_eq!(parse_roots(None, None), Vec::<PathBuf>::new());
        // An explicitly emptied list stays empty.
        assert_eq!(parse_roots(Some("[]"), Some(music.clone())), Vec::<PathBuf>::new());
        assert_eq!(parse_roots(Some(r#"["/a","/b c"]"#), None), vec![PathBuf::from("/a"), PathBuf::from("/b c")]);
        assert_eq!(parse_roots(Some("garbage"), Some(music)), Vec::<PathBuf>::new());
    }

    #[test]
    fn file_names_split_into_artist_and_title() {
        let f = ExplorerFile::from_name(PathBuf::from("/m/Frink - Summa (Original Mix).flac"));
        assert_eq!((f.artist.as_str(), f.title.as_str()), ("Frink", "Summa (Original Mix)"));
        let g = ExplorerFile::from_name(PathBuf::from("/m/untitled.wav"));
        assert_eq!((g.artist.as_str(), g.title.as_str()), ("", "untitled"));
        assert!(!g.tagged && g.track.is_none());
    }

    #[test]
    fn bpm_tags() {
        assert_eq!(parse_bpm("128"), Some(128.0));
        assert_eq!(parse_bpm(" 127,5 BPM"), Some(127.5));
        assert_eq!(parse_bpm("0"), None);
        assert_eq!(parse_bpm("fast"), None);
    }

    #[test]
    fn hidden_folders() {
        assert!(hidden_dir(".git") && hidden_dir("PIONEER") && hidden_dir("PioneerDJ") && hidden_dir("rekordbox"));
        assert!(!hidden_dir("Techno"));
    }

    #[test]
    fn lists_audio_files_and_folders() {
        let dir = std::env::temp_dir().join(format!("od-explorer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::create_dir_all(dir.join("PIONEER")).unwrap();
        for f in ["b.mp3", "A.flac", "notes.txt", ".x.wav"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let (dirs, files) = list_dir(&dir);
        assert_eq!(dirs, vec![dir.join("Sub")]);
        assert_eq!(files, vec![dir.join("A.flac"), dir.join("b.mp3")]);

        let mut st = ExplorerState { roots: vec![dir.clone()], ..Default::default() };
        let rows = st.rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].root && rows[0].has_children && !rows[0].expanded);
        st.expanded.insert(dir.clone());
        let rows = st.rows();
        assert_eq!(rows.iter().map(|r| r.name.to_string()).collect::<Vec<_>>()[1], "Sub");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(st.path_of(1), Some(dir.join("Sub")));
        // "Sub" is listed on expand only; until then it may have children.
        assert!(rows[1].has_children);
        st.expanded.insert(dir.join("Sub"));
        assert!(!st.rows()[1].has_children);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sorting_keeps_selection_on_files() {
        let mut st = ExplorerState::default();
        st.files = vec![
            ExplorerFile { path: "/b".into(), title: "Beta".into(), bpm: Some(120.0), ..Default::default() },
            ExplorerFile { path: "/a".into(), title: "Alpha".into(), bpm: Some(130.0), ..Default::default() },
        ];
        let mut sel: BTreeSet<usize> = [0].into_iter().collect();
        st.sort("title", false, &mut sel);
        assert_eq!(st.files[0].title, "Alpha");
        assert_eq!(sel.iter().copied().collect::<Vec<_>>(), vec![1]);
        st.sort("", false, &mut sel);
        assert_eq!(st.files[0].path, PathBuf::from("/a"));
        let rows = file_rows(&st.files, &sel, None, od_core::KeyNotation::default(), &[]);
        assert_eq!(rows[0].bpm, "130.0");
        assert!(rows[1].selected);
        assert!(rows[0].id < 0);
    }
}
