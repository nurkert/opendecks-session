//! Translations for text produced in Rust code (status messages, states).
//! Text in the Slint UI is translated with Slint's own gettext bundles.
//!
//! Messages are looked up by their English text; `{}` placeholders are
//! filled in order by [`trf`].

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

pub fn set_language(lang: Language) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    if LANG.load(Ordering::Relaxed) == 1 { Language::German } else { Language::English }
}

const DE: &[(&str, &str)] = &[
    ("All tracks", "Alle Tracks"),
    ("{} TRACKS  ·  {}", "{} TRACKS  ·  {}"),
    ("Not connected", "Nicht verbunden"),
    ("In sync", "Synchron"),
    ("No moved files found in that folder", "In diesem Ordner wurden keine verschobenen Dateien gefunden"),
    ("Found {} moved file(s)", "{} verschobene Datei(en) gefunden"),
    ("Split into {} track files next to the recording", "In {} Track-Dateien neben der Aufnahme geteilt"),
    ("Split failed: {}", "Teilen fehlgeschlagen: {}"),
    ("Tracks on a stick are read-only here — edit them in your library and the stick follows", "Tracks auf einem Stick sind hier schreibgeschützt — bearbeite sie in deiner Bibliothek, der Stick folgt"),
    ("In sync · {}", "Synchron · {}"),
    ("Add tracks", "Tracks hinzufügen"),
    ("Remove tracks", "Tracks entfernen"),
    ("Reorder", "Neu ordnen"),
    ("Edit cues", "Cues bearbeiten"),
    ("Edit beatgrid", "Beatgrid bearbeiten"),
    ("Edit key", "Tonart bearbeiten"),
    ("Edit tags", "Tags bearbeiten"),
    ("Rename", "Umbenennen"),
    ("Move", "Verschieben"),
    ("Delete", "Löschen"),
    ("Edit transition note", "Übergangsnotiz bearbeiten"),
    ("Edit backup tracks", "Ersatztracks bearbeiten"),
    ("Edit smart rules", "Smart-Regeln bearbeiten"),
    ("Edit set", "Set bearbeiten"),
    ("Create playlist", "Playlist erstellen"),
    ("Create set", "Set erstellen"),
    ("Create folder", "Ordner erstellen"),
    ("Create smart", "Smart-Liste erstellen"),
    ("ANALYSIS", "ANALYSE"),
    ("IMPORTED", "IMPORTIERT"),
    ("MANUAL", "MANUELL"),
    ("NONE", "KEINS"),
    ("{} change waiting", "{} Änderung offen"),
    ("{} changes waiting", "{} Änderungen offen"),
    ("Copying {}", "Kopiere {}"),
    ("Writing {} data", "Schreibe {}-Daten"),
    ("Preparing", "Vorbereitung"),
    ("{} FREE OF {}", "{} FREI VON {}"),
    ("Adding music: {} of {} files", "Musik wird hinzugefügt: {} von {} Dateien"),
    ("Added {} tracks", "{} Tracks hinzugefügt"),
    ("Added {} tracks, {} files could not be read", "{} Tracks hinzugefügt, {} Dateien nicht lesbar"),
    ("Analysis complete", "Analyse abgeschlossen"),
    ("ANALYSING {}/{}", "ANALYSE {}/{}"),
    ("{} synced: {} copied, {} updated, {} removed{}", "{} synchronisiert: {} kopiert, {} aktualisiert, {} entfernt{}"),
    ("Sync failed: {}", "Sync fehlgeschlagen: {}"),
    ("A player changed cues or wrote history on a stick — review it under Devices", "Ein Player hat Cues geändert oder einen Verlauf geschrieben — unter Geräte prüfen"),
    ("{} can be removed safely", "{} kann sicher entfernt werden"),
    ("Eject failed: {}", "Auswerfen fehlgeschlagen: {}"),
    ("Added {} track(s) to {}", "{} Track(s) zu {} hinzugefügt"),
    ("Created — click the title to rename it", "Erstellt — Titel anklicken zum Umbenennen"),
    ("Renamed", "Umbenannt"),
    ("Deleted — ⌘Z brings it back", "Gelöscht — ⌘Z holt es zurück"),
    ("Rules updated", "Regeln aktualisiert"),
    ("Hot cue {} set", "Hot Cue {} gesetzt"),
    ("Hot cue removed", "Hot Cue entfernt"),
    ("Memory cue added", "Memory Cue hinzugefügt"),
    ("Queued for analysis", "Zur Analyse eingereiht"),
    ("Transition note saved", "Übergangsnotiz gespeichert"),
    ("Rebuilding stick…", "Stick wird neu geschrieben…"),
    ("Ejecting…", "Wird ausgeworfen…"),
    ("Stick forgotten. Its files were left untouched.", "Stick vergessen. Seine Dateien bleiben unverändert."),
    ("Select the playlist, set or folder that should go on the stick first", "Wähle zuerst die Playlist, das Set oder den Ordner für den Stick"),
    ("Registering {} with {} — the first sync starts now", "{} wird mit {} registriert — der erste Sync startet jetzt"),
    ("Removed {} track(s) — ⌘Z to undo", "{} Track(s) entfernt — ⌘Z macht es rückgängig"),
    ("Undid: {}", "Rückgängig: {}"),
    ("Redid: {}", "Wiederholt: {}"),
    ("Nothing to undo", "Nichts rückgängig zu machen"),
    ("Nothing to redo", "Nichts zu wiederholen"),
    ("Change taken over", "Änderung übernommen"),
    ("Kept the library version", "Version der Bibliothek behalten"),
    ("This track has no beatgrid yet — analyse it first", "Dieser Track hat noch kein Beatgrid — zuerst analysieren"),
    ("Adding music…", "Musik wird hinzugefügt…"),
    ("Controller connected: {}", "Controller verbunden: {}"),
    ("Controller disconnected: {}", "Controller getrennt: {}"),
    ("Loaded {} on deck {}", "{} auf Deck {} geladen"),
    ("Recording saved: {} ({} tracks)", "Aufnahme gespeichert: {} ({} Tracks)"),
    ("Audio starts when you load a track", "Audio startet, sobald du einen Track lädst"),
];

/// Translates `english` into the current language.
pub fn tr(english: &str) -> &str {
    match language() {
        Language::English => english,
        Language::German => DE.iter().find(|(e, _)| *e == english).map(|(_, d)| *d).unwrap_or(english),
    }
}

/// Translates `english` and fills its `{}` placeholders with `args`.
pub fn trf(english: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::new();
    let mut parts = tr(english).split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for (i, p) in parts.enumerate() {
        if let Some(a) = args.get(i) {
            out.push_str(&a.to_string());
        }
        out.push_str(p);
    }
    out
}

/// Matches `text` against `template` (with `{}` placeholders) and returns
/// the captured values.
fn capture<'a>(template: &str, text: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = template.split("{}").collect();
    let mut rest = text.strip_prefix(parts[0])?;
    let mut caps = Vec::new();
    for (i, lit) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        let end = if last {
            if !rest.ends_with(lit) {
                return None;
            }
            rest.len() - lit.len()
        } else if lit.is_empty() {
            return None;
        } else {
            rest.find(lit)?
        };
        caps.push(&rest[..end]);
        rest = &rest[end + lit.len()..];
    }
    (rest.is_empty() || parts.len() == 1 && rest.is_empty()).then_some(caps)
}

/// Translates a finished message by finding the template it was formatted
/// from. Unknown messages are returned unchanged.
pub fn translate_message(text: &str) -> String {
    if language() == Language::English {
        return text.to_string();
    }
    for (e, d) in DE {
        if !e.contains("{}") {
            if *e == text {
                return d.to_string();
            }
            continue;
        }
        if let Some(caps) = capture(e, text) {
            let mut out = String::new();
            let mut parts = d.split("{}");
            out.push_str(parts.next().unwrap_or_default());
            for (i, p) in parts.enumerate() {
                out.push_str(caps.get(i).copied().unwrap_or_default());
                out.push_str(p);
            }
            return out;
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_formats() {
        set_language(Language::English);
        assert_eq!(trf("Hot cue {} set", &[&'A']), "Hot cue A set");
        set_language(Language::German);
        assert_eq!(trf("Hot cue {} set", &[&'A']), "Hot Cue A gesetzt");
        assert_eq!(tr("unknown text"), "unknown text");
        assert_eq!(translate_message("Added 3 track(s) to Warmup"), "3 Track(s) zu Warmup hinzugefügt");
        assert_eq!(translate_message("Red synced: 1 copied, 2 updated, 0 removed"), "Red synced: 1 copied, 2 updated, 0 removed".replace("synced", "synchronisiert").replace("copied", "kopiert").replace("updated", "aktualisiert").replace("removed", "entfernt"));
        assert_eq!(translate_message("Nothing to undo"), "Nichts rückgängig zu machen");
        set_language(Language::English);
    }

    #[test]
    fn placeholder_counts_match() {
        for (e, d) in DE {
            assert_eq!(e.matches("{}").count(), d.matches("{}").count(), "{e}");
        }
    }
}
