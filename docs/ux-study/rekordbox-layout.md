# rekordbox 7 layout study (clean room)

Purpose: describe how rekordbox 7 arranges its screens and guides the user, precisely enough for OpenDecks Session to reproduce the arrangement and the flows (PRODUCT.md: "same regions, proportions and control order"). This document describes; it copies no assets, icons, colours or wording beyond the functional names of controls. The visual language stays the emission-line world from DESIGN.md.

Method: rekordbox 7 (Free Plus plan, macOS) launched on 2026-11-06, window resized to 1442 x 901 pt (title bar 28 pt, content 873 pt) and driven by synthetic clicks. Geometry was measured from pixel transitions in window captures. The accessibility tree could not be used: rekordbox exposes no AX windows to System Events, and the REA native-UI observer was blocked by a stale analysis session, so every measurement below comes from screenshots (±2 pt). The browser font was at a user-chosen "Large" size, which makes list rows about twice as tall as the default.

Screenshots (outside the repo):
`/private/tmp/claude-501/-Users-nurkert-Documents-Development-Workspaces-Rust-opendecks-session/a4f2fbc4-1d00-40f7-bd7e-fa0d22395087/scratchpad/rb-study/`

All coordinates are in points at a 1442 x 901 window unless stated otherwise. Percentages are of the content width (1442) or content height (873, below the title bar).

---

## 1. Region maps

### 1.1 Global principle

- Bands stack vertically and keep a **fixed height**: top bar, waveform zone, deck row, global bar. The **browser takes all remaining height**. Resizing the window from 1442 x 901 to 2056 x 1211 left the waveform zone (142), deck row (248) and global bar (40) unchanged and gave all of the extra height to the browser.
- Widths scale proportionally. The tree pane stayed at 28% of the width (404 pt to 578 pt), and the decks and mixer scaled with the window.
- Regions are separated by 3 pt black gutters, not by drawn borders. Panel backgrounds come in two near-black steps (waveform wells are pure black; control areas are a dark grey).

### 1.2 Top bar (28 pt, present in every mode)

Left to right:

1. **Mode selector** (caps label plus a chevron, about 110 pt): EXPORT / PERFORMANCE / LIGHTING (disabled) / EDIT. Switching modes while a track is loaded opens a confirmation ("changing the mode unloads the tracks"), with Yes and No.
2. **Layout selector** (an icon of the current layout plus a chevron). The menu items have small schematic icons:
   - Performance: 2Deck Horizontal, 2Deck Vertical, 4Deck Horizontal, 4Deck Vertical, Browse.
   - Export: 1 PLAYER, 2 PLAYER, DUAL PLAYER, SIMPLE PLAYER, FULL BROWSER (also ⌘7, ⌘8, ⌘9, ⌘0 in the View menu).
3. **Panel toggles** (Performance; square icon buttons about 20 x 20 at a 41 pt pitch; filled when on): **FX**, **MIX POINT LINK**, **SAMPLER**, **MIXER**, **RECORDING**. Export has a single **RECORDING** toggle.
4. **LINK** (network link), a text button with an underline.
5. (Flexible gap.)
6. Right cluster: PAD and MIDI status words (dim when inactive), an info button, a plan badge, a **settings gear** (opens Preferences), a cloud/sync spinner, the **master level meter** (two thin horizontal bars), a **master volume** slider and a **clock** (HH:MM).

### 1.3 Performance mode, 2Deck Horizontal (default)

| Band | y range (pt) | Height | Share of 873 |
|---|---|---|---|
| Top bar | 28–56 | 28 | 3% |
| Enlarged waveform zone | 56–198 | 142 | 16% |
| Deck row (deck / mixer / deck) | 198–446 | 248 | 28% |
| Global bar | 449–489 | 40 | 5% |
| Browser (toolbar, list title, header, rows) | 492–871 | 379 | 43% |
| Status bar | 871–901 | 30 | 3% |

**Enlarged waveform zone.** One horizontal lane per deck, stacked at full width. The playhead is a fixed vertical line at the **horizontal centre**, and the waveform scrolls through it. A bar counter ("--.-Bars") sits just left of the playhead at the top. A narrow 14 pt column on the far left holds collapse (^), zoom in (+), RST (reset zoom) with a "<" toggle, zoom out (−) and expand (v). In 4-deck layouts the lane numbers 1–4 are shown at the right edge.

**Deck row widths.** Deck 1 runs 0–667 (46%), the mixer 668–777 (110 pt, 7.6%) and deck 2 777–1442 (46%).

**Deck internals, top to bottom** (deck 2 is a mirror image):

1. **Header, 54 pt.** The deck number sits small in the top-left corner. Next to it is a 32 pt artwork square, then the title (bold, about 14 pt) over the artist (grey, about 11 pt), then the BPM value, and then remaining and elapsed time (large, light; "-02:52.4 00:00.0"). Toward the mixer side are **KEY SYNC**, a key-shift stepper ("< ±0 >"), **BEAT SYNC** and **MASTER** (an outlined plate). An empty deck reads "Not Loaded." in dim text.
2. **Overview waveform, 43 pt.** The full track. A position marker sits at the left, and a thin progress line runs under it.
3. **Stem strip, 21 pt.** Right-aligned: the label "MUTE" followed by three outlined toggles, VOCAL, INST and DRUMS.
4. **Control area, 130 pt.** From the outer edge inward:
   - A 25 pt rail with two icons: the pad view, and the grid-edit/phrase-edit view (which swaps the pads for a GRID EDIT | PHRASE EDIT tool panel).
   - A **pad grid** of 2 columns x 4 rows (A–D | E–H), about 330 pt wide with 21 pt rows. Below it sits a **pad-mode dropdown**: HOT CUE, PAD FX, SLICER, BEAT JUMP, BEAT LOOP, KEYBOARD, KEY SHIFT, SEQ. CALL, ACT. CENSR, STEMS, MEMORY CUE.
   - A **loop column**: the loop length (a big "16"), "<" and ">" (halve/double), then **AU** (auto) and **MA** (manual) loop.
   - A **tempo-mode dropdown** ("INT"), with round **CUE** (outlined) and round **PLAY** (outlined) buttons below it, each about 32 pt across.
   - A **jog/platter disc** about 105 pt across, showing the BPM (large) and the pitch % with its range ("0.0% ±16").
   - The column nearest the mixer: **SLIP** at the top, **Q** (quantize) in the middle, **MT** (master tempo) at the bottom.

**Mixer (2 channels, 110 pt).** Each channel is one vertical column: TRIM, HIGH, MID and LOW knobs, each knob above its caps label, then a **CUE** (headphone) button, the **channel fader** (vertical), and a **STEM** button under the fader. Columns are separated by a hairline, and channel numbers are not repeated (the column position implies them).

**Global bar (40 pt).** From the centre-left:
- A headphone icon, a **MIX** knob (cue/master blend) and a **LEVEL** knob (headphone volume).
- The **crossfader** sits centred under the mixer (a short horizontal fader about 100 pt wide). Each side has a 2 x 2 crossfader-assign grid (decks 1, 2, 3, 4).
- At the right is the **recording section**: a source dropdown ("Master Out"), an input level knob, a level meter (two bars), the **REC button** (padlocked on Free Plus) and **elapsed / limit** time ("00:00:00 / 03:00:00").

**Browser** (detail in 1.6). The tree pane runs 0–404 (28%), then a 7 pt splitter, the track list 411–1405 and a right panel strip 1405–1442.

### 1.4 Other Performance layouts

- **4Deck Horizontal.** The waveform zone grows to 198 pt (56–254) with four lanes of about 49 pt each. Decks sit 2 x 2: 1 above 3 on the left, 2 above 4 on the right, each about 209 pt tall. The mixer widens to about 190 pt with **channel order 3 – 1 – 2 – 4** (outer channels are the lower decks). The global bar stays at 40 pt, and the browser shrinks to about 152 pt.
- **4Deck Vertical.** There is no full-width waveform zone. The waveforms run **vertically** in a centre column about 280 pt wide above the mixer, with zoom and RST controls at the top of that column. Decks sit 1/3 on the left and 2/4 on the right, and the browser gets about 300 pt.
- **2Deck Vertical.** As above, with two vertical lanes in the centre column (about 150 pt tall) and the 2-channel mixer under them (EQ knobs arranged across the column). The decks are taller, so the pad list shows A–H as one vertical list.
- **Browse.** Only each deck's header and overview remain (about 100 pt), and the browser fills the rest. The panel toggles in the top bar are dimmed.
- **Mixer toggle off.** The mixer column and the left half of the global bar (headphone and crossfader) disappear. The decks widen to meet in the centre, and the recording section moves to the centre of the global bar.
- **Recording toggle off.** The recording section disappears from the global bar.

### 1.5 Performance panels (opened from the top-bar toggles)

- **FX** (inserted **above** the waveform zone, about 85 pt, two rows):
  - Row 1 is mirrored as FX1 (left half) and FX2 (right half). Each unit has a channel-assign grid (1 2 S / 3 4 M), a "..." menu button, then **three effect slots**. Each slot has an effect-name dropdown, a depth knob and a beat value with "<" and ">". After the slots come a **beat-FX** dropdown, **AUTO** and **TAP**.
  - At the far right is a column of stem-target toggles (VOCAL / INST / DRUMS).
  - Row 2 (CFX, colour/channel FX): DEFAULT / USER preset tabs, the selected effect (FILTER), a PARAM knob, and one CFX knob per channel (S, 3, 1, 2, 4, M).
- **MIX POINT LINK** (above the waveform zone): MIX IN / MIX OUT tabs, "<" and ">", EDIT and a link button. A small panel per deck shows bracketed in/out points, with "MIX IN :" and "MIX OUT :" readouts.
- **SAMPLER** (inserted **between the global bar and the browser**, about 117 pt):
  - Two rows of 8 sample slots, split 4 | 4 around a centre column that holds GAIN, CUE and the sampler BPM (120.00) with + and −.
  - Footer: BANK selector (4 banks), Q, BPM SYNC, MASTER, sequencer record, play and save, a PATTERN selector, a "1Bar" length, a position bar, MUTE and ERASE.
- **RECORDING**: described in 1.3 (global bar, right).

### 1.6 Browser (shared by all modes)

**Toolbar row (29 pt).** Above the tree are a collapse chevron, a layout toggle and small mode chips (TAG, REQ, three blank palette slots, a list-style menu). Above the list is the **Tag List** header, with two view buttons and a blue panel toggle at the far right.

**Source rail.** A 38 pt column of square icons at the left edge of the tree pane. Each icon **switches the whole tree** to one source, and the active icon is highlighted. Order observed:

1. Collection
2. Playlists
3. Related Tracks
4. Sampler
5. Apple Music
6. Streaming (Spotify)
7. Streaming (Beatport)
8. Explorer
9. Devices
10. Cloud Export
11. Histories
12. Recordings

Which sources appear is configurable in Preferences > View > Layout > Media Browser.

**Tree content per source.**
- **Collection:** All, Date Added, Genre, Artist, Album (rows indented under a root).
- **Playlists:** a *Tree View | Column View* switch, a filter field, "All Tracks 585", then a "Playlists" root with a "+" button. Each playlist shows its track count right-aligned.
- **Related Tracks:** rule entries (BPM + KEY, Same Genre in 30day, Same Artist). The list shows a deck-source selector (MASTER, 1, 2, 3, 4).
- **Sampler:** All Samples, Capture, then sample-pack folders.
- **Explorer:** home, user folder and volumes as expandable folder nodes (detail in 2.2).
- **Devices:** connected USB/SD media (empty here).
- **Histories:** year folders, then sessions.
- **Recordings:** a list of recorded mixes. Its columns include a share flag, Preview, Summary, Track Title, Artist, Album and Genre.
- **Apple Music:** an in-pane promotional login panel. Not relevant to us.

**List area.**
- **Title row (34 pt):** "Collection (585 Tracks)" (list name plus count), and for playlists a sort chevron. Toward the right sit the **target selector** (Performance "MASTER ∨", Export "PLAYER A ∨"), two list-density buttons (artwork row versus compact), two panel buttons, and the **search field** (about 235 pt, with a magnifier and a filter-scope chevron and the placeholder "Search within this track list").
- **Column header (29 pt):** a filter funnel, an attribute column (a load-arrow icon and a My Tag star), then the chosen columns. The sort column is highlighted with an arrow.
- **Rows:** about 29 pt at the default font, 58 pt at the Large font with the two-line "Summary" column (title over BPM). The selected row has a muted blue fill. A **loaded track is shown in green text with the deck number next to the star**.
- **Right panel strip** (25–37 pt): stacked icons for My Tag, Related Tracks, Sampler/Capture, **Info** (opens a right-hand panel with Summary / Info / Artwork tabs and key/value rows: time, file type, size, created, sample rate, bitrate, play count), **Sub-browser** (opens a second, narrower browser with its own source rail and tree beside the list) and a link panel.

**Status bar (30 pt).** Brand mark at the left. At the right a list summary ("18 Tracks, 1 hours 45 minutes, 1.1 GB") and a window/panel icon. In Export mode a **device-sync** icon sits at the bottom-left of the tree pane.

### 1.7 Export mode (the preparation mode)

**1 PLAYER (default).** The player runs 56–312 (256 pt, 29%) and the browser 315–873.

The player, left to right:
- **Transport column (80 pt):** loop in and loop out, beat-jump "<" and ">", a jump size ("4Beats ∨"), a round **CUE** and a round **PLAY**.
- **Waveform block (80–1233):**
  - Top line: a 55 pt artwork square at the top left, the title, a separator, the artist, then remaining and elapsed time, then BPM.
  - The **overview waveform** spans the block right under the title line, with the position marker and cue marks.
  - Below it the **enlarged waveform** (about 150 pt) with the playhead at about the 45% point, the bar counter, and beat-grid ticks above and below. A zoom column (+, RST, −) sits at its left.
- **Bottom toolbar of the player (28 pt):** two stacked tabs at the left, **CUE/LOOP** and **GRID**. Then the hot-cue buttons **A–H** (square), **MEMORY** (set memory cue) with ◀, ▶ and ✕ (previous, next, delete), loop **AU** and **MA** with "< 16 >", **Q**, and a ≡ menu.
  - The GRID tab swaps the toolbar for grid tools: BPM field, TAP, grid shift, ×2 and ×½, metronome, undo/redo, lock, and a CUT/CLEAR group for phrases.
- **Cue list panel (1233–1442, 209 pt):** a list of 10 memory-cue rows, each with an ✕, and tabs **MEMORY | HOT CUE | INFO** at the bottom.

**2 PLAYER.** Two stacked players (A and B). Each has a header line (title, overview, times, BPM, BEAT SYNC, MASTER), a control strip and its own cue list on the right. They share one enlarged-waveform area between them, and a single transport/crossfader column sits at the left.

**DUAL PLAYER.** Like 2 PLAYER, plus a small mixer column (TRIM/HIGH/MID/LOW per player) and KEY SYNC.

**SIMPLE PLAYER.** A single 40 pt line: play, artwork, title, overview waveform, times, BPM and rating. Everything else goes to the browser.

**FULL BROWSER.** No player at all.

**Recording in Export** (top-bar REC toggle): a 30 pt bar appears **under the player** with an input-source dropdown, a level knob, a meter, a large **red REC** button (enabled in Export even on the free plan) and "00:00:00 / 03:00:00".

### 1.8 Preferences window

- A separate 800 x 828 window.
- **Left column (about 150 pt):** a search field, then categories with small icons: PLAN, CLOUD, View, Audio, Analysis, Controller, Keyboard, Advanced, DVS, Extensions. The selected category gets a blue fill.
- **Top tabs per category:**
  - View: Display Type | Layout | Color
  - Audio: Configuration | Input/Output
  - Analysis: Track Analysis | CUE Analysis
  - Controller: Deck | Mixer | Effect | Sampler | Recordings | MIX POINT LINK | Others
  - Advanced: Database | Browse | Others
- **Content:** stacked dark cards, each with a bold section heading, then indented controls (radio groups, toggles, checkboxes, sliders with "Small … Large" end labels, dropdowns). A fixed **"Reset to defaults"** button sits at the bottom left of the content.
- Notable settings that shape the layout:
  - View > Display Type: Language; Tooltips; **Browse Font Size** slider plus Bold; **Line Space** slider; "Show the selected track to the center"; coloring of played tracks; deck track information display.
  - View > Layout: Media Browser (which sources appear in the rail); Browser panel options (CUE Analysis Playlist, Playlist Palette, cue markers on preview, playlist search filter, track counts in tree); Deck: Display SYNC RATE; **Phrases: Phrase (Enlarged Waveform), Phrase (Full Waveform), Always show types of phrases**.

### 1.9 Menus

- **macOS menu bar:** rekordbox, File, View, Track, Playlist, Help.
- **View (Performance):** Full screen, Appearance ▸, the five deck layouts, Show/Hide Pad and Platter, Show Split Screen, My Tag Configuration Window, "Related tracks" on Subpanel, "Track Suggestion" on Subpanel, Information Window (⌘I), Vertical sub-browser, Horizontal sub-browser, Automix.
- **View (Export):** the same, but with the five player layouts (⌘7–⌘0) instead of deck layouts.
- **Playlist:** Export Playlist ▸, Import Playlist, Copy to Playlists category, Create New Playlist, Create New Intelligent Playlist, Create New Folder, Delete Playlist, Sort Items, Renumber Track Order, Export a playlist to a file ▸, Collaborative playlist.
- **Column header context menu** (right-click any header): Auto-size this column, Auto-size all columns, a separator, then a checkable list of about 38 columns. Defaults are checked: Attribute, Preview, Summary, Album, Genre, Rating, Time, Date Added. Others available: Track Title, Artist, Comments, Size, Disc number, Album Artist, Track number, BPM, Composer, Lyricist, File Type, Year, Mix Name, Remixer, Label, Original Artist, Key, Release Date, Sample Rate, Bitrate, Bitdepth, File Name, Location, Date Created, Hot Cue, Message, Color, DJ Play Count, My Tag, and more.
- **Track row context menu:** Load ▸ (target deck/player), Import To Collection, Analyze Track, Analysis Lock ▸, Add To Playlist ▸, Add To Tag List, Reload Tag, Get Info from iTunes, Track Type ▸, Export Track ▸ (needs a device), Auto Load Hot Cue ▸, Reset DJ Play Count, Add New Analysis Data, Convert Memory Cues to Hot Cues, Sort Hot Cues by time, Remove from Playlist / Collection / History, Show information, Show in Finder, Track information ▸, Add To Request Catalog. Groups are separated by rules, and unavailable items are dimmed, not hidden.

---

## 2. Interaction and flow notes

### 2.1 Loading a track
- **Drag** a row from the list onto a deck (anywhere in that deck's area) or onto the player. Observed in Performance: the deck fills immediately (header, overview, lane in the waveform zone, BPM on the platter).
- **Double-click** a row in **Export** mode loads it into the player (observed). In Performance a double-click did not load in our test. Loading there goes through drag, the context menu "Load ▸ Deck n", or the controller's load buttons.
- **Feedback in the list:** the loaded row turns green and shows the deck number next to the star (Performance), or a small ▶ marker (Export).
- **Mode change** unloads everything after a confirmation dialog.
- **Target selector** in the list title row (MASTER / deck number / PLAYER A) chooses which deck "related" and preview functions refer to.

### 2.2 Browsing folders with Explorer and playing directly
- Choose the Explorer icon in the source rail. The tree shows home, user folder and volumes as expandable nodes (a disclosure triangle plus a folder icon). Clicking a folder name lists its audio files immediately. Disclosure only expands the node.
- The Explorer list has a reduced column set: Preview, Track Title, **BPM (sorted ascending by default)**, Key, Artist. Analysis values appear only for files rekordbox already knows.
- Files are loaded by dragging them to a deck or player. (Inference, not tested to avoid changing the library: loading a file from Explorer imports it into Collection.)

### 2.3 Recording a mix
- **Performance:** the recording section always sits at the **right end of the global bar**, directly under deck 2. It shows the source dropdown (Master Out), an input level knob, a meter, REC, and elapsed/limit time. A top-bar toggle hides or shows it. On the free plan REC shows a padlock.
- **Export:** the REC toggle adds a bar under the player with the same elements and a big red REC button.
- Results appear in the **Recordings** source of the browser, a list with preview waveforms. Recording-related preferences live under Preferences > Controller > Recordings.

### 2.4 Stems
- Every deck has a permanent **stem strip** under the overview waveform: "MUTE" then VOCAL / INST / DRUMS toggles in fixed colours.
- The **STEMS pad mode** turns the pad grid into VOCAL / INST / DRUMS mute pads (left column) and FX VOCAL / FX INST / FX DRUMS (right column).
- Each mixer channel has a **STEM** button under its fader.
- The FX panel has a stem-target column (apply FX to VOCAL / INST / DRUMS only).
- No separate "separate" button was found. The stem controls are always visible, even on an empty deck. (Inference, not verified: separation runs automatically when a track is loaded.)

### 2.5 Phrases
- Phrase analysis is a checkbox in Preferences > Analysis > Track Analysis Setting (BPM/Grid, KEY, Phrase, Vocal).
- Display is opt-in under Preferences > View > Layout > Phrases, separately for the enlarged waveform and the full (overview) waveform, plus "Always show types of phrases".
- These options were off in this installation, so phrase bands were **not observed**. We did not change them, to keep the settings untouched.
- Phrase editing lives in the deck's alternate panel (GRID EDIT | PHRASE EDIT) and in the Export player's GRID tab (CUT / CLEAR).

### 2.6 Resizing and splitters
- **Tree | list:** a vertical splitter (7 pt grab zone at x ≈ 404). Dragging it worked live; the tree went from 404 to about 270 pt.
- **Waveform zone:** has no drag splitter. Its height is set by the collapse (^) and expand (v) buttons in its left zoom column, and the zoom by + / RST / −.
- **Deck row, global bar, player:** fixed heights. The browser absorbs all vertical change (see 1.1).
- **Sub-panels** (Info, sub-browser) open on the right and take width from the list.
- **Columns:** drag the header edges, or use "Auto-size" from the header context menu.

### 2.7 Switching 2/4 decks and layouts
- Use the layout dropdown in the top bar (schematic icons per option) or the View menu (layouts listed, Export ones with ⌘7–⌘0).
- Switching is instant and keeps the loaded tracks. The browser height changes accordingly (4 decks leave about 150 pt for the browser).

### 2.8 Opening sampler / FX / recording panels
- Use the top-bar toggles. Each panel docks at a **fixed place**: FX and Mix Point Link go above the waveform zone, the Sampler between the global bar and the browser, and Recording at the right of the global bar.
- The browser loses exactly the panel's height.
- Toggle state shows as a filled icon background.

---

## 3. Density and typography

- **Typeface:** a neutral system sans throughout. Control legends are **caps, condensed, 8–9 pt** (KEY SYNC, MASTER, TRIM, HIGH, CUE, SLIP); two-word legends stack on two lines (BEAT / SYNC).
- **Size ranking:**
  - Deck title about 14 pt bold
  - Remaining/elapsed time about 13 pt light, with fractional digits smaller
  - Artist about 11 pt grey
  - Platter BPM about 15 pt with the fraction digits smaller ("128.00" with a small ".00")
  - Pitch line about 8 pt
  - Mixer labels 8 pt caps under their knobs
  - Browser list title about 17 pt (at the Large browser font); column headers the same size as the cells
- **Buttons:** plates 16–20 pt tall, square or with tiny corner radius. Toggled state is a solid fill; idle is a dark plate or an outline. Round CUE/PLAY about 32 pt. Pads 21 pt tall rows about 160 pt wide. Top-bar icon buttons 20 x 20.
- **Knobs:** about 14 pt arcs with a single indicator tick, the caps label beneath, stacked at about 37 pt pitch.
- **Density:** very high. The whole 2-deck performance surface (header, overview, stems, pads, loop, transport, platter, sync) fits into 248 pt of height. Labels are terse caps abbreviations (AU, MA, MT, Q, INT, CFX, S/M for sampler/master). Disabled states are shown by dimming, never by hiding.
- **Numbers:** right-aligned in fixed columns (Time, BPM, Date) in the list. Times use 00:00 in the list and -mm:ss.t on decks.
- **Empty states:** dim placeholder text in place ("Not Loaded."). Placeholder controls stay visible but dim, so the layout does not jump when a track loads.
- **The browser is user-scalable:** a Font Size slider plus a Line Space slider in Preferences. Row height follows the font.

---

## 4. Prioritised layout changes for OpenDecks Session

These map rekordbox's arrangement onto our own visual language (DESIGN.md: hairline emission lines, bone ink, Barlow Semi Condensed). Sizes are the rekordbox proportions at 1440 x 900, rounded to our 4 px grid.

**P1: the band structure (biggest "feels like home" gain).**
1. **Perform:** order the bands as top bar (28–32) → **full-width waveform zone** (one stacked lane per deck, 140 px for 2 decks, 196 px for 4, playhead at the horizontal centre, zoom column with + / RST / − / collapse on the far left) → **deck row 248 px** (deck 46% | mixer 7.5% | deck 46%) → **global bar 40 px** → browser takes the rest → status band 30 px. Today we put the lanes per deck and the PHONES/crossfader/MASTER footer under the decks. Keep the footer, but make it this 40 px global bar.
2. **Fixed heights, flexible browser:** deck row, global bar and player keep their height when the window grows. Only the browser grows. Widths scale proportionally, and the tree stays at about 28% (currently a fixed 260 px sidebar).
3. **Prepare:** use a **256 px player band** with three columns: an 80 px transport column (loop in/out, jump, CUE, PLAY) | waveform block (title line, then overview, then enlarged waveform, then the hot cue A–H / memory / loop / Q toolbar) | a **~210 px cue list on the right** with MEMORY | HOT CUE | INFO tabs. Our current 236 px player puts the cue tools at the right instead of a list. Move them to the bottom toolbar and make the right column a cue list.

**P2: control order inside the deck and mixer.**
4. **Deck order, top to bottom:** header (numeral, artwork, title over artist, BPM, remain/elapsed, KEY SYNC + key shift, BEAT SYNC, MASTER) → overview (≈43) → **stem strip** (MUTE + VOCAL/INST/DRUMS, right-aligned, ≈20) → control row. The control row from the outer edge inward: view rail (pads / grid edit) → pad grid A–D | E–H with a **pad-mode dropdown** below → loop column (size, ½/2×, auto/manual) → tempo mode, CUE, PLAY → platter/BPM readout → SLIP / Q / MT column next to the mixer. **Mirror deck 2** (pads on the outer right edge). Our current row (pads, LOOP, BEAT JUMP, Q/SLIP, CUE/PLAY, tempo) has the right parts but not this order or the mirroring.
5. **Mixer channel:** TRIM, HIGH, MID, LOW stacked vertically → CUE → fader → STEM. In 4-deck layouts use **channel order 3-1-2-4**. Our CFX knob moves into the FX panel's CFX row.
6. **Global bar:** headphone MIX + LEVEL left of centre, crossfader centred under the mixer with 2 x 2 assign grids on both sides, **recording at the right end** (source, level, meter, REC, elapsed / limit). Move RECORD out of the top bar; the top bar keeps only a show/hide toggle for it.

**P3: top bar and layout switching.**
7. **Top bar, left to right:** mode selector (PREPARE / PERFORM, a dropdown or tabs), **layout selector** with schematic glyphs (Perform: 2 Horizontal, 2 Vertical, 4 Horizontal, 4 Vertical, Browse; Prepare: 1 Player, 2 Players, Simple, Full Browser), then panel toggles **FX, SAMPLER, MIXER, REC**. Right: status (engine/MIDI), settings, master meter + master volume, clock. Add keyboard shortcuts for layouts (⌘7–⌘0 pattern) and a matching View menu on macOS.
8. **Docked panels** at fixed places: FX above the waveform zone (2 rows: FX1 | FX2 with three slots each plus a CFX row), Sampler between the global bar and the browser (2 x 8 slots split 4 | 4, bank row). The browser shrinks by exactly the panel height. Confirm before mode switches that unload decks (but never during playback; PRODUCT principle 2).

**P4: browser conventions.**
9. **Source rail:** a narrow icon column at the left edge of the tree pane that switches the tree's source: Collection, Playlists (our sets/smart lists), Explorer, Devices (our sticks with sync), Histories, Recordings, Related. This replaces one long sectioned tree. Keep DEVICES sync state visible in the Devices source and as a small icon at the bottom of the tree pane.
10. **List header row:** list name + "(N Tracks)", a deck/player **target selector**, a density toggle, and the search field at the right. A status band at the bottom with "N tracks, total time, size".
11. **Column header context menu:** "Auto-size this column / all columns" plus a checkable list of every available column. The leftmost attribute column carries the loaded-on-deck marker. Keep our doubled-sodium + numeral marker, in the position where rekordbox users look for it (next to the first column).
12. **Right-side panel strip:** Info (key/value track details), sub-browser (a second tree + list for dragging between playlists) and Related. These open as panels that narrow the list, not as modal windows.

**P5: flows and details.**
13. **Loading:** drag to deck/player; double-click loads in Prepare; the context menu "Load ▸ Deck n"; loaded rows recolour (for us: the doubled sodium line + deck numeral). Explorer folders list files immediately with BPM/Key columns, sorted by BPM.
14. **Stems everywhere:** a per-deck stem strip, a STEMS pad mode, a STEM button per mixer channel, and stem targets in the FX panel. Show the controls even while separating (disabled, with a SEPARATING legend), never hide them.
15. **Phrases:** opt-in display toggles for the enlarged and the overview waveform, plus "always show phrase types". Phrase editing goes next to grid editing (a deck alt panel, and the Prepare GRID tab).
16. **Splitters:** a draggable tree | list splitter. Collapse and expand buttons for the waveform zone instead of a drag handle. Remember sizes per layout.
17. **Preferences:** a separate window with a category list + search on the left, tabs at the top, card sections, and a fixed "Reset to defaults". Expose the browser **font size and line spacing** sliders (rekordbox users rely on them) and a Media Browser checklist for which sources appear in the rail.
18. **Density:** match rekordbox's compactness in Perform (8–9 pt caps legends, 16–20 pt plates) on desktop. Keep our kiosk 44 px touch sizes as the separate profile. Show disabled controls dimmed in place so nothing jumps when a track loads.
