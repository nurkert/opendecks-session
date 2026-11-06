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
    ("Stems ready on deck {}", "Stems auf Deck {} bereit"),
    ("Downloading the stem model … {} %", "Stem-Modell wird geladen … {} %"),
    ("OpenDecks Session {} is available", "OpenDecks Session {} ist verfügbar"),
    (" (pre-release)", " (Vorabversion)"),
    ("Installing OpenDecks Session {} …", "OpenDecks Session {} wird installiert …"),
    ("Downloading OpenDecks Session {} — {} %", "OpenDecks Session {} wird geladen — {} %"),
    ("Downloading OpenDecks Session {} …", "OpenDecks Session {} wird geladen …"),
    ("OpenDecks Session {} is ready — restart to finish", "OpenDecks Session {} ist bereit — zum Abschließen neu starten"),
    ("Update failed: {} — this version keeps working", "Update fehlgeschlagen: {} — diese Version läuft weiter"),
    ("OpenDecks Session {} is up to date", "OpenDecks Session {} ist aktuell"),
    ("Could not check for updates: {}", "Suche nach Updates fehlgeschlagen: {}"),
    ("Checking for updates …", "Suche nach Updates …"),
    ("An update is already in progress", "Ein Update läuft bereits"),
    ("Version {} skipped — newer versions are offered again", "Version {} übersprungen — neuere Versionen werden wieder angeboten"),
    ("The update is installed when you restart", "Das Update wird beim Neustart installiert"),
    ("OpenDecks Session {} is verified and open — drag it to Applications, then restart", "OpenDecks Session {} ist geprüft und geöffnet — in Programme ziehen, dann neu starten"),
    ("This copy cannot update itself — download OpenDecks Session {} from the release page", "Diese Installation kann sich nicht selbst aktualisieren — OpenDecks Session {} von der Release-Seite laden"),
    ("Version {} · last checked {}", "Version {} · zuletzt geprüft {}"),
    ("Checking …", "Prüfe …"),
    ("never", "nie"),
    ("just now", "gerade eben"),
    ("{} min ago", "vor {} Min."),
    ("{} h ago", "vor {} Std."),
    ("{} days ago", "vor {} Tagen"),
    ("Check for updates", "Nach Updates suchen"),
    ("Stem separation failed: {}", "Stem-Trennung fehlgeschlagen: {}"),
    ("Recording saved: {} ({} tracks)", "Aufnahme gespeichert: {} ({} Tracks)"),
    ("Audio starts when you load a track", "Audio startet, sobald du einen Track lädst"),
    ("Adding {} to the collection…", "{} wird zur Sammlung hinzugefügt…"),
    ("Added {} to the collection — analysing", "{} zur Sammlung hinzugefügt — wird analysiert"),
    ("Could not add {}: {}", "{} konnte nicht hinzugefügt werden: {}"),
    ("Add a folder to the Explorer", "Ordner zum Explorer hinzufügen"),
    // Preferences and keyboard (SPEC §12.2).
    ("Memory cue removed", "Memory Cue entfernt"),
    ("Recording failed: {}", "Aufnahme fehlgeschlagen: {}"),
    ("Recording could not start: {}", "Aufnahme konnte nicht starten: {}"),
    ("Recording saved as {}", "Aufnahme gespeichert als {}"),
    ("Converting the recording failed — the WAV file is kept", "Umwandeln der Aufnahme fehlgeschlagen — die WAV-Datei bleibt"),
    ("Backup saved: {}", "Backup gespeichert: {}"),
    ("Library location saved — applies after restart", "Speicherort der Bibliothek gespeichert — gilt nach Neustart"),
    ("All keys are back to their defaults", "Alle Tasten sind wieder auf Standard"),
    ("AUTO", "AUTO"),
    ("Automatic", "Automatisch"),
    ("DEVICE", "GERÄT"),
    ("Controller mapping file", "Controller-Mapping-Datei"),
    ("Export keymap", "Tastenbelegung exportieren"),
    ("Import keymap", "Tastenbelegung importieren"),
    ("Folder for recordings", "Ordner für Aufnahmen"),
    ("Library file (library.sqlite)", "Bibliotheksdatei (library.sqlite)"),
    ("Needs ffmpeg on the PATH; without it the recording stays a WAV file.", "Braucht ffmpeg im PATH; ohne bleibt die Aufnahme eine WAV-Datei."),
    ("Recorded as WAV, then converted with ffmpeg when the recording stops.", "Wird als WAV aufgenommen und beim Stoppen mit ffmpeg umgewandelt."),
    ("No backups yet", "Noch keine Backups"),
    ("{} backups · newest {}", "{} Backups · neuestes {}"),
    ("OPEN WITH ⌘,  ·  CLOSE WITH ESC OR ⌘W", "ÖFFNEN MIT ⌘,  ·  SCHLIESSEN MIT ESC ODER ⌘W"),
    ("OPEN WITH CTRL+,  ·  CLOSE WITH ESC OR CTRL+W", "ÖFFNEN MIT STRG+,  ·  SCHLIESSEN MIT ESC ODER STRG+W"),
    ("PREPARE", "PREPARE"),
    ("PERFORM", "PERFORM"),
    ("{} (not connected)", "{} (nicht verbunden)"),
    ("{} — connected", "{} — verbunden"),
    ("MIDI input: {}", "MIDI-Eingang: {}"),
    ("{} now on {}", "{} jetzt auf {}"),
    ("{} is already used by {}. Replace it there?", "{} ist schon belegt mit {}. Dort ersetzen?"),
    ("{} has no key now", "{} hat jetzt keine Taste"),
    ("{} is back to its default", "{} ist wieder auf Standard"),
    ("Press the new keys for {}", "Drücke die neuen Tasten für {}"),
    ("Keymap exported to {}", "Tastenbelegung exportiert nach {}"),
    ("Imported {} bindings", "{} Belegungen importiert"),
    (" · skipped {} unknown, {} invalid", " · {} unbekannte, {} ungültige übersprungen"),
    ("Not a keymap file: {}", "Keine Tastenbelegungs-Datei: {}"),
    ("Not imported: {} is used by both {} and {}", "Nicht importiert: {} ist doppelt belegt mit {} und {}"),
    ("Deck {}: {}", "Deck {}: {}"),
    ("Deck {}", "Deck {}"),
    ("General", "Allgemein"),
    ("Browser", "Browser"),
    ("Prepare player", "Prepare-Player"),
    ("Sampler", "Sampler"),
    // Keyboard actions.
    ("Open preferences", "Einstellungen öffnen"),
    ("Undo", "Rückgängig"),
    ("Redo", "Wiederholen"),
    ("Switch to Prepare", "Zu Prepare wechseln"),
    ("Switch to Perform", "Zu Perform wechseln"),
    ("Show the library", "Bibliothek zeigen"),
    ("Show USB sticks", "USB-Sticks zeigen"),
    ("Show recordings", "Aufnahmen zeigen"),
    ("Start / stop recording", "Aufnahme starten / stoppen"),
    ("Sync USB sticks now", "USB-Sticks jetzt synchronisieren"),
    ("Eject the selected stick", "Gewählten Stick auswerfen"),
    ("Quantize on / off", "Quantisierung an / aus"),
    ("Search", "Suchen"),
    ("Previous track", "Vorheriger Track"),
    ("Next track", "Nächster Track"),
    ("Previous list", "Vorherige Liste"),
    ("Next list", "Nächste Liste"),
    ("Select all tracks", "Alle Tracks auswählen"),
    ("Load the selected track (free deck)", "Gewählten Track laden (freies Deck)"),
    ("Load the selected track to deck {}", "Gewählten Track auf Deck {} laden"),
    ("Remove the selected tracks from the list", "Gewählte Tracks aus der Liste entfernen"),
    ("New playlist", "Neue Playlist"),
    ("New set", "Neues Set"),
    ("New folder", "Neuer Ordner"),
    ("New smart list", "Neue Smart-Liste"),
    ("Delete the open list", "Offene Liste löschen"),
    ("Add a music folder", "Musikordner hinzufügen"),
    ("Import from rekordbox", "Aus rekordbox importieren"),
    ("Find moved files", "Verschobene Dateien finden"),
    ("Preview play / pause", "Vorhören Play / Pause"),
    ("Back to the cue point", "Zurück zum Cue-Punkt"),
    ("Hot cue {}", "Hot Cue {}"),
    ("Clear hot cue {}", "Hot Cue {} löschen"),
    ("Add a memory cue", "Memory Cue setzen"),
    ("Delete the memory cue at the cursor", "Memory Cue am Cursor löschen"),
    ("Previous cue", "Vorheriger Cue"),
    ("Next cue", "Nächster Cue"),
    ("Cursor back (fine)", "Cursor zurück (fein)"),
    ("Cursor forward (fine)", "Cursor vor (fein)"),
    ("Cursor back 2 s", "Cursor 2 s zurück"),
    ("Cursor forward 2 s", "Cursor 2 s vor"),
    ("Beatgrid 10 ms earlier", "Beatgrid 10 ms früher"),
    ("Beatgrid 10 ms later", "Beatgrid 10 ms später"),
    ("Beatgrid double tempo", "Beatgrid doppeltes Tempo"),
    ("Beatgrid half tempo", "Beatgrid halbes Tempo"),
    ("Set the downbeat at the cursor", "Downbeat am Cursor setzen"),
    ("Lock / unlock the beatgrid", "Beatgrid sperren / entsperren"),
    ("Analyse the track", "Track analysieren"),
    ("Play / pause", "Play / Pause"),
    ("Cue", "Cue"),
    ("Jump to the track start", "Zum Trackanfang springen"),
    ("Beat sync", "Beat Sync"),
    ("Sync master", "Sync-Master"),
    ("Master tempo (key lock)", "Master Tempo (Key Lock)"),
    ("Pitch bend + (hold)", "Pitch Bend + (halten)"),
    ("Pitch bend − (hold)", "Pitch Bend − (halten)"),
    ("Tempo up", "Tempo hoch"),
    ("Tempo down", "Tempo runter"),
    ("Beat jump forward", "Beat Jump vor"),
    ("Beat jump back", "Beat Jump zurück"),
    ("Loop in", "Loop In"),
    ("Loop out", "Loop Out"),
    ("Exit / reloop", "Exit / Reloop"),
    ("Auto beat loop", "Auto Beat Loop"),
    ("Halve the loop", "Loop halbieren"),
    ("Double the loop", "Loop verdoppeln"),
    ("{}-beat loop", "{}-Beat-Loop"),
    ("Next memory cue", "Nächster Memory Cue"),
    ("Previous memory cue", "Vorheriger Memory Cue"),
    ("Sampler slot {}", "Sampler-Slot {}"),
];

/// Whether `english` has a German translation (independent of the
/// current language).
pub fn has_german(english: &str) -> bool {
    DE.iter().any(|(e, _)| *e == english)
}

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
        assert_eq!(translate_message("Added Deeper to the collection — analysing"), "Deeper zur Sammlung hinzugefügt — wird analysiert");
        set_language(Language::English);
    }

    #[test]
    fn no_duplicate_entries() {
        let mut seen = std::collections::HashSet::new();
        for (e, _) in DE {
            assert!(seen.insert(*e), "duplicate entry {e}");
        }
        assert!(has_german("Undo") && !has_german("no such text"));
    }

    #[test]
    fn placeholder_counts_match() {
        for (e, d) in DE {
            assert_eq!(e.matches("{}").count(), d.matches("{}").count(), "{e}");
        }
    }
}
