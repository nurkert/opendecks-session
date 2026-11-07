//! Editing a rig outside the LIGHTING view (the MCP server, tools) and the
//! live inbox through which they reach a running app.
//!
//! The rig itself is `lighting.json`; the app reloads it when the file
//! changes. Live actions (cue go/back, flash, strobe, fog, tap, blackout)
//! cannot wait for a file edit: each is dropped as one small JSON file into
//! `lighting-inbox/` next to the rig, written to a temporary name and renamed,
//! so the app never reads half a command. The app takes them in order and
//! deletes them.

use crate::{FixtureDef, FixtureRole, OutputKind, Patch, Patched};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A live action for the running show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LiveCommand {
    /// Next cue of the cue stack (Pro mode).
    CueGo,
    /// Previous cue.
    CueBack,
    /// A cue of the stack by index.
    Cue {
        index: usize,
    },
    Flash {
        on: bool,
    },
    Strobe {
        on: bool,
    },
    Fog {
        on: bool,
    },
    Blackout {
        on: bool,
    },
    /// One tap of the tap tempo (used while no deck plays).
    Tap,
}

/// The rig file of the library at `library`.
pub fn rig_path(library: &Path) -> PathBuf {
    library
        .parent()
        .map(|d| d.join("lighting.json"))
        .unwrap_or_else(|| PathBuf::from("lighting.json"))
}

/// The inbox of the rig at `rig` (`lighting.json`).
pub fn inbox_dir(rig: &Path) -> PathBuf {
    rig.with_file_name("lighting-inbox")
}

/// Drops `cmd` into the inbox.
pub fn send(rig: &Path, cmd: &LiveCommand) -> std::io::Result<()> {
    let dir = inbox_dir(rig);
    std::fs::create_dir_all(&dir)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!("{nanos:024}-{}", std::process::id());
    let tmp = dir.join(format!("{name}.part"));
    std::fs::write(
        &tmp,
        serde_json::to_vec(cmd).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(tmp, dir.join(format!("{name}.json")))
}

/// Takes the waiting commands in the order they were sent (unreadable
/// files are dropped).
pub fn drain(rig: &Path) -> Vec<LiveCommand> {
    let Ok(entries) = std::fs::read_dir(inbox_dir(rig)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let cmd = std::fs::read(&p)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            let _ = std::fs::remove_file(&p);
            cmd
        })
        .collect()
}

/// Patches a built-in fixture type at the next free address (pure on the patch).
pub fn add_fixture(
    patch: &mut Patch,
    type_index: usize,
    name: &str,
    group: &str,
) -> Result<usize, crate::Error> {
    let kind = FixtureDef::BUILTIN
        .get(type_index)
        .ok_or_else(|| crate::Error::Fixture("unknown type".into()))?;
    let def = FixtureDef::builtin(kind).expect("built-in");
    let len = def.channels.len() as u16;
    let (universe, address) = (0..16u16)
        .find_map(|u| patch.next_free(u, len).map(|a| (u, a)))
        .ok_or_else(|| crate::Error::Patch("no free DMX address".into()))?;
    let group = if group.trim().is_empty() {
        match def.role() {
            FixtureRole::Mover => "Movers",
            FixtureRole::Bar => "Bars",
            FixtureRole::Wash => "Wash",
            FixtureRole::Effect => "Effects",
        }
        .to_string()
    } else {
        group.trim().to_string()
    };
    let name = if name.trim().is_empty() {
        format!(
            "{} {}",
            def.model,
            patch
                .fixtures
                .iter()
                .filter(|f| f.def.model == def.model)
                .count()
                + 1
        )
    } else {
        name.trim().to_string()
    };
    patch.add(Patched {
        name,
        def,
        universe,
        address,
        group,
        invert_pan: false,
    })
}

/// Output from the view's kind and target text (pure).
pub fn parse_output(kind: &str, target: &str) -> Result<OutputKind, String> {
    let target = target.trim();
    match kind {
        "artnet" => {
            let t = if target.is_empty() {
                "255.255.255.255"
            } else {
                target
            };
            let t = if t.contains(':') {
                t.to_string()
            } else {
                format!("{t}:6454")
            };
            t.parse()
                .map(|target| OutputKind::ArtNet {
                    target,
                    universe: 0,
                })
                .map_err(|_| format!("not an address: {t}"))
        }
        "sacn" => {
            let u = if target.is_empty() {
                1
            } else {
                target
                    .parse()
                    .map_err(|_| format!("not a universe: {target}"))?
            };
            if !(1..=63999).contains(&u) {
                return Err(format!("sACN universes are 1 to 63999, not {u}"));
            }
            Ok(OutputKind::Sacn { universe: u })
        }
        "enttec" if !target.is_empty() => Ok(OutputKind::EnttecPro {
            device: target.to_string(),
        }),
        "enttec" => Err("enter the serial device of the USB-DMX interface".into()),
        _ => Err(format!("unknown output {kind}")),
    }
}
