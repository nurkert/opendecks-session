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
    ("Rating set", "Bewertung gesetzt"),
    ("Tags saved", "Tags gespeichert"),
    ("Recorded as WAV, then encoded when the recording stops.", "Als WAV aufgenommen und nach dem Stopp kodiert."),
    ("Colour set", "Farbe gesetzt"),
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
    ("Switch to Lighting", "Zu Lighting wechseln"),
    ("CDJ practice mode on / off", "CDJ-Übungsmodus an / aus"),
    ("Load tracks on two decks", "Tracks auf zwei Decks laden"),
    ("Set a cue point while paused", "Im Pausenzustand einen Cue-Punkt setzen"),
    ("Start a deck with PLAY", "Ein Deck mit PLAY starten"),
    ("Match the other deck's tempo by ear", "Das Tempo des anderen Decks nach Gehör angleichen"),
    ("Start it on the one and keep the beats together", "Auf der Eins starten und die Beats zusammenhalten"),
    ("CDJ practice mode: sync is off, beatmatch by ear", "CDJ-Übungsmodus: Sync ist aus, Beatmatching nach Gehör"),
    ("CDJ practice mode off", "CDJ-Übungsmodus aus"),
    ("Audio is not running", "Audio läuft nicht"),
    ("Show / hide the lighting strip", "Lichtleiste ein- / ausblenden"),
    ("Fixture file (Open Fixture Library)", "Scheinwerfer-Datei (Open Fixture Library)"),
    ("TRACKS", "TRACKS"),
    ("PLAYLISTS", "PLAYLISTS"),
    ("Playlists", "Playlists"),
    ("SOURCE", "QUELLE"),
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
    // Keymap: pad modes, key, effects, sampler (Perform panels).
    ("Next pad mode", "Nächster Pad-Modus"),
    ("Previous pad mode", "Vorheriger Pad-Modus"),
    ("Next hot cue bank", "Nächste Hot-Cue-Bank"),
    ("Key sync on / off", "Key Sync an / aus"),
    ("Key shift up a semitone", "Tonart einen Halbton höher"),
    ("Key shift down a semitone", "Tonart einen Halbton tiefer"),
    ("Effects", "Effekte"),
    ("Show / hide the Beat FX panel", "Beat-FX-Panel ein- / ausblenden"),
    ("FX {} on / off", "FX {} an / aus"),
    ("FX {}: next effect", "FX {}: nächster Effekt"),
    ("FX {}: shorter beat", "FX {}: kürzerer Beat"),
    ("FX {}: longer beat", "FX {}: längerer Beat"),
    ("FX {}: tap the tempo", "FX {}: Tempo tappen"),
    ("FX {}: vinyl brake (hold)", "FX {}: Vinyl Brake (halten)"),
    ("FX {}: backspin (hold)", "FX {}: Backspin (halten)"),
    ("FX {}: echo out (hold)", "FX {}: Echo Out (halten)"),
    ("Colour FX: next", "Color FX: nächster"),
    ("Colour FX: previous", "Color FX: vorheriger"),
    ("Show / hide the sampler panel", "Sampler-Panel ein- / ausblenden"),
    ("Previous sampler bank", "Vorherige Sampler-Bank"),
    ("Next sampler bank", "Nächste Sampler-Bank"),
    ("Start / stop the sequencer", "Sequencer starten / stoppen"),
    ("Stop all samples", "Alle Samples stoppen"),
    // Status messages of the Perform panels.
    ("Sample could not be loaded: {}", "Sample konnte nicht geladen werden: {}"),
    ("Loaded {} into sampler slot {}", "{} in Sampler-Slot {} geladen"),
    ("Select a track to load into the sampler", "Wähle einen Track, der in den Sampler geladen werden soll"),
    ("Loading {} into the sampler …", "Lade {} in den Sampler …"),
    ("Memory cue deleted", "Memory Cue gelöscht"),
    ("The beatgrid is locked — unlock it in Prepare to tap the tempo", "Das Beatgrid ist gesperrt — entsperre es in Prepare, um das Tempo zu tappen"),
    ("Tapped tempo: {} BPM", "Getapptes Tempo: {} BPM"),
    // Browser: columns, filter, My Tag, tag list, related tracks, histories.
    ("PREVIEW", "VORSCHAU"),
    ("TITLE", "TITEL"),
    ("ARTIST", "INTERPRET"),
    ("ALBUM", "ALBUM"),
    ("GENRE", "GENRE"),
    ("LABEL", "LABEL"),
    ("COMMENT", "KOMMENTAR"),
    ("MY TAG", "MY TAG"),
    ("COLOUR", "FARBE"),
    ("BPM", "BPM"),
    ("KEY", "TONART"),
    ("TIME", "ZEIT"),
    ("YEAR", "JAHR"),
    ("ADDED", "HINZUGEFÜGT"),
    ("PLAYS", "GESPIELT"),
    ("BITRATE", "BITRATE"),
    ("RATING", "BEWERTUNG"),
    ("Tag list", "Tag-Liste"),
    ("Related tracks", "Passende Tracks"),
    ("Fits after {}", "Passt nach {}"),
    ("MASTER", "MASTER"),
    ("SELECTED", "AUSGEWÄHLT"),
    ("Edit My Tag", "My Tag bearbeiten"),
    ("Select tracks to add them to the tag list", "Wähle Tracks, um sie zur Tag-Liste hinzuzufügen"),
    ("Added {} track(s) to the tag list", "{} Track(s) zur Tag-Liste hinzugefügt"),
    ("Saved the tag list as a playlist ({} tracks)", "Tag-Liste als Playlist gespeichert ({} Tracks)"),
    ("Tag list cleared", "Tag-Liste geleert"),
    ("Select tracks to tag them", "Wähle Tracks, um sie zu taggen"),
    ("My Tag {} updated", "My Tag {} aktualisiert"),
    ("Type a My Tag as Category: Value, e.g. Mood: Peak", "Gib einen My Tag als Kategorie: Wert ein, z. B. Stimmung: Peak"),
    ("Sort the list by # to reorder it", "Sortiere die Liste nach #, um sie neu zu ordnen"),
    ("Reordered — ⌘Z to undo", "Neu geordnet — ⌘Z macht es rückgängig"),
    ("Moved — ⌘Z to undo", "Verschoben — ⌘Z macht es rückgängig"),
    ("Add the selected tracks to the tag list", "Ausgewählte Tracks zur Tag-Liste hinzufügen"),
    ("Show / hide the filter bar", "Filterleiste ein- / ausblenden"),
    ("Prepare layout: player and browser", "Prepare-Layout: Player und Browser"),
    ("Prepare layout: full browser", "Prepare-Layout: ganzer Browser"),
    ("Prepare layout: 2 players", "Prepare-Layout: 2 Player"),
    ("Prepare layout: dual player", "Prepare-Layout: Dual-Player"),
    ("Prepare layout: simple player", "Prepare-Layout: einfacher Player"),
    ("Show / hide the info panel", "Info-Bereich ein- / ausblenden"),
    ("Show / hide the recording section", "Aufnahmebereich ein- / ausblenden"),
    ("Show / hide the mixer", "Mixer ein- / ausblenden"),
    ("Show / hide Mix Point Link", "Mix Point Link ein- / ausblenden"),
    ("Separate the parts of the selected tracks", "Stems der ausgewählten Tracks trennen"),
    ("Show / hide related tracks", "Passende Tracks ein- / ausblenden"),
    ("Show / hide My Tag", "My Tag ein- / ausblenden"),
    ("Import a library XML (rekordbox-compatible)", "Bibliotheks-XML importieren (rekordbox-kompatibel)"),
    ("Export the library as XML (rekordbox-compatible)", "Bibliothek als XML exportieren (rekordbox-kompatibel)"),
    ("Export a library archive", "Bibliotheksarchiv exportieren"),
    ("Restore a library archive", "Bibliotheksarchiv wiederherstellen"),
    ("Consolidate the library", "Bibliothek zusammenführen"),
    ("Import rekordbox preferences and shortcuts", "rekordbox-Einstellungen und -Tastenkürzel importieren"),
    ("Show duplicate tracks", "Doppelte Tracks anzeigen"),
    ("Tap the tempo", "Tempo tappen"),
    ("Tap {} of {}", "Tap {} von {}"),
    ("Beatgrid set to {} BPM", "Beatgrid auf {} BPM gesetzt"),
    ("Type a tempo between 30 and 300 BPM", "Gib ein Tempo zwischen 30 und 300 BPM ein"),
    ("The beatgrid is locked — unlock it to edit", "Das Beatgrid ist gesperrt — entsperre es zum Bearbeiten"),
    ("Recording started automatically", "Aufnahme automatisch gestartet"),
    ("{} TRACKS  ·  {}  ·  {}", "{} TRACKS  ·  {}  ·  {}"),
    ("{} SELECTED", "{} AUSGEWÄHLT"),
    ("Duplicate tracks", "Doppelte Tracks"),
    ("Removed {} track(s) from the collection — the files stay on disk", "{} Track(s) aus der Sammlung entfernt — die Dateien bleiben erhalten"),
    ("Planned start saved", "Geplanter Start gespeichert"),
    ("Select a position of the set first", "Wähle zuerst eine Position im Set"),
    ("The tag list is empty — collect the backup tracks there first", "Die Tag-Liste ist leer — sammle die Ersatz-Tracks zuerst dort"),
    ("Backup tracks removed", "Ersatz-Tracks entfernt"),
    ("{} backup track(s) set", "{} Ersatz-Track(s) gesetzt"),
    ("Export library XML", "Bibliotheks-XML exportieren"),
    ("Import library XML", "Bibliotheks-XML importieren"),
    ("Export library archive", "Bibliotheksarchiv exportieren"),
    ("Include the audio files? The archive then holds everything to move the library to another computer.", "Audiodateien einschließen? Dann enthält das Archiv alles, um die Bibliothek auf einen anderen Computer umzuziehen."),
    ("Restore library archive", "Bibliotheksarchiv wiederherstellen"),
    ("Folder for the restored library", "Ordner für die wiederhergestellte Bibliothek"),
    ("Folder to collect all tracks in", "Ordner, in dem alle Tracks gesammelt werden"),
    ("Exported {} tracks to {}", "{} Tracks nach {} exportiert"),
    ("XML export failed: {}", "XML-Export fehlgeschlagen: {}"),
    ("Imported {} tracks, {} playlists and {} cues from XML", "{} Tracks, {} Playlists und {} Cues aus XML importiert"),
    (" · {} files not found", " · {} Dateien nicht gefunden"),
    ("XML import failed: {}", "XML-Import fehlgeschlagen: {}"),
    ("Writing the library archive…", "Bibliotheksarchiv wird geschrieben …"),
    ("Library archive saved: {} ({} audio files)", "Bibliotheksarchiv gespeichert: {} ({} Audiodateien)"),
    ("Library archive failed: {}", "Bibliotheksarchiv fehlgeschlagen: {}"),
    ("Library restored to {} — it opens at the next start", "Bibliothek nach {} wiederhergestellt — sie öffnet sich beim nächsten Start"),
    ("Restoring the archive failed: {}", "Wiederherstellen des Archivs fehlgeschlagen: {}"),
    ("Consolidating the library…", "Bibliothek wird zusammengeführt …"),
    ("Consolidated: {} tracks copied to {}", "Zusammengeführt: {} Tracks nach {} kopiert"),
    ("Consolidating failed: {}", "Zusammenführen fehlgeschlagen: {}"),
    ("No rekordbox preferences found: {}", "Keine rekordbox-Einstellungen gefunden: {}"),
    ("Took over {} rekordbox preferences and {} shortcuts", "{} rekordbox-Einstellungen und {} Tastenkürzel übernommen"),
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
