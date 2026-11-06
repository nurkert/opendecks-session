---
name: OpenDecks Session
description: A DJ library and performance instrument where every signal is a hairline emission line on a charcoal continuum.
colors:
  ground: "#151514"
  ground-band: "#1A1A19"
  plate: "#21211F"
  plate-raised: "#2A2A27"
  rule: "#33322F"
  rule-strong: "#4B4A46"
  bone: "#E6E0D2"
  bone-2: "#B7B1A4"
  bone-3: "#8B867B"
  w405-violet: "#9B7BFF"
  w436-indigo: "#7C83FF"
  w486-cyan: "#4FB8F0"
  w546-green: "#82D16B"
  w589-sodium: "#F5CE4A"
  w615-orange: "#F39047"
  w656-deep-red: "#EF5A5A"
  w405-violet-cb: "#CC79A7"
  w486-cyan-cb: "#56B4E9"
  w546-green-cb: "#009E73"
  w589-sodium-cb: "#F0E442"
  w615-orange-cb: "#E69F00"
  w656-deep-red-cb: "#D55E00"
  ground-hc: "#0B0B0B"
  ground-band-hc: "#121212"
  plate-hc: "#1C1C1B"
  rule-hc: "#4B4A46"
  bone-hc: "#FFFFFF"
  bone-2-hc: "#E6E0D2"
  bone-3-hc: "#B7B1A4"
typography:
  display:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "34px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "normal"
  headline:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "24px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "normal"
  title:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: "normal"
  body:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.35
    letterSpacing: "normal"
  nav:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "13px"
    fontWeight: 600
    letterSpacing: "2.2px"
  button-label:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "12px"
    fontWeight: 500
    letterSpacing: "1.8px"
  label:
    fontFamily: "Barlow Semi Condensed"
    fontSize: "11px"
    fontWeight: 500
    letterSpacing: "1.6px"
rounded:
  none: "0px"
  rail-node: "3.5px"
spacing:
  s1: "4px"
  s2: "8px"
  s3: "12px"
  s4: "16px"
  s5: "24px"
  s6: "32px"
  rail-x: "28px"
  row: "28px"
  row-kiosk: "44px"
components:
  button-plate:
    backgroundColor: "transparent"
    textColor: "{colors.bone}"
    typography: "{typography.button-label}"
    rounded: "{rounded.none}"
    padding: "0 30px 0 22px"
    height: "32px"
  button-plate-compact:
    backgroundColor: "transparent"
    textColor: "{colors.bone}"
    rounded: "{rounded.none}"
    height: "26px"
  button-plate-tall:
    backgroundColor: "transparent"
    textColor: "{colors.bone}"
    rounded: "{rounded.none}"
    height: "44px"
  button-plate-hover:
    backgroundColor: "{colors.plate}"
  button-plate-pressed:
    backgroundColor: "{colors.plate-raised}"
  button-plate-disabled:
    backgroundColor: "transparent"
    textColor: "{colors.bone-3}"
  field:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.bone}"
    typography: "{typography.body}"
    rounded: "{rounded.none}"
    padding: "0 8px 0 22px"
    height: "32px"
  field-kiosk:
    backgroundColor: "{colors.plate}"
    height: "44px"
  track-row:
    backgroundColor: "transparent"
    textColor: "{colors.bone-2}"
    typography: "{typography.body}"
    padding: "0 12px 0 16px"
    height: "{spacing.row}"
  track-row-hover:
    backgroundColor: "{colors.ground-band}"
  track-row-selected:
    textColor: "{colors.bone}"
  track-row-kiosk:
    height: "{spacing.row-kiosk}"
  rail:
    backgroundColor: "transparent"
    textColor: "{colors.bone-2}"
    typography: "{typography.nav}"
    width: "232px"
  tree-item:
    textColor: "{colors.bone-2}"
    typography: "{typography.body}"
    height: "26px"
  tree-item-hover:
    backgroundColor: "{colors.plate}"
  device-line:
    textColor: "{colors.bone}"
    height: "64px"
  device-line-roomy:
    height: "78px"
  waveform-well:
    backgroundColor: "{colors.ground-band}"
    height: "92px"
---

# Design System: OpenDecks Session

## Overview

**Creative North Star: "The Emission-Line Rail"**

OpenDecks Session is a calibrated instrument, not a dashboard. Every surface registers against one vertical rail set off-centre at the left edge (28px in from the window), and every signal (deck, cue point, sync state, harmonic fit, transition clash) appears as a 1 to 2px emission line at one of seven fixed wavelengths on a charcoal continuum. Bone legends set in one condensed grotesque carry the words; the lines carry the state. The result reads like a spectrograph plate: dense, quiet, and exact, legible in a dark club, on a bright terrace (high-contrast variant), and on a 7" Pi touchscreen.

State is carried by the form of a line, never by its hue alone. Solid means live or in sync, dashed means pending or out of date, half-height means stale or missing, struck (dimmed line crossed by a deep-red tick) means error or disabled, and doubled means the element outranks its neighbours. Colour is a second, redundant channel, and every deck colour sits next to its numeral, every cue colour next to its letter.

The world refuses the category default of grey panels, glowing neon waveforms, gradients and boxed widgets. Containers are not drawn; columns are separated by single hairline rules and alignment. Waveforms are bone hairline columns with charcoal gaps; colour enters them only as a 1px band mark where one frequency band clearly dominates.

**Key Characteristics:**
- One off-centre rail that brand, sections, the library tree and selection ticks all hang from.
- Colour only as hairline emission lines, deck numerals, cue letters and short semantic legends.
- State by line form: solid, dashed, half, struck, double.
- Outlined plates, never filled buttons or rounded cards; square corners everywhere except the rail's two terminal nodes.
- One family (Barlow Semi Condensed), ranked by size, tracking and ink.
- Flat: no shadows, no glow, no gradients; depth is three steps of charcoal.

## Colors

A charcoal continuum with bone ink and seven narrow spectral lines; each wavelength is a fixed role, not a decoration.

### Primary
- **Sodium 589** (w589-sodium): the attention line. Playheads and preview cursors (doubled), selection ticks, pending sync (dashed), loaded-on-deck marker (doubled), current mode tab underline, set kind marker, LOCKED legend, CUE and LOCK plate leads, analysis progress.

### Secondary
- **Green 546** (w546-green): the live line. In-sync sticks (solid), syncing fill, PLAY, ADD MUSIC / SYNC / REGISTER plate leads, harmonic-fit marks in the KEY column, on-device dot in the tree, deck 3, hot cue A.
- **Deep Red 656** (w656-deep-red): the fault line. Struck crossbars, error detail text, missing files, DELETE / FORGET / RECORD plates, recordings, level-meter clip, hot cue G.

### Tertiary (deck and cue wavelengths)
- **Cyan 486** (w486-cyan): deck 1; LIBRARY section; SYNC state; field focus default; selection highlight in text inputs (35% alpha); hot cue B.
- **Orange 615** (w615-orange): deck 2; LOOP state; hot cue C.
- **Violet 405** (w405-violet): deck 4; SLIP state; hot cue D.
- **Indigo 436** (w436-indigo): smart lists (dashed marker, rule field focus), KEYLOCK state, hot cue F. It has no colour-blind variant in the build.
- Hot cue E is sodium; hot cue H falls back to bone-2.

### Neutral
- **Charcoal Ground** (ground): the window. Overlaid at start-up by a 640x4 texture that random-walks ±3 levels around the ground value, giving faint vertical banding.
- **Band Charcoal** (ground-band): waveform wells, track-row hover, rendered waveform background.
- **Ash Plate** (plate): field backgrounds and plate-button / tree hover.
- **Raised Ash** (plate-raised): pressed plate buttons and the tree's hover add-target.
- **Rule** (rule) and **Strong Rule** (rule-strong): hairline dividers; strong rule for plate outlines, the rail itself, rulers, off-state lines.
- **Bone, Bone 2, Bone 3** (bone, bone-2, bone-3): three inks at 13.6:1, 8.7:1 and 5.0:1 on ground. Bone for titles and selected/primary values, bone-2 for secondary text and unselected rows, bone-3 for caps labels, metadata and disabled text.

### Accessibility variants
- **Colour-blind safe** (the `-cb` tokens): the Okabe-Ito set replaces 405, 486, 546, 589, 615 and 656; 436 is unchanged.
- **High contrast** (the `-hc` tokens): darker ground, brighter bone ramp, rule lifted to strong rule, and the hairline weight doubles from 1px to 2px. Plate-raised and rule-strong have no high-contrast variant.

### Named Rules
**The Second Channel Rule.** Colour never carries state alone. Every coloured line also has a form, every deck colour sits beside its numeral, every cue colour beside its letter.

**The Fixed Wavelength Rule.** Each wavelength keeps its role across the product: deck 1 = 486, deck 2 = 615, deck 3 = 546, deck 4 = 405; live = 546, attention = 589, fault = 656. Never introduce an eighth hue or a tint between lines.

**The Doubled Sodium Rule.** A doubled sodium line means "this is where now is" or "this needs you": playheads and preview cursors, a track loaded on a deck, changes from players awaiting review. A master deck is its own deck line doubled. Do not double anything else.

## Typography

**Display Font:** Barlow Semi Condensed (bundled Regular 400, Medium 500, SemiBold 600)
**Label/Mono Font:** the same family; numerals sit in right-aligned fixed-width columns rather than a separate mono face.

**Character:** One condensed grotesque doing all the work, ranked by size, tracking and ink rather than by family or weight contrast. Uppercase tracked caps are the instrument's legends; mixed case is reserved for content (titles, artists, prose).

### Hierarchy
- **Display** (600, 34px): deck numerals on wide deck plates (26px compact) and the large BPM / REMAIN readouts in kiosk (34px). Readout values otherwise run 28px / 24px / 18px at weight 500.
- **Headline** (600, 24px): the list title (editable in place), section titles, the PERFORM title (4px tracking; 17px / 3px on tight screens). Track-plate readings use 24px at 500.
- **Title** (600, 17px): the selected track on the track plate; deck titles 18px (15px compact); stick and recording names 15px.
- **Body** (400, 14px): track table cells, tree names (600 for folders and sticks), prose. Genre drops to 13px, position and status text to 12px.
- **Nav** (600, 13px, 2.2px tracking, caps): rail section labels. The brand is 20px / 600 / 3px.
- **Button label** (500, 12px, 1.8px tracking, caps; compact 11px / 1.4px).
- **Label** (500, 11px, 1.6px tracking, caps, bone-3): table headers, readout labels, metadata (10px under rail ticks and on devices; 9px for the set-flow legend).

### Named Rules
**The Ink Rank Rule.** Hierarchy is size, tracking and ink (bone, bone-2, bone-3), not colour. Coloured text is limited to deck numerals, fault messages and short semantic legends such as LOCKED.

**The Tabular Column Rule.** BPM, KEY, TIME and RATING sit right-aligned in fixed columns (58, 46, 52, 74px) so values read down the page like a register.

## Layout

The arrangement follows the layout conventions DJs know from DJ hardware and software (owner decision 2026-11-06); the visual language is our own. Desktop (preferred 1440x900, min 800x480) stacks four bands:

1. **Top bar** (44px): brand, PREPARE / PERFORM tabs (2px sodium underline on the current one), engine state, undo, RECORD on the right.
2. **Player band.** Prepare: the 236px track player (title, BPM / KEY / TIME, round CUE and PLAY, position; detail waveform over the overview; HOT CUE A–D | E–H, MEMORY CUE, grid tools on the right). Perform: one enlarged waveform lane per deck (84px, 58px with four decks, 46px tight) with the deck numeral and centre playhead, then odd decks left, mixer centre, even decks right, then the PHONES / crossfader / MASTER footer (44px).
3. **Browser:** the 260px sidebar tree (COLLECTION, PLAYLISTS, DEVICES with sync and eject, RECORDINGS, SET / SMART / FOLDER footer) beside the list: title, count and duration, list actions, search (260px), set flow and transition note for sets, and the track table with a PREVIEW waveform column.
4. **Status band** (30px).

Rhythm is a 4px grid (s1 to s6: 4, 8, 12, 16, 24, 32). Desktop rows are 28px tall; tree rows 26px; table header 30px.

**Kiosk** (Raspberry Pi, full screen, min 480x320): opens in Perform; the sidebar is hidden and the browser is a separate screen. Spare height goes to the waveform lanes; deck panels keep their natural height. Touch sizes: hot cue pads 40px (32px tight), transports 52px (44px tight), rows and fields 44px. Below 620px of height (set from Rust) the mixer is hidden and lanes shrink to 46px.

## Elevation & Depth

Completely flat. There are no shadows, glows or gradients anywhere in the build. Depth is three charcoal steps (ground, ground-band, plate, with plate-raised for press) plus hairline rules. The only translucency is functional: the played portion of a deck overview is veiled with ground at 55% alpha, loop regions are a 3px bar at 60% alpha, and struck lines dim to 45%.

### Named Rules
**The Flat Plate Rule.** Surfaces never lift. A press darkens to raised ash; a hover warms to ash plate; nothing casts a shadow or emits light.

## Shapes

Square corners throughout (0px). Borders are 1px hairlines (2px in high contrast), drawn solid at rest and as 4px-on / 3px-off dashes when active or focused. The only curves are the rail's two 7px terminal nodes (3.5px radius). Glyphs are hand-drawn 10x10 hairline paths (plus, minus, up, down, close, arrow) at 1.2px stroke, never typed symbols or an icon font. Rating is five 6px outlined cells, filled bone-2 to the rating.

## Components

### Emission Lines (the primitive)
- **Forms:** solid (full height); dashed (4px dash, 7px period); half (lower half only); struck (line at 45% alpha crossed by a 1px deep-red tick extending 4px each side); double (two lines 3px apart).
- **Weights:** hair 1px (2px high contrast); line 2px; 3px only for the selected set-flow position.
- **Progress:** a dashed line at 50% alpha filled solid from the start by progress, 180ms ease-out.

### Buttons (plate buttons)
- **Shape:** an outlined rectangle, square (0px), hairline strong-rule outline, transparent at rest.
- **Lead line:** a 2px vertical emission line 10px in from the left, inset 6px top and bottom, in the plate's wavelength; the caps label starts at 22px. An optional hairline glyph sits 20px from the right.
- **Sizes:** 32px default, 26px compact, 44px tall (kiosk touch).
- **Hover / Press:** background eases to ash plate (hover) or raised ash (press) over 90ms.
- **Active (toggled on):** the outline turns dashed in the plate's tone (PLAY, SYNC, LOCK, RECORD, deck count, BROWSE).
- **Disabled:** the lead line is struck and the label drops to bone-3.

### Inputs / Fields
- **Style:** ash plate fill, 1px rule outline, a hairline bone-3 lead line at 10px, 22px text inset, 32px tall (44px in the kiosk browser). Placeholder in 12px caps at 1.4px tracking, bone-3.
- **Focus:** the outline turns dashed and the lead line takes the field's tone (cyan by default, indigo for smart rules, sodium for transition notes).
- **Error:** a 12px deep-red message below the field.

### Toggles (line toggles)
- A 2px vertical line plus a 13px label: solid in its tone when on, half-height strong-rule when off. Perform state marks (PLAY, SYNC, MASTER, KEYLOCK, LOOP, SLIP) use the same grammar at 11px / 600 / 1.5px caps.

### Navigation (sidebar and top bar)
- **Sidebar sections:** caps headings (COLLECTION, PLAYLISTS, DEVICES, RECORDINGS) with a hairline plus glyph for creation where it applies.
- **Tree:** 26px rows indented 14px per level; kind marker lines left of the name (sticks solid green 2px, sets solid sodium 1px, smart lists dashed indigo 1px); a 1px strong-rule length bar under each list proportional to its playing time; counts right-aligned in bone-3. Selection is an 8px sodium tick at the left edge, never a fill.
- **Mode tabs:** PREPARE / PERFORM in the top bar; the current one sits on plate with a 2px sodium underline.

### Track Table
- No boxes, no zebra fills. Caps header with a bone sort label and hairline arrow, 1px rule beneath.
- **Selection:** a 12x2px sodium tick at the row's left edge, ink lifting to bone and the title to weight 500.
- **Loaded on a deck:** a doubled sodium hairline, then the deck's 2px line, then the deck numeral in its colour.
- **Harmonic fit** (vs. the selected track): a 1px green line in the KEY column, solid for same or adjacent key, dashed for two steps.
- **Analysis state:** nothing when analysed; half-height sodium when pending; struck deep red when the file is missing (ink drops to bone-3).

### Track Plate and Hot Cue Ruler
- Title (17px), artist, then BPM / KEY / TIME readings (11px caps over 24px values).
- **Overview well:** 92px of band charcoal; bone hairline columns every 2px with 1px band marks (656 low, 546 mid, 486 high) where one band holds over 55% of the energy; dimmed sodium phrase ticks every 16 bars in the top and bottom margins; cue lines (2px solid in cue colour with their letter, 1px dashed bone for memory cues); the cursor as doubled sodium.
- **Hot cue ruler:** letters A to H on a strong-rule baseline 18px from the bottom. A set cue is a 2px solid line in its colour with a bone letter and its time; an empty slot is a 1px dashed bone-3 line.
- **Beatgrid:** compact plates, struck while the grid is locked; LOCK goes dashed-active in sodium with a LOCKED caps legend.

### Device Line (signature)
- One stick, one line: 2px, inset 8px top and bottom, 64px rows (78px in the Devices section). In sync is solid green; pending is dashed sodium; error is struck with the detail text in deep red; unknown or unregistered is half-height.
- **Syncing:** the dashed sodium line (60% alpha) fills solid green from the bottom by progress, animated 320ms on the spring easing (one damped overshoot). Motion off sets it to 0ms.
- Name 15px / 600, detail 12px, capacity and formats in 10px caps; compact SYNC / EJECT plates on the right.

### Set Flow
- 150px. Each position is a vertical line whose height is the track's energy (min 6px, max 92px), bone-2 at 2px, or sodium at 3px when selected; BPM and key are written beside it.
- Transitions between positions are 1px lines whose form is the fit: solid bone-3 good, dashed sodium caution, half unknown, struck deep red clash. A 9px caps legend spells the code out.

### Deck Panel and Mixer (Perform)
- The deck's 2px line runs the full panel height on the left (doubled when master) beside its numeral in the deck colour.
- **Header:** numeral, title and artist, REMAIN / KEY / BPM readouts (24px, BPM 28px), SYNC and MASTER toggles.
- **Overview** (34px, 26px compact) with played-portion veil, cue lines, doubled sodium position line and needle search; LOAD SELECTED when empty.
- **Stems row:** VOCAL / INST / DRUMS / BASS mute toggles (disabled until the track has stems, SEPARATING… while separating) and KEY LOCK.
- **Performance row:** hot cue pads A–D | E–H in two columns, LOOP (½, size, 2×), BEAT JUMP −4 / +4, Q and SLIP, round CUE and PLAY, the tempo fader with its range.
- **Mixer:** per channel a knob column (TRIM, HI, MID, LOW, CFX) beside the channel numeral, CUE toggle, a 4px level line (deep red at clip) and the channel fader. Knobs are drawn rings with a single indicator line; a changed knob's line takes its band wavelength; double-click resets.
- The crossfader is a single 2px bone line on a strong-rule ruler between A and B.

## Do's and Don'ts

### Do:
- **Do** carry every state in line form first (solid live, dashed pending, half stale/missing, struck error/disabled, double outranks) and in colour second.
- **Do** pair every deck colour with its numeral and every cue colour with its letter.
- **Do** mark selection with a sodium tick on the rail side, not a background.
- **Do** build buttons as outlined square plates led by a short emission line; show the active state with a dashed outline.
- **Do** keep text in the three bone inks and rank it by size and tracking; set legends in tracked caps.
- **Do** draw glyphs as hairline paths and waveforms as bone hairline columns with 1px band marks.
- **Do** raise touch targets to 44px in kiosk (rows, perform header plates, kiosk browser field).
- **Do** gate every animation on the motion switch, at 0ms when off.

### Don't:
- **Don't** use gradients, glow, blur or drop shadows anywhere, including on waveforms and playheads.
- **Don't** box content into cards or panels, and don't round corners; columns are separated by hairline rules only.
- **Don't** fill a waveform with colour or tint it by frequency; colour enters only as a 1px mark on dominant-band columns.
- **Don't** let colour be the only carrier of state, and don't add a hue outside the seven wavelengths.
- **Don't** use doubled sodium for anything other than playheads and cursors, a track loaded on a deck, or something that needs the DJ's attention.
- **Don't** use typed symbols or an icon font for glyphs; draw hairline paths.
- **Don't** resemble rekordbox or any AlphaTheta / Pioneer DJ visual identity.
