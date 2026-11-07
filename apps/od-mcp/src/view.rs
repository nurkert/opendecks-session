//! Compact JSON views of library data for an LLM: short keys, rounded
//! numbers, empty fields left out.

use od_core::{CueColor, CueKind, KeyNotation, MusicalKey, TrackId};
use od_library::{Library, MyTag, Node, NodeKind, TrackRecord, smart_text};
use serde_json::{Map, Value, json};

/// The eight named track colours. The names match the colour set of common
/// DJ players, so colours survive an export to a stick.
pub const TRACK_COLORS: [(&str, CueColor); 8] = [
    ("pink", CueColor::rgb(0xf8, 0x70, 0xf8)),
    ("red", CueColor::rgb(0xf8, 0x2c, 0x30)),
    ("orange", CueColor::rgb(0xf8, 0x80, 0x00)),
    ("yellow", CueColor::rgb(0xf8, 0xe3, 0x31)),
    ("green", CueColor::rgb(0x10, 0xe0, 0x20)),
    ("aqua", CueColor::rgb(0x10, 0xb0, 0xf8)),
    ("blue", CueColor::rgb(0x30, 0x5a, 0xff)),
    ("purple", CueColor::rgb(0x91, 0x48, 0xfc)),
];

/// Colour as its name when it is one of the named colours, else `#rrggbb`.
pub fn color_name(c: CueColor) -> String {
    TRACK_COLORS
        .iter()
        .find(|(_, p)| *p == c)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| c.to_hex())
}

/// Parses a colour name or `#rrggbb`; `none`/empty clears.
pub fn parse_color(s: &str) -> Result<Option<CueColor>, String> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() || s == "none" || s == "clear" {
        return Ok(None);
    }
    if let Some((_, c)) = TRACK_COLORS.iter().find(|(n, _)| *n == s) {
        return Ok(Some(*c));
    }
    CueColor::from_hex(&s)
        .map(Some)
        .ok_or_else(|| format!("unknown colour \"{s}\" (use pink, red, orange, yellow, green, aqua, blue, purple, #rrggbb or none)"))
}

pub fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

pub fn mmss(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn key_text(k: Option<MusicalKey>) -> Value {
    k.map(|k| Value::String(k.to_string()))
        .unwrap_or(Value::Null)
}

fn put(m: &mut Map<String, Value>, k: &str, v: impl Into<Value>) {
    m.insert(k.to_string(), v.into());
}

fn put_str(m: &mut Map<String, Value>, k: &str, v: &str) {
    if !v.is_empty() {
        put(m, k, v);
    }
}

pub fn my_tags_json(tags: &[MyTag]) -> Value {
    Value::Array(
        tags.iter()
            .map(|t| Value::String(format!("{}: {}", t.category, t.value)))
            .collect(),
    )
}

/// One line of a track list: id, title, artist, bpm, key, genre, rating,
/// colour, energy, duration, plays, My Tag.
pub fn track_summary(t: &TrackRecord, energy: Option<f32>, my_tags: Option<&Vec<MyTag>>) -> Value {
    let mut m = Map::new();
    put(&mut m, "id", t.id.0);
    put(&mut m, "title", t.tags.title.as_str());
    put(&mut m, "artist", t.tags.artist.as_str());
    if let Some(b) = t.bpm {
        put(&mut m, "bpm", round(b, 1));
    }
    if let Some(k) = t.key {
        put(&mut m, "key", k.to_string());
    }
    put_str(&mut m, "genre", &t.tags.genre);
    if t.tags.rating > 0 {
        put(&mut m, "rating", t.tags.rating);
    }
    if let Some(c) = t.tags.color {
        put(&mut m, "color", color_name(c));
    }
    if let Some(e) = energy {
        put(&mut m, "energy", round(e as f64, 2));
    }
    put(&mut m, "len", mmss(t.duration));
    if t.play_count > 0 {
        put(&mut m, "plays", t.play_count);
    }
    if let Some(tags) = my_tags.filter(|t| !t.is_empty()) {
        put(&mut m, "my_tag", my_tags_json(tags));
    }
    if t.missing {
        put(&mut m, "missing", true);
    }
    Value::Object(m)
}

/// Every stored field of a track, with analysis summaries.
pub fn track_full(lib: &Library, t: &TrackRecord) -> od_library::Result<Value> {
    let a = lib.analysis(t.id)?;
    let mut m = Map::new();
    put(&mut m, "id", t.id.0);
    let tg = &t.tags;
    for (k, v) in [
        ("title", &tg.title),
        ("artist", &tg.artist),
        ("album", &tg.album),
        ("genre", &tg.genre),
        ("label", &tg.label),
        ("remixer", &tg.remixer),
        ("composer", &tg.composer),
        ("comment", &tg.comment),
    ] {
        put_str(&mut m, k, v);
    }
    if let Some(y) = tg.year {
        put(&mut m, "year", y);
    }
    put(&mut m, "rating", tg.rating);
    put(
        &mut m,
        "color",
        tg.color
            .map(color_name)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    put(&mut m, "my_tag", my_tags_json(&lib.my_tags(t.id)?));
    put(
        &mut m,
        "bpm",
        t.bpm.map(|b| json!(round(b, 2))).unwrap_or(Value::Null),
    );
    if let Some(g) = lib.beatgrid_with_source(t.id)? {
        put(
            &mut m,
            "grid",
            json!({"source": g.1.as_str(), "locked": g.0.locked, "variable_tempo": g.0.beats.windows(2).any(|w| (w[0].bpm - w[1].bpm).abs() > 0.05)}),
        );
    }
    match lib.key_with_source(t.id)? {
        Some((k, src)) => put(
            &mut m,
            "key",
            json!({
                "camelot": k.format(KeyNotation::Camelot),
                "open_key": k.format(KeyNotation::OpenKey),
                "musical": k.format(KeyNotation::Musical),
                "source": src.as_str(),
                "compatible": k.compatible().iter().map(|c| c.to_string()).collect::<Vec<_>>(),
            }),
        ),
        None => put(&mut m, "key", Value::Null),
    }
    put(&mut m, "duration_s", round(t.duration, 1));
    put(&mut m, "len", mmss(t.duration));
    if let Some(l) = a.loudness_lufs {
        put(&mut m, "loudness_lufs", round(l as f64, 1));
    }
    if a.peak > 0.0 {
        put(&mut m, "peak", round(a.peak as f64, 3));
    }
    if !a.energy.is_empty() {
        let mean = a.energy.iter().sum::<f32>() / a.energy.len() as f32;
        put(&mut m, "energy", round(mean as f64, 2));
        // The curve in 16 equal slices: enough to see intro, build, drop, outro.
        let n = 16.min(a.energy.len());
        let curve: Vec<f64> = (0..n)
            .map(|i| {
                let s = &a.energy[i * a.energy.len() / n..(i + 1) * a.energy.len() / n];
                round((s.iter().sum::<f32>() / s.len().max(1) as f32) as f64, 2)
            })
            .collect();
        put(&mut m, "energy_curve", curve);
    }
    if !a.phrases.is_empty() {
        let grid = &a.beatgrid;
        let phrases: Vec<Value> = a
            .phrases
            .iter()
            .map(|p| {
                let start = grid.time_of_beat(p.start_beat as f64);
                let end = grid.time_of_beat(p.end_beat as f64);
                json!({
                    "kind": serde_json::to_value(p.kind).unwrap_or(Value::Null),
                    "start": start.map(mmss),
                    "bars": (p.end_beat - p.start_beat) / 4,
                    "end": end.map(mmss),
                })
            })
            .collect();
        put(
            &mut m,
            "phrase_mood",
            serde_json::to_value(a.phrase_mood).unwrap_or(Value::Null),
        );
        put(&mut m, "phrases", phrases);
    }
    let cues: Vec<Value> = lib
        .cues(t.id)?
        .iter()
        .map(|c| {
            let mut cm = Map::new();
            match c.kind {
                CueKind::Hot(_) => put(
                    &mut cm,
                    "hot",
                    c.kind
                        .hot_slot_letter()
                        .map(String::from)
                        .unwrap_or_default(),
                ),
                CueKind::Memory => put(&mut cm, "memory", true),
            }
            put(&mut cm, "at", mmss(c.start));
            put(&mut cm, "at_s", round(c.start, 3));
            if let Some(e) = c.loop_end {
                put(&mut cm, "loop_s", round(e - c.start, 3));
            }
            put_str(&mut cm, "label", &c.label);
            if let Some(col) = c.color {
                put(&mut cm, "color", col.to_hex());
            }
            Value::Object(cm)
        })
        .collect();
    put(&mut m, "cues", cues);
    put(&mut m, "play_count", t.play_count);
    put(
        &mut m,
        "last_played",
        t.last_played
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    put(&mut m, "date_added", t.date_added.as_str());
    put(&mut m, "format", t.format.map(|f| f.as_str()).unwrap_or(""));
    if let Some(b) = t.bit_rate {
        put(&mut m, "bitrate_kbps", b);
    }
    put(&mut m, "sample_rate", t.sample_rate);
    put(&mut m, "path", t.path.to_string_lossy().as_ref());
    put(&mut m, "missing", t.missing);
    put(&mut m, "analysed", t.analysis_version > 0);
    Ok(Value::Object(m))
}

pub fn node_kind(k: NodeKind) -> &'static str {
    k.as_str()
}

/// Short node description (no tracks).
pub fn node_brief(lib: &Library, n: &Node) -> od_library::Result<Value> {
    let mut m = Map::new();
    put(&mut m, "id", n.id.0);
    put(&mut m, "kind", node_kind(n.kind));
    put(&mut m, "name", n.name.as_str());
    match n.kind {
        NodeKind::Playlist | NodeKind::Set => put(&mut m, "tracks", lib.entries(n.id)?.len()),
        NodeKind::Smart => {
            if let Some(r) = &n.smart_rule {
                put(&mut m, "rule", smart_text::format(r));
            }
        }
        NodeKind::Folder => {}
    }
    Ok(Value::Object(m))
}

/// The node tree below `parent` (root for `None`).
pub fn node_tree(
    lib: &Library,
    parent: Option<od_library::NodeId>,
    depth: usize,
) -> od_library::Result<Vec<Value>> {
    let mut out = Vec::new();
    for n in lib.children(parent)? {
        let mut v = node_brief(lib, &n)?;
        if n.kind == NodeKind::Folder {
            if depth > 0 {
                v["children"] = Value::Array(node_tree(lib, Some(n.id), depth - 1)?);
            } else {
                v["children_count"] = json!(lib.children(Some(n.id))?.len());
            }
        }
        out.push(v);
    }
    Ok(out)
}

pub fn ids(v: &[TrackId]) -> Vec<i64> {
    v.iter().map(|t| t.0).collect()
}
