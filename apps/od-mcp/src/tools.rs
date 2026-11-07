//! The MCP tools: argument types (their JSON schemas are derived from these
//! structs and their doc comments), and one function per tool.
//!
//! Every write tool runs as one [`Library::edit_with`] call: one write lock,
//! one undo step labelled "MCP: …" in the app's edit history, and change-log
//! rows so that USB sticks holding the touched lists are synced as usual.

use crate::view::{self, color_name, mmss, node_brief, node_tree, parse_color, round, track_full, track_summary};
use od_core::{MusicalKey, TrackId};
use od_library::journal::EntityState;
use od_library::sets::{Fit, bpm_change_pct};
use od_library::smart::{Field, Op, Rule};
use od_library::{Entry, Library, LibraryError, MyTag, NodeId, NodeKind, SetMeta, SortKey, Subtree, TrackRecord, smart_text};
use rmcp::model::{JsonObject, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// Result of a tool: structured data plus a one-line summary for the chat.
pub struct ToolOut {
    pub data: Value,
    pub summary: String,
}

pub type ToolResult = Result<ToolOut, String>;

pub(crate) fn out(summary: impl Into<String>, data: Value) -> ToolResult {
    Ok(ToolOut { data, summary: summary.into() })
}

fn lib_err(e: LibraryError) -> String {
    match e {
        LibraryError::NotFound(what) => format!("not found: {what}"),
        LibraryError::Invalid(why) => why,
        e => e.to_string(),
    }
}

type Runner = Box<dyn Fn(&mut Library, Value) -> ToolResult + Send + Sync>;

pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub schema: Arc<JsonObject>,
    pub read_only: bool,
    pub destructive: bool,
    run: Runner,
}

impl ToolDef {
    pub fn run(&self, lib: &mut Library, args: Value) -> ToolResult {
        (self.run)(lib, args)
    }

    pub fn to_tool(&self) -> rmcp::model::Tool {
        rmcp::model::Tool::new(self.name, self.description, self.schema.clone()).with_title(self.title).annotate(
            ToolAnnotations::new()
                .read_only(self.read_only)
                .destructive(self.destructive)
                .idempotent(self.read_only)
                .open_world(false),
        )
    }
}

pub(crate) fn def<P: DeserializeOwned + JsonSchema + 'static>(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
    f: fn(&mut Library, P) -> ToolResult,
) -> ToolDef {
    ToolDef {
        name,
        title,
        description,
        schema: rmcp::handler::server::common::schema_for_type::<P>(),
        read_only,
        destructive,
        run: Box::new(move |lib, args| {
            let args = if args.is_null() { json!({}) } else { args };
            let p: P = serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))?;
            f(lib, p)
        }),
    }
}

pub fn all() -> Vec<ToolDef> {
    vec![
        def("library_overview", "Library overview",
            "Start here. Counts, genres, BPM and key distribution, ratings, My Tag catalog, the playlist/set tree (ids) and history sessions.",
            true, false, library_overview),
        def("search_tracks", "Search tracks",
            "Find tracks by text (title/artist/album/genre/label/comment) and filters: BPM range, exact key or harmonic compatibility (Camelot ±1 and relative), genres, rating, colour, My Tag, energy, duration, played/unplayed, recently added, within a list. Returns compact track lines; use get_tracks for details.",
            true, false, search_tracks),
        def("get_tracks", "Get tracks",
            "Every field of up to 50 tracks: tags, rating, colour, comment, My Tag, BPM and grid, key in Camelot/Open Key/musical with compatible keys, duration, loudness, energy mean and curve, phrases (intro/verse/chorus/…, with times and bars), hot and memory cues, play count, last played, date added, file info.",
            true, false, get_tracks),
        def("list_nodes", "List playlists and sets",
            "The tree of folders, playlists, smart lists and sets with ids, track counts and smart rules.",
            true, false, list_nodes),
        def("get_node", "Get playlist or set",
            "One folder, playlist, set or smart list. Sets and playlists return ordered entries (0-based positions) with transition notes and backup tracks; smart lists return the rule and current matches.",
            true, false, get_node),
        def("harmonic_neighbours", "Harmonic neighbours",
            "Tracks that mix well after (or before) a given track: within ±bpm_tolerance_pct (half/double time counted), harmonically close keys, optional energy direction. Scored 0–100, best first, with the tempo change and key relation for each.",
            true, false, harmonic_neighbours),
        def("set_flow", "Set flow",
            "Analyse a set or playlist in order: start time, BPM, key and energy per position, every transition rated (tempo/key fit, energy jump) with warnings for clashes, repeats and missing analysis, and the existing transition notes.",
            true, false, set_flow),
        def("history", "Play history",
            "Without session: the history sessions (newest first) and the most recent plays. With session: the plays of that session in order.",
            true, false, history),
        def("create_set", "Create set",
            "Create a set (ordered, with transition notes), optionally inside a folder and already filled with tracks in order. One undo step in the app.",
            false, false, create_set),
        def("create_playlist", "Create playlist",
            "Create a playlist, optionally inside an existing folder or a new folder, optionally filled with tracks. One undo step.",
            false, false, create_playlist),
        def("add_tracks", "Add tracks",
            "Insert tracks into a playlist or set at a 0-based position (default: append). One undo step.",
            false, false, add_tracks),
        def("remove_entries", "Remove entries",
            "Remove entries from a playlist or set by 0-based position and/or by track id (all occurrences). The tracks stay in the collection. One undo step.",
            false, false, remove_entries),
        def("reorder", "Reorder entries",
            "Reorder a playlist or set: either `order` (the full new order as a permutation of current positions) or `from` positions moved to `to`. Notes and backups move with their entries. One undo step.",
            false, false, reorder),
        def("set_transition_note", "Set transition note",
            "Write the note for the transition from the entry at `position` into the next one (e.g. \"long blend over the outro, cut bass at drop\"), optionally with backup tracks for that position. One undo step.",
            false, false, set_transition_note),
        def("set_track_tags", "Set track tags",
            "Set rating (0–5), colour, comment and/or My Tag values on one or more tracks. Only the given fields change. One undo step for all tracks.",
            false, false, set_track_tags),
        def("create_smart_list", "Create smart list",
            "Create a live smart list from rule text, e.g. \"bpm 122-128, key 8A~, genre house, -genre techno, rating >= 3, added < 30d\". Fields: bpm, rating, year, plays, duration (N, A-B, >N, >=N, <N, <=N); key 8A (exact) or 8A~ (harmonic); title/artist/album/genre/label/comment/format contain (leading - negates); added < 30d (d, w, m, y). Returns the current matches. One undo step.",
            false, false, create_smart_list),
        def("rename_node", "Rename",
            "Rename a folder, playlist, set or smart list. One undo step.",
            false, false, rename_node),
        def("delete_node", "Delete",
            "Delete a folder (with everything in it), playlist, set or smart list. Tracks stay in the collection. Requires confirm=true; without it, reports what would be deleted. Undoable in the app.",
            false, true, delete_node),
    ]
    .into_iter()
    .chain(crate::lighting::tools())
    .collect()
}

// ---------------------------------------------------------------- helpers

fn parse_key(s: &str) -> Result<MusicalKey, String> {
    MusicalKey::parse(s).ok_or_else(|| format!("cannot read key \"{s}\" (use Camelot 8A, Open Key 1m or musical Am / F#)"))
}

fn node_of(lib: &Library, id: i64) -> Result<od_library::Node, String> {
    lib.node(NodeId(id)).map_err(|_| format!("no playlist, set or folder with id {id}"))
}

fn check_tracks(lib: &Library, ids: &[i64]) -> od_library::Result<Vec<TrackId>> {
    ids.iter()
        .map(|&i| lib.track(TrackId(i)).map(|t| t.id).map_err(|_| LibraryError::Invalid(format!("no track with id {i}"))))
        .collect()
}

fn require_list(n: &od_library::Node) -> od_library::Result<()> {
    if n.kind.has_entries() {
        Ok(())
    } else {
        Err(LibraryError::Invalid(format!("\"{}\" is a {}; only playlists and sets hold tracks", n.name, n.kind.as_str())))
    }
}

/// Mean energy and My Tag of all tracks, for summaries.
struct Extras {
    energy: HashMap<TrackId, f32>,
    my_tags: HashMap<TrackId, Vec<MyTag>>,
}

impl Extras {
    fn load(lib: &Library) -> Result<Self, String> {
        Ok(Self { energy: lib.energy_means().map_err(lib_err)?, my_tags: lib.all_my_tags().map_err(lib_err)? })
    }

    fn summary(&self, t: &TrackRecord) -> Value {
        track_summary(t, self.energy.get(&t.id).copied(), self.my_tags.get(&t.id))
    }
}

fn key_relation(a: MusicalKey, b: MusicalKey) -> String {
    if a == b {
        return "same key".into();
    }
    let (na, nb) = (a.camelot_number() as i32, b.camelot_number() as i32);
    if a.mode == b.mode {
        match (nb - na).rem_euclid(12) {
            1 => return "+1 (up a fifth)".into(),
            11 => return "-1 (down a fifth)".into(),
            2 => return "+2 (energy boost)".into(),
            7 => return "+7 (semitone up)".into(),
            _ => {}
        }
    } else if na == nb {
        return "relative major/minor".into();
    }
    format!("distance {}", a.camelot_distance(b))
}

// ---------------------------------------------------------------- read tools

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct NoArgs {}

fn library_overview(lib: &mut Library, _: NoArgs) -> ToolResult {
    let tracks = lib.tracks(SortKey::DateAdded, true).map_err(lib_err)?;
    let energy = lib.energy_means().map_err(lib_err)?;
    let mut genres: HashMap<String, u32> = HashMap::new();
    let mut bpm: BTreeMap<i64, u32> = BTreeMap::new();
    let mut keys: BTreeMap<String, u32> = BTreeMap::new();
    let mut ratings = [0u32; 6];
    let mut colors: BTreeMap<String, u32> = BTreeMap::new();
    let mut energy_hist = [0u32; 5];
    let (mut analysed, mut missing, mut unplayed, mut total) = (0, 0, 0, 0.0);
    for t in &tracks {
        let g = t.tags.genre.trim();
        *genres.entry(if g.is_empty() { "(none)".into() } else { g.to_string() }).or_default() += 1;
        if let Some(b) = t.bpm {
            *bpm.entry((b / 5.0).floor() as i64 * 5).or_default() += 1;
        }
        if let Some(k) = t.key {
            *keys.entry(k.to_string()).or_default() += 1;
        }
        ratings[t.tags.rating.min(5) as usize] += 1;
        if let Some(c) = t.tags.color {
            *colors.entry(color_name(c)).or_default() += 1;
        }
        if let Some(e) = energy.get(&t.id) {
            energy_hist[((e * 5.0) as usize).min(4)] += 1;
        }
        analysed += usize::from(t.analysis_version > 0);
        missing += usize::from(t.missing);
        unplayed += usize::from(t.play_count == 0);
        total += t.duration;
    }
    let mut genres: Vec<(String, u32)> = genres.into_iter().collect();
    genres.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    genres.truncate(40);
    let sessions = lib.history_sessions().map_err(lib_err)?;
    let data = json!({
        "tracks": tracks.len(),
        "analysed": analysed,
        "missing_files": missing,
        "unplayed": unplayed,
        "total_hours": round(total / 3600.0, 1),
        "genres": genres.iter().map(|(g, n)| json!([g, n])).collect::<Vec<_>>(),
        "bpm_buckets": bpm.iter().map(|(b, n)| json!([format!("{b}-{}", b + 4), n])).collect::<Vec<_>>(),
        "keys": keys,
        "ratings": ratings,
        "colors": colors,
        "energy_buckets": {"0.0-0.2": energy_hist[0], "0.2-0.4": energy_hist[1], "0.4-0.6": energy_hist[2], "0.6-0.8": energy_hist[3], "0.8-1.0": energy_hist[4]},
        "my_tag": lib.my_tag_catalog().map_err(lib_err)?,
        "nodes": node_tree(lib, None, 8).map_err(lib_err)?,
        "history_sessions": sessions.len(),
        "last_session": sessions.first().map(|s| json!({"session": s.0, "started": s.1, "plays": s.2})),
        "notes": "genres: [name, count] top 40; bpm_buckets: [range, count]; ratings: count per 0..5 stars; energy is the mean analysed energy 0..1",
    });
    out(
        format!("{} tracks ({analysed} analysed, {:.1} h), {} genres, {} lists", tracks.len(), total / 3600.0, data["genres"].as_array().map_or(0, Vec::len), lib.all_nodes().map_err(lib_err)?.len()),
        data,
    )
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct SearchArgs {
    /// Words that must all match (prefix) in title, artist, album, genre, label, remixer or comment.
    pub query: Option<String>,
    /// Minimum BPM (inclusive).
    pub bpm_min: Option<f64>,
    /// Maximum BPM (inclusive).
    pub bpm_max: Option<f64>,
    /// Also match tracks whose half or double tempo is in the BPM range.
    pub bpm_half_double: bool,
    /// Exact key in any notation (8A, 1m, Am).
    pub key: Option<String>,
    /// Only keys that mix harmonically with this key: same, ±1 on the Camelot wheel, relative major/minor.
    pub compatible_with: Option<String>,
    /// Genre must contain one of these (case-insensitive).
    pub genres: Vec<String>,
    /// Minimum rating 0–5.
    pub rating_min: Option<u8>,
    /// Maximum rating 0–5.
    pub rating_max: Option<u8>,
    /// Colour name (pink, red, orange, yellow, green, aqua, blue, purple), #rrggbb, or "none" for uncoloured tracks.
    pub color: Option<String>,
    /// My Tag values that must all be present, as "Category: Value" or just "Value".
    pub my_tags: Vec<String>,
    /// Minimum mean energy 0..1 (unanalysed tracks are excluded when an energy filter is set).
    pub energy_min: Option<f64>,
    /// Maximum mean energy 0..1.
    pub energy_max: Option<f64>,
    /// Minimum length in seconds.
    pub duration_min_s: Option<f64>,
    /// Maximum length in seconds.
    pub duration_max_s: Option<f64>,
    /// true = only tracks played at least once, false = only never-played tracks.
    pub played: Option<bool>,
    /// Only tracks added within this many days.
    pub added_within_days: Option<f64>,
    /// Only tracks in this playlist, set, smart list or folder.
    pub in_node: Option<i64>,
    /// Leave out these track ids.
    pub exclude_ids: Vec<i64>,
    /// Sort: relevance (text query), bpm, key, energy, rating, added, plays, title, artist, duration.
    pub sort: Option<String>,
    /// Sort descending.
    pub descending: bool,
    /// Maximum results (default 50, max 500).
    pub limit: Option<usize>,
    /// Skip this many results (paging).
    pub offset: usize,
}

fn search_tracks(lib: &mut Library, a: SearchArgs) -> ToolResult {
    let ex = Extras::load(lib)?;
    let query = a.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let mut tracks = match query {
        Some(q) => lib.search(q, 1_000_000).map_err(lib_err)?,
        None => lib.tracks(SortKey::DateAdded, true).map_err(lib_err)?,
    };
    if let Some(n) = a.in_node {
        node_of(lib, n)?;
        let ids: HashSet<TrackId> = lib.node_tracks(NodeId(n)).map_err(lib_err)?.iter().map(|t| t.id).collect();
        tracks.retain(|t| ids.contains(&t.id));
    }
    let key = a.key.as_deref().map(parse_key).transpose()?;
    let compat = a.compatible_with.as_deref().map(parse_key).transpose()?.map(|k| k.compatible());
    let color = a.color.as_deref().map(parse_color).transpose()?;
    let genres: Vec<String> = a.genres.iter().map(|g| g.trim().to_lowercase()).filter(|g| !g.is_empty()).collect();
    let my: Vec<(Option<String>, String)> = a
        .my_tags
        .iter()
        .map(|s| match s.split_once(':') {
            Some((c, v)) => (Some(c.trim().to_lowercase()), v.trim().to_lowercase()),
            None => (None, s.trim().to_lowercase()),
        })
        .collect();
    let added = a.added_within_days.map(|d| Rule::Cond { field: Field::AddedDaysAgo, op: Op::LessThan, value: json!(d + 1e-9) });
    let exclude: HashSet<i64> = a.exclude_ids.iter().copied().collect();
    let bpm_ok = |b: f64| {
        let in_range = |x: f64| a.bpm_min.is_none_or(|m| x >= m - 1e-9) && a.bpm_max.is_none_or(|m| x <= m + 1e-9);
        in_range(b) || (a.bpm_half_double && (in_range(b * 2.0) || in_range(b / 2.0)))
    };
    tracks.retain(|t| {
        let e = ex.energy.get(&t.id).map(|&e| e as f64);
        !exclude.contains(&t.id.0)
            && (a.bpm_min.is_none() && a.bpm_max.is_none() || t.bpm.is_some_and(bpm_ok))
            && key.is_none_or(|k| t.key == Some(k))
            && compat.is_none_or(|c| t.key.is_some_and(|k| c.contains(&k)))
            && (genres.is_empty() || genres.iter().any(|g| t.tags.genre.to_lowercase().contains(g)))
            && a.rating_min.is_none_or(|r| t.tags.rating >= r)
            && a.rating_max.is_none_or(|r| t.tags.rating <= r)
            && color.is_none_or(|c| t.tags.color == c)
            && (a.energy_min.is_none() || e.is_some_and(|e| e >= a.energy_min.unwrap_or(0.0)))
            && (a.energy_max.is_none() || e.is_some_and(|e| e <= a.energy_max.unwrap_or(1.0)))
            && a.duration_min_s.is_none_or(|d| t.duration >= d)
            && a.duration_max_s.is_none_or(|d| t.duration <= d)
            && a.played.is_none_or(|p| (t.play_count > 0) == p)
            && added.as_ref().is_none_or(|r| r.matches(t))
            && my.iter().all(|(cat, val)| {
                ex.my_tags.get(&t.id).is_some_and(|tags| {
                    tags.iter().any(|tag| {
                        tag.value.to_lowercase() == *val && cat.as_ref().is_none_or(|c| tag.category.to_lowercase() == *c)
                    })
                })
            })
    });
    let sort = a.sort.as_deref().unwrap_or(if query.is_some() { "relevance" } else { "added" });
    let energy_of = |t: &TrackRecord| ex.energy.get(&t.id).copied().unwrap_or(-1.0) as f64;
    let opt = |v: Option<f64>| v.unwrap_or(f64::MAX);
    match sort {
        "relevance" => {}
        "bpm" => tracks.sort_by(|x, y| opt(x.bpm).total_cmp(&opt(y.bpm))),
        "key" => tracks.sort_by_key(|t| t.key.map(|k| (k.camelot_number(), k.mode == od_core::Mode::Major)).unwrap_or((99, false))),
        "energy" => tracks.sort_by(|x, y| energy_of(x).total_cmp(&energy_of(y))),
        "rating" => tracks.sort_by_key(|t| t.tags.rating),
        "added" => tracks.sort_by(|x, y| y.date_added.cmp(&x.date_added)),
        "plays" => tracks.sort_by_key(|t| t.play_count),
        "title" => tracks.sort_by_key(|t| t.tags.title.to_lowercase()),
        "artist" => tracks.sort_by_key(|t| t.tags.artist.to_lowercase()),
        "duration" => tracks.sort_by(|x, y| x.duration.total_cmp(&y.duration)),
        other => return Err(format!("unknown sort \"{other}\"")),
    }
    if a.descending {
        tracks.reverse();
    }
    let total = tracks.len();
    let limit = a.limit.unwrap_or(50).clamp(1, 500);
    let page: Vec<Value> = tracks.iter().skip(a.offset).take(limit).map(|t| ex.summary(t)).collect();
    let n = page.len();
    out(
        format!("{total} tracks match; showing {n} from {}", a.offset),
        json!({"total": total, "offset": a.offset, "returned": n, "tracks": page}),
    )
}

#[derive(Deserialize, JsonSchema)]
pub struct GetTracksArgs {
    /// Track ids (max 50).
    pub ids: Vec<i64>,
}

fn get_tracks(lib: &mut Library, a: GetTracksArgs) -> ToolResult {
    if a.ids.len() > 50 {
        return Err("at most 50 ids per call".into());
    }
    let mut res = Vec::new();
    let mut unknown = Vec::new();
    for id in &a.ids {
        match lib.track(TrackId(*id)) {
            Ok(t) => res.push(track_full(lib, &t).map_err(lib_err)?),
            Err(_) => unknown.push(*id),
        }
    }
    let n = res.len();
    let mut data = json!({"tracks": res});
    if !unknown.is_empty() {
        data["unknown_ids"] = json!(unknown);
    }
    out(format!("{n} tracks{}", if unknown.is_empty() { String::new() } else { format!(", unknown ids {unknown:?}") }), data)
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct ListNodesArgs {
    /// List below this folder (default: the whole tree).
    pub parent: Option<i64>,
}

fn list_nodes(lib: &mut Library, a: ListNodesArgs) -> ToolResult {
    if let Some(p) = a.parent {
        node_of(lib, p)?;
    }
    let tree = node_tree(lib, a.parent.map(NodeId), 16).map_err(lib_err)?;
    out(format!("{} lists in total", lib.all_nodes().map_err(lib_err)?.len()), json!({"nodes": tree}))
}

#[derive(Deserialize, JsonSchema)]
pub struct GetNodeArgs {
    /// Node id from list_nodes.
    pub id: i64,
    /// Maximum tracks to return (default 200).
    #[serde(default)]
    pub limit: Option<usize>,
}

pub fn node_json(lib: &Library, id: i64, limit: usize) -> Result<Value, String> {
    let n = node_of(lib, id)?;
    let ex = Extras::load(lib)?;
    let mut v = node_brief(lib, &n).map_err(lib_err)?;
    v["parent"] = json!(n.parent.map(|p| p.0));
    v["created"] = json!(n.created);
    match n.kind {
        NodeKind::Set | NodeKind::Playlist => {
            if let Some(meta) = &n.set_meta {
                v["planned_start"] = json!(meta.planned_start);
                if !meta.notes.is_empty() {
                    v["notes"] = json!(meta.notes);
                }
            }
            let entries = lib.entries(n.id).map_err(lib_err)?;
            let tracks = lib.node_tracks(n.id).map_err(lib_err)?;
            let mut total = 0.0;
            let list: Vec<Value> = entries
                .iter()
                .zip(&tracks)
                .enumerate()
                .map(|(i, (e, t))| {
                    let mut s = ex.summary(t);
                    s["pos"] = json!(i);
                    s["start"] = json!(mmss(total));
                    total += t.duration;
                    if !e.note.is_empty() {
                        s["note"] = json!(e.note);
                    }
                    if !e.backups.is_empty() {
                        s["backups"] = json!(view::ids(&e.backups));
                    }
                    s
                })
                .take(limit)
                .collect();
            v["duration"] = json!(mmss(tracks.iter().map(|t| t.duration).sum()));
            v["entries"] = Value::Array(list);
        }
        NodeKind::Smart => {
            if let Some(r) = &n.smart_rule {
                v["rule_json"] = serde_json::to_value(r).unwrap_or(Value::Null);
            }
            let tracks = lib.node_tracks(n.id).map_err(lib_err)?;
            v["matches"] = json!(tracks.len());
            v["tracks"] = Value::Array(tracks.iter().take(limit).map(|t| ex.summary(t)).collect());
        }
        NodeKind::Folder => {
            v["children"] = Value::Array(node_tree(lib, Some(n.id), 16).map_err(lib_err)?);
        }
    }
    Ok(v)
}

fn get_node(lib: &mut Library, a: GetNodeArgs) -> ToolResult {
    let v = node_json(lib, a.id, a.limit.unwrap_or(200))?;
    out(format!("{} \"{}\"", v["kind"].as_str().unwrap_or(""), v["name"].as_str().unwrap_or("")), v)
}

#[derive(Deserialize, JsonSchema)]
pub struct NeighboursArgs {
    /// The track to mix from (or into, with direction "before").
    pub track_id: i64,
    /// Maximum tempo change in percent (default 6). Half/double time counts.
    #[serde(default)]
    pub bpm_tolerance_pct: Option<f64>,
    /// Maximum Camelot distance (0 = same key, 1 = ±1 or relative, 2 = includes +2 energy boosts; default 1).
    #[serde(default)]
    pub max_key_distance: Option<u8>,
    /// Energy direction for the next track: up, down, same or any (default any).
    #[serde(default)]
    pub energy: Option<String>,
    /// "after" (default): candidates to play after the track; "before": candidates to play before it.
    #[serde(default)]
    pub direction: Option<String>,
    /// Leave out tracks of this playlist or set (e.g. the set being built).
    #[serde(default)]
    pub exclude_node: Option<i64>,
    /// Leave out these track ids.
    #[serde(default)]
    pub exclude_ids: Vec<i64>,
    /// Genre must contain one of these.
    #[serde(default)]
    pub genres: Vec<String>,
    /// Maximum candidates (default 20, max 100).
    #[serde(default)]
    pub limit: Option<usize>,
}

fn harmonic_neighbours(lib: &mut Library, a: NeighboursArgs) -> ToolResult {
    let base = lib.track(TrackId(a.track_id)).map_err(|_| format!("no track with id {}", a.track_id))?;
    let ex = Extras::load(lib)?;
    let tol = a.bpm_tolerance_pct.unwrap_or(6.0).max(0.1);
    let max_dist = a.max_key_distance.unwrap_or(1);
    let before = matches!(a.direction.as_deref(), Some("before"));
    let mut exclude: HashSet<TrackId> = a.exclude_ids.iter().map(|&i| TrackId(i)).collect();
    exclude.insert(base.id);
    if let Some(n) = a.exclude_node {
        node_of(lib, n)?;
        exclude.extend(lib.node_tracks(NodeId(n)).map_err(lib_err)?.iter().map(|t| t.id));
    }
    let genres: Vec<String> = a.genres.iter().map(|g| g.to_lowercase()).collect();
    let base_e = ex.energy.get(&base.id).copied();
    let want = a.energy.as_deref().unwrap_or("any");
    if !["up", "down", "same", "any"].contains(&want) {
        return Err(format!("energy must be up, down, same or any, not \"{want}\""));
    }
    let Some(bpm) = base.bpm else { return Err(format!("track {} has no BPM yet (not analysed)", base.id)) };
    let mut cands: Vec<(f64, Value)> = Vec::new();
    for t in lib.tracks(SortKey::DateAdded, true).map_err(lib_err)? {
        if exclude.contains(&t.id) || t.missing {
            continue;
        }
        if !genres.is_empty() && !genres.iter().any(|g| t.tags.genre.to_lowercase().contains(g)) {
            continue;
        }
        let Some(tb) = t.bpm else { continue };
        let pct = if before { bpm_change_pct(tb, bpm) } else { bpm_change_pct(bpm, tb) };
        if pct.abs() > tol {
            continue;
        }
        let dist = match (base.key, t.key) {
            (Some(x), Some(y)) => Some(x.camelot_distance(y)),
            _ => None,
        };
        if dist.is_none_or(|d| d > max_dist) {
            continue;
        }
        let e = ex.energy.get(&t.id).copied();
        let de = match (base_e, e) {
            (Some(x), Some(y)) => Some(if before { x - y } else { y - x }),
            _ => None,
        };
        let energy_ok = match (want, de) {
            ("any", _) => true,
            (_, None) => false,
            ("up", Some(d)) => d > 0.02,
            ("down", Some(d)) => d < -0.02,
            (_, Some(d)) => d.abs() <= 0.08,
        };
        if !energy_ok {
            continue;
        }
        let key_score = match dist.unwrap_or(9) {
            0 => 40.0,
            1 => 34.0,
            2 => 18.0,
            _ => 0.0,
        };
        let tempo_score = 40.0 * (1.0 - pct.abs() / tol);
        let energy_score = match de {
            Some(d) if want == "any" => 15.0 * (1.0 - (d.abs() as f64 * 4.0).min(1.0)),
            Some(_) => 15.0,
            None => 5.0,
        };
        let score = key_score + tempo_score + energy_score + t.tags.rating as f64;
        let (from, to) = if before { (t.key, base.key) } else { (base.key, t.key) };
        let mut s = ex.summary(&t);
        s["score"] = json!(round(score, 1));
        s["bpm_change_pct"] = json!(round(pct, 1));
        s["key_relation"] = json!(match (from, to) {
            (Some(x), Some(y)) => key_relation(x, y),
            _ => "unknown".into(),
        });
        if let Some(d) = de {
            s["energy_delta"] = json!(round(d as f64, 2));
        }
        cands.push((score, s));
    }
    cands.sort_by(|x, y| y.0.total_cmp(&x.0));
    let total = cands.len();
    let limit = a.limit.unwrap_or(20).clamp(1, 100);
    let list: Vec<Value> = cands.into_iter().take(limit).map(|(_, v)| v).collect();
    out(
        format!("{total} candidates {} \"{}\" ({} BPM, {}); showing {}", if before { "before" } else { "after" }, base.tags.title, round(bpm, 1), base.key.map(|k| k.to_string()).unwrap_or("no key".into()), list.len()),
        json!({"track": ex.summary(&base), "total": total, "candidates": list}),
    )
}

#[derive(Deserialize, JsonSchema)]
pub struct NodeArg {
    /// Set or playlist id.
    pub node_id: i64,
}

fn fit(f: Fit) -> &'static str {
    match f {
        Fit::Good => "good",
        Fit::Ok => "ok",
        Fit::Clash => "clash",
        Fit::Unknown => "unknown",
    }
}

fn set_flow(lib: &mut Library, a: NodeArg) -> ToolResult {
    let n = node_of(lib, a.node_id)?;
    let flow = lib.set_flow(n.id).map_err(lib_err)?;
    let entries = if n.kind.has_entries() { lib.entries(n.id).map_err(lib_err)? } else { vec![] };
    let mut warnings: Vec<String> = Vec::new();
    let mut seen: HashMap<TrackId, usize> = HashMap::new();
    let points: Vec<Value> = flow
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if let Some(first) = seen.insert(p.track.id, i) {
                warnings.push(format!("pos {i}: \"{}\" already played at pos {first}", p.track.tags.title));
            }
            if p.bpm.is_none() || p.key.is_none() {
                warnings.push(format!("pos {i}: \"{}\" is not analysed (no BPM/key)", p.track.tags.title));
            }
            if p.track.missing {
                warnings.push(format!("pos {i}: \"{}\" file is missing", p.track.tags.title));
            }
            json!({
                "pos": i,
                "id": p.track.id.0,
                "title": p.track.tags.title,
                "artist": p.track.tags.artist,
                "start": mmss(p.start_offset),
                "bpm": p.bpm.map(|b| round(b, 1)),
                "key": p.key.map(|k| k.to_string()),
                "energy": p.energy.map(|e| round(e as f64, 2)),
            })
        })
        .collect();
    let transitions: Vec<Value> = flow
        .transitions
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (pa, pb) = (&flow.points[i], &flow.points[i + 1]);
            let de = match (pa.energy, pb.energy) {
                (Some(x), Some(y)) => Some(y - x),
                _ => None,
            };
            let mut w: Vec<String> = Vec::new();
            if t.tempo_fit == Fit::Clash {
                w.push(format!("tempo jump {:+.1}%", t.bpm_change_pct.unwrap_or(0.0)));
            }
            if t.key_fit == Fit::Clash {
                w.push(format!("key clash {} -> {}", view::key_text(pa.key), view::key_text(pb.key)));
            }
            if de.is_some_and(|d| d.abs() > 0.25) {
                w.push(format!("energy {} {:+.2}", if de.unwrap_or(0.0) > 0.0 { "jump" } else { "drop" }, de.unwrap_or(0.0)));
            }
            if !pa.track.tags.artist.is_empty() && pa.track.tags.artist == pb.track.tags.artist {
                w.push("same artist back to back".into());
            }
            for x in &w {
                warnings.push(format!("{i}->{}: {x}", i + 1));
            }
            let mut v = json!({
                "from": i,
                "to": i + 1,
                "bpm_change_pct": t.bpm_change_pct.map(|p| round(p, 1)),
                "key_distance": t.key_distance,
                "key_relation": match (pa.key, pb.key) { (Some(x), Some(y)) => key_relation(x, y), _ => "unknown".into() },
                "tempo_fit": fit(t.tempo_fit),
                "key_fit": fit(t.key_fit),
                "energy_delta": de.map(|d| round(d as f64, 2)),
            });
            if let Some(note) = entries.get(i).map(|e| e.note.as_str()).filter(|n| !n.is_empty()) {
                v["note"] = json!(note);
            }
            if !w.is_empty() {
                v["warnings"] = json!(w);
            }
            v
        })
        .collect();
    let bpms: Vec<f64> = flow.points.iter().filter_map(|p| p.bpm).collect();
    let bpm_range = (!bpms.is_empty()).then(|| {
        json!([round(bpms.iter().copied().fold(f64::MAX, f64::min), 1), round(bpms.iter().copied().fold(f64::MIN, f64::max), 1)])
    });
    let clashes = transitions.iter().filter(|t| t.get("warnings").is_some()).count();
    out(
        format!("\"{}\": {} tracks, {}, {} of {} transitions with warnings", n.name, points.len(), mmss(flow.total_duration), clashes, transitions.len()),
        json!({
            "node": {"id": n.id.0, "name": n.name, "kind": n.kind.as_str()},
            "duration": mmss(flow.total_duration),
            "bpm_range": bpm_range,
            "points": points,
            "transitions": transitions,
            "warnings": warnings,
            "legend": "tempo_fit good <=3%, ok <=6%, clash >6% (half/double time counted); key_fit good = Camelot distance 0-1, ok = 2, clash >= 3",
        }),
    )
}

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct HistoryArgs {
    /// A session name from the session list.
    pub session: Option<String>,
    /// Maximum plays (default 100).
    pub limit: Option<usize>,
}

fn history(lib: &mut Library, a: HistoryArgs) -> ToolResult {
    let ex = Extras::load(lib)?;
    let plays = lib.history(a.session.as_deref(), a.limit.unwrap_or(100).clamp(1, 2000)).map_err(lib_err)?;
    let list: Vec<Value> = plays
        .iter()
        .map(|p| {
            let mut v = match p.track.and_then(|t| lib.track(t).ok()) {
                Some(t) => ex.summary(&t),
                None => json!({"id": null, "title": "(removed from library)"}),
            };
            v["played_at"] = json!(p.played_at);
            if a.session.is_none() {
                v["session"] = json!(p.session);
            }
            if p.source != "opendecks" {
                v["source"] = json!(p.source);
            }
            v
        })
        .collect();
    let mut data = json!({"plays": list});
    if a.session.is_none() {
        let sessions = lib.history_sessions().map_err(lib_err)?;
        data["sessions"] =
            Value::Array(sessions.iter().take(50).map(|(s, at, n)| json!({"session": s, "started": at, "plays": n})).collect());
    }
    out(format!("{} plays", plays.len()), data)
}

// ---------------------------------------------------------------- write tools

fn entries_of(ids: &[TrackId]) -> Vec<Entry> {
    ids.iter().map(|&t| Entry::new(t)).collect()
}

fn created(kind: &str, l: &Library, id: NodeId) -> Value {
    json!({"created": kind, "id": id.0, "tracks": l.entries(id).map(|e| e.len()).unwrap_or(0)})
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateSetArgs {
    /// Name of the set.
    pub name: String,
    /// Folder to put it in (default: top level).
    #[serde(default)]
    pub folder_id: Option<i64>,
    /// Tracks in play order.
    #[serde(default)]
    pub track_ids: Vec<i64>,
    /// Planned start time, free text such as "23:30".
    #[serde(default)]
    pub planned_start: Option<String>,
    /// Notes for the whole set (theme, venue, energy plan).
    #[serde(default)]
    pub notes: Option<String>,
}

fn create_set(lib: &mut Library, a: CreateSetArgs) -> ToolResult {
    let name = a.name.trim().to_string();
    if name.is_empty() {
        return Err("name must not be empty".into());
    }
    let id = lib
        .edit_with("MCP: Create set", |l| {
            let tracks = check_tracks(l, &a.track_ids)?;
            let mut node = l.new_node(a.folder_id.map(NodeId), NodeKind::Set, &name)?;
            node.set_meta = Some(SetMeta { planned_start: a.planned_start.clone(), notes: a.notes.clone().unwrap_or_default() });
            let id = node.id;
            let tree = Subtree { nodes: vec![node], entries: vec![(id, entries_of(&tracks))], device_links: vec![] };
            Ok((vec![EntityState::Subtree { root: id, tree: Some(tree) }], id))
        })
        .map_err(lib_err)?;
    out(format!("Created set \"{name}\" (id {}) with {} tracks", id.0, a.track_ids.len()), created("set", lib, id))
}

#[derive(Deserialize, JsonSchema)]
pub struct CreatePlaylistArgs {
    /// Name of the playlist.
    pub name: String,
    /// Existing folder to put it in.
    #[serde(default)]
    pub folder_id: Option<i64>,
    /// Or: create a new folder with this name (inside folder_id, if given) and put the playlist in it.
    #[serde(default)]
    pub new_folder: Option<String>,
    /// Tracks in order.
    #[serde(default)]
    pub track_ids: Vec<i64>,
}

fn create_playlist(lib: &mut Library, a: CreatePlaylistArgs) -> ToolResult {
    let name = a.name.trim().to_string();
    if name.is_empty() {
        return Err("name must not be empty".into());
    }
    let new_folder = a.new_folder.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(String::from);
    let (id, folder) = lib
        .edit_with("MCP: Create playlist", |l| {
            let tracks = check_tracks(l, &a.track_ids)?;
            let parent = a.folder_id.map(NodeId);
            match &new_folder {
                None => {
                    let node = l.new_node(parent, NodeKind::Playlist, &name)?;
                    let id = node.id;
                    let tree = Subtree { nodes: vec![node], entries: vec![(id, entries_of(&tracks))], device_links: vec![] };
                    Ok((vec![EntityState::Subtree { root: id, tree: Some(tree) }], (id, None)))
                }
                Some(f) => {
                    let folder = l.new_node(parent, NodeKind::Folder, f)?;
                    let pl = od_library::Node {
                        id: NodeId(folder.id.0 + 1),
                        parent: Some(folder.id),
                        kind: NodeKind::Playlist,
                        name: name.clone(),
                        sort_order: 0,
                        smart_rule: None,
                        set_meta: None,
                        created: folder.created.clone(),
                    };
                    let (fid, pid) = (folder.id, pl.id);
                    let tree = Subtree { nodes: vec![folder, pl], entries: vec![(pid, entries_of(&tracks))], device_links: vec![] };
                    Ok((vec![EntityState::Subtree { root: fid, tree: Some(tree) }], (pid, Some(fid))))
                }
            }
        })
        .map_err(lib_err)?;
    let mut data = created("playlist", lib, id);
    if let Some(f) = folder {
        data["folder_id"] = json!(f.0);
    }
    out(format!("Created playlist \"{name}\" (id {}) with {} tracks", id.0, a.track_ids.len()), data)
}

#[derive(Deserialize, JsonSchema)]
pub struct AddTracksArgs {
    /// Playlist or set id.
    pub node_id: i64,
    /// Tracks to insert, in order.
    pub track_ids: Vec<i64>,
    /// 0-based position to insert at (default: append at the end).
    #[serde(default)]
    pub position: Option<usize>,
}

fn add_tracks(lib: &mut Library, a: AddTracksArgs) -> ToolResult {
    if a.track_ids.is_empty() {
        return Err("track_ids is empty".into());
    }
    let node = NodeId(a.node_id);
    let (at, len) = lib
        .edit_with("MCP: Add tracks", |l| {
            require_list(&l.node(node)?)?;
            let tracks = check_tracks(l, &a.track_ids)?;
            let mut entries = l.entries(node)?;
            let at = a.position.unwrap_or(entries.len()).min(entries.len());
            entries.splice(at..at, entries_of(&tracks));
            let len = entries.len();
            Ok((vec![EntityState::Entries { node, entries }], (at, len)))
        })
        .map_err(lib_err)?;
    out(
        format!("Added {} tracks at position {at}; the list now has {len}", a.track_ids.len()),
        json!({"node_id": a.node_id, "inserted_at": at, "count": a.track_ids.len(), "length": len}),
    )
}

#[derive(Deserialize, JsonSchema)]
pub struct RemoveArgs {
    /// Playlist or set id.
    pub node_id: i64,
    /// 0-based positions to remove.
    #[serde(default)]
    pub positions: Vec<usize>,
    /// Remove every entry of these tracks.
    #[serde(default)]
    pub track_ids: Vec<i64>,
}

fn remove_entries(lib: &mut Library, a: RemoveArgs) -> ToolResult {
    if a.positions.is_empty() && a.track_ids.is_empty() {
        return Err("give positions and/or track_ids".into());
    }
    let node = NodeId(a.node_id);
    let (removed, len) = lib
        .edit_with("MCP: Remove entries", |l| {
            require_list(&l.node(node)?)?;
            let entries = l.entries(node)?;
            if let Some(bad) = a.positions.iter().find(|&&p| p >= entries.len()) {
                return Err(LibraryError::Invalid(format!("position {bad} out of range (list has {})", entries.len())));
            }
            let pos: HashSet<usize> = a.positions.iter().copied().collect();
            let tracks: HashSet<i64> = a.track_ids.iter().copied().collect();
            let before = entries.len();
            let kept: Vec<Entry> = entries
                .into_iter()
                .enumerate()
                .filter(|(i, e)| !pos.contains(i) && !tracks.contains(&e.track.0))
                .map(|(_, e)| e)
                .collect();
            let (removed, len) = (before - kept.len(), kept.len());
            if removed == 0 {
                return Err(LibraryError::Invalid("nothing matched; nothing removed".into()));
            }
            Ok((vec![EntityState::Entries { node, entries: kept }], (removed, len)))
        })
        .map_err(lib_err)?;
    out(format!("Removed {removed} entries; {len} left"), json!({"node_id": a.node_id, "removed": removed, "length": len}))
}

#[derive(Deserialize, JsonSchema)]
pub struct ReorderArgs {
    /// Playlist or set id.
    pub node_id: i64,
    /// The complete new order as current 0-based positions, e.g. [2, 0, 1].
    #[serde(default)]
    pub order: Option<Vec<usize>>,
    /// Or: positions to move (keep their relative order)…
    #[serde(default)]
    pub from: Vec<usize>,
    /// …to this 0-based index in the current list (the moved block starts there).
    #[serde(default)]
    pub to: Option<usize>,
}

fn reorder(lib: &mut Library, a: ReorderArgs) -> ToolResult {
    let node = NodeId(a.node_id);
    let order = lib
        .edit_with("MCP: Reorder", |l| {
            require_list(&l.node(node)?)?;
            let entries = l.entries(node)?;
            let n = entries.len();
            let new: Vec<Entry> = if let Some(order) = &a.order {
                let mut seen = vec![false; n];
                for &p in order {
                    if p >= n || std::mem::replace(&mut seen[p], true) {
                        return Err(LibraryError::Invalid(format!("order must be a permutation of 0..{n}")));
                    }
                }
                if order.len() != n {
                    return Err(LibraryError::Invalid(format!("order must list all {n} positions")));
                }
                order.iter().map(|&p| entries[p].clone()).collect()
            } else {
                let to = a.to.ok_or_else(|| LibraryError::Invalid("give `order`, or `from` and `to`".into()))?;
                if a.from.is_empty() || a.from.iter().any(|&p| p >= n) || to > n {
                    return Err(LibraryError::Invalid(format!("positions must be within 0..{n}")));
                }
                let mut moving = Vec::new();
                let mut rest = Vec::new();
                let mut insert_at = 0;
                for (i, e) in entries.into_iter().enumerate() {
                    if a.from.contains(&i) {
                        moving.push(e);
                    } else {
                        if i < to {
                            insert_at += 1;
                        }
                        rest.push(e);
                    }
                }
                rest.splice(insert_at..insert_at, moving);
                rest
            };
            let ids: Vec<i64> = new.iter().map(|e| e.track.0).collect();
            Ok((vec![EntityState::Entries { node, entries: new }], ids))
        })
        .map_err(lib_err)?;
    out(format!("Reordered {} entries", order.len()), json!({"node_id": a.node_id, "track_order": order}))
}

#[derive(Deserialize, JsonSchema)]
pub struct NoteArgs {
    /// Set (or playlist) id.
    pub node_id: i64,
    /// 0-based position of the outgoing track; the note describes the mix into position + 1.
    pub position: usize,
    /// The note; empty clears it.
    pub note: String,
    /// Optional backup tracks for this position (replaces the current backups).
    #[serde(default)]
    pub backup_track_ids: Option<Vec<i64>>,
}

fn set_transition_note(lib: &mut Library, a: NoteArgs) -> ToolResult {
    let node = NodeId(a.node_id);
    lib.edit_with("MCP: Transition note", |l| {
        require_list(&l.node(node)?)?;
        let mut entries = l.entries(node)?;
        let n = entries.len();
        let e = entries
            .get_mut(a.position)
            .ok_or_else(|| LibraryError::Invalid(format!("position {} out of range (list has {n})", a.position)))?;
        e.note = a.note.trim().to_string();
        if let Some(b) = &a.backup_track_ids {
            e.backups = check_tracks(l, b)?;
        }
        Ok((vec![EntityState::Entries { node, entries }], ()))
    })
    .map_err(lib_err)?;
    out(
        format!("Note set for transition {} -> {}", a.position, a.position + 1),
        json!({"node_id": a.node_id, "position": a.position, "note": a.note.trim()}),
    )
}

#[derive(Deserialize, JsonSchema)]
pub struct MyTagArg {
    /// Category, e.g. "Mood".
    pub category: String,
    /// Value, e.g. "Peak time".
    pub value: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TagArgs {
    /// Tracks to change (max 500).
    pub track_ids: Vec<i64>,
    /// Rating 0–5.
    #[serde(default)]
    pub rating: Option<u8>,
    /// Colour name (pink, red, orange, yellow, green, aqua, blue, purple), #rrggbb, or "none".
    #[serde(default)]
    pub color: Option<String>,
    /// Replace the comment.
    #[serde(default)]
    pub comment: Option<String>,
    /// Replace all My Tag values with these.
    #[serde(default)]
    pub my_tags: Option<Vec<MyTagArg>>,
    /// Add these My Tag values.
    #[serde(default)]
    pub add_my_tags: Vec<MyTagArg>,
    /// Remove these My Tag values.
    #[serde(default)]
    pub remove_my_tags: Vec<MyTagArg>,
}

fn set_track_tags(lib: &mut Library, a: TagArgs) -> ToolResult {
    if a.track_ids.is_empty() || a.track_ids.len() > 500 {
        return Err("give 1 to 500 track_ids".into());
    }
    if a.rating.is_some_and(|r| r > 5) {
        return Err("rating must be 0..5".into());
    }
    let color = a.color.as_deref().map(parse_color).transpose()?;
    let tags_change = a.rating.is_some() || color.is_some() || a.comment.is_some();
    let my_change = a.my_tags.is_some() || !a.add_my_tags.is_empty() || !a.remove_my_tags.is_empty();
    if !tags_change && !my_change {
        return Err("nothing to change: give rating, color, comment or My Tag values".into());
    }
    let conv = |v: &[MyTagArg]| v.iter().map(|t| MyTag::new(&t.category, &t.value)).collect::<Vec<_>>();
    let n = lib
        .edit_with("MCP: Edit tags", |l| {
            let mut states = Vec::new();
            for id in check_tracks(l, &a.track_ids)? {
                if tags_change {
                    let mut tags = l.track(id)?.tags;
                    if let Some(r) = a.rating {
                        tags.rating = r;
                    }
                    if let Some(c) = color {
                        tags.color = c;
                    }
                    if let Some(c) = &a.comment {
                        tags.comment = c.clone();
                    }
                    states.push(EntityState::Tags { track: id, tags });
                }
                if my_change {
                    let mut mine = match &a.my_tags {
                        Some(v) => conv(v),
                        None => l.my_tags(id)?,
                    };
                    for t in conv(&a.add_my_tags) {
                        if !mine.contains(&t) {
                            mine.push(t);
                        }
                    }
                    let remove = conv(&a.remove_my_tags);
                    mine.retain(|t| !remove.contains(t));
                    mine.sort();
                    states.push(EntityState::MyTags { track: id, tags: mine });
                }
            }
            Ok((states, a.track_ids.len()))
        })
        .map_err(lib_err)?;
    let ex = Extras::load(lib)?;
    let tracks: Vec<Value> =
        a.track_ids.iter().filter_map(|&i| lib.track(TrackId(i)).ok()).take(50).map(|t| ex.summary(&t)).collect();
    out(format!("Updated {n} tracks"), json!({"updated": n, "tracks": tracks}))
}

#[derive(Deserialize, JsonSchema)]
pub struct SmartArgs {
    /// Name of the smart list.
    pub name: String,
    /// Rule text, e.g. "bpm 122-128, key 8A~, genre house, rating >= 3".
    pub rule: String,
    /// Folder to put it in.
    #[serde(default)]
    pub folder_id: Option<i64>,
}

fn create_smart_list(lib: &mut Library, a: SmartArgs) -> ToolResult {
    let rule = smart_text::parse(&a.rule)?;
    let name = a.name.trim().to_string();
    if name.is_empty() {
        return Err("name must not be empty".into());
    }
    let id = lib
        .edit_with("MCP: Create smart list", |l| {
            let mut node = l.new_node(a.folder_id.map(NodeId), NodeKind::Smart, &name)?;
            node.smart_rule = Some(rule.clone());
            let id = node.id;
            let tree = Subtree { nodes: vec![node], entries: vec![], device_links: vec![] };
            Ok((vec![EntityState::Subtree { root: id, tree: Some(tree) }], id))
        })
        .map_err(lib_err)?;
    let v = node_json(lib, id.0, 20)?;
    out(
        format!("Created smart list \"{name}\" (id {}): {} matches for \"{}\"", id.0, v["matches"], smart_text::format(&rule)),
        v,
    )
}

#[derive(Deserialize, JsonSchema)]
pub struct RenameArgs {
    /// Node id.
    pub node_id: i64,
    /// New name.
    pub name: String,
}

fn rename_node(lib: &mut Library, a: RenameArgs) -> ToolResult {
    let name = a.name.trim().to_string();
    if name.is_empty() {
        return Err("name must not be empty".into());
    }
    let old = lib
        .edit_with("MCP: Rename", |l| {
            let mut node = l.node(NodeId(a.node_id))?;
            let old = std::mem::replace(&mut node.name, name.clone());
            Ok((vec![EntityState::Node { node }], old))
        })
        .map_err(lib_err)?;
    out(format!("Renamed \"{old}\" to \"{name}\""), json!({"node_id": a.node_id, "old_name": old, "name": name}))
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteArgs {
    /// Node id.
    pub node_id: i64,
    /// Must be true to delete. Ask the DJ first.
    #[serde(default)]
    pub confirm: bool,
}

fn delete_node(lib: &mut Library, a: DeleteArgs) -> ToolResult {
    let n = node_of(lib, a.node_id)?;
    let sub = lib.descendants(n.id).map_err(lib_err)?;
    let entries: usize = sub.iter().map(|&d| lib.entries(d).map(|e| e.len()).unwrap_or(0)).sum();
    let what = format!(
        "{} \"{}\"{} ({entries} entries)",
        n.kind.as_str(),
        n.name,
        if sub.len() > 1 { format!(" with {} nested lists", sub.len() - 1) } else { String::new() }
    );
    if !a.confirm {
        return Err(format!("not deleted: this would delete {what}. Call again with confirm=true after the DJ agrees."));
    }
    lib.edit_with("MCP: Delete", |_| Ok((vec![EntityState::Subtree { root: n.id, tree: None }], ())))
        .map_err(lib_err)?;
    out(format!("Deleted {what}; undo in the app restores it"), json!({"deleted": a.node_id, "lists": sub.len(), "entries": entries}))
}
