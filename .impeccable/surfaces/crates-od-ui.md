---
version: 1
slug: "crates-od-ui"
primary_target: "crates/od-ui"
related_targets: ["apps/opendecks"]
---

# Surface brief: OpenDecks Session app shell (Prepare, Perform, Kiosk)

Scope: the native Slint application, all three surfaces share one world. Mode: Operate.
Audience/job: hobby to semi-pro DJs preparing at home (MacBook) and performing in dark clubs, bright outdoor parties, and on a 7–10" Pi touchscreen. Tasks: browse/sort library, set cues and grids, build sets, watch USB sticks sync, perform with 2/4 decks, record.
Constraints: colour-blind safe (state never by hue alone), scalable UI, no rekordbox resemblance, Lite profile at 30 fps on Pi 4.

## Direction contract

THESIS: The library is a calibrated instrument: every element registers against one off-centre rail, and signal (decks, cue points, sync state) appears only as hairline emission lines on a charcoal continuum. Refuses the category default of grey panels, glowing neon waveforms and boxed widgets.

OWN-WORLD: Charcoal continuum ground with faint vertical banding; bone legends; ash plates for prose and inputs. One condensed grotesque, ranked by size, tracking and ink. Colour only as 1–2px emission lines at seven wavelengths (405 violet, 436 indigo, 486 cyan, 546 green, 589 sodium, 615 orange, 656 deep red). Deck 1 = 486, deck 2 = 615, deck 3 = 546, deck 4 = 405, each always paired with its numeral. State by line form: solid = live/in sync, dashed = pending/out of date, half-height = stale/missing, struck = error/disabled, doubled sodium = master/now playing/needs attention. Buttons are outlined plates led by a short emission line; active = dashed outline. No gradients, no glow, no rounded cards.

STORY: The DJ sees at a glance which stick is in sync and which is pending, edits a cue and watches the stick's line go dashed then solid. In a set they read BPM/key/energy as a calibrated band of lines and spot clashes as struck transitions. In Perform, decks read like spectrograph plates: the playhead is the doubled sodium line.

FIRST VIEWPORT: Prepare at 1440×900. Left: the rail (x=56) with ticks LIBRARY / SETS / DEVICES / RECORDINGS and the playlist tree hanging off it, each list's length as a hairline bar. Centre: the track table, header in tracked caps, rows bone on charcoal, BPM/KEY/TIME in tabular columns, the selected row marked by a rail tick, not a fill. Right column (360px): the selected track as a plate — overview waveform as dense band lines, hot cues A–H as numbered lines on a ruler, beatgrid ticks, then the DEVICES board: one row per stick with a full-height line (in sync), dashed (pending n), sliding fill line (syncing %), struck (error). Bottom: status band with analysis queue as a thin progress line.

FORM: Emission-line rail (challenger, chosen by user over assigned split-flap board; position 6 on my ordered list), seed key 40e4f5ab. Signature interaction: a state change slides the line to its new form with one damped overshoot (spring), sync progress fills a stick's line from dashed to solid. Motion off in Lite profile.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
