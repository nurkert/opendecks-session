# Controller mapping files

Controller mappings live in `mappings/<vendor>/<model>.toml` and are loaded by
`od-controller` (`Mapping::load`, `Mapping::load_dir`). This document is the
reference for the file format (schema version 1).

Mapping files are authored independently for OpenDecks Session (SPEC §2).
Vendor mapping files may be consulted for message facts, but their content,
structure and comments are never copied. Every file lists where its message
facts came from (header comment and `device.sources`), and marks entries
not yet confirmed on real hardware with `verify = true`.

## Conventions

| Thing | Numbering |
|---|---|
| MIDI channel (`channel`, channel sets) | wire value **0–15** (low nibble of the status byte: `0x92` → 2) |
| Note / CC numbers | 0–127, hex allowed (`0x1F`) |
| Decks, mixer channels, FX units (`deck`, `[layers]`) | **1-based** |
| Hot cue slots, pads, sampler slots | **0-based** (slot 0 = hot cue A) |
| Continuous values in actions | `0.0..=1.0`; tempo and colour FX knobs `-1.0..=1.0` |

## Top level

```toml
schema = 1                 # required, must be 1

[device]
id = "pioneer-ddj-flx4"    # stable identifier used by the application
name = "DDJ-FLX4"
vendor = "Pioneer DJ"
model = "DDJ-FLX4"
decks = 2
module = "pioneer-ddj"     # optional Rust device module (see below)
sources = ["..."]          # where the message facts came from

[device.match]
port_names = ["DDJ-FLX4"]  # case-insensitive substrings of the MIDI port name
usb_vendor_id = 0x2B73     # optional, informational
usb_product_id = 0x0041    # optional, informational
```

Port name matching is the primary key. USB IDs are reported alongside the
connection when the device is visible on the USB bus.

### Channel sets

```toml
[channels]
deck = [0, 1]          # entry i = wire channel of deck slot i+1
pad = [7, 9]
pad_shift = [8, 10]
```

An input or output whose `channel` is a set name (`channel = "deck"`) is
expanded once per entry and bound to deck slot `i + 1`. A numeric
`channel` is fixed; deck-scoped actions then need an explicit `deck = n`.

### Deck layers (4 decks on a 2-sided surface)

```toml
[layers]
mode = "hardware"          # or "software"
sides = [[1, 3], [2, 4]]   # decks reachable from each side; first is active initially
```

- `hardware`: the controller itself changes the MIDI channel when a deck
  button is pressed (DDJ-FLX10). Channel sets name decks directly. The layer
  state is only tracked (from `deck.select` inputs) so that soft takeover
  re-arms for the newly selected deck.
- `software`: the controller always sends on the same channel per side.
  Slot `i` of every channel set is side `i`, and inputs are redirected to the
  side's active deck. Deck-scoped outputs are rendered only for the active
  deck and refreshed on a switch. Use `deck.select` with `args = { deck = n }`
  to switch.

Every layer switch drops the 14-bit MSB cache and forgets the physical
position of the new deck's parameters (soft takeover pickup).

### Options

```toml
[options]
soft_takeover = true          # default for absolute inputs
takeover_threshold = 0.03     # normalised pickup distance
jog_ticks_per_revolution = 720
blink_ms = 500                # software blink period
blink_fast_ms = 200
meter_interval_ms = 33        # minimum interval between messages to a level output
max_messages_per_poll = 256   # output budget per poll; the rest goes out next poll
```

### Raw messages

```toml
[[init]]                       # sent once after connecting
bytes = [0xF0, 0x00, 0xF7]
verify = true

[[keepalive]]                  # sent periodically while connected
bytes = [0xF0, 0x00, 0xF7]
interval_ms = 250
```

### Palettes

```toml
[[palettes.pads]]
value = 0x14          # note-on value the hardware understands
color = "#00ff00"     # colour it shows
```

Colour outputs pick the entry nearest to the requested RGB colour.

## Inputs

```toml
[[input]]
name = "Hot cue pad"     # shown in the MIDI monitor
note = 0x00              # exactly one of note / cc
channel = "pad"          # number or channel set
action = "deck.hot_cue"
args = { }               # action arguments
repeat = 8               # expand to 8 consecutive numbers (step = 1)
index_arg = "slot"       # set this argument to the repetition index
verify = false
```

| Key | Meaning |
|---|---|
| `note` / `cc` | Message type and number. Note-off and note-on with velocity 0 both mean "released". |
| `lsb` | LSB controller of a 14-bit pair; implies `mode = "absolute14"`. Default in that mode is `cc + 0x20`. |
| `channel` | Wire channel or channel set name. |
| `deck` | Explicit deck / mixer channel / slot for fixed-channel inputs. |
| `mode` | `button`, `absolute` (7-bit 0–127), `absolute14`, `relative`. Defaults from the action. |
| `encoding` | For `relative`: `offset64` (64 = 0, 65 = +1, 63 = −1; default), `twos_complement` (1 = +1, 127 = −1), `sign_magnitude` (bit 6 = minus). |
| `invert` | Absolute: `1 - v`. Relative: negate. |
| `takeover` | Override soft takeover for this input (default: `options.soft_takeover` for absolute modes). |
| `shift` | Software shift condition: `true` = only while a `deck.shift` input of the same slot (any slot for global inputs) is held, `false` = only while not held. Leave out for controllers that apply shift in hardware. |
| `repeat`, `step` | Expand into `repeat` bindings on numbers `n, n+step, …`. |
| `index_arg`, `index_base` | Set argument `index_arg` to `index_base + i` for repetition `i`. |
| `args` | Action arguments. An **array** value supplies one element per repetition: `args = { beats = [-1.0, 1.0, -2.0, 2.0] }`. |
| `verify` | Not yet confirmed on hardware; flagged in the MIDI monitor. |

14-bit pairs: the MSB is stored and the value is emitted when the LSB
arrives (`(msb << 7 | lsb) / 16383`).

### Actions

Deck-scoped actions take the deck from the channel set (or `deck`).
Buttons marked *press* fire on press only; *momentary* report press and
release.

| Action | Value | Arguments | Produces |
|---|---|---|---|
| `deck.play_pause` | press | | `DeckAction::PlayPause` |
| `deck.cue` | momentary | | `CuePress` / `CueRelease` |
| `deck.jump_to_start` | press | | `JumpToStart` |
| `deck.sync`, `deck.sync_off`, `deck.master` | press | | `Sync`, `SyncOff`, `SetMaster` |
| `deck.keylock`, `deck.key_sync`, `deck.key_reset` | press | | |
| `deck.tempo` | absolute | | `Tempo(-1..1)` |
| `deck.tempo_range`, `deck.tempo_reset`, `deck.tap_bpm`, `deck.quantize` | press | | |
| `deck.jog_touch` | momentary | | `JogTouch(bool)` |
| `deck.jog_turn` | relative | `mode` = `scratch` / `bend` / `search` | `JogTurn { ticks, mode }` |
| `deck.hot_cue` | momentary | `slot` | `HotCue { slot, pressed }` |
| `deck.hot_cue_delete` | press | `slot` | |
| `deck.memory_cue_set` / `_delete` / `_prev` / `_next` | press | | |
| `deck.loop_in`, `deck.loop_out`, `deck.loop_halve`, `deck.loop_double`, `deck.reloop_exit` | press | | |
| `deck.auto_loop` | press | `beats` | `AutoLoop { beats }` |
| `deck.loop_roll` | momentary | `beats` | `LoopRoll { beats, pressed }` |
| `deck.beat_jump` | press | `beats` (signed) | |
| `deck.search` | momentary | `forward` (bool) | |
| `deck.slip`, `deck.vinyl`, `deck.reverse`, `deck.load` | press | | |
| `deck.censor` | momentary | | `Censor(bool)` |
| `deck.shift` | momentary | | `Shift(bool)`; also drives software shift |
| `deck.pad_mode` | press | `mode` = `hot_cue`, `beat_loop`, `beat_jump`, `sampler`, `pad_fx1`, `pad_fx2`, `keyboard`, `key_shift`, `slicer` | |
| `deck.pad` | momentary | `index` | `Pad { index, pressed, velocity }` (generic, interpreted by pad mode) |
| `deck.needle_search` | absolute | | `NeedleSearch(0..1)` |
| `deck.select` | press | optional `deck` | `DeckSelect` for the bound deck, or for `deck` |
| `mixer.trim`, `mixer.eq_high`, `mixer.eq_mid`, `mixer.eq_low`, `mixer.fader` | absolute | | `MixerAction::*` |
| `mixer.color_fx` | absolute | | `ColorFx(-1..1)` |
| `mixer.cue` | press | | `CueToggle` (headphone cue) |
| `mixer.crossfader_assign` | press | `side` = `a` / `thru` / `b` | |
| `crossfader`, `master_level`, `booth_level`, `headphone_mix`, `headphone_level` | absolute | | |
| `master_cue` | press | | |
| `beat_fx.toggle`, `beat_fx.beat_down`, `beat_fx.beat_up`, `beat_fx.auto_beat`, `beat_fx.tap`, `beat_fx.select_next`, `beat_fx.select_prev` | press | `unit` (default 1) | |
| `beat_fx.release` | momentary | `unit` | |
| `beat_fx.level` | absolute | `unit` | |
| `beat_fx.select` | press | `effect` (name), `unit` | |
| `beat_fx.assign` | press | `channel` (0 = master), `unit` | |
| `color_fx.select` | press | `effect` | |
| `color_fx.next`, `color_fx.toggle` | press | | |
| `browse.rotate` | relative | `shifted` (bool) | `Rotate { delta, shifted }` |
| `browse.press` | press | `shifted` | |
| `browse.back`, `browse.view_toggle`, `browse.related` | press | | |
| `sampler.pad` | momentary | `slot` | |
| `sampler.stop` | press | `slot` | |
| `sampler.bank_next`, `sampler.bank_prev`, `sampler.cue` | press | | |
| `sampler.volume` | absolute | | |
| `recording.toggle` | press | | |
| `ignore` | any | | `ControlAction::Ignored` (known control without a function) |

In the application (`crates/od-ui/src/controller.rs`) every action above
reaches the engine: `deck.pad` plays the pad in the deck's current pad mode
(the same logic as the on-screen pads, `pads.rs`), `deck.tap_bpm` sets a
constant grid at the tapped tempo (three taps, keeps the first downbeat),
`deck.memory_cue_set` / `_delete` edit the track's memory cues in the
library, `beat_fx.release` engages Echo Out while held, `beat_fx.auto_beat`
returns a tapped unit to the deck tempo, `color_fx.next` / `toggle` step
through the Colour FX, and `sampler.pad` / `stop` address the bank shown in
the sampler panel (`sampler.bank_*`).

## Outputs

```toml
[[output]]
target = "deck.pad"
note = 0x00
repeat = 8
index_arg = "index"
channel = "pad"
args = { mode = "hot_cue" }
palette = "pads"
```

| Key | Meaning |
|---|---|
| `target` | Feedback element, see below. |
| `note` / `cc`, `channel`, `deck`, `repeat`, `step`, `index_arg`, `args` | As for inputs. |
| `mode` | `led` (default), `level` (default for meters and the jog indicator), `color` (default when `palette` is set). |
| `on`, `off` | Values for lit / dark (default `0x7F` / `0x00`). |
| `dim` | Value for `LedState::Dim` (default: `off`). |
| `blink` | Value that makes the hardware blink by itself; without it blinking is done in software. |
| `min`, `max` | Range for `level` outputs. |
| `palette` | Palette name for colour outputs. |
| `min_interval_ms` | Rate limit for this output (default: `options.meter_interval_ms` for levels, 0 otherwise). |

Several outputs may share a target (e.g. one LED on the button and one
status indicator). Output is diffed per MIDI address: a message is only sent
when its byte value differs from what was last sent.

### Targets

| Target | Arguments |
|---|---|
| `deck.play`, `deck.cue`, `deck.sync`, `deck.master`, `deck.keylock`, `deck.key_sync`, `deck.slip`, `deck.quantize`, `deck.reverse`, `deck.censor`, `deck.vinyl`, `deck.tempo_reset`, `deck.tempo_range`, `deck.loop_in`, `deck.loop_out`, `deck.reloop`, `deck.loop_active`, `deck.loaded`, `deck.shift`, `deck.selected` | |
| `deck.jog_indicator` | level |
| `deck.pad_mode` | `mode` |
| `deck.pad` | `mode`, `index` |
| `mixer.cue`, `mixer.vu` (level) | |
| `mixer.crossfader_assign` | `side` |
| `master.vu_left`, `master.vu_right` (level), `master_cue` | |
| `beat_fx.on`, `beat_fx.release` | `unit` (default 1) |
| `color_fx` | `effect` |
| `sampler.cue`, `recording` | |

## Device modules

`device.module` selects built-in Rust code for behaviour the TOML cannot
express. A module sees every raw message first (and may consume it), then
every action the mapping produced (and may rewrite, drop or add actions).

| Module | Behaviour |
|---|---|
| `pioneer-ddj` | Tempo centre detent (±0.4 % snaps to 0), jog search acceleration (×8), bend scaling with sub-tick remainder. |

New modules implement `od_controller::device_module::DeviceModule` and are
registered in `device_module::module_for`.

## Authoring workflow

1. Start the monitor: `cargo run -p od-controller --example midi_monitor`.
   Every incoming message is printed with the bindings it hit, suppressed
   actions (soft takeover) and `[unverified]` flags.
2. Unmapped controls show `(unmapped)`. Use MIDI learn to get a starting
   point: `--learn deck.play_pause` prints a ready `[[input]]` entry after
   five seconds of input (`LearnSession` in the API).
3. Check outputs with `--leds` (cycles Play/Cue/Sync LEDs and VU meters).
4. Remove `verify = true` once an entry is confirmed on hardware.
5. `cargo test -p od-controller` parses every bundled mapping and checks
   that no MIDI address is bound twice.
