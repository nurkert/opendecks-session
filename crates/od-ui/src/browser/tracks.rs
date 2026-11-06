//! The track list: loading and sorting rows, the table view model, the
//! PREVIEW column waveforms and the set flow.

use crate::app::App;
use crate::models::model;
use crate::state::AppState;
use crate::{FlowPoint, TrackRow, format, render, smart_text};
use od_core::TrackId;
use od_library::sets::Fit;
use od_library::{NodeKind, SortKey, TrackRecord};
use slint::{ComponentHandle, SharedString};
use std::cmp::Ordering;
use std::collections::HashSet;

/// Column name (as used by the table header) → library sort key.
pub(crate) fn sort_key(s: &str) -> Option<SortKey> {
    Some(match s {
        "date_added" => SortKey::DateAdded,
        "title" => SortKey::Title,
        "artist" => SortKey::Artist,
        "genre" => SortKey::Genre,
        "bpm" => SortKey::Bpm,
        "key" => SortKey::Key,
        "rating" => SortKey::Rating,
        "duration" => SortKey::Duration,
        _ => return None,
    })
}

/// Whether `column` is a sortable column.
pub(crate) fn sort_key_known(column: &str) -> bool {
    sort_key(column).is_some()
}

/// Initial direction when a column is first chosen for sorting.
pub(crate) fn default_descending(column: &str) -> bool {
    matches!(column, "date_added" | "rating" | "bpm")
}

/// What the list sort needs from a row (library tracks and Explorer files).
pub(crate) trait SortFields {
    fn title(&self) -> &str;
    fn artist(&self) -> &str;
    fn genre(&self) -> &str;
    fn bpm(&self) -> Option<f64>;
    fn key(&self) -> Option<od_core::MusicalKey>;
    fn rating(&self) -> u8;
    fn duration(&self) -> f64;
    fn date_added(&self) -> &str;
}

impl SortFields for TrackRecord {
    fn title(&self) -> &str {
        &self.tags.title
    }
    fn artist(&self) -> &str {
        &self.tags.artist
    }
    fn genre(&self) -> &str {
        &self.tags.genre
    }
    fn bpm(&self) -> Option<f64> {
        self.bpm
    }
    fn key(&self) -> Option<od_core::MusicalKey> {
        self.key
    }
    fn rating(&self) -> u8 {
        self.tags.rating
    }
    fn duration(&self) -> f64 {
        self.duration
    }
    fn date_added(&self) -> &str {
        &self.date_added
    }
}

/// Compares two rows by `key` (ascending).
pub(crate) fn compare<T: SortFields>(a: &T, b: &T, key: SortKey) -> Ordering {
    let text = |x: &str, y: &str| x.to_lowercase().cmp(&y.to_lowercase());
    match key {
        SortKey::Title => text(a.title(), b.title()),
        SortKey::Artist => text(a.artist(), b.artist()),
        SortKey::Genre => text(a.genre(), b.genre()),
        SortKey::Bpm => a.bpm().unwrap_or(0.0).total_cmp(&b.bpm().unwrap_or(0.0)),
        SortKey::Key => a.key().map(|k| (k.camelot_number(), k.mode as u8)).cmp(&b.key().map(|k| (k.camelot_number(), k.mode as u8))),
        SortKey::Rating => a.rating().cmp(&b.rating()),
        SortKey::Duration => a.duration().total_cmp(&b.duration()),
        _ => a.date_added().cmp(b.date_added()),
    }
}

/// Sorts `rows` in place by column name; unknown columns ("position") keep
/// the order.
pub(crate) fn sort_rows<T: SortFields>(rows: &mut [T], column: &str, descending: bool) {
    if let Some(k) = sort_key(column) {
        rows.sort_by(|a, b| {
            let o = compare(a, b, k);
            if descending { o.reverse() } else { o }
        });
    }
}

/// Whether a track matches a list-local search.
fn matches_search(t: &TrackRecord, q: &str) -> bool {
    format!("{} {} {} {}", t.tags.title, t.tags.artist, t.tags.album, t.tags.genre).to_lowercase().contains(q)
}

impl AppState {
    pub fn node_kind(&self) -> Option<NodeKind> {
        self.browser.node.and_then(|n| self.src().node(n).ok()).map(|n| n.kind)
    }

    /// Library tracks of the selected rows.
    pub fn selected_ids(&self) -> Vec<TrackId> {
        if self.explorer.open.is_some() {
            return self.explorer.selected_ids(&self.browser.selection);
        }
        self.browser.selection.iter().filter_map(|&i| self.browser.rows.get(i)).map(|t| t.id).collect()
    }

    /// Number of rows in the list on screen.
    pub fn row_count(&self) -> usize {
        if self.explorer.open.is_some() { self.explorer.files.len() } else { self.browser.rows.len() }
    }

    /// Reloads the rows of the current list, keeping the selection on the
    /// same tracks.
    pub fn load_rows(&mut self) {
        if self.explorer.open.is_some() {
            let (col, desc) = (self.browser.sort.clone(), self.browser.descending);
            self.explorer.sort(&col, desc, &mut self.browser.selection);
            return;
        }
        let b = &self.browser;
        let searching = !b.search.trim().is_empty();
        let mut rows = match b.node {
            None if searching => self.src().search(&b.search, 5000).unwrap_or_default(),
            None => self.src().tracks(sort_key(&b.sort).unwrap_or_default(), b.descending).unwrap_or_default(),
            Some(n) => {
                let mut r = self.src().node_tracks(n).unwrap_or_default();
                if searching {
                    let q = b.search.to_lowercase();
                    r.retain(|t| matches_search(t, &q));
                }
                r
            }
        };
        if b.node.is_some() || searching {
            sort_rows(&mut rows, &b.sort, b.descending);
        }
        let selected: HashSet<TrackId> = self.selected_ids().into_iter().collect();
        let b = &mut self.browser;
        b.rows = rows;
        b.selection = b.rows.iter().enumerate().filter(|(_, t)| selected.contains(&t.id)).map(|(i, _)| i).collect();
    }
}

/// Table rows compare equal when every field but the preview image matches
/// and the preview is the same size (previews are cached per track and
/// never change once rendered). Avoids comparing pixel buffers.
pub(crate) fn same_row(a: &TrackRow, b: &TrackRow) -> bool {
    a.id == b.id
        && a.position == b.position
        && a.title == b.title
        && a.artist == b.artist
        && a.album == b.album
        && a.genre == b.genre
        && a.bpm == b.bpm
        && a.key == b.key
        && a.key_distance == b.key_distance
        && a.time == b.time
        && a.rating == b.rating
        && a.analysed == b.analysed
        && a.missing == b.missing
        && a.selected == b.selected
        && a.note == b.note
        && a.deck == b.deck
        && a.preview.size() == b.preview.size()
}

/// Camelot distance between two keys, -1 if either is unknown.
pub(crate) fn key_distance(a: Option<od_core::MusicalKey>, b: Option<od_core::MusicalKey>) -> i32 {
    match (a, b) {
        (Some(a), Some(b)) => a.camelot_distance(b) as i32,
        _ => -1,
    }
}

/// Renders missing PREVIEW-column waveforms (bounded so very large lists
/// stay fast; the cache keeps them for later refreshes).
fn fill_previews(app: &App) {
    const LIMIT: usize = 3000;
    let scale = app.ui.window().scale_factor();
    let (w, h) = ((110.0 * scale) as u32, (20.0 * scale) as u32);
    let mut st = app.state.borrow_mut();
    let ro = st.read_only();
    let missing: Vec<TrackId> =
        st.browser.rows.iter().take(LIMIT).map(|t| t.id).filter(|id| !st.browser.previews.contains_key(&(ro, *id))).collect();
    for id in missing {
        let a = st.src().analysis(id).unwrap_or_default();
        if !a.waveform.columns.is_empty() {
            let img = render::preview(&a.waveform, w, h);
            st.browser.previews.insert((ro, id), img);
        }
    }
}

/// Projects the current list into the table, header and set flow.
pub(crate) fn refresh_tracks(app: &App) {
    if app.state.borrow().explorer.open.is_some() {
        crate::explorer::refresh_files(app);
        return;
    }
    fill_previews(app);
    let st = app.state.borrow();
    let b = &st.browser;
    let current_key = st.player.current.and_then(|id| b.rows.iter().find(|t| t.id == id)).and_then(|t| t.key);
    let kind = st.node_kind();
    let loaded = {
        let d = app.decks.borrow();
        if st.read_only() == d.source_is_stick() { d.loaded_decks() } else { Vec::new() }
    };
    let entries = match (b.node, kind) {
        (Some(n), Some(NodeKind::Set)) => st.src().entries(n).unwrap_or_default(),
        _ => vec![],
    };
    let rows: Vec<TrackRow> = b
        .rows
        .iter()
        .enumerate()
        .map(|(i, t)| TrackRow {
            preview: b.previews.get(&(st.read_only(), t.id)).cloned().unwrap_or_default(),
            id: t.id.0 as i32,
            position: i as i32 + 1,
            title: t.tags.title.clone().into(),
            artist: t.tags.artist.clone().into(),
            album: t.tags.album.clone().into(),
            genre: if kind == Some(NodeKind::Set) {
                entries.get(i).map(|e| e.note.clone()).unwrap_or_default().into()
            } else {
                t.tags.genre.clone().into()
            },
            bpm: t.bpm.map(|b| format!("{b:.1}")).unwrap_or_default().into(),
            key: st.key_text(t).into(),
            key_distance: key_distance(current_key, t.key),
            time: format::time(t.duration).into(),
            rating: t.tags.rating as i32,
            analysed: t.analysis_version >= od_analysis::ANALYSIS_VERSION,
            missing: t.missing,
            selected: b.selection.contains(&i),
            note: SharedString::new(),
            deck: loaded.iter().find(|(id, _)| *id == t.id).map(|(_, d)| *d as i32 + 1).unwrap_or(0),
        })
        .collect();
    let total: f64 = b.rows.iter().map(|t| t.duration).sum();
    app.browser.tracks.sync(rows, same_row);
    let ui = &app.ui;
    ui.set_missing_count(st.lib.conn().query_row("SELECT count(*) FROM track WHERE missing = 1", [], |r| r.get::<_, i32>(0)).unwrap_or(0));
    ui.set_sort_column(b.sort.clone().into());
    ui.set_sort_descending(b.descending);
    ui.set_has_selection(!b.selection.is_empty());
    ui.set_selection_count(b.selection.len() as i32);
    let (title, kind_str) = match (b.node.and_then(|n| st.src().node(n).ok()), &b.stick) {
        (Some(n), _) => (n.name.clone(), n.kind.as_str()),
        (None, Some((i, _))) => (b.sticks.get(*i).map(|s| s.0.clone()).unwrap_or_default(), "stick"),
        (None, None) => (od_i18n::tr("All tracks").to_string(), "all"),
    };
    ui.set_node_title(title.clone().into());
    ui.set_node_kind(kind_str.into());
    ui.set_node_sub(od_i18n::trf("{} TRACKS  ·  {}", &[&b.rows.len(), &format::time(total)]).into());
    ui.set_read_only(st.read_only());
    ui.set_register_target(if kind_str == "all" || st.read_only() { SharedString::new() } else { title.to_uppercase().into() });
    if kind == Some(NodeKind::Smart) {
        let rule = b.node.and_then(|n| st.src().node(n).ok()).and_then(|n| n.smart_rule);
        ui.set_smart_rule_text(rule.map(|r| smart_text::format(&r)).unwrap_or_default().into());
    }
    if let (Some(n), Some(NodeKind::Set)) = (b.node, kind) {
        if let Ok(flow) = st.src().set_flow(n) {
            ui.set_flow(model(flow_points(&flow, st.settings.key_notation())));
            ui.set_flow_total(format::time(flow.total_duration).into());
        }
        ui.set_flow_selected(b.flow_selected.map(|i| i as i32).unwrap_or(-1));
        let note = b.flow_selected.and_then(|i| entries.get(i)).map(|e| e.note.clone()).unwrap_or_default();
        ui.set_transition_note(note.into());
    }
}

fn fit_name(f: Fit) -> &'static str {
    match f {
        Fit::Good => "good",
        Fit::Ok => "ok",
        Fit::Clash => "clash",
        Fit::Unknown => "unknown",
    }
}

/// Set flow points, energy scaled to the set so differences are visible.
pub(crate) fn flow_points(flow: &od_library::sets::SetFlow, notation: od_core::KeyNotation) -> Vec<FlowPoint> {
    let mut pts: Vec<FlowPoint> = flow
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let tr = flow.transitions.get(i);
            FlowPoint {
                title: p.track.tags.title.clone().into(),
                bpm: p.bpm.map(|b| format!("{b:.0}")).unwrap_or_default().into(),
                key: p.key.map(|k| k.format(notation)).unwrap_or_default().into(),
                energy: p.energy.unwrap_or(0.3),
                start: format::time(p.start_offset).into(),
                tempo_fit: tr.map(|t| fit_name(t.tempo_fit)).unwrap_or("none").into(),
                key_fit: tr.map(|t| fit_name(t.key_fit)).unwrap_or("none").into(),
                change: tr.and_then(|t| t.bpm_change_pct).map(|p| format!("{p:+.1}%")).unwrap_or_default().into(),
            }
        })
        .collect();
    let mut energies: Vec<f32> = pts.iter().map(|p| p.energy).collect();
    normalize_energy(&mut energies);
    for (p, e) in pts.iter_mut().zip(energies) {
        p.energy = e;
    }
    pts
}

/// Rescales energies to 0.15..1 across the set (unchanged when flat).
pub(crate) fn normalize_energy(e: &mut [f32]) {
    let (lo, hi) = e.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    if hi > lo {
        for x in e.iter_mut() {
            *x = 0.15 + 0.85 * (*x - lo) / (hi - lo);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct R(&'static str, f64, &'static str);
    impl SortFields for R {
        fn title(&self) -> &str {
            self.0
        }
        fn artist(&self) -> &str {
            ""
        }
        fn genre(&self) -> &str {
            ""
        }
        fn bpm(&self) -> Option<f64> {
            Some(self.1)
        }
        fn key(&self) -> Option<od_core::MusicalKey> {
            None
        }
        fn rating(&self) -> u8 {
            0
        }
        fn duration(&self) -> f64 {
            self.1
        }
        fn date_added(&self) -> &str {
            self.2
        }
    }

    #[test]
    fn sorting() {
        let mut v = vec![R("beta", 124.0, "2"), R("Alpha", 128.0, "1"), R("gamma", 120.0, "3")];
        sort_rows(&mut v, "title", false);
        assert_eq!(v.iter().map(|r| r.0).collect::<Vec<_>>(), ["Alpha", "beta", "gamma"]);
        sort_rows(&mut v, "bpm", true);
        assert_eq!(v.iter().map(|r| r.1).collect::<Vec<_>>(), [128.0, 124.0, 120.0]);
        sort_rows(&mut v, "date_added", false);
        assert_eq!(v.iter().map(|r| r.2).collect::<Vec<_>>(), ["1", "2", "3"]);
        // "position" keeps the order.
        sort_rows(&mut v, "position", true);
        assert_eq!(v.iter().map(|r| r.2).collect::<Vec<_>>(), ["1", "2", "3"]);
        assert!(default_descending("bpm") && !default_descending("title"));
    }

    #[test]
    fn energy_is_scaled_to_the_set() {
        let mut e = [0.2, 0.4, 0.6];
        normalize_energy(&mut e);
        assert!((e[0] - 0.15).abs() < 1e-6 && (e[2] - 1.0).abs() < 1e-6);
        let mut flat = [0.3, 0.3];
        normalize_energy(&mut flat);
        assert_eq!(flat, [0.3, 0.3]);
    }

    #[test]
    fn rows_compare_without_pixels() {
        let a = TrackRow { id: 1, title: "x".into(), ..Default::default() };
        let mut b = a.clone();
        assert!(same_row(&a, &b));
        b.selected = true;
        assert!(!same_row(&a, &b));
        assert_eq!(key_distance(None, None), -1);
    }
}
