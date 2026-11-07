//! The command registry and the keymap (SPEC §12.2).
//!
//! Every user action has an id ("deck1.play", "browser.search", …), a
//! label, a context (global, Prepare or Perform) and default key bindings
//! per platform. The [`Keymap`] holds the user's overrides on top of the
//! defaults, finds the action for a key press and detects conflicts. Keymaps
//! are exchanged as TOML.
//!
//! Bindings are written portably: `Mod` is the primary modifier (⌘ on
//! macOS, Ctrl on Linux), `Meta` the secondary one (Ctrl on macOS, Super on
//! Linux), then `Alt` and `Shift`, e.g. `Mod+Shift+Z`. Keys are characters
//! (`A`, `1`, `,`) or names (`Up`, `Space`, `Return`, `F1`, …). Slint maps
//! ⌘ to its `control` modifier on macOS, so the primary modifier is always
//! Slint's `control`.
//!
//! Decks 1 and 2 use the left / right keyboard halves, a layout common in
//! DJ software; see `docs/keyboard.md` for the full table.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Linux,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") { Platform::Mac } else { Platform::Linux }
    }
}

/// Keyboard layout used to undo Shift on digits and punctuation (Shift+1
/// arrives as "!" on US and German layouts alike, Shift+7 as "&" or "/").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Us,
    German,
}

impl Layout {
    /// Guessed from the locale (`LC_ALL`, `LANG`): German locales usually
    /// type on a German layout.
    pub fn detect() -> Self {
        let loc = std::env::var("LC_ALL").ok().filter(|v| !v.is_empty()).or_else(|| std::env::var("LANG").ok()).unwrap_or_default();
        if loc.starts_with("de") { Layout::German } else { Layout::Us }
    }

    fn shifted(self) -> &'static [(char, char)] {
        const US: &[(char, char)] = &[
            ('!', '1'), ('@', '2'), ('#', '3'), ('$', '4'), ('%', '5'), ('^', '6'), ('&', '7'), ('*', '8'), ('(', '9'), (')', '0'),
            (':', ';'), ('<', ','), ('>', '.'), ('?', '/'), ('"', '\''), ('{', '['), ('}', ']'), ('|', '\\'), ('_', '-'), ('+', '='), ('~', '`'),
        ];
        const DE: &[(char, char)] = &[
            ('!', '1'), ('"', '2'), ('§', '3'), ('$', '4'), ('%', '5'), ('&', '6'), ('/', '7'), ('(', '8'), (')', '9'), ('=', '0'),
            (';', ','), (':', '.'), ('_', '-'), ('\'', '#'), ('*', '+'), ('>', '<'), ('?', 'ß'), ('°', '^'),
        ];
        match self {
            Layout::Us => US,
            Layout::German => DE,
        }
    }

    /// The unshifted key for a character typed with Shift, if known.
    fn unshift(self, c: char) -> Option<char> {
        let other = if self == Layout::Us { Layout::German } else { Layout::Us };
        self.shifted().iter().chain(other.shifted()).find(|(s, _)| *s == c).map(|(_, b)| *b)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mods {
    /// ⌘ on macOS, Ctrl on Linux (Slint's `control`).
    pub primary: bool,
    /// Ctrl on macOS, Super on Linux (Slint's `meta`).
    pub secondary: bool,
    pub alt: bool,
    pub shift: bool,
}

/// A key with modifiers.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub mods: Mods,
    /// Canonical key: an upper-case character or a key name.
    pub key: String,
}

/// Named keys: canonical name, Slint key character, extra names accepted
/// when parsing.
const NAMED: &[(&str, char, &[&str])] = &[
    ("Backspace", '\u{0008}', &[]),
    ("Tab", '\u{0009}', &[]),
    ("Return", '\u{000a}', &["Enter"]),
    ("Escape", '\u{001b}', &["Esc"]),
    ("Delete", '\u{007f}', &["Del"]),
    ("Space", ' ', &[]),
    ("Up", '\u{F700}', &["UpArrow"]),
    ("Down", '\u{F701}', &["DownArrow"]),
    ("Left", '\u{F702}', &["LeftArrow"]),
    ("Right", '\u{F703}', &["RightArrow"]),
    ("Insert", '\u{F727}', &[]),
    ("Home", '\u{F729}', &[]),
    ("End", '\u{F72B}', &[]),
    ("PageUp", '\u{F72C}', &[]),
    ("PageDown", '\u{F72D}', &[]),
    ("Plus", '+', &[]),
];

/// Slint key characters of modifier keys (pressed alone they are no chord).
const MODIFIER_KEYS: &[char] = &['\u{0010}', '\u{0011}', '\u{0012}', '\u{0013}', '\u{0014}', '\u{0015}', '\u{0016}', '\u{0017}', '\u{0018}', '\u{0019}'];

fn key_from_char(c: char) -> Option<String> {
    if MODIFIER_KEYS.contains(&c) {
        return None;
    }
    if c == '\r' {
        return Some("Return".into());
    }
    if let Some((name, _, _)) = NAMED.iter().find(|(n, ch, _)| *ch == c && *n != "Plus") {
        return Some((*name).into());
    }
    if ('\u{F704}'..='\u{F71B}').contains(&c) {
        return Some(format!("F{}", c as u32 - 0xF704 + 1));
    }
    if ('\u{F700}'..='\u{F8FF}').contains(&c) || c.is_control() {
        return None;
    }
    Some(c.to_uppercase().collect())
}

fn key_from_name(name: &str) -> Option<String> {
    if let Some((n, _, _)) = NAMED.iter().find(|(n, _, alt)| n.eq_ignore_ascii_case(name) || alt.iter().any(|a| a.eq_ignore_ascii_case(name))) {
        return Some(if *n == "Plus" { "+".into() } else { (*n).into() });
    }
    if let Some(num) = name.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&num).then(|| format!("F{num}"));
    }
    if name.eq_ignore_ascii_case("comma") {
        return Some(",".into());
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => key_from_char(c),
        _ => None,
    }
}

impl Chord {
    #[cfg(test)]
    pub fn new(key: &str, mods: Mods) -> Self {
        Self { mods, key: key.into() }
    }

    /// The chord of a Slint key event; `None` for modifier keys alone and
    /// keys without a name.
    pub fn from_event(text: &str, control: bool, meta: bool, alt: bool, shift: bool, layout: Layout) -> Option<Self> {
        let mut chars = text.chars();
        let c = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        // Shift on digits and punctuation: name the key, not the symbol.
        let c = if shift { layout.unshift(c).unwrap_or(c) } else { c };
        let key = key_from_char(c)?;
        Some(Self { mods: Mods { primary: control, secondary: meta, alt, shift }, key })
    }

    /// Parses a portable binding ("Mod+Shift+Z"). `Cmd` always means the
    /// primary modifier; `Ctrl` is the primary modifier on Linux and the
    /// secondary one on macOS.
    pub fn parse(s: &str, platform: Platform) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty binding".into());
        }
        // A trailing "+" is the plus key ("Mod++").
        let (head, key_part) = match s.strip_suffix("++") {
            Some(h) => (h, "+"),
            None if s == "+" => ("", "+"),
            None => match s.rsplit_once('+') {
                Some((h, k)) => (h, k),
                None => ("", s),
            },
        };
        let mut mods = Mods::default();
        for t in head.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            match t.to_ascii_lowercase().as_str() {
                "mod" | "cmd" | "command" | "primary" | "⌘" => mods.primary = true,
                "ctrl" | "control" | "⌃" => {
                    if platform == Platform::Mac {
                        mods.secondary = true
                    } else {
                        mods.primary = true
                    }
                }
                "meta" | "super" | "win" | "secondary" => mods.secondary = true,
                "alt" | "opt" | "option" | "⌥" => mods.alt = true,
                "shift" | "⇧" => mods.shift = true,
                other => return Err(format!("unknown modifier \"{other}\"")),
            }
        }
        let key = key_from_name(key_part.trim()).ok_or_else(|| format!("unknown key \"{}\"", key_part.trim()))?;
        Ok(Self { mods, key })
    }

    /// Portable text, the form stored in keymap files.
    pub fn portable(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.mods.primary {
            parts.push("Mod");
        }
        if self.mods.secondary {
            parts.push("Meta");
        }
        if self.mods.alt {
            parts.push("Alt");
        }
        if self.mods.shift {
            parts.push("Shift");
        }
        let key = if self.key == "+" { "Plus" } else { self.key.as_str() };
        parts.push(key);
        parts.join("+")
    }

    /// How the chord is shown: "⌃⌥⇧⌘Z" on macOS, "Ctrl+Shift+Z" on Linux.
    pub fn display(&self, platform: Platform) -> String {
        let key = match (platform, self.key.as_str()) {
            (_, "Up") => "↑",
            (_, "Down") => "↓",
            (_, "Left") => "←",
            (_, "Right") => "→",
            (Platform::Mac, "Backspace") => "⌫",
            (Platform::Mac, "Delete") => "⌦",
            (Platform::Mac, "Tab") => "⇥",
            (_, "Escape") => "Esc",
            (_, k) => k,
        };
        match platform {
            Platform::Mac => {
                let mut s = String::new();
                if self.mods.secondary {
                    s.push('⌃');
                }
                if self.mods.alt {
                    s.push('⌥');
                }
                if self.mods.shift {
                    s.push('⇧');
                }
                if self.mods.primary {
                    s.push('⌘');
                }
                s + key
            }
            Platform::Linux => {
                let mut parts: Vec<&str> = Vec::new();
                if self.mods.primary {
                    parts.push("Ctrl");
                }
                if self.mods.secondary {
                    parts.push("Super");
                }
                if self.mods.alt {
                    parts.push("Alt");
                }
                if self.mods.shift {
                    parts.push("Shift");
                }
                parts.push(key);
                parts.join("+")
            }
        }
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.portable())
    }
}

/// Where an action applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Context {
    Global,
    Prepare,
    Perform,
}

impl Context {
    pub fn overlaps(self, other: Context) -> bool {
        self == other || self == Context::Global || other == Context::Global
    }
}

/// Groups shown on the Keyboard page, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    General,
    Browser,
    Player,
    Deck(u8),
    Fx,
    Sampler,
}

impl Group {
    /// English title; translated by the caller.
    pub fn title(self) -> String {
        match self {
            Group::General => "General".into(),
            Group::Browser => "Browser".into(),
            Group::Player => "Prepare player".into(),
            Group::Deck(n) => format!("Deck {n}"),
            Group::Fx => "Effects".into(),
            Group::Sampler => "Sampler".into(),
        }
    }
}

/// One user action.
#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    /// English label; may contain one `{}` filled with `arg`.
    pub label: &'static str,
    pub arg: Option<String>,
    pub group: Group,
    pub context: Context,
    defaults_mac: Vec<Chord>,
    defaults_linux: Vec<Chord>,
}

impl Action {
    pub fn defaults(&self, platform: Platform) -> &[Chord] {
        match platform {
            Platform::Mac => &self.defaults_mac,
            Platform::Linux => &self.defaults_linux,
        }
    }

    /// The label in the current language ("Deck 1: Hot cue A").
    pub fn title(&self) -> String {
        let core = match &self.arg {
            Some(a) => od_i18n::trf(self.label, &[a]),
            None => od_i18n::tr(self.label).to_string(),
        };
        match self.group {
            Group::Deck(n) => od_i18n::trf("Deck {}: {}", &[&n, &core]),
            _ => core,
        }
    }
}

struct Builder {
    actions: Vec<Action>,
}

impl Builder {
    /// Adds an action with the same defaults on both platforms.
    fn add(&mut self, id: impl Into<String>, label: &'static str, arg: Option<String>, group: Group, context: Context, keys: &[&str]) {
        self.add_split(id, label, arg, group, context, keys, keys);
    }

    #[allow(clippy::too_many_arguments)]
    fn add_split(&mut self, id: impl Into<String>, label: &'static str, arg: Option<String>, group: Group, context: Context, mac: &[&str], linux: &[&str]) {
        let parse = |keys: &[&str], p| keys.iter().map(|k| Chord::parse(k, p).unwrap_or_else(|e| panic!("default binding {k}: {e}"))).collect();
        self.actions.push(Action {
            id: id.into(),
            label,
            arg,
            group,
            context,
            defaults_mac: parse(mac, Platform::Mac),
            defaults_linux: parse(linux, Platform::Linux),
        });
    }
}

const LETTERS: [char; 8] = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];

fn build() -> Vec<Action> {
    use Context::*;
    let mut b = Builder { actions: Vec::new() };
    let g = Group::General;
    // ── General ──
    b.add("prefs.open", "Open preferences", None, g, Global, &["Mod+,"]);
    b.add_split("app.check-updates", "Check for updates", None, g, Global, &["Mod+Alt+U"], &["Alt+U"]);
    b.add_split("library.undo", "Undo", None, g, Global, &["Mod+Z"], &["Mod+Z"]);
    b.add_split("library.redo", "Redo", None, g, Global, &["Mod+Shift+Z"], &["Mod+Shift+Z", "Mod+Y"]);
    b.add_split("mode.prepare", "Switch to Prepare", None, g, Global, &["Mod+Alt+1"], &["Alt+1"]);
    b.add_split("mode.perform", "Switch to Perform", None, g, Global, &["Mod+Alt+2"], &["Alt+2"]);
    b.add_split("mode.lighting", "Switch to Lighting", None, g, Global, &["Mod+Alt+3"], &["Alt+3"]);
    b.add_split("section.library", "Show the library", None, g, Global, &["Mod+Alt+L"], &["Alt+L"]);
    b.add_split("section.devices", "Show USB sticks", None, g, Global, &["Mod+Alt+D"], &["Alt+D"]);
    b.add_split("section.recordings", "Show recordings", None, g, Global, &["Mod+Alt+R"], &["Alt+R"]);
    b.add("record.toggle", "Start / stop recording", None, g, Global, &["Mod+Return"]);
    b.add("devices.sync", "Sync USB sticks now", None, g, Global, &["Mod+S"]);
    b.add("devices.eject", "Eject the selected stick", None, g, Global, &["Mod+E"]);
    b.add("perform.quantize", "Quantize on / off", None, g, Perform, &["Mod+K"]);
    b.add_split("view.layout.player", "Prepare layout: player and browser", None, g, Global, &["Mod+Alt+4"], &["Alt+4"]);
    b.add_split("view.layout.browser", "Prepare layout: full browser", None, g, Global, &["Mod+Alt+5"], &["Alt+5"]);
    b.add_split("view.layout.two", "Prepare layout: 2 players", None, g, Global, &["Mod+Alt+6"], &["Alt+6"]);
    b.add_split("view.layout.dual", "Prepare layout: dual player", None, g, Global, &["Mod+Alt+7"], &["Alt+7"]);
    b.add_split("view.layout.simple", "Prepare layout: simple player", None, g, Global, &["Mod+Alt+8"], &["Alt+8"]);
    b.add_split("view.panel.info", "Show / hide the info panel", None, g, Global, &["Mod+Alt+I"], &["Alt+I"]);
    b.add_split("view.panel.related", "Show / hide related tracks", None, g, Global, &["Mod+Alt+E"], &["Alt+E"]);
    b.add_split("view.panel.my-tag", "Show / hide My Tag", None, g, Global, &["Mod+Alt+G"], &["Alt+G"]);
    b.add_split("view.panel.rec", "Show / hide the recording section", None, g, Global, &["Mod+Alt+Shift+R"], &["Alt+Shift+R"]);
    b.add_split("view.panel.sub-browser", "Show / hide the sub-browser", None, g, Global, &["Mod+Alt+W"], &["Alt+W"]);
    b.add_split("perform.mixer.panel", "Show / hide the mixer", None, g, Perform, &["Mod+Alt+M"], &["Alt+M"]);
    b.add_split("perform.mpl.panel", "Show / hide Mix Point Link", None, g, Perform, &["Mod+Alt+P"], &["Alt+P"]);
    b.add_split("library.import.xml", "Import a library XML (rekordbox-compatible)", None, g, Global, &["Mod+Alt+Shift+X"], &["Alt+Shift+X"]);
    b.add_split("library.export.xml", "Export the library as XML (rekordbox-compatible)", None, g, Global, &["Mod+Alt+X"], &["Alt+X"]);
    b.add_split("library.import.itunes", "Import a Music / iTunes library XML", None, g, Global, &["Mod+Alt+Shift+M"], &["Alt+Shift+M"]);
    b.add_split("library.export.archive", "Export a library archive", None, g, Global, &["Mod+Alt+A"], &["Alt+A"]);
    b.add_split("library.restore.archive", "Restore a library archive", None, g, Global, &["Mod+Alt+Shift+A"], &["Alt+Shift+A"]);
    b.add_split("library.consolidate", "Consolidate the library", None, g, Global, &["Mod+Alt+C"], &["Alt+C"]);
    b.add_split("library.import.rekordbox-settings", "Import rekordbox preferences and shortcuts", None, g, Global, &["Mod+Alt+Shift+I"], &["Alt+Shift+I"]);

    // ── Browser ──
    let g = Group::Browser;
    b.add_split("browser.search", "Search", None, g, Global, &["Mod+F", "Meta+F"], &["Mod+F"]);
    b.add("browser.up", "Previous track", None, g, Global, &["Up"]);
    b.add("browser.down", "Next track", None, g, Global, &["Down"]);
    b.add("browser.list.prev", "Previous list", None, g, Global, &["Alt+Up"]);
    b.add("browser.list.next", "Next list", None, g, Global, &["Alt+Down"]);
    b.add("browser.select.all", "Select all tracks", None, g, Global, &["Mod+A"]);
    b.add("browser.load", "Load the selected track (free deck)", None, g, Global, &["Return"]);
    b.add("browser.load.deck1", "Load the selected track to deck {}", Some("1".into()), g, Global, &["Shift+Left"]);
    b.add("browser.load.deck2", "Load the selected track to deck {}", Some("2".into()), g, Global, &["Shift+Right"]);
    b.add("browser.load.deck3", "Load the selected track to deck {}", Some("3".into()), g, Global, &["Alt+Shift+Left"]);
    b.add("browser.load.deck4", "Load the selected track to deck {}", Some("4".into()), g, Global, &["Alt+Shift+Right"]);
    b.add("browser.remove", "Remove the selected tracks from the list", None, g, Global, &["Delete", "Backspace"]);
    b.add("browser.new.playlist", "New playlist", None, g, Global, &["Mod+N"]);
    b.add("browser.new.set", "New set", None, g, Global, &["Mod+Shift+N"]);
    b.add("browser.new.folder", "New folder", None, g, Global, &["Mod+Shift+F"]);
    b.add("browser.new.smart", "New smart list", None, g, Global, &["Mod+Shift+M"]);
    b.add("browser.delete.list", "Delete the open list", None, g, Global, &["Mod+Backspace"]);
    b.add("browser.add.music", "Add a music folder", None, g, Global, &["Mod+Shift+O"]);
    b.add("browser.import.rekordbox", "Import from rekordbox", None, g, Global, &["Mod+Shift+I"]);
    b.add("browser.find.moved", "Find moved files", None, g, Global, &["Mod+Shift+L"]);
    b.add_split("browser.taglist.add", "Add the selected tracks to the tag list", None, g, Global, &["Mod+Alt+T"], &["Alt+T"]);
    b.add("browser.filter", "Show / hide the filter bar", None, g, Global, &["Mod+J"]);
    b.add_split("browser.duplicates", "Show duplicate tracks", None, g, Global, &["Mod+Alt+Y"], &["Alt+Y"]);
    b.add_split("browser.stems", "Separate the parts of the selected tracks", None, g, Global, &["Mod+Alt+Shift+S"], &["Alt+Shift+S"]);

    // ── Prepare player ──
    let g = Group::Player;
    b.add("player.preview", "Preview play / pause", None, g, Prepare, &["Space"]);
    b.add("player.cue", "Back to the cue point", None, g, Prepare, &["C"]);
    for (i, l) in LETTERS.iter().enumerate() {
        b.add(format!("player.hotcue.{}", l.to_ascii_lowercase()), "Hot cue {}", Some(l.to_string()), g, Prepare, &[&(i + 1).to_string()]);
    }
    for (i, l) in LETTERS.iter().enumerate() {
        b.add(format!("player.hotcue.clear.{}", l.to_ascii_lowercase()), "Clear hot cue {}", Some(l.to_string()), g, Prepare, &[&format!("Mod+Shift+{}", i + 1)]);
    }
    b.add("player.memory.add", "Add a memory cue", None, g, Prepare, &["M"]);
    b.add("player.memory.delete", "Delete the memory cue at the cursor", None, g, Prepare, &["Shift+M"]);
    b.add("player.cue.prev", "Previous cue", None, g, Prepare, &["R"]);
    b.add("player.cue.next", "Next cue", None, g, Prepare, &["T"]);
    b.add("player.nudge.back", "Cursor back (fine)", None, g, Prepare, &["Left"]);
    b.add("player.nudge.fwd", "Cursor forward (fine)", None, g, Prepare, &["Right"]);
    b.add("player.nudge.back.coarse", "Cursor back 2 s", None, g, Prepare, &["Alt+Left"]);
    b.add("player.nudge.fwd.coarse", "Cursor forward 2 s", None, g, Prepare, &["Alt+Right"]);
    b.add("player.grid.earlier", "Beatgrid 10 ms earlier", None, g, Prepare, &["Mod+Left"]);
    b.add("player.grid.later", "Beatgrid 10 ms later", None, g, Prepare, &["Mod+Right"]);
    b.add("player.grid.double", "Beatgrid double tempo", None, g, Prepare, &["Mod+Up"]);
    b.add("player.grid.half", "Beatgrid half tempo", None, g, Prepare, &["Mod+Down"]);
    b.add("player.grid.downbeat", "Set the downbeat at the cursor", None, g, Prepare, &["D"]);
    b.add("player.grid.lock", "Lock / unlock the beatgrid", None, g, Prepare, &["L"]);
    b.add("player.grid.tap", "Tap the tempo", None, g, Prepare, &["B"]);
    b.add("player.phrase.cut", "Cut the phrase at the cursor", None, g, Prepare, &["Shift+X"]);
    b.add("player.phrase.boundary", "Move the nearest phrase boundary to the cursor", None, g, Prepare, &["Shift+B"]);
    b.add("player.phrase.clear", "Clear the phrases", None, g, Prepare, &["Mod+Shift+Backspace"]);
    b.add("player.edit.in", "Edit region start at the cursor", None, g, Prepare, &["Shift+I"]);
    b.add("player.edit.out", "Edit region end at the cursor", None, g, Prepare, &["Shift+O"]);
    b.add("player.edit.cut", "New track without the edit region", None, g, Prepare, &["Mod+Shift+X"]);
    b.add("player.edit.trim", "New track with only the edit region", None, g, Prepare, &["Mod+Shift+K"]);
    b.add("player.edit.repeat", "New track with the edit region repeated", None, g, Prepare, &["Mod+Shift+E"]);
    b.add("player.analyse", "Analyse the track", None, g, Prepare, &["Mod+Shift+A"]);

    // ── Decks 1 and 2: left / right keyboard halves ──
    struct Side {
        play: &'static str,
        cue: &'static str,
        sync: &'static str,
        bend_up: &'static str,
        bend_down: &'static str,
        jump_fwd: &'static str,
        jump_back: &'static str,
        hot: [&'static str; 8],
        loop_in: &'static str,
        loop_out: &'static str,
        exit: &'static str,
        mem_next: &'static str,
        mem_prev: &'static str,
        keylock: &'static str,
        beat_loops: [&'static str; 5],
        clear_extra: [&'static str; 3],
        pad_next: &'static str,
        pad_prev: &'static str,
        cue_bank: &'static str,
        key_sync: &'static str,
        key_up: &'static str,
        key_down: &'static str,
    }
    let sides = [
        Side {
            play: "Z",
            cue: "A",
            sync: "Q",
            bend_up: "E",
            bend_down: "W",
            jump_fwd: "Mod+T",
            jump_back: "Mod+R",
            hot: ["1", "2", "3", "4", "5", "Shift+1", "Shift+2", "Shift+3"],
            loop_in: "D",
            loop_out: "F",
            exit: "G",
            mem_next: "T",
            mem_prev: "R",
            keylock: "Shift+5",
            beat_loops: ["Mod+1", "Mod+2", "Mod+3", "Mod+4", "Mod+5"],
            clear_extra: ["Mod+Alt+Shift+1", "Mod+Alt+Shift+2", "Mod+Alt+Shift+3"],
            pad_next: "Mod+Shift+D",
            pad_prev: "Mod+Shift+S",
            cue_bank: "Mod+Shift+G",
            key_sync: "Mod+Shift+X",
            key_up: "Mod+Shift+V",
            key_down: "Mod+Shift+C",
        },
        Side {
            play: "N",
            cue: "H",
            sync: "Y",
            bend_up: "I",
            bend_down: "U",
            jump_fwd: "Mod+P",
            jump_back: "Mod+O",
            hot: ["6", "7", "8", "9", "0", "Shift+6", "Shift+7", "Shift+8"],
            loop_in: "K",
            loop_out: "L",
            exit: ";",
            mem_next: "P",
            mem_prev: "O",
            keylock: "Shift+0",
            beat_loops: ["Mod+6", "Mod+7", "Mod+8", "Mod+9", "Mod+0"],
            clear_extra: ["Mod+Alt+Shift+6", "Mod+Alt+Shift+7", "Mod+Alt+Shift+8"],
            pad_next: "Mod+Shift+;",
            pad_prev: "Mod+Shift+K",
            cue_bank: "Mod+Shift+J",
            key_sync: "Mod+Shift+B",
            key_up: "Mod+Shift+.",
            key_down: "Mod+Shift+,",
        },
    ];
    let shift = |k: &str| format!("Shift+{k}");
    for (i, s) in sides.iter().enumerate() {
        let n = i as u8 + 1;
        let d = |x: &str| format!("deck{n}.{x}");
        let g = Group::Deck(n);
        b.add(d("play"), "Play / pause", None, g, Perform, &[s.play]);
        b.add(d("cue"), "Cue", None, g, Perform, &[s.cue]);
        b.add(d("start"), "Jump to the track start", None, g, Perform, &[&shift(s.cue)]);
        b.add(d("sync"), "Beat sync", None, g, Perform, &[s.sync]);
        b.add(d("master"), "Sync master", None, g, Perform, &[&shift(s.sync)]);
        b.add(d("keylock"), "Master tempo (key lock)", None, g, Perform, &[s.keylock]);
        b.add(d("bend.up"), "Pitch bend + (hold)", None, g, Perform, &[s.bend_up]);
        b.add(d("bend.down"), "Pitch bend − (hold)", None, g, Perform, &[s.bend_down]);
        b.add(d("tempo.up"), "Tempo up", None, g, Perform, &[&shift(s.bend_up)]);
        b.add(d("tempo.down"), "Tempo down", None, g, Perform, &[&shift(s.bend_down)]);
        b.add(d("jump.fwd"), "Beat jump forward", None, g, Perform, &[s.jump_fwd]);
        b.add(d("jump.back"), "Beat jump back", None, g, Perform, &[s.jump_back]);
        for (j, l) in LETTERS.iter().enumerate() {
            b.add(d(&format!("hotcue.{}", l.to_ascii_lowercase())), "Hot cue {}", Some(l.to_string()), g, Perform, &[s.hot[j]]);
        }
        for (j, l) in LETTERS.iter().enumerate() {
            let key = if j < 5 { format!("Mod+Shift+{}", s.hot[j]) } else { s.clear_extra[j - 5].to_string() };
            b.add(d(&format!("hotcue.clear.{}", l.to_ascii_lowercase())), "Clear hot cue {}", Some(l.to_string()), g, Perform, &[&key]);
        }
        b.add(d("loop.in"), "Loop in", None, g, Perform, &[s.loop_in]);
        b.add(d("loop.out"), "Loop out", None, g, Perform, &[s.loop_out]);
        b.add(d("loop.exit"), "Exit / reloop", None, g, Perform, &[s.exit]);
        b.add(d("loop.auto"), "Auto beat loop", None, g, Perform, &[&shift(s.loop_out)]);
        b.add(d("loop.half"), "Halve the loop", None, g, Perform, &[&shift(s.loop_in)]);
        b.add(d("loop.double"), "Double the loop", None, g, Perform, &[&shift(s.exit)]);
        for (j, (beats, label)) in [("1_2", "1/2"), ("1", "1"), ("2", "2"), ("4", "4"), ("8", "8")].iter().enumerate() {
            b.add(d(&format!("loop.{beats}")), "{}-beat loop", Some(label.to_string()), g, Perform, &[s.beat_loops[j]]);
        }
        b.add(d("memory.next"), "Next memory cue", None, g, Perform, &[s.mem_next]);
        b.add(d("memory.prev"), "Previous memory cue", None, g, Perform, &[s.mem_prev]);
        b.add(d("padmode.next"), "Next pad mode", None, g, Perform, &[s.pad_next]);
        b.add(d("padmode.prev"), "Previous pad mode", None, g, Perform, &[s.pad_prev]);
        b.add(d("hotcue.bank"), "Next hot cue bank", None, g, Perform, &[s.cue_bank]);
        b.add(d("keysync"), "Key sync on / off", None, g, Perform, &[s.key_sync]);
        b.add(d("keyshift.up"), "Key shift up a semitone", None, g, Perform, &[s.key_up]);
        b.add(d("keyshift.down"), "Key shift down a semitone", None, g, Perform, &[s.key_down]);
    }

    // ── Decks 3 and 4: function keys ──
    for n in [3u8, 4] {
        let prefix = if n == 3 { "" } else { "Shift+" };
        let f = |k: u8| format!("{prefix}F{k}");
        let d = |x: &str| format!("deck{n}.{x}");
        let g = Group::Deck(n);
        b.add(d("cue"), "Cue", None, g, Perform, &[&f(1)]);
        b.add(d("play"), "Play / pause", None, g, Perform, &[&f(2)]);
        b.add(d("sync"), "Beat sync", None, g, Perform, &[&f(3)]);
        for (j, l) in LETTERS.iter().take(4).enumerate() {
            b.add(d(&format!("hotcue.{}", l.to_ascii_lowercase())), "Hot cue {}", Some(l.to_string()), g, Perform, &[&f(4 + j as u8)]);
        }
        b.add(d("loop.4"), "{}-beat loop", Some("4".into()), g, Perform, &[&f(8)]);
        b.add(d("loop.exit"), "Exit / reloop", None, g, Perform, &[&f(9)]);
        b.add(d("jump.back"), "Beat jump back", None, g, Perform, &[&f(10)]);
        b.add(d("jump.fwd"), "Beat jump forward", None, g, Perform, &[&f(11)]);
    }

    // ── Effects: Shift on the bottom row, FX 1 left, FX 2 right ──
    let g = Group::Fx;
    b.add_split("perform.fx.panel", "Show / hide the Beat FX panel", None, g, Perform, &["Mod+Alt+F"], &["Alt+F"]);
    b.add_split("perform.cdj", "CDJ practice mode on / off", None, g, Perform, &["Mod+Alt+J"], &["Alt+J"]);
    b.add_split("perform.light.panel", "Show / hide the lighting strip", None, g, Perform, &["Mod+Alt+K"], &["Alt+K"]);
    b.add_split("perform.layout.browse", "Browse layout on / off", None, g, Perform, &["Mod+Alt+B"], &["Alt+B"]);
    b.add_split("perform.layout.vertical", "Vertical waveforms on / off", None, g, Perform, &["Mod+Alt+V"], &["Alt+V"]);
    struct Unit {
        on: &'static str,
        next: &'static str,
        down: &'static str,
        up: &'static str,
        tap: &'static str,
        release: [&'static str; 3],
    }
    let units = [
        Unit { on: "Shift+Z", next: "Shift+X", down: "Shift+C", up: "Shift+V", tap: "Shift+B", release: ["Mod+Shift+W", "Mod+Shift+E", "Mod+Shift+R"] },
        Unit { on: "Shift+N", next: "Shift+M", down: "Shift+,", up: "Shift+.", tap: "Shift+/", release: ["Mod+Shift+Y", "Mod+Shift+U", "Mod+Shift+H"] },
    ];
    for (i, u) in units.iter().enumerate() {
        let n = (i + 1).to_string();
        let f = |x: &str| format!("fx{}.{x}", i + 1);
        b.add(f("on"), "FX {} on / off", Some(n.clone()), g, Perform, &[u.on]);
        b.add(f("next"), "FX {}: next effect", Some(n.clone()), g, Perform, &[u.next]);
        b.add(f("beat.down"), "FX {}: shorter beat", Some(n.clone()), g, Perform, &[u.down]);
        b.add(f("beat.up"), "FX {}: longer beat", Some(n.clone()), g, Perform, &[u.up]);
        b.add(f("tap"), "FX {}: tap the tempo", Some(n.clone()), g, Perform, &[u.tap]);
        b.add(f("release.brake"), "FX {}: vinyl brake (hold)", Some(n.clone()), g, Perform, &[u.release[0]]);
        b.add(f("release.backspin"), "FX {}: backspin (hold)", Some(n.clone()), g, Perform, &[u.release[1]]);
        b.add(f("release.echo"), "FX {}: echo out (hold)", Some(n.clone()), g, Perform, &[u.release[2]]);
    }
    b.add("colorfx.next", "Colour FX: next", None, g, Perform, &["Shift+J"]);
    b.add("colorfx.prev", "Colour FX: previous", None, g, Perform, &["Shift+S"]);

    // ── Sampler: the bottom letter row ──
    let g = Group::Sampler;
    b.add_split("perform.sampler.panel", "Show / hide the sampler panel", None, g, Perform, &["Mod+Alt+S"], &["Alt+S"]);
    for (i, k) in ["X", "C", "V", "B", "M", ",", ".", "/"].iter().enumerate() {
        b.add(format!("sampler.slot{}", i + 1), "Sampler slot {}", Some((i + 1).to_string()), g, Perform, &[k]);
    }
    b.add("sampler.bank.prev", "Previous sampler bank", None, g, Perform, &["Shift+R"]);
    b.add("sampler.bank.next", "Next sampler bank", None, g, Perform, &["Shift+T"]);
    b.add("sampler.seq.play", "Start / stop the sequencer", None, g, Perform, &["Shift+O"]);
    b.add("sampler.stop.all", "Stop all samples", None, g, Perform, &["Shift+P"]);
    b.actions
}

/// All actions, in display order.
pub fn registry() -> &'static [Action] {
    static R: OnceLock<Vec<Action>> = OnceLock::new();
    R.get_or_init(build)
}

pub fn action(id: &str) -> Option<&'static Action> {
    registry().iter().find(|a| a.id == id)
}

/// The keymap file: `[bindings]` maps action ids to lists of portable
/// bindings; an empty list unbinds the action.
#[derive(Debug, Default, Serialize, Deserialize)]
struct KeymapFile {
    #[serde(default)]
    bindings: BTreeMap<String, Vec<String>>,
}

/// What an import changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub applied: usize,
    pub unknown: Vec<String>,
    pub invalid: Vec<String>,
}

/// Defaults plus the user's overrides.
#[derive(Clone, Debug)]
pub struct Keymap {
    platform: Platform,
    overrides: BTreeMap<String, Vec<Chord>>,
    index: HashMap<Chord, Vec<usize>>,
}

impl Keymap {
    pub fn new(platform: Platform) -> Self {
        let mut k = Self { platform, overrides: BTreeMap::new(), index: HashMap::new() };
        k.reindex();
        k
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, a) in registry().iter().enumerate() {
            for c in self.bindings(&a.id) {
                self.index.entry(c).or_default().push(i);
            }
        }
    }

    /// Effective bindings of an action.
    pub fn bindings(&self, id: &str) -> Vec<Chord> {
        if let Some(o) = self.overrides.get(id) {
            return o.clone();
        }
        action(id).map(|a| a.defaults(self.platform).to_vec()).unwrap_or_default()
    }

    pub fn is_default(&self, id: &str) -> bool {
        !self.overrides.contains_key(id)
    }

    #[cfg(test)]
    pub fn has_overrides(&self) -> bool {
        !self.overrides.is_empty()
    }

    /// Sets the bindings of an action (an empty list unbinds it).
    pub fn set(&mut self, id: &str, chords: Vec<Chord>) {
        let Some(a) = action(id) else { return };
        if chords == a.defaults(self.platform) {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.to_string(), chords);
        }
        self.reindex();
    }

    /// Replaces the bindings of `id` with `chord` and unbinds it from every
    /// conflicting action.
    pub fn assign(&mut self, id: &str, chord: Chord) {
        for other in self.conflicts(id, &chord) {
            let rest: Vec<Chord> = self.bindings(&other.id).into_iter().filter(|c| *c != chord).collect();
            let oid = other.id.clone();
            self.set(&oid, rest);
        }
        self.set(id, vec![chord]);
    }

    pub fn reset(&mut self, id: &str) {
        self.overrides.remove(id);
        self.reindex();
    }

    pub fn reset_all(&mut self) {
        self.overrides.clear();
        self.reindex();
    }

    /// The action bound to `chord` in `mode` (Prepare or Perform); an action
    /// of that mode wins over a global one.
    pub fn lookup(&self, chord: &Chord, mode: Context) -> Option<&'static Action> {
        let reg = registry();
        let hits = self.index.get(chord)?;
        hits.iter().map(|&i| &reg[i]).filter(|a| a.context.overlaps(mode)).min_by_key(|a| if a.context == mode { 0 } else { 1 })
    }

    /// Other actions that already use `chord` where `id` applies.
    pub fn conflicts(&self, id: &str, chord: &Chord) -> Vec<&'static Action> {
        let Some(me) = action(id) else { return Vec::new() };
        let reg = registry();
        self.index
            .get(chord)
            .map(|hits| hits.iter().map(|&i| &reg[i]).filter(|a| a.id != id && a.context.overlaps(me.context)).collect())
            .unwrap_or_default()
    }

    /// Every pair of actions sharing a binding in overlapping contexts.
    pub fn all_conflicts(&self) -> Vec<(String, String, Chord)> {
        let reg = registry();
        let mut out = Vec::new();
        for (chord, hits) in &self.index {
            for (x, &i) in hits.iter().enumerate() {
                for &j in &hits[x + 1..] {
                    if reg[i].context.overlaps(reg[j].context) {
                        out.push((reg[i].id.clone(), reg[j].id.clone(), chord.clone()));
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// Only the user's overrides, as TOML (stored in the library).
    pub fn overrides_toml(&self) -> String {
        let file = KeymapFile { bindings: self.overrides.iter().map(|(k, v)| (k.clone(), v.iter().map(Chord::portable).collect())).collect() };
        toml::to_string(&file).unwrap_or_default()
    }

    /// The complete keymap as TOML (export).
    pub fn export_toml(&self) -> String {
        let file = KeymapFile { bindings: registry().iter().map(|a| (a.id.clone(), self.bindings(&a.id).iter().map(Chord::portable).collect())).collect() };
        format!(
            "# OpenDecks Session keymap\n# Mod = {} · Meta = {} · bindings per action id; [] unbinds.\n\n{}",
            if self.platform == Platform::Mac { "⌘" } else { "Ctrl" },
            if self.platform == Platform::Mac { "Ctrl" } else { "Super" },
            toml::to_string(&file).unwrap_or_default()
        )
    }

    /// Applies a keymap file on top of the defaults (replacing all
    /// overrides). Unknown ids and invalid bindings are reported and
    /// skipped.
    pub fn import_toml(&mut self, text: &str) -> Result<ImportReport, String> {
        let file: KeymapFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut report = ImportReport::default();
        let mut overrides = BTreeMap::new();
        for (id, keys) in file.bindings {
            let Some(a) = action(&id) else {
                report.unknown.push(id);
                continue;
            };
            let mut chords = Vec::new();
            let mut ok = true;
            for k in &keys {
                match Chord::parse(k, self.platform) {
                    Ok(c) => chords.push(c),
                    Err(_) => {
                        report.invalid.push(format!("{id} = {k}"));
                        ok = false;
                    }
                }
            }
            if !ok {
                continue;
            }
            if chords != a.defaults(self.platform) {
                overrides.insert(id, chords);
            }
            report.applied += 1;
        }
        self.overrides = overrides;
        self.reindex();
        Ok(report)
    }
}

/// The default bindings as a Markdown table (docs/keyboard.md).
#[cfg(test)]
pub fn markdown_table() -> String {
    let mut out = String::from("| Action | Id | Where | macOS | Linux |\n|---|---|---|---|---|\n");
    let mut group = None;
    for a in registry() {
        if group != Some(a.group) {
            group = Some(a.group);
            out.push_str(&format!("| **{}** | | | | |\n", a.group.title()));
        }
        let keys = |p: Platform| a.defaults(p).iter().map(|c| format!("`{}`", c.display(p).replace('|', "\\|"))).collect::<Vec<_>>().join(" ");
        let label = match &a.arg {
            Some(x) => a.label.replace("{}", x),
            None => a.label.to_string(),
        };
        let place = match a.context {
            Context::Global => "both",
            Context::Prepare => "Prepare",
            Context::Perform => "Perform",
        };
        out.push_str(&format!("| {label} | `{}` | {place} | {} | {} |\n", a.id, keys(Platform::Mac), keys(Platform::Linux)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_table_is_current() {
        // Regenerate with: cargo test -p od-ui keymap::tests::print_table -- --ignored --nocapture
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/keyboard.md")).unwrap();
        assert!(doc.contains(&markdown_table()), "docs/keyboard.md is out of date with the registry");
        assert!(doc.contains(&format!("**{} actions**", registry().len())), "action count in docs/keyboard.md");
    }

    #[test]
    #[ignore]
    fn print_table() {
        print!("{}", markdown_table());
    }

    fn c(s: &str) -> Chord {
        Chord::parse(s, Platform::Mac).unwrap()
    }

    #[test]
    fn parses_and_prints_portably() {
        let ch = Chord::parse("mod+shift+z", Platform::Linux).unwrap();
        assert_eq!(ch, Chord::new("Z", Mods { primary: true, shift: true, ..Default::default() }));
        assert_eq!(ch.portable(), "Mod+Shift+Z");
        assert_eq!(Chord::parse(ch.portable().as_str(), Platform::Mac).unwrap(), ch);
        assert_eq!(c("Mod++").key, "+");
        assert_eq!(c("Mod++").portable(), "Mod+Plus");
        assert_eq!(c("Mod+Plus"), c("Mod++"));
        assert_eq!(c("Mod+,").key, ",");
        assert_eq!(c("Esc").key, "Escape");
        assert_eq!(c("enter").key, "Return");
        assert_eq!(c("f12").key, "F12");
        assert!(Chord::parse("Hyper+A", Platform::Mac).is_err());
        assert!(Chord::parse("Mod+Banana", Platform::Mac).is_err());
        assert!(Chord::parse("", Platform::Mac).is_err());
        assert!(Chord::parse("F25", Platform::Mac).is_err());
    }

    #[test]
    fn ctrl_means_the_platform_modifier() {
        // On Linux Ctrl is the primary modifier; on macOS it is the secondary one.
        assert!(Chord::parse("Ctrl+S", Platform::Linux).unwrap().mods.primary);
        let mac = Chord::parse("Ctrl+S", Platform::Mac).unwrap();
        assert!(mac.mods.secondary && !mac.mods.primary);
        assert!(Chord::parse("Cmd+S", Platform::Linux).unwrap().mods.primary);
    }

    #[test]
    fn displays_per_platform() {
        let ch = c("Mod+Alt+Shift+Z");
        assert_eq!(ch.display(Platform::Mac), "⌥⇧⌘Z");
        assert_eq!(ch.display(Platform::Linux), "Ctrl+Alt+Shift+Z");
        assert_eq!(c("Shift+Left").display(Platform::Mac), "⇧←");
        assert_eq!(c("Meta+F").display(Platform::Mac), "⌃F");
        assert_eq!(c("Return").display(Platform::Linux), "Return");
    }

    #[test]
    fn events_become_chords() {
        let ev = |t: &str, ctl, shift| Chord::from_event(t, ctl, false, false, shift, Layout::Us);
        assert_eq!(ev("z", true, false), Some(c("Mod+Z")));
        assert_eq!(ev("Z", true, true), Some(c("Mod+Shift+Z")));
        assert_eq!(ev("\u{F702}", false, true), Some(c("Shift+Left")));
        assert_eq!(ev(" ", false, false), Some(c("Space")));
        assert_eq!(ev("\n", true, false), Some(c("Mod+Return")));
        assert_eq!(ev("\u{F704}", false, true), Some(c("Shift+F1")));
        // Shifted digits name the digit, on US and German layouts.
        assert_eq!(ev("!", true, true), Some(c("Mod+Shift+1")));
        assert_eq!(Chord::from_event("/", false, false, false, true, Layout::German), Some(c("Shift+7")));
        assert_eq!(Chord::from_event("&", false, false, false, true, Layout::Us), Some(c("Shift+7")));
        assert_eq!(Chord::from_event("&", false, false, false, true, Layout::German), Some(c("Shift+6")));
        // Unshifted symbols stay themselves.
        assert_eq!(ev("/", false, false), Some(c("/")));
        // Modifier keys alone are no chord.
        assert_eq!(ev("\u{0010}", false, true), None);
        assert_eq!(ev("\u{0011}", true, false), None);
        assert_eq!(ev("", false, false), None);
    }

    #[test]
    fn every_action_has_a_default_binding() {
        for p in [Platform::Mac, Platform::Linux] {
            for a in registry() {
                assert!(!a.defaults(p).is_empty(), "{} has no default on {p:?}", a.id);
            }
        }
    }

    #[test]
    fn ids_are_unique_and_labels_translated() {
        let mut ids: Vec<&str> = registry().iter().map(|a| a.id.as_str()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate action ids");
        assert!(n >= 150, "registry shrank: {n}");
    }

    #[test]
    fn labels_and_groups_have_german_translations() {
        for a in registry() {
            assert!(od_i18n::has_german(a.label), "no German for \"{}\"", a.label);
            if let Group::Deck(_) = a.group {
                continue;
            }
            assert!(od_i18n::has_german(&a.group.title()), "no German for group {:?}", a.group);
        }
        assert!(od_i18n::has_german("Deck {}: {}"));
    }

    #[test]
    fn no_default_conflicts_per_platform() {
        for p in [Platform::Mac, Platform::Linux] {
            let k = Keymap::new(p);
            assert_eq!(k.all_conflicts(), vec![], "{p:?}");
        }
    }

    #[test]
    fn prepare_actions_are_all_reachable() {
        // SPEC §16.4: 100 % of Prepare actions reachable by keyboard.
        for p in [Platform::Mac, Platform::Linux] {
            let k = Keymap::new(p);
            for a in registry().iter().filter(|a| a.context != Context::Perform) {
                let bound = k.bindings(&a.id);
                assert!(!bound.is_empty(), "{}", a.id);
                assert!(bound.iter().any(|ch| k.lookup(ch, Context::Prepare).is_some_and(|hit| hit.id == a.id)), "{} shadowed in Prepare", a.id);
            }
            for a in registry().iter().filter(|a| a.context != Context::Prepare) {
                assert!(k.bindings(&a.id).iter().any(|ch| k.lookup(ch, Context::Perform).is_some_and(|hit| hit.id == a.id)), "{} shadowed in Perform", a.id);
            }
        }
    }

    #[test]
    fn registry_covers_the_main_actions() {
        for id in [
            "prefs.open", "library.undo", "library.redo", "mode.prepare", "mode.perform", "record.toggle", "browser.search", "browser.up", "browser.down",
            "browser.load.deck1", "player.memory.add", "player.cue.next", "player.hotcue.a", "deck1.play", "deck1.cue", "deck1.sync", "deck1.hotcue.a",
            "deck1.hotcue.h", "deck1.loop.4", "deck1.jump.fwd", "deck2.play", "deck4.play", "sampler.slot8", "perform.fx.panel", "perform.sampler.panel",
            "fx1.on", "fx2.release.echo", "colorfx.next", "deck1.padmode.next", "deck2.keysync", "deck1.hotcue.bank", "sampler.bank.next",
        ] {
            assert!(action(id).is_some(), "{id}");
        }
    }

    #[test]
    fn rekordbox_defaults() {
        let k = Keymap::new(Platform::Mac);
        assert_eq!(k.bindings("deck1.play"), vec![c("Z")]);
        assert_eq!(k.bindings("deck2.play"), vec![c("N")]);
        assert_eq!(k.bindings("deck1.cue"), vec![c("A")]);
        assert_eq!(k.bindings("deck1.hotcue.a"), vec![c("1")]);
        assert_eq!(k.bindings("deck2.hotcue.a"), vec![c("6")]);
        assert_eq!(k.bindings("browser.load.deck1"), vec![c("Shift+Left")]);
        assert_eq!(k.bindings("record.toggle"), vec![c("Mod+Return")]);
        assert_eq!(Keymap::new(Platform::Linux).bindings("library.redo").len(), 2);
    }

    #[test]
    fn lookup_prefers_the_current_mode() {
        let k = Keymap::new(Platform::Mac);
        // R: previous cue in Prepare, previous memory cue of deck 1 in Perform.
        assert_eq!(k.lookup(&c("R"), Context::Prepare).unwrap().id, "player.cue.prev");
        assert_eq!(k.lookup(&c("R"), Context::Perform).unwrap().id, "deck1.memory.prev");
        assert_eq!(k.lookup(&c("Mod+Z"), Context::Perform).unwrap().id, "library.undo");
        assert!(k.lookup(&c("Z"), Context::Prepare).is_none());
        assert!(k.lookup(&c("Mod+Shift+Q"), Context::Perform).is_none());
    }

    #[test]
    fn conflicts_and_reassignment() {
        let mut k = Keymap::new(Platform::Mac);
        // Z plays deck 1 in Perform; binding it to deck 2's play conflicts.
        let hits: Vec<&str> = k.conflicts("deck2.play", &c("Z")).iter().map(|a| a.id.as_str()).collect();
        assert_eq!(hits, vec!["deck1.play"]);
        // A Prepare action does not conflict with a Perform one.
        assert!(k.conflicts("player.memory.add", &c("Z")).is_empty());
        // A global action conflicts with both.
        assert_eq!(k.conflicts("browser.search", &c("Z")).len(), 1);
        k.assign("deck2.play", c("Z"));
        assert_eq!(k.bindings("deck2.play"), vec![c("Z")]);
        assert!(k.bindings("deck1.play").is_empty());
        assert_eq!(k.lookup(&c("Z"), Context::Perform).unwrap().id, "deck2.play");
        assert!(k.all_conflicts().is_empty());
        k.reset("deck1.play");
        assert_eq!(k.bindings("deck1.play"), vec![c("Z")]);
        k.reset_all();
        assert!(!k.has_overrides());
        assert_eq!(k.lookup(&c("Z"), Context::Perform).unwrap().id, "deck1.play");
    }

    #[test]
    fn setting_the_default_is_no_override() {
        let mut k = Keymap::new(Platform::Linux);
        k.set("deck1.play", vec![Chord::parse("Z", Platform::Linux).unwrap()]);
        assert!(k.is_default("deck1.play"));
        k.set("deck1.play", vec![]);
        assert!(!k.is_default("deck1.play"));
        assert!(k.bindings("deck1.play").is_empty());
    }

    #[test]
    fn toml_round_trip() {
        let mut k = Keymap::new(Platform::Mac);
        k.assign("deck1.play", c("Space"));
        k.set("deck2.play", vec![]);
        k.set("browser.search", vec![c("Mod+Shift+F"), c("F3")]);
        let stored = k.overrides_toml();
        let mut back = Keymap::new(Platform::Mac);
        let r = back.import_toml(&stored).unwrap();
        assert_eq!(r.applied, 3);
        for id in ["deck1.play", "deck2.play", "browser.search", "browser.new.folder"] {
            assert_eq!(back.bindings(id), k.bindings(id), "{id}");
        }
        // A full export imports to the same keymap with no overrides left
        // over for unchanged actions.
        let full = k.export_toml();
        assert!(full.starts_with("# OpenDecks Session keymap"));
        let mut again = Keymap::new(Platform::Mac);
        again.import_toml(&full).unwrap();
        assert_eq!(again.overrides_toml(), k.overrides_toml());
    }

    #[test]
    fn import_reports_unknown_and_invalid() {
        let mut k = Keymap::new(Platform::Mac);
        let r = k.import_toml("[bindings]\n\"deck1.play\" = [\"X\"]\n\"no.such\" = [\"A\"]\n\"deck2.play\" = [\"Mod+Nope\"]\n").unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(r.unknown, vec!["no.such".to_string()]);
        assert_eq!(r.invalid, vec!["deck2.play = Mod+Nope".to_string()]);
        assert_eq!(k.bindings("deck1.play"), vec![c("X")]);
        assert!(k.import_toml("not toml [").is_err());
    }
}
