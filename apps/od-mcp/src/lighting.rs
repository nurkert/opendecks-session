//! Lighting tools: the DMX rig (patch, outputs, modes, group looks, the cue
//! stack and cues stored with tracks) and live control of the running show.
//!
//! The rig is `lighting.json` next to the library; the app reloads it when it
//! changes. Live actions go through the rig's inbox
//! ([`od_lighting::rig::send`]), which the running app empties several times
//! a second.

use crate::tools::{ToolDef, ToolResult, def, out};
use od_library::Library;
use od_lighting::{
    Attr, Effect, FixtureDef, GroupSettings, LightCue, LiveCommand, Mode, OutputKind, Program, RigConfig, TrackCue,
    Wave, rig,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub fn tools() -> Vec<ToolDef> {
    vec![
        def("lighting_overview", "Lighting overview",
            "The DMX rig of the running show: on/off, outputs per universe, mode (auto follows the music, easy = one look, pro = per-group looks and a cue stack), master, blackout, the patched fixtures (address, group, role), the per-group looks, the cue stack and the lighting cues stored with tracks (by bar). Also lists the fixture types, programs, colours and effects the other lighting tools accept.",
            true, false, overview),
        def("lighting_patch", "Patch fixtures",
            "Add, change or remove a fixture. add: a built-in type (see lighting_overview) at the next free DMX address unless universe/address are given; the group defaults to the type's role (Wash, Movers, Bars, Effects). update/remove: the fixture by its index or name. The app picks the change up at once.",
            false, false, patch),
        def("lighting_settings", "Lighting settings",
            "Switch lighting on or off and set the mode (auto, easy, pro), master level 0–1, blackout, and the easy-mode program and colour (\"#RRGGBB\", or null to follow the track's key).",
            false, false, settings),
        def("lighting_group", "Group look",
            "Set the look of a fixture group used in pro mode: program, colour \"#RRGGBB\", intensity 0–1, pan / tilt 0–1 (movers), speed 0.25–4 and an effect (attribute dimmer/pan/tilt, wave sine/saw/square/random, length in beats, depth and spread 0–1). Omitted fields stay; null clears a field back to the automatic choice; clear=true resets the whole group.",
            false, false, group),
        def("lighting_output", "DMX output",
            "Send a universe (1-based) to an output: artnet (target IP, default broadcast), sacn (universe number, default 1), enttec (serial device of a USB-DMX Pro interface), or none to remove it.",
            false, false, output),
        def("lighting_cue", "Cue stack",
            "Edit the pro-mode cue stack: add a cue (name, fade in beats, looks per group), update or remove one by index, or move one to another position. Track cues that point at moved or removed cues are kept in step.",
            false, false, cue),
        def("lighting_track_cues", "Track lighting cues",
            "The lighting cues stored with a library track: each fires a cue of the stack (by index or name) when the playing track reaches a bar (1-based) or beat (0-based). Replaces the track's list; an empty list removes them.",
            false, false, track_cues),
        def("lighting_live", "Live lighting",
            "Act on the running show now: cue_go / cue_back through the stack, cue (fire one by index), flash, strobe and fog (on true / false), blackout (on true / false) and tap (one tap of the tap tempo). Needs the app to be running.",
            false, false, live),
    ]
}

fn rig_file(lib: &Library) -> Result<PathBuf, String> {
    lib.path().map(rig::rig_path).ok_or_else(|| "the library has no file, so it has no lighting rig".into())
}

fn load(lib: &Library) -> Result<(PathBuf, RigConfig), String> {
    let path = rig_file(lib)?;
    let config = RigConfig::load(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    Ok((path, config))
}

fn store(path: &PathBuf, config: &RigConfig) -> Result<(), String> {
    config.save(path).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn hex(c: [f32; 3]) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

fn parse_hex(s: &str) -> Result<[f32; 3], String> {
    let h = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).ok().filter(|_| h.len() == 6).ok_or_else(|| format!("not a colour: {s} (use #RRGGBB)"))?;
    Ok([((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

/// A serde enum from a lower-case name ("pulse" → `Program::Pulse`).
fn named<T: serde::de::DeserializeOwned>(what: &str, s: &str) -> Result<T, String> {
    let mut c = s.trim().chars();
    let cap: String = c.next().map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect()).unwrap_or_default();
    serde_json::from_value(json!(cap)).map_err(|_| format!("unknown {what}: {s}"))
}

fn lower(v: impl serde::Serialize) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_lowercase)).unwrap_or_default()
}

fn group_json(g: &GroupSettings) -> Value {
    json!({
        "program": g.program.map(lower),
        "color": g.color.map(hex),
        "intensity": g.intensity,
        "pan": g.pan,
        "tilt": g.tilt,
        "speed": g.speed,
        "effect": g.effect.map(|e| json!({
            "attr": lower(e.attr), "wave": lower(e.wave), "beats": e.beats, "depth": e.depth, "spread": e.spread,
        })),
    })
}

fn output_json(k: &OutputKind) -> Value {
    match k {
        OutputKind::ArtNet { target, .. } => json!({"kind": "artnet", "target": target.to_string()}),
        OutputKind::Sacn { universe } => json!({"kind": "sacn", "target": universe.to_string()}),
        OutputKind::EnttecPro { device } => json!({"kind": "enttec", "target": device}),
    }
}

// ---------------------------------------------------------------- overview

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct NoArgs {}

fn overview(lib: &mut Library, _: NoArgs) -> ToolResult {
    let (path, c) = load(lib)?;
    let s = &c.settings;
    let fixtures: Vec<Value> = c
        .patch
        .fixtures
        .iter()
        .enumerate()
        .map(|(i, f)| {
            json!({
                "index": i, "name": f.name, "model": f.def.model, "manufacturer": f.def.manufacturer,
                "universe": f.universe + 1, "address": f.address, "channels": f.def.channels.len(),
                "group": f.group, "role": lower(f.def.role()),
            })
        })
        .collect();
    let groups: serde_json::Map<String, Value> = c
        .patch
        .groups()
        .into_iter()
        .map(|g| {
            let look = s.groups.get(&g).cloned().unwrap_or_default();
            (g, group_json(&look))
        })
        .collect();
    let cues: Vec<Value> = s
        .cues
        .iter()
        .enumerate()
        .map(|(i, q)| {
            json!({
                "index": i, "name": q.name, "fade_beats": q.fade_beats,
                "groups": q.groups.iter().map(|(g, l)| (g.clone(), group_json(l))).collect::<serde_json::Map<_, _>>(),
            })
        })
        .collect();
    let mut track_cues: Vec<Value> = s
        .track_cues
        .iter()
        .map(|(track, list)| {
            let mut list = list.clone();
            list.sort_by_key(|c| c.beat);
            let title = lib.track(od_core::TrackId(*track)).map(|t| t.tags.title).unwrap_or_default();
            json!({
                "track_id": track, "title": title,
                "cues": list.iter().map(|c| json!({
                    "bar": c.beat / 4 + 1, "beat": c.beat, "cue": c.cue,
                    "cue_name": s.cues.get(c.cue).map(|q| q.name.as_str()),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    track_cues.sort_by_key(|v| v["track_id"].as_i64());
    let data = json!({
        "rig_file": path.display().to_string(),
        "enabled": c.enabled,
        "mode": lower(s.mode),
        "master": s.master,
        "blackout": s.blackout,
        "easy": {"program": lower(s.easy_program), "color": s.easy_color.map(hex)},
        "outputs": c.outputs.iter().map(|(u, k)| {
            let mut o = output_json(k);
            o["universe"] = json!(u + 1);
            o
        }).collect::<Vec<_>>(),
        "fixtures": fixtures,
        "groups": groups,
        "cues": cues,
        "track_cues": track_cues,
        "accepts": {
            "fixture_types": FixtureDef::BUILTIN,
            "programs": Program::ALL.iter().map(|p| lower(*p)).collect::<Vec<_>>(),
            "modes": ["auto", "easy", "pro"],
            "effect_attrs": ["dimmer", "pan", "tilt"],
            "effect_waves": ["sine", "saw", "square", "random"],
            "outputs": ["artnet", "sacn", "enttec"],
        },
    });
    out(
        format!(
            "Lighting {}: {} mode, {} fixtures in {} groups, {} cues, {} outputs",
            if c.enabled { "on" } else { "off" },
            lower(s.mode),
            c.patch.fixtures.len(),
            c.patch.groups().len(),
            s.cues.len(),
            c.outputs.len()
        ),
        data,
    )
}

// ---------------------------------------------------------------- patch

#[derive(Deserialize, JsonSchema)]
pub struct PatchArgs {
    /// "add", "update" or "remove".
    pub action: String,
    /// Built-in fixture type for add (lighting_overview → accepts.fixture_types).
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// Fixture to update / remove: its index or name.
    #[serde(default)]
    pub fixture: Option<Value>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    /// Universe, 1-based.
    #[serde(default)]
    pub universe: Option<u16>,
    /// First DMX channel, 1–512.
    #[serde(default)]
    pub address: Option<u16>,
    /// Mirror pan (movers hung the other way round).
    #[serde(default)]
    pub invert_pan: Option<bool>,
}

fn find_fixture(c: &RigConfig, which: &Option<Value>) -> Result<usize, String> {
    let which = which.as_ref().ok_or("give the fixture by index or name")?;
    if let Some(i) = which.as_u64() {
        return (i < c.patch.fixtures.len() as u64).then_some(i as usize).ok_or_else(|| format!("no fixture {i}"));
    }
    let name = which.as_str().unwrap_or_default();
    c.patch.fixtures.iter().position(|f| f.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("no fixture named {name}"))
}

fn patch(lib: &mut Library, a: PatchArgs) -> ToolResult {
    let (path, mut c) = load(lib)?;
    let summary = match a.action.as_str() {
        "add" => {
            let kind = a.kind.as_deref().ok_or("add needs a type")?;
            let ti = FixtureDef::BUILTIN.iter().position(|k| k.eq_ignore_ascii_case(kind)).ok_or_else(|| format!("unknown fixture type {kind}"))?;
            let i = od_lighting::add_fixture(&mut c.patch, ti, a.name.as_deref().unwrap_or(""), a.group.as_deref().unwrap_or(""))
                .map_err(|e| e.to_string())?;
            if a.universe.is_some() || a.address.is_some() || a.invert_pan.is_some() {
                let mut f = c.patch.fixtures.remove(i);
                if let Some(u) = a.universe {
                    f.universe = u.max(1) - 1;
                }
                if let Some(ad) = a.address {
                    f.address = ad;
                }
                if let Some(p) = a.invert_pan {
                    f.invert_pan = p;
                }
                c.patch.add(f).map_err(|e| e.to_string())?;
            }
            let f = c.patch.fixtures.iter().rev().find(|f| a.name.as_deref().is_none_or(|n| n.trim().is_empty() || f.name == n.trim()));
            f.map(|f| format!("Patched {} at U{}.{} in {}", f.name, f.universe + 1, f.address, f.group)).unwrap_or_else(|| "Patched".into())
        }
        "update" => {
            let i = find_fixture(&c, &a.fixture)?;
            let mut f = c.patch.fixtures.remove(i);
            if let Some(n) = a.name.filter(|n| !n.trim().is_empty()) {
                f.name = n.trim().to_string();
            }
            if let Some(g) = a.group.filter(|g| !g.trim().is_empty()) {
                f.group = g.trim().to_string();
            }
            if let Some(u) = a.universe {
                f.universe = u.max(1) - 1;
            }
            if let Some(ad) = a.address {
                f.address = ad;
            }
            if let Some(p) = a.invert_pan {
                f.invert_pan = p;
            }
            let label = format!("Updated {} (U{}.{}, {})", f.name, f.universe + 1, f.address, f.group);
            let backup = f.clone();
            match c.patch.add(f) {
                // Keep the fixture's place in the patch.
                Ok(j) => {
                    let f = c.patch.fixtures.remove(j);
                    c.patch.fixtures.insert(i, f);
                }
                Err(e) => {
                    c.patch.fixtures.insert(i, backup);
                    return Err(e.to_string());
                }
            }
            label
        }
        "remove" => {
            let i = find_fixture(&c, &a.fixture)?;
            let f = c.patch.fixtures.remove(i);
            format!("Removed {}", f.name)
        }
        other => return Err(format!("unknown action {other} (add, update, remove)")),
    };
    store(&path, &c)?;
    out(summary, json!({"fixtures": c.patch.fixtures.len(), "groups": c.patch.groups()}))
}

// ---------------------------------------------------------------- settings

#[derive(Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct SettingsArgs {
    pub enabled: Option<bool>,
    /// auto, easy or pro.
    pub mode: Option<String>,
    /// 0–1.
    pub master: Option<f32>,
    pub blackout: Option<bool>,
    /// Easy-mode program (lighting_overview → accepts.programs).
    pub easy_program: Option<String>,
    /// Easy-mode colour "#RRGGBB"; null follows the track's key.
    #[serde(deserialize_with = "some_value")]
    pub easy_color: Option<Value>,
}

/// Keeps an explicit `null` (as `Some(Value::Null)`) apart from a missing field.
fn some_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

fn settings(lib: &mut Library, a: SettingsArgs) -> ToolResult {
    let (path, mut c) = load(lib)?;
    let s = &mut c.settings;
    if let Some(on) = a.enabled {
        c.enabled = on;
    }
    if let Some(m) = a.mode {
        s.mode = named::<Mode>("mode", &m)?;
    }
    if let Some(m) = a.master {
        s.master = m.clamp(0.0, 1.0);
    }
    if let Some(b) = a.blackout {
        s.blackout = b;
    }
    if let Some(p) = a.easy_program {
        s.easy_program = named::<Program>("program", &p)?;
    }
    match a.easy_color {
        Some(Value::Null) => s.easy_color = None,
        Some(Value::String(h)) => s.easy_color = Some(parse_hex(&h)?),
        Some(v) => return Err(format!("easy_color: use \"#RRGGBB\" or null, not {v}")),
        None => {}
    }
    let summary = format!("Lighting {}, {} mode, master {:.0} %{}", if c.enabled { "on" } else { "off" }, lower(c.settings.mode), c.settings.master * 100.0, if c.settings.blackout { ", blackout" } else { "" });
    store(&path, &c)?;
    out(summary, json!({"enabled": c.enabled, "mode": lower(c.settings.mode), "master": c.settings.master, "blackout": c.settings.blackout}))
}

// ---------------------------------------------------------------- group looks

#[derive(Deserialize, JsonSchema, Clone, Default)]
#[serde(default)]
pub struct EffectArg {
    /// dimmer, pan or tilt.
    pub attr: String,
    /// sine, saw, square or random.
    pub wave: String,
    /// Length of one cycle in beats.
    pub beats: f32,
    /// 0–1.
    pub depth: f32,
    /// Phase offset between neighbouring fixtures, 0–1.
    pub spread: f32,
}

/// A look for one group; each field may be omitted (unchanged) or null (automatic).
#[derive(Deserialize, JsonSchema, Default, Clone)]
#[serde(default)]
pub struct LookArg {
    #[serde(deserialize_with = "some_value")]
    pub program: Option<Value>,
    /// "#RRGGBB".
    #[serde(deserialize_with = "some_value")]
    pub color: Option<Value>,
    #[serde(deserialize_with = "some_value")]
    pub intensity: Option<Value>,
    #[serde(deserialize_with = "some_value")]
    pub pan: Option<Value>,
    #[serde(deserialize_with = "some_value")]
    pub tilt: Option<Value>,
    #[serde(deserialize_with = "some_value")]
    pub speed: Option<Value>,
    /// {attr, wave, beats, depth, spread} or null.
    #[serde(deserialize_with = "some_value")]
    pub effect: Option<Value>,
}

fn num(what: &str, v: &Value, lo: f32, hi: f32) -> Result<Option<f32>, String> {
    match v {
        Value::Null => Ok(None),
        v => v.as_f64().map(|x| Some((x as f32).clamp(lo, hi))).ok_or_else(|| format!("{what}: a number, or null")),
    }
}

fn apply_look(g: &mut GroupSettings, l: &LookArg) -> Result<(), String> {
    if let Some(v) = &l.program {
        g.program = match v {
            Value::Null => None,
            Value::String(p) => Some(named::<Program>("program", p)?),
            v => return Err(format!("program: a name or null, not {v}")),
        };
    }
    if let Some(v) = &l.color {
        g.color = match v {
            Value::Null => None,
            Value::String(h) => Some(parse_hex(h)?),
            v => return Err(format!("color: \"#RRGGBB\" or null, not {v}")),
        };
    }
    if let Some(v) = &l.intensity {
        g.intensity = num("intensity", v, 0.0, 1.0)?;
    }
    if let Some(v) = &l.pan {
        g.pan = num("pan", v, 0.0, 1.0)?;
    }
    if let Some(v) = &l.tilt {
        g.tilt = num("tilt", v, 0.0, 1.0)?;
    }
    if let Some(v) = &l.speed {
        g.speed = num("speed", v, 0.25, 4.0)?;
    }
    if let Some(v) = &l.effect {
        g.effect = match v {
            Value::Null => None,
            v => {
                let e: EffectArg = serde_json::from_value(v.clone()).map_err(|e| format!("effect: {e}"))?;
                Some(Effect {
                    attr: named::<Attr>("effect attribute", if e.attr.is_empty() { "dimmer" } else { &e.attr })?,
                    wave: named::<Wave>("effect wave", if e.wave.is_empty() { "sine" } else { &e.wave })?,
                    beats: if e.beats > 0.0 { e.beats } else { 1.0 },
                    depth: if e.depth > 0.0 { e.depth.min(1.0) } else { 1.0 },
                    spread: e.spread.clamp(0.0, 1.0),
                })
            }
        };
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
pub struct GroupArgs {
    /// Group name as patched (lighting_overview → groups).
    pub group: String,
    #[serde(flatten)]
    pub look: LookArg,
    /// Reset the group to the automatic choice first.
    #[serde(default)]
    pub clear: bool,
}

fn group(lib: &mut Library, a: GroupArgs) -> ToolResult {
    let (path, mut c) = load(lib)?;
    let name = c.patch.groups().into_iter().find(|g| g.eq_ignore_ascii_case(a.group.trim())).ok_or_else(|| format!("no group {}", a.group))?;
    let mut g = if a.clear { GroupSettings::default() } else { c.settings.groups.get(&name).cloned().unwrap_or_default() };
    apply_look(&mut g, &a.look)?;
    let data = group_json(&g);
    c.settings.groups.insert(name.clone(), g);
    store(&path, &c)?;
    out(format!("Look of {name} set (used in pro mode)"), json!({"group": name, "look": data}))
}

// ---------------------------------------------------------------- outputs

#[derive(Deserialize, JsonSchema)]
pub struct OutputArgs {
    /// Universe, 1-based.
    pub universe: u16,
    /// artnet, sacn, enttec or none.
    pub kind: String,
    /// artnet: IP (default broadcast); sacn: universe number; enttec: serial device.
    #[serde(default)]
    pub target: String,
}

fn output(lib: &mut Library, a: OutputArgs) -> ToolResult {
    let (path, mut c) = load(lib)?;
    let u = a.universe.max(1) - 1;
    c.outputs.retain(|(x, _)| *x != u);
    let summary = if a.kind.eq_ignore_ascii_case("none") {
        format!("Universe {} has no output", u + 1)
    } else {
        let kind = od_lighting::parse_output(&a.kind.to_lowercase(), &a.target)?;
        let text = output_json(&kind);
        c.outputs.push((u, kind));
        c.outputs.sort_by_key(|(x, _)| *x);
        format!("Universe {} → {} {}", u + 1, text["kind"].as_str().unwrap_or(""), text["target"].as_str().unwrap_or(""))
    };
    store(&path, &c)?;
    out(summary, json!({"outputs": c.outputs.iter().map(|(u, k)| { let mut o = output_json(k); o["universe"] = json!(u + 1); o }).collect::<Vec<_>>()}))
}

// ---------------------------------------------------------------- cue stack

#[derive(Deserialize, JsonSchema)]
pub struct CueArgs {
    /// add, update, remove or move.
    pub action: String,
    /// The cue (0-based) for update / remove / move.
    #[serde(default)]
    pub index: Option<usize>,
    /// New position (0-based) for move.
    #[serde(default)]
    pub to: Option<usize>,
    #[serde(default)]
    pub name: Option<String>,
    /// Fade into the cue, in beats.
    #[serde(default)]
    pub fade_beats: Option<f32>,
    /// Looks per group (same fields as lighting_group); for update they change the given groups only.
    #[serde(default)]
    pub groups: Option<std::collections::HashMap<String, LookArg>>,
}

fn cue(lib: &mut Library, a: CueArgs) -> ToolResult {
    let (path, mut c) = load(lib)?;
    let patched = c.patch.groups();
    let group_name = |g: &str| patched.iter().find(|p| p.eq_ignore_ascii_case(g.trim())).cloned().ok_or_else(|| format!("no group {g}"));
    let n = c.settings.cues.len();
    let index = |i: Option<usize>| i.filter(|&i| i < n).ok_or_else(|| format!("give a cue index below {n}"));
    let summary = match a.action.as_str() {
        "add" | "update" => {
            let mut q = if a.action == "add" {
                LightCue { name: format!("Cue {}", n + 1), groups: Default::default(), fade_beats: 0.0 }
            } else {
                c.settings.cues[index(a.index)?].clone()
            };
            if let Some(name) = a.name.filter(|s| !s.trim().is_empty()) {
                q.name = name.trim().to_string();
            }
            if let Some(f) = a.fade_beats {
                q.fade_beats = f.max(0.0);
            }
            for (g, look) in a.groups.unwrap_or_default() {
                let g = group_name(&g)?;
                let mut s = q.groups.get(&g).cloned().unwrap_or_default();
                apply_look(&mut s, &look)?;
                q.groups.insert(g, s);
            }
            let label = format!("{} cue {}", if a.action == "add" { "Added" } else { "Updated" }, q.name);
            match a.index.filter(|_| a.action == "update") {
                Some(i) => c.settings.cues[i] = q,
                None => c.settings.cues.push(q),
            }
            label
        }
        "remove" => {
            let i = index(a.index)?;
            let q = c.settings.cues.remove(i);
            for list in c.settings.track_cues.values_mut() {
                list.retain(|t| t.cue != i);
                for t in list.iter_mut().filter(|t| t.cue > i) {
                    t.cue -= 1;
                }
            }
            c.settings.track_cues.retain(|_, l| !l.is_empty());
            format!("Removed cue {}", q.name)
        }
        "move" => {
            let from = index(a.index)?;
            let to = a.to.filter(|&t| t < n).ok_or_else(|| format!("give `to` below {n}"))?;
            let q = c.settings.cues.remove(from);
            let label = format!("Moved cue {} to {}", q.name, to);
            c.settings.cues.insert(to, q);
            // Where each old index went.
            let map = |i: usize| -> usize {
                if i == from {
                    to
                } else if from < to && i > from && i <= to {
                    i - 1
                } else if to < from && i >= to && i < from {
                    i + 1
                } else {
                    i
                }
            };
            for list in c.settings.track_cues.values_mut() {
                for t in list.iter_mut() {
                    t.cue = map(t.cue);
                }
            }
            label
        }
        other => return Err(format!("unknown action {other} (add, update, remove, move)")),
    };
    store(&path, &c)?;
    out(summary, json!({"cues": c.settings.cues.iter().enumerate().map(|(i, q)| json!({"index": i, "name": q.name})).collect::<Vec<_>>()}))
}

// ---------------------------------------------------------------- track cues

#[derive(Deserialize, JsonSchema)]
pub struct TrackCueArg {
    /// Bar, 1-based (or give beat).
    #[serde(default)]
    pub bar: Option<u32>,
    /// Beat, 0-based.
    #[serde(default)]
    pub beat: Option<u32>,
    /// Cue of the stack: index or name.
    pub cue: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct TrackCuesArgs {
    pub track_id: i64,
    pub cues: Vec<TrackCueArg>,
}

fn track_cues(lib: &mut Library, a: TrackCuesArgs) -> ToolResult {
    let title = lib.track(od_core::TrackId(a.track_id)).map_err(|_| format!("no track {}", a.track_id))?.tags.title;
    let (path, mut c) = load(lib)?;
    let mut list = Vec::new();
    for t in &a.cues {
        let beat = match (t.beat, t.bar) {
            (Some(b), _) => b,
            (None, Some(bar)) => bar.max(1).saturating_sub(1) * 4,
            (None, None) => return Err("each cue needs a bar or a beat".into()),
        };
        let cue = match &t.cue {
            Value::Number(n) => n.as_u64().map(|i| i as usize).filter(|&i| i < c.settings.cues.len()),
            Value::String(name) => c.settings.cues.iter().position(|q| q.name.eq_ignore_ascii_case(name.trim())),
            _ => None,
        }
        .ok_or_else(|| format!("no cue {} in the stack", t.cue))?;
        list.push(TrackCue { beat, cue });
    }
    list.sort_by_key(|c| c.beat);
    let n = list.len();
    if list.is_empty() {
        c.settings.track_cues.remove(&a.track_id);
    } else {
        c.settings.track_cues.insert(a.track_id, list);
    }
    store(&path, &c)?;
    out(format!("{n} lighting cues on {title}"), json!({"track_id": a.track_id, "cues": n}))
}

// ---------------------------------------------------------------- live

#[derive(Deserialize, JsonSchema)]
pub struct LiveArgs {
    /// cue_go, cue_back, cue, flash, strobe, fog, blackout or tap.
    pub action: String,
    /// For flash / strobe / fog / blackout (default true).
    #[serde(default)]
    pub on: Option<bool>,
    /// For cue: the cue index (0-based).
    #[serde(default)]
    pub index: Option<usize>,
}

fn live(lib: &mut Library, a: LiveArgs) -> ToolResult {
    let path = rig_file(lib)?;
    let on = a.on.unwrap_or(true);
    let cmd = match a.action.as_str() {
        "cue_go" => LiveCommand::CueGo,
        "cue_back" => LiveCommand::CueBack,
        "cue" => LiveCommand::Cue { index: a.index.ok_or("cue needs an index")? },
        "flash" => LiveCommand::Flash { on },
        "strobe" => LiveCommand::Strobe { on },
        "fog" => LiveCommand::Fog { on },
        "blackout" => LiveCommand::Blackout { on },
        "tap" => LiveCommand::Tap,
        other => return Err(format!("unknown action {other}")),
    };
    rig::send(&path, &cmd).map_err(|e| format!("sending to the app: {e}"))?;
    out(format!("Sent {} to the running show", a.action), serde_json::to_value(&cmd).unwrap_or_default())
}
