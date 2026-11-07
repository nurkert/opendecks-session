# Parity verification harness

SPEC §16 makes feature completeness and the quality goals a
binding, automated acceptance. This page explains the harness that tracks
it: the crate `crates/od-parity` and three tables under `tests/parity/`.

| File | What it holds | SPEC |
|---|---|---|
| `tests/parity/features.toml` | every feature of the professional feature set (SPEC §1, §5.5, §10, §11, §12), with scope, status, tests and evidence | §16.1 |
| `tests/parity/views.toml` | every view, layout and panel from the layout reference (SPEC §12) with its required elements (accessible labels), their band, region and order; sizes and layout invariants | §16.3 |
| `tests/parity/flows.toml` | the quality-goal flows with the usual step count and our target | §16.4 |
| `docs/feature-matrix.md` | **generated** from `features.toml`; do not edit by hand | §16.1 |

## Running it

```sh
# Gap report (Markdown on stdout; --out FILE also writes it)
cargo run -p od-parity --bin parity -- check
# Same, but gaps fail (exit code 1) — the final gate
cargo run -p od-parity --bin parity -- check --strict     # or OD_PARITY_STRICT=1

# Regenerate docs/feature-matrix.md after editing features.toml
cargo run -p od-parity --bin parity -- matrix
cargo run -p od-parity --bin parity -- matrix --check    # exit 1 if stale

# Every test id the harness knows (optionally filtered)
cargo run -p od-parity --bin parity -- list-tests od-engine::render

# The tests (part of `cargo test --workspace`)
cargo test -p od-parity
OD_PARITY_STRICT=1 cargo test -p od-parity --test parity feature_set_complete
```

The tests in `crates/od-parity/tests/parity.rs`:

| Test | Enforced | Fails when |
|---|---|---|
| `features_toml_is_valid` | always | schema error, duplicate id, unknown area, exclusion not named in SPEC §1, out-of-scope row without exclusion, a referenced test id that does not exist, an invalid label in `views.toml`, a flow whose target is worse than the usual count or whose step lists do not match its counts, a flow label missing from `views.toml` |
| `feature_matrix_is_generated` | always | `docs/feature-matrix.md` differs from what `parity matrix` renders |
| `feature_set_complete` | only with `OD_PARITY_STRICT=1` | an in-scope, non-deferred feature is not `done`; a `done` feature lists no test; a flow has no `ui_test`. Without the variable it prints the report and passes |

So `cargo test --workspace` stays green while the product is incomplete, but
the tables can never silently rot: a renamed or deleted test breaks
`features_toml_is_valid` immediately.

## Headless UI suites (`od-ui`)

The UI layer of SPEC §16.2–§16.4 runs in `crates/od-ui/src/ui_tests/`
(part of `cargo test --workspace`; no display needed):

| Module | What it does |
|---|---|
| `harness` | Temporary library (generated 120 BPM WAVs imported into the collection, a playlist, an Explorer root with a folder holding a file outside the collection, English UI, recordings in the temp dir), the real `App` window on Slint's testing backend (`i-slint-backend-testing`, one backend per test thread, mock time) with `AppConfig { null_audio, headless }`: the engine renders into a null host thread, no MIDI service, no update check, no stem model download. Elements are found by `accessible-label`; input (click, double-click, right-click, drag, wheel, keys, menu items) is dispatched at the element's centre and every action counts one step. `OPENDECKS_VOLUMES_DIR` / `OPENDECKS_DATA_DIR` point at a per-process temp dir. |
| `views` | SPEC §16.3: for each size of `views.toml` (desktop, laptop, kiosk = the kiosk configuration at 800×480) and each view, a fresh app is opened in that view and each required element is checked: present (exactly once), visible, inside the window, no overlap within its region, order along the region axis, touch target ≥ `min_target` for interactive roles. Views the app cannot show are reported as unavailable. Writes `target/parity/views-<size>.{json,md}`. |
| `flows` | SPEC §16.4: the six flows of `flows.toml`, driven through the window; the measured step count and whether the end state was reached go to `target/parity/flows/<id>.json`. |
| `features` | Behaviour tests linked from `features.toml` (drag to deck, row menu, info panel, waveform zone and zoom, splitters, colour-blind / contrast, panel toggles, …). |

Report-only by default: the view and flow tests assert what the app does
(a flow's end state, a feature's behaviour) but not the parity targets;
with `OD_PARITY_STRICT=1` every view must pass at every size and every
flow must be achievable within its target. `parity check` reads
`target/parity/` and adds labels present (%), views passing per size and
the flow table; in strict mode failing views and flows are gaps.

```sh
cargo test -p od-ui --lib ui_tests -- --nocapture   # writes target/parity/
cargo run -p od-parity --bin parity -- check
```

Context-menu items are native `MenuItem`s and cannot carry an accessible
label; the view test maps `ctx.*` and `menu.*` labels to their English titles
(`MENU_ITEMS` in `ui_tests/views.rs`), listing only items that exist. The
View menu (`menu.view`) is the platform menu bar, which the testing backend
draws in the window on macOS only; elsewhere that view reports as unavailable.
Element debug info (needed by the testing backend's element search) is
compiled into non-release builds (`crates/od-ui/build.rs`).

## Gap categories in the report

- **In-scope features not done** — grouped by PARTIAL / ENGINE-ONLY / MISSING with priority.
- **DONE without a test** — the feature works but nothing proves it (SPEC §16.1: not acceptable at the gate).
- **Unknown test ids** — a listed test does not exist (always an error).
- **Invalid exclusions** — `scope = "out"` with an exclusion SPEC §1 does not name (always an error).
- **Deferred** — in-scope features with a `milestone` listed in `meta.deferred_milestones` (today `M6`: OneLibrary, Pro DJ Link). Tracked, shown, not counted. Remove the milestone from `deferred_milestones` when that milestone becomes current.
- **Views and flows** — label coverage (how many required labels already appear literally as `accessible-label` in `crates/od-ui`; a progress indicator only) and flows without a driving UI test.

## How test ids are found

The checker parses the Rust sources of every package under `crates/` and
`apps/` instead of running `cargo test --workspace -- --list`. Reasons: the
list needs a full build of every test binary (Slint, Rubber Band, ONNX), it
cannot run inside `cargo test` without contending for the build lock, and
it prints names without their package. Parsing is fast, build-free and
gives the same names cargo prints, prefixed with the package:

| Test location | Id |
|---|---|
| `crates/od-engine/src/mixer.rs`, `mod tests { #[test] fn crossfader_curves }` | `od-engine::mixer::tests::crossfader_curves` |
| `crates/od-engine/src/dsp/eq.rs` (module `dsp::eq`) | `od-engine::dsp::eq::tests::isolator_kills` |
| `crates/od-engine/tests/render.rs` (integration test binary `render`) | `od-engine::render::cdj_cue_button` |

A test is any `fn` after `#[test]` or an attribute ending in `::test`
(`#[tokio::test]`); comments and string literals are skipped, inline
`mod x { … }` blocks extend the path. `#[ignore]`d tests are found but
produce a warning when a feature relies on them. Not seen: tests generated
by macros and modules relocated with `#[path]` (none exist today). Use
`parity list-tests` to see exactly what is found.

"The test ran" (SPEC §16.1) is covered on CI by the test job itself: every
listed test lives in a crate that `cargo test --workspace` runs; an
`#[ignore]`d test is flagged.

## Adding or changing a feature

1. Add a `[[feature]]` to `tests/parity/features.toml` in its area block:

   ```toml
   [[feature]]
   id = "fx-echo-out"             # kebab-case, stable forever (other files refer to it)
   area = "effects"               # one of the [[area]] ids; order of areas = order in the matrix
   name = "Release FX: echo out"
   scope = "in"                   # "out" needs exclusion = streaming | cloud | video | dvs
   status = "done"                # done | partial | engine-only | missing
   prio = "P2"                    # optional, for gaps: P1 | P2 | P3
   milestone = "M6"               # optional; deferred if listed in meta.deferred_milestones
   qualifier = "beyond the usual" # optional, shown after the status
   tests = ["od-engine::render::echo_out_tails_after_release"]
   evidence = ["crates/od-engine/src/release_fx.rs::EchoOut::process"]
   note = "What is missing or special"
   spec = "SPEC §10.2"            # optional, where it is required
   ```

2. Run `cargo run -p od-parity --bin parity -- matrix` and commit both files.
3. When a feature becomes `done`, link at least one test at the lowest level
   that proves the behaviour and, where a user reaches it, an end-to-end
   test (SPEC §16.2). Only something SPEC §1 excludes may be `scope = "out"`;
   any other professional feature (for example a track edit mode) stays in scope
   until SPEC §1 is amended.

## Adding a view or a flow

**View** (`tests/parity/views.toml`): give it an `id` (`perform.…`,
`prepare.…`, `panel.…`, `browser.…`, `prefs.…`, `ctx.…`, `menu.…`), the layout reference (SPEC §12)
`source`, the `sizes` it must pass (empty = all), its `bands` from top to
bottom, and one `[[view.region]]` per group of controls with its `band`,
`place` (as the layout reference (SPEC §12) describes it), `order` and the required `elements`
in that order:

- `row`: left edges non-decreasing (stacked elements may share an x),
- `column`: top edges non-decreasing,
- `grid`: row-major,
- `any`: presence only.

**Flow** (`tests/parity/flows.toml`): state the task and start state, cite
a note on how the usual count was established (`reference_note`), list both
paths step by step (lengths must equal the counts; the step rule is in
`meta.step_rule`), set `inferred = true` where the reference path was not
observed, list the labels the flow touches, and set `ui_test` to the id of
the UI test that drives it once it exists.

## Accessible-label convention

Every required element carries a stable `accessible-label` so the headless
view test (Slint testing backend) can find it. Labels are identifiers, not
UI text: they are never translated and never shown.

```
<scope>[n].<part>[.<part>…]
```

- Parts are lower-case kebab-case (`key-sync`, `eq-high`, `time-remaining`).
- An instance number is glued to its noun: `deck1`…`deck4`, `player1`,
  `ch3`, `unit2`, `slot12`, `lane4`. No separate numeric parts.
- The last part may instead be a single pad / hot-cue letter `A`–`H`
  (`deck1.pad.A`, `player1.hot-cue.H`).
- The scope is one of:

| Scope | Region (layout reference (SPEC §12)) | Examples |
|---|---|---|
| `app` | window-level: mode-change confirmation, dialogs | `app.confirm.yes` |
| `topbar` | top bar | `topbar.mode`, `topbar.layout`, `topbar.toggle.fx`, `topbar.settings`, `topbar.clock` |
| `wave` | enlarged waveform zone | `wave.lane1`, `wave.zoom-in`, `wave.zoom-reset`, `wave.collapse` |
| `deck1`…`deck4` | Perform deck | `deck1.play`, `deck1.cue`, `deck1.pad.A`, `deck1.pad-mode`, `deck1.loop.halve`, `deck1.stem.vocal`, `deck1.key-sync`, `deck1.master-tempo` |
| `player1`, `player2` | Prepare player | `player1.hot-cue.A`, `player1.memory.set`, `player1.grid.tap`, `player1.cue-list.rows` |
| `mixer` | mixer channels and master | `mixer.ch1.trim`, `mixer.ch1.eq-high`, `mixer.ch3.fader`, `mixer.ch2.stem` |
| `global` | global bar | `global.phones.mix`, `global.xfader`, `global.xf-a.deck1` |
| `rec` | recording section / bar | `rec.toggle`, `rec.time`, `rec.meter` |
| `fx` | FX panel | `fx.unit1.slot2.depth`, `fx.unit2.tap`, `fx.cfx.ch1`, `fx.stem.vocal` |
| `mpl` | Mix Point Link | `mpl.mix-in`, `mpl.deck1.points` |
| `sampler` | sampler panel | `sampler.slot1`, `sampler.bank`, `sampler.seq.rec` |
| `browser` | browser | `browser.search`, `browser.rail.explorer`, `browser.tree`, `browser.rows`, `browser.header.bpm`, `browser.panel.info` |
| `info`, `subbrowser` | right-hand panels | `info.bitrate`, `subbrowser.tree` |
| `status` | status band | `status.summary`, `status.activity` |
| `lighting` | LIGHTING view | `lighting.rig.add`, `lighting.output.artnet`, `lighting.stage`, `lighting.mode.auto`, `lighting.blackout`, `lighting.cue.go` |
| `prefs` | Preferences window | `prefs.nav.audio`, `prefs.audio.buffer`, `prefs.reset` |
| `menu`, `ctx` | menu bar, context menus | `menu.view.info`, `ctx.row.load`, `ctx.header.auto-size` |

Rules for the UI team:

1. Set the label on the element the user operates (the touch area), not on
   a decorative child, so size and hit-test checks measure the right thing.
2. Labels are unique within a window. Repeated components take the instance
   from a property: `accessible-label: "deck\{root.deck}.play";`.
3. Mirrored decks keep the same part names; only the instance changes.
4. Readouts that are not interactive (`deck1.bpm`, `rec.time`) carry labels
   too, so presence, order and clipping can be checked.
5. Hidden-by-layout is allowed only for views that do not list the element;
   unavailable controls stay in place, dimmed (layout reference (SPEC §12) §3).

`parity check` reports how many required labels already appear literally
in `crates/od-ui` (labels built from expressions are not counted; the view
test is the real gate).

## Sizes and invariants

`views.toml` lists the sizes every view is opened at — 1440×900 and
1280×800 (desktop, touch target ≥ 24 px) and 800×480 (kiosk, ≥ 44 px; Perform
views and panels) — and the invariants the view test asserts on each:
presence, visibility, no clipping, no sibling overlap, order, region and band
placement, minimum target size, shared rules aligned (±1 px), no elision
where text fits, contrast ≥ 4.5:1 (large text ≥ 3:1), no horizontal window
scroll, fixed band heights with the browser absorbing growth, disabled
controls dimmed in place, scales 0.75–2.0, and no modal dialog in Perform.

## Kiosk exemptions (controller-first layout)

The kiosk (800×480, 44 px targets) is operated mainly through the
controller (SPEC §3.3), so its touch layout shows what the controller
cannot show or do and leaves out what the controller carries physically. A
view lists these per group, only for the `kiosk` size:

```toml
[[view.kiosk]]
omit = ["mixer.ch1.trim", "mixer.ch1.fader"]
reason = "The channel strips of the controller's mixer"
# shown_in = "perform.browse"   # only for elements shown on another kiosk screen
```

Rules (checked by `features_toml_is_valid`, applied by the view test):

- Only elements the controller carries may be omitted: mixer channel strips
  (trim, EQ, cue, fader), headphone knobs, crossfader and its assign,
  master level, pads and pad modes, the loop section, the tempo fader and its
  range, the Beat FX level / beat keys / channel select, Colour FX, sampler
  pads. Each group names the hardware control in `reason`.
- Never omitted: track info (number, artwork, title, artist), waveforms and
  overviews, BPM / key / time read-outs, the platter read-out, SYNC /
  MASTER, CUE / PLAY, part mutes, load, record, settings, the clock, MIDI and
  stick state, the browser list. Such elements may only move to another
  kiosk screen: `shown_in` names the kiosk view that checks them (the
  decks screen lists the browser through `perform.browse`).
- Views that are not part of the kiosk (Prepare, Preferences) drop `kiosk`
  from `sizes` with a note; the kiosk boots into Perform (SPEC §3.3).

The touch layout itself turns on with the kiosk configuration (or a window
no taller than 480 px): `Theme.touch` raises every key to 44 px, the top
bar condenses (mode key, panel switches, settings, meter, clock, the rest in
⋯), decks become `KioskDeck` (header, overview with part mutes, transport
row), the browse screen has a two-column 44 px source rail, 44 px rows,
headers and tree items.

## Becoming the CI gate

Today CI runs `parity check` in a non-blocking job (`parity` in
`.github/workflows/ci.yml`) and uploads the report as the
`parity-report` artifact and the job summary; the always-enforced tests run
in the normal test job. When the gap count reaches zero:

1. Set `OD_PARITY_STRICT: "1"` in the `env` of the test job (makes
   `feature_set_complete` fail on any gap) and remove `continue-on-error`
   from the `parity` job, passing `--strict`.
2. The headless view suite and the flow UI tests exist (see above); the
   parity job runs them before the report.
3. From then on a feature can only be marked `done` together with its tests,
   and a regression in any linked test fails CI.
