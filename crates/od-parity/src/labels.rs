//! Accessible-label convention (see `docs/parity.md`).
//!
//! A label is `<scope>.<part>[.<part>…]`:
//!
//! - every part is lower-case kebab-case (`key-sync`, `eq-high`), may end
//!   in an instance number (`deck1`, `ch3`, `slot12`, `unit2`);
//! - the last part may instead be one pad / hot cue letter `A`–`H`;
//! - the scope is one of [`SCOPES`], optionally with an instance number
//!   (`deck1`…`deck4`, `player1`, `player2`).

use std::collections::BTreeSet;
use std::path::Path;

/// Top-level label scopes: one per region kind of the UX study.
pub const SCOPES: &[(&str, &str)] = &[
    ("app", "window-level things: mode switch confirmation, dialogs"),
    ("topbar", "top bar: mode, layout, panel toggles, settings, master meter/volume, clock"),
    ("wave", "enlarged waveform zone: lanes, zoom column"),
    ("deck", "Perform deck N (deck1…deck4): header, overview, stem strip, controls"),
    ("player", "Prepare player N (player1, player2): transport, waveform block, toolbar, cue list"),
    ("mixer", "mixer: channels (mixer.ch1.*), master"),
    ("global", "global bar: headphones, crossfader and assign"),
    ("rec", "recording section / recording bar"),
    ("fx", "FX panel: Beat FX units, CFX row, stem targets"),
    ("mpl", "Mix Point Link panel"),
    ("sampler", "sampler panel: slots, bank, sequencer"),
    ("browser", "browser: toolbar, source rail, tree, list title row, header, rows, panel strip"),
    ("info", "Info panel (right of the list)"),
    ("subbrowser", "sub-browser (second tree + list)"),
    ("status", "status band"),
    ("prefs", "Preferences window"),
    ("menu", "menus (menu bar and View menu entries)"),
    ("ctx", "context menus (ctx.row.*, ctx.header.*)"),
];

fn is_kebab(part: &str) -> bool {
    let mut prev_dash;
    let mut chars = part.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit() => prev_dash = false,
        _ => return false,
    }
    for c in chars {
        match c {
            'a'..='z' | '0'..='9' => prev_dash = false,
            '-' if !prev_dash => prev_dash = true,
            _ => return false,
        }
    }
    !prev_dash
}

/// Scope name without its instance number: `deck2` → `deck`.
pub fn scope_of(label: &str) -> &str {
    let first = label.split('.').next().unwrap_or("");
    first.trim_end_matches(|c: char| c.is_ascii_digit())
}

pub fn is_valid_label(label: &str) -> bool {
    let parts: Vec<&str> = label.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    let scope = scope_of(label);
    if !SCOPES.iter().any(|(s, _)| *s == scope) || !is_kebab(parts[0]) {
        return false;
    }
    let last = parts.len() - 1;
    parts.iter().enumerate().skip(1).all(|(i, p)| {
        is_kebab(p) || (i == last && p.len() == 1 && ('A'..='H').contains(&p.chars().next().unwrap()))
    })
}

/// Literal `accessible-label: "…"` values in the UI sources (Slint and
/// Rust under `crates/od-ui`). Labels built at run time from expressions
/// are not seen; this is a progress indicator, the view test is the gate.
pub fn labels_in_ui_sources(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.join("crates/od-ui")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.extension().is_some_and(|e| e == "slint" || e == "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let mut rest = text.as_str();
            while let Some(pos) = rest.find("accessible-label") {
                rest = &rest[pos + "accessible-label".len()..];
                let after = rest.trim_start();
                let Some(after) = after.strip_prefix(':') else { continue };
                let after = after.trim_start();
                if let Some(body) = after.strip_prefix('"') {
                    if let Some(end) = body.find('"') {
                        let value = &body[..end];
                        if !value.contains('\\') {
                            out.insert(value.to_owned());
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_grammar() {
        for ok in [
            "deck1.play",
            "deck1.cue",
            "deck1.pad.A",
            "deck4.pad.H",
            "mixer.ch1.trim",
            "mixer.ch3.eq-high",
            "browser.search",
            "fx.unit2.slot3.depth",
            "topbar.toggle.fx",
            "prefs.view.layout",
            "sampler.slot16",
        ] {
            assert!(is_valid_label(ok), "{ok}");
        }
        for bad in [
            "play",
            "Deck1.play",
            "deck1.Play",
            "deck1.pad.I",
            "deck1.pad.A.x",
            "deck1..play",
            "deck1.key_sync",
            "deck1.-x",
            "deck1.x-",
            "foo.bar",
        ] {
            assert!(!is_valid_label(bad), "{bad}");
        }
    }
}
