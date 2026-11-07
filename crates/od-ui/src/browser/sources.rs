//! Browser sources beyond the library tree — the tag list (a holding area),
//! related tracks (what fits after a deck), play histories — plus the My Tag
//! panel, the Related panel and the reference track of the quick filter.

use super::filter::{Reference, my_tag_text, parse_my_tag};
use super::{Special, refresh_tracks};
use crate::app::{App, handler};
use crate::models::model;
use crate::{MyTagChip, SuggestionRow, TreeRow, decks, engine};
use od_core::TrackId;
use od_library::{MyTag, NodeKind, TrackRecord};
use slint::SharedString;
use std::rc::Rc;

/// Tree ids of the fixed rows and the history sessions.
pub(crate) const TAG_LIST: i32 = -3;
pub(crate) const RELATED: i32 = -4;
pub(crate) const DUPLICATES: i32 = -5;
pub(crate) const HISTORY_BASE: i32 = -2000;
const HISTORY_MAX: usize = 900_000;

/// Library meta key of the tag list (track ids, JSON).
const META_TAG_LIST: &str = "browser.tag_list";
/// Tempo window of related tracks (percent).
pub(crate) const RELATED_PCT: f64 = 6.0;
const RELATED_LIMIT: usize = 200;

/// The special source of a tree id, if it is one.
pub(crate) fn decode(id: i32, sessions: &[String]) -> Option<Special> {
    match id {
        TAG_LIST => Some(Special::TagList),
        RELATED => Some(Special::Related),
        DUPLICATES => Some(Special::Duplicates),
        _ if id <= HISTORY_BASE && id > HISTORY_BASE - HISTORY_MAX as i32 => sessions
            .get((HISTORY_BASE - id) as usize)
            .cloned()
            .map(Special::History),
        _ => None,
    }
}

pub(crate) fn history_id(i: usize) -> i32 {
    HISTORY_BASE - i as i32
}

/// Tracks of `ids` in that order (duplicates kept, removed tracks skipped).
pub(crate) fn tracks_in_order(lib: &od_library::Library, ids: &[TrackId]) -> Vec<TrackRecord> {
    ids.iter().filter_map(|id| lib.track(*id).ok()).collect()
}

/// Rows of a special source (the main library only).
pub(crate) fn rows(
    lib: &od_library::Library,
    special: &Special,
    tag_list: &[TrackId],
    related: Option<TrackId>,
) -> Vec<TrackRecord> {
    match special {
        Special::TagList => tracks_in_order(lib, tag_list),
        Special::Related => related
            .and_then(|t| lib.suggest_next(t, RELATED_PCT, RELATED_LIMIT).ok())
            .unwrap_or_default(),
        Special::History(s) => {
            let ids: Vec<TrackId> = lib
                .history(Some(s), 100_000)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|p| p.track)
                .collect();
            tracks_in_order(lib, &ids)
        }
        Special::Duplicates => tracks_in_order(
            lib,
            &duplicate_ids(&lib.find_duplicates().unwrap_or_default()),
        ),
    }
}

/// The tracks of the duplicate groups, group after group, each track once
/// (pure).
pub(crate) fn duplicate_ids(groups: &[od_library::duplicates::DuplicateGroup]) -> Vec<TrackId> {
    let mut out = Vec::new();
    for g in groups {
        add_unique(&mut out, &g.tracks);
    }
    out
}

/// The tracks "keep one of each" removes: every track of a group but its
/// first (oldest), never a track another group keeps (pure).
pub(crate) fn extra_copies(groups: &[od_library::duplicates::DuplicateGroup]) -> Vec<TrackId> {
    let kept: Vec<TrackId> = groups
        .iter()
        .filter_map(|g| g.tracks.first().copied())
        .collect();
    let mut out = Vec::new();
    for g in groups {
        let extra: Vec<TrackId> = g
            .tracks
            .iter()
            .skip(1)
            .filter(|t| !kept.contains(t))
            .copied()
            .collect();
        add_unique(&mut out, &extra);
    }
    out
}

/// Duplicates view: "remove-selected" or "keep-first" remove tracks from
/// the collection (their files stay on disk).
pub(crate) fn duplicates_action(app: &App, action: &str) {
    if app.guard_read_only() {
        return;
    }
    let ids = match action {
        "remove-selected" => app.state.borrow().selected_ids(),
        "keep-first" => extra_copies(&app.state.borrow().lib.find_duplicates().unwrap_or_default()),
        _ => return,
    };
    if ids.is_empty() {
        return;
    }
    let mut removed = 0;
    for id in &ids {
        let r = app.state.borrow_mut().lib.remove_track(*id);
        if app.report(r).is_some() {
            removed += 1;
        }
    }
    {
        let mut st = app.state.borrow_mut();
        st.browser.selection.clear();
        st.browser.tag_list.retain(|t| !ids.contains(t));
    }
    app.refresh_library();
    app.status(od_i18n::trf(
        "Removed {} track(s) from the collection — the files stay on disk",
        &[&removed],
    ));
}

/// Adds tracks to the tag list (each track once, in order of adding).
pub(crate) fn add_unique(list: &mut Vec<TrackId>, ids: &[TrackId]) -> usize {
    let mut n = 0;
    for id in ids {
        if !list.contains(id) {
            list.push(*id);
            n += 1;
        }
    }
    n
}

pub(crate) fn load_tag_list(lib: &od_library::Library) -> Vec<TrackId> {
    lib.meta(META_TAG_LIST)
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str::<Vec<i64>>(&j).ok())
        .unwrap_or_default()
        .into_iter()
        .map(TrackId)
        .collect()
}

fn save_tag_list(app: &App) {
    let mut st = app.state.borrow_mut();
    let ids: Vec<i64> = st.browser.tag_list.iter().map(|t| t.0).collect();
    if let Err(e) = st.lib.set_meta(
        META_TAG_LIST,
        &serde_json::to_string(&ids).unwrap_or_default(),
    ) {
        tracing::warn!(error = %e, "tag list not saved");
    }
}

/// Adds the selected tracks to the tag list.
pub(crate) fn add_selected_to_tag_list(app: &App) {
    if app.guard_read_only() {
        return;
    }
    let ids = app.state.borrow().selected_ids();
    if ids.is_empty() {
        app.status("Select tracks to add them to the tag list");
        return;
    }
    let n = add_unique(&mut app.state.borrow_mut().browser.tag_list, &ids);
    save_tag_list(app);
    app.refresh_library();
    app.status(od_i18n::trf("Added {} track(s) to the tag list", &[&n]));
}

/// Removes rows from the open tag list (positions in the list).
pub(crate) fn remove_from_tag_list(app: &App, positions: &[usize]) {
    {
        let mut st = app.state.borrow_mut();
        let ids: Vec<TrackId> = positions
            .iter()
            .filter_map(|&i| st.browser.rows.get(i))
            .map(|t| t.id)
            .collect();
        st.browser.tag_list.retain(|t| !ids.contains(t));
        st.browser.selection.clear();
    }
    save_tag_list(app);
    app.refresh_library();
}

/// "save": the tag list becomes a playlist; "clear": empties it.
fn tag_list_action(app: &App, action: &str) {
    match action {
        "save" => {
            let ids = app.state.borrow().browser.tag_list.clone();
            if ids.is_empty() {
                return;
            }
            let r = app.state.borrow_mut().lib.create_node(
                None,
                NodeKind::Playlist,
                od_i18n::tr("Tag list"),
            );
            let Some(node) = app.report(r) else { return };
            let r = app.state.borrow_mut().lib.add_to_node(node, &ids, None);
            if app.report(r).is_some() {
                app.after_edit(&od_i18n::trf(
                    "Saved the tag list as a playlist ({} tracks)",
                    &[&ids.len()],
                ));
            }
        }
        "clear" => {
            {
                let mut st = app.state.borrow_mut();
                st.browser.tag_list.clear();
                st.browser.selection.clear();
            }
            save_tag_list(app);
            app.after_edit("Tag list cleared");
        }
        _ => {}
    }
}

/// The deck a reference comes from: `deck` (0-based) or the master deck,
/// else the first loaded deck. Stick tracks do not count.
fn deck_track(app: &App, deck: Option<usize>) -> Option<(usize, TrackId)> {
    let d = app.decks.borrow();
    if d.source_is_stick() {
        return None;
    }
    if let Some(i) = deck {
        return d.loaded_id(i).map(|t| (i, t));
    }
    let snap = engine::snapshot(app);
    let master = snap.as_ref().and_then(|s| {
        (0..d.deck_count)
            .find(|&i| s.decks.get(i).is_some_and(|x| x.is_master) && d.loaded_id(i).is_some())
    });
    master
        .or_else(|| (0..d.deck_count).find(|&i| d.loaded_id(i).is_some()))
        .and_then(|i| d.loaded_id(i).map(|t| (i, t)))
}

/// The filter's reference: the master deck (its playing tempo), else the
/// selected track.
pub(crate) fn resolve_reference(app: &App) -> Option<Reference> {
    let (track, bpm, who) = match deck_track(app, None) {
        Some((i, t)) => {
            let live = engine::snapshot(app)
                .and_then(|s| s.decks.get(i).map(|d| d.bpm))
                .filter(|b| *b > 0.0);
            (t, live, od_i18n::tr("MASTER").to_string())
        }
        None => (
            app.state.borrow().player.current?,
            None,
            od_i18n::tr("SELECTED").to_string(),
        ),
    };
    let st = app.state.borrow();
    let rec = st.lib.track(track).ok()?;
    let bpm = bpm.or(rec.bpm);
    let key = rec.key;
    let label = format!(
        "{who} {} · {}",
        bpm.map(|b| format!("{b:.1}")).unwrap_or_else(|| "—".into()),
        key.map(|k| k.format(st.settings.key_notation()))
            .unwrap_or_else(|| "—".into())
    );
    Some(Reference { bpm, key, label })
}

/// Picks the track the Related list follows (deck, master, or the selected
/// track when no deck is loaded).
pub(crate) fn resolve_related(app: &App) {
    let source = app.state.borrow().browser.related_source.clone();
    let deck = source.parse::<usize>().ok().map(|n| n.saturating_sub(1));
    let track = deck_track(app, deck)
        .map(|(_, t)| t)
        .or_else(|| app.state.borrow().player.current);
    app.state.borrow_mut().browser.related_track = track;
}

/// Sidebar rows of the play history sessions.
pub(crate) fn history_rows(
    lib: &od_library::Library,
    year: Option<&str>,
) -> (Vec<String>, Vec<TreeRow>) {
    let sessions: Vec<(String, String, u32)> = lib
        .history_sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| year.is_none_or(|y| s.1.starts_with(y)))
        .collect();
    let names = sessions
        .iter()
        .map(|s| s.0.clone())
        .take(HISTORY_MAX)
        .collect();
    let rows = sessions
        .iter()
        .take(HISTORY_MAX)
        .enumerate()
        .map(|(i, (name, _, n))| TreeRow {
            id: history_id(i),
            name: name.clone().into(),
            kind: "history".into(),
            count: *n as i32,
            ..Default::default()
        })
        .collect();
    (names, rows)
}

/// Years with play history, oldest first ("2025", "2026").
pub(crate) fn history_years(lib: &od_library::Library) -> Vec<String> {
    let mut years: Vec<String> = lib
        .history_sessions()
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.1.get(..4).map(str::to_string))
        .collect();
    years.sort();
    years.dedup();
    years
}

/// The year the Histories tree shows: the chosen one while it has
/// sessions, else the latest.
pub(crate) fn history_year(years: &[String], chosen: Option<&str>) -> Option<String> {
    chosen
        .filter(|c| years.iter().any(|y| y == c))
        .map(str::to_string)
        .or_else(|| years.last().cloned())
}

/// My Tag panel chips: every value in the library, `on` when every
/// selected track carries it.
pub(crate) fn my_tag_chips(
    catalog: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, u32>>,
    selected: &[Vec<MyTag>],
) -> Vec<MyTagChip> {
    let mut out = Vec::new();
    for (cat, values) in catalog {
        for (i, (v, n)) in values.iter().enumerate() {
            let tag = MyTag::new(cat, v);
            out.push(MyTagChip {
                category: cat.into(),
                value: v.into(),
                count: *n as i32,
                on: !selected.is_empty() && selected.iter().all(|t| t.contains(&tag)),
                first: i == 0,
            });
        }
    }
    out
}

/// Tags or untags: when every track has `tag` it is removed, else added.
pub(crate) fn toggle_tag(selected: &[Vec<MyTag>], tag: &MyTag) -> Vec<Vec<MyTag>> {
    let all = !selected.is_empty() && selected.iter().all(|t| t.contains(tag));
    selected
        .iter()
        .map(|t| {
            let mut t = t.clone();
            t.retain(|x| x != tag);
            if !all {
                t.push(tag.clone());
                t.sort();
            }
            t
        })
        .collect()
}

fn set_tag(app: &App, tag: MyTag, force_on: bool) {
    if app.guard_read_only() {
        return;
    }
    let ids = app.state.borrow().selected_ids();
    if ids.is_empty() {
        app.status("Select tracks to tag them");
        return;
    }
    let current: Vec<Vec<MyTag>> = ids
        .iter()
        .map(|id| app.state.borrow().lib.my_tags(*id).unwrap_or_default())
        .collect();
    let next = if force_on {
        current
            .iter()
            .map(|t| {
                let mut t = t.clone();
                if !t.contains(&tag) {
                    t.push(tag.clone());
                }
                t
            })
            .collect()
    } else {
        toggle_tag(&current, &tag)
    };
    for (id, (old, new)) in ids.iter().zip(current.iter().zip(next)) {
        if *old != new {
            let r = app.state.borrow_mut().lib.set_my_tags(*id, new);
            if app.report(r).is_none() {
                return;
            }
        }
    }
    app.after_edit(&od_i18n::trf("My Tag {} updated", &[&my_tag_text(&tag)]));
}

/// Projects the My Tag panel, the filter's My Tag menu, the Related panel,
/// the tag list count and the histories.
pub(crate) fn refresh(app: &App) {
    let ui = &app.ui;
    let (catalog, selected, current, read_only) = {
        let st = app.state.borrow();
        let lib = st.src();
        let catalog = lib.my_tag_catalog().unwrap_or_default();
        let ids = st.selected_ids();
        let selected: Vec<Vec<MyTag>> = ids
            .iter()
            .map(|id| st.browser.my_tags.get(id).cloned().unwrap_or_default())
            .collect();
        (catalog, selected, st.player.current, st.read_only())
    };
    let options: Vec<SharedString> = catalog
        .iter()
        .flat_map(|(c, vs)| {
            vs.keys()
                .map(move |v| SharedString::from(format!("{c}: {v}")))
        })
        .collect();
    ui.set_my_tag_options(model(options));
    ui.set_my_tag_chips(model(my_tag_chips(&catalog, &selected)));
    let st = app.state.borrow();
    ui.set_tag_list_count(st.browser.tag_list.len() as i32);
    ui.set_related_source(st.browser.related_source.clone().into());
    if ui.get_related_open() && !read_only {
        let (reference, rows) = match current.and_then(|t| st.lib.track(t).ok()) {
            Some(t) => {
                let rows = st
                    .lib
                    .suggest_next(t.id, RELATED_PCT, 40)
                    .unwrap_or_default();
                (t.tags.title.clone(), rows)
            }
            None => (String::new(), Vec::new()),
        };
        let rows: Vec<SuggestionRow> = rows
            .iter()
            .map(|t| SuggestionRow {
                id: t.id.0 as i32,
                title: t.tags.title.clone().into(),
                artist: t.tags.artist.clone().into(),
                bpm: t.bpm.map(|b| format!("{b:.1}")).unwrap_or_default().into(),
                key: st.key_text(t).into(),
            })
            .collect();
        ui.set_suggestion_reference(reference.into());
        ui.set_suggestions(model(rows));
    }
    if st.browser.special == Some(Special::Related) {
        ui.set_related_count(st.browser.rows.len() as i32);
    }
}

/// Opens a special source in the list.
pub(crate) fn open(app: &App, id: i32) {
    let sessions = app.state.borrow().browser.histories.clone();
    let Some(special) = decode(id, &sessions) else {
        return;
    };
    {
        let mut st = app.state.borrow_mut();
        st.explorer.close();
        if st.browser.stick.take().is_some() {
            st.player.current = None;
        }
        st.browser.node = None;
        st.browser.special = Some(special.clone());
        st.browser.selection.clear();
        st.browser.flow_selected = None;
        st.browser.search.clear();
        st.browser.reset_sort(true);
    }
    if special == Special::Related {
        if app.state.borrow().browser.related_source == "track" {
            app.state.borrow_mut().browser.related_source = "master".into();
        }
        resolve_related(app);
    }
    app.ui.set_search_text(SharedString::new());
    app.ui.set_section("library".into());
    app.refresh_library();
    crate::explorer::refresh_tree(app);
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.on_add_to_tag_list(handler!(app, |a| add_selected_to_tag_list(&a)));
    ui.on_tag_list_action(handler!(app, |a, action: SharedString| tag_list_action(
        &a,
        action.as_str()
    )));
    ui.on_related_source_set(handler!(app, |a, source: SharedString| {
        a.state.borrow_mut().browser.related_source = source.to_string();
        resolve_related(&a);
        a.refresh_library();
    }));
    ui.on_my_tag_toggle(handler!(
        app,
        |a, category: SharedString, value: SharedString| set_tag(
            &a,
            MyTag::new(&category, &value),
            false
        )
    ));
    ui.on_my_tag_add(handler!(app, |a, text: SharedString| {
        match parse_my_tag(&text) {
            Some(tag) => set_tag(&a, tag, true),
            None => a.status("Type a My Tag as Category: Value, e.g. Mood: Peak"),
        }
    }));
    ui.on_panels_changed(handler!(app, |a| refresh(&a)));
    ui.on_suggestion_activated(handler!(app, |a, id: i32| {
        let playing: Vec<bool> = match engine::snapshot(&a) {
            Some(s) => s.decks.iter().map(|d| d.playing).collect(),
            None => vec![false; decks::DECKS],
        };
        let deck = {
            let d = a.decks.borrow();
            decks::free_deck(d.deck_count, &playing, |i| d.loaded_id(i).is_some())
        };
        decks::load_track(&a, deck, TrackId(id as i64), None);
    }));
    ui.on_suggestion_open(handler!(app, |a| {
        let current = a.state.borrow().player.current;
        open(&a, RELATED);
        a.state.borrow_mut().browser.related_track = current;
        a.state.borrow_mut().browser.related_source = "track".into();
        a.refresh_library();
        refresh_tracks(&a);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_rows_and_extra_copies() {
        use od_library::duplicates::{DuplicateGroup, DuplicateKind};
        let g = |ids: &[i64]| DuplicateGroup {
            kind: DuplicateKind::SameRelease,
            tracks: ids.iter().map(|i| TrackId(*i)).collect(),
        };
        let groups = vec![g(&[1, 4]), g(&[2, 5, 6]), g(&[4, 7])];
        assert_eq!(duplicate_ids(&groups), [1, 4, 2, 5, 6, 7].map(TrackId));
        // 4 is kept by the third group, so the first group does not remove it.
        assert_eq!(extra_copies(&groups), [5, 6, 7].map(TrackId));
        assert!(extra_copies(&[]).is_empty());
    }

    #[test]
    fn tag_list_and_ids() {
        let mut list = vec![TrackId(1)];
        assert_eq!(
            add_unique(&mut list, &[TrackId(1), TrackId(2), TrackId(3), TrackId(2)]),
            2
        );
        assert_eq!(list, [TrackId(1), TrackId(2), TrackId(3)]);
        let sessions = vec!["a".to_string(), "b".to_string()];
        assert_eq!(decode(TAG_LIST, &sessions), Some(Special::TagList));
        assert_eq!(decode(RELATED, &sessions), Some(Special::Related));
        assert_eq!(decode(DUPLICATES, &sessions), Some(Special::Duplicates));
        assert_eq!(
            decode(history_id(1), &sessions),
            Some(Special::History("b".into()))
        );
        assert_eq!(decode(history_id(5), &sessions), None);
        assert_eq!(decode(-1, &sessions), None);
        // No clash with the stick ids of the tree.
        assert_eq!(crate::browser::tree::decode_stick_id(history_id(0)), None);
        assert_eq!(crate::browser::tree::decode_stick_id(TAG_LIST), None);
    }

    #[test]
    fn my_tag_toggle_and_chips() {
        let peak = MyTag::new("Mood", "Peak");
        let a = vec![peak.clone()];
        let b = vec![MyTag::new("Mood", "Warm")];
        // Not all have it: added to every track.
        let next = toggle_tag(&[a.clone(), b.clone()], &peak);
        assert!(next.iter().all(|t| t.contains(&peak)));
        // All have it: removed from every track.
        let next = toggle_tag(&next, &peak);
        assert!(next.iter().all(|t| !t.contains(&peak)));
        assert_eq!(next[1], b);
        let mut cat = std::collections::BTreeMap::new();
        cat.entry("Mood".to_string())
            .or_insert_with(std::collections::BTreeMap::new)
            .insert("Peak".to_string(), 3);
        cat.entry("Mood".to_string())
            .or_insert_with(std::collections::BTreeMap::new)
            .insert("Warm".to_string(), 1);
        let chips = my_tag_chips(&cat, &[a]);
        assert_eq!(chips.len(), 2);
        assert!(chips[0].first && chips[0].on && chips[0].count == 3);
        assert!(!chips[1].first && !chips[1].on);
    }
}
