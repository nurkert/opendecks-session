# Keyboard

Every user action of OpenDecks Session is a command in one registry
(`crates/od-ui/src/keymap.rs`) with an id, a default binding per platform
and optional user bindings (SPEC §12.2). The registry currently holds
**219 actions**; every one has a default binding, and no two defaults
collide where they apply (checked by tests, together with this table).

## Changing bindings

Preferences (⌘, on macOS, Ctrl+, on Linux, or the gear in the top bar) >
Keyboard lists all actions with their bindings, filtered by the search
field (action names, ids or keys). Click a binding (or pick it with ↑ ↓ and
press Return) and press the new keys; Esc cancels, Backspace removes the
binding. A key that another action already uses where this one applies is
reported by name; REPLACE (or Return) moves it, CANCEL keeps both as they
were. DEFAULT resets one action, RESET ALL every action. IMPORT / EXPORT
read and write the keymap as TOML:

```toml
[bindings]
"deck1.play" = ["Space"]
"browser.search" = ["Mod+F", "F3"]
"deck2.play" = []          # unbound
```

`Mod` is ⌘ on macOS and Ctrl on Linux, `Meta` is Ctrl on macOS and Super
on Linux; then `Alt` and `Shift`. Keys are characters (`A`, `1`, `,`) or
names (`Up`, `Down`, `Left`, `Right`, `Space`, `Return`, `Escape`,
`Backspace`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `F1`…`F24`).
Only the user's changes are stored (library meta `keymap`); an imported file
is applied on top of the defaults and rejected if it would bind one key
twice.

Shift on digits and punctuation names the key, not the symbol, on US and
German layouts (⇧1 arrives as "!" but matches `Shift+1`). On macOS the
Option key composes characters (⌥Z types "Ω"), so defaults use ⌥ only with
⌘ or with arrow keys.

## Where actions apply

"Prepare" actions work in Prepare, "Perform" actions in Perform, "both"
everywhere; the same key can mean different things in the two modes (R is
"previous cue" in the Prepare player and "previous memory cue" of deck 1 in
Perform). Text fields keep the keys they type.

## Defaults

- **Decks 1 and 2** use the left / right keyboard halves, a layout DJs
  commonly use with DJ software: play Z / N, cue A / H, sync Q / Y, hot
  cues A–E on 1–5 / 6–0 and F–H on ⇧1–⇧3 / ⇧6–⇧8, loop in / out / exit
  D F G / K L ;, beat loops ⌘1–⌘5 / ⌘6–⌘0, beat jump ⌘R ⌘T / ⌘O ⌘P,
  memory cues R T / O P, pitch bend W E / U I (held), tempo ⇧W ⇧E / ⇧U ⇧I,
  key lock ⇧5 / ⇧0, load to deck ⇧← / ⇧→, record ⌘Return, new playlist ⌘N,
  sampler slots X C V B M , . /. Search is ⌘F (⌃F as a second binding).
- **Decks 3 and 4** use F1–F11 (deck 3) and ⇧F1–⇧F11 (deck 4) for cue,
  play, sync, hot cues A–D, 4-beat loop, exit and beat jump. On Mac laptops
  the function keys need fn unless "Use F1, F2, etc. keys as standard
  function keys" is on.
- **Prepare**: Space previews,
  1–8 set or jump to hot cues A–H, ⇧⌘1–8 clear them, M adds a memory cue,
  R / T step through cues, ← → move the cursor, ⌘← ⌘→ shift the grid by
  10 ms, ⌘↑ ⌘↓ double / halve it, D sets the downbeat, L locks the grid.
- On Linux, mode and section switches use Alt+1 / Alt+2 and Alt+L / D / R,
  because Ctrl+Alt combinations are taken by most desktops.

## All defaults

| Action | Id | Where | macOS | Linux |
|---|---|---|---|---|
| **General** | | | | |
| Open preferences | `prefs.open` | both | `⌘,` | `Ctrl+,` |
| Check for updates | `app.check-updates` | both | `⌥⌘U` | `Alt+U` |
| Undo | `library.undo` | both | `⌘Z` | `Ctrl+Z` |
| Redo | `library.redo` | both | `⇧⌘Z` | `Ctrl+Shift+Z` `Ctrl+Y` |
| Switch to Prepare | `mode.prepare` | both | `⌥⌘1` | `Alt+1` |
| Switch to Perform | `mode.perform` | both | `⌥⌘2` | `Alt+2` |
| Switch to Lighting | `mode.lighting` | both | `⌥⌘3` | `Alt+3` |
| Show the library | `section.library` | both | `⌥⌘L` | `Alt+L` |
| Show USB sticks | `section.devices` | both | `⌥⌘D` | `Alt+D` |
| Show recordings | `section.recordings` | both | `⌥⌘R` | `Alt+R` |
| Start / stop recording | `record.toggle` | both | `⌘Return` | `Ctrl+Return` |
| Sync USB sticks now | `devices.sync` | both | `⌘S` | `Ctrl+S` |
| Eject the selected stick | `devices.eject` | both | `⌘E` | `Ctrl+E` |
| Quantize on / off | `perform.quantize` | Perform | `⌘K` | `Ctrl+K` |
| **Browser** | | | | |
| Search | `browser.search` | both | `⌘F` `⌃F` | `Ctrl+F` |
| Previous track | `browser.up` | both | `↑` | `↑` |
| Next track | `browser.down` | both | `↓` | `↓` |
| Previous list | `browser.list.prev` | both | `⌥↑` | `Alt+↑` |
| Next list | `browser.list.next` | both | `⌥↓` | `Alt+↓` |
| Select all tracks | `browser.select.all` | both | `⌘A` | `Ctrl+A` |
| Load the selected track (free deck) | `browser.load` | both | `Return` | `Return` |
| Load the selected track to deck 1 | `browser.load.deck1` | both | `⇧←` | `Shift+←` |
| Load the selected track to deck 2 | `browser.load.deck2` | both | `⇧→` | `Shift+→` |
| Load the selected track to deck 3 | `browser.load.deck3` | both | `⌥⇧←` | `Alt+Shift+←` |
| Load the selected track to deck 4 | `browser.load.deck4` | both | `⌥⇧→` | `Alt+Shift+→` |
| Remove the selected tracks from the list | `browser.remove` | both | `⌦` `⌫` | `Delete` `Backspace` |
| New playlist | `browser.new.playlist` | both | `⌘N` | `Ctrl+N` |
| New set | `browser.new.set` | both | `⇧⌘N` | `Ctrl+Shift+N` |
| New folder | `browser.new.folder` | both | `⇧⌘F` | `Ctrl+Shift+F` |
| New smart list | `browser.new.smart` | both | `⇧⌘M` | `Ctrl+Shift+M` |
| Delete the open list | `browser.delete.list` | both | `⌘⌫` | `Ctrl+Backspace` |
| Add a music folder | `browser.add.music` | both | `⇧⌘O` | `Ctrl+Shift+O` |
| Import from rekordbox | `browser.import.rekordbox` | both | `⇧⌘I` | `Ctrl+Shift+I` |
| Find moved files | `browser.find.moved` | both | `⇧⌘L` | `Ctrl+Shift+L` |
| Add the selected tracks to the tag list | `browser.taglist.add` | both | `⌥⌘T` | `Alt+T` |
| Show / hide the filter bar | `browser.filter` | both | `⌘J` | `Ctrl+J` |
| **Prepare player** | | | | |
| Preview play / pause | `player.preview` | Prepare | `Space` | `Space` |
| Back to the cue point | `player.cue` | Prepare | `C` | `C` |
| Hot cue A | `player.hotcue.a` | Prepare | `1` | `1` |
| Hot cue B | `player.hotcue.b` | Prepare | `2` | `2` |
| Hot cue C | `player.hotcue.c` | Prepare | `3` | `3` |
| Hot cue D | `player.hotcue.d` | Prepare | `4` | `4` |
| Hot cue E | `player.hotcue.e` | Prepare | `5` | `5` |
| Hot cue F | `player.hotcue.f` | Prepare | `6` | `6` |
| Hot cue G | `player.hotcue.g` | Prepare | `7` | `7` |
| Hot cue H | `player.hotcue.h` | Prepare | `8` | `8` |
| Clear hot cue A | `player.hotcue.clear.a` | Prepare | `⇧⌘1` | `Ctrl+Shift+1` |
| Clear hot cue B | `player.hotcue.clear.b` | Prepare | `⇧⌘2` | `Ctrl+Shift+2` |
| Clear hot cue C | `player.hotcue.clear.c` | Prepare | `⇧⌘3` | `Ctrl+Shift+3` |
| Clear hot cue D | `player.hotcue.clear.d` | Prepare | `⇧⌘4` | `Ctrl+Shift+4` |
| Clear hot cue E | `player.hotcue.clear.e` | Prepare | `⇧⌘5` | `Ctrl+Shift+5` |
| Clear hot cue F | `player.hotcue.clear.f` | Prepare | `⇧⌘6` | `Ctrl+Shift+6` |
| Clear hot cue G | `player.hotcue.clear.g` | Prepare | `⇧⌘7` | `Ctrl+Shift+7` |
| Clear hot cue H | `player.hotcue.clear.h` | Prepare | `⇧⌘8` | `Ctrl+Shift+8` |
| Add a memory cue | `player.memory.add` | Prepare | `M` | `M` |
| Delete the memory cue at the cursor | `player.memory.delete` | Prepare | `⇧M` | `Shift+M` |
| Previous cue | `player.cue.prev` | Prepare | `R` | `R` |
| Next cue | `player.cue.next` | Prepare | `T` | `T` |
| Cursor back (fine) | `player.nudge.back` | Prepare | `←` | `←` |
| Cursor forward (fine) | `player.nudge.fwd` | Prepare | `→` | `→` |
| Cursor back 2 s | `player.nudge.back.coarse` | Prepare | `⌥←` | `Alt+←` |
| Cursor forward 2 s | `player.nudge.fwd.coarse` | Prepare | `⌥→` | `Alt+→` |
| Beatgrid 10 ms earlier | `player.grid.earlier` | Prepare | `⌘←` | `Ctrl+←` |
| Beatgrid 10 ms later | `player.grid.later` | Prepare | `⌘→` | `Ctrl+→` |
| Beatgrid double tempo | `player.grid.double` | Prepare | `⌘↑` | `Ctrl+↑` |
| Beatgrid half tempo | `player.grid.half` | Prepare | `⌘↓` | `Ctrl+↓` |
| Set the downbeat at the cursor | `player.grid.downbeat` | Prepare | `D` | `D` |
| Lock / unlock the beatgrid | `player.grid.lock` | Prepare | `L` | `L` |
| Analyse the track | `player.analyse` | Prepare | `⇧⌘A` | `Ctrl+Shift+A` |
| **Deck 1** | | | | |
| Play / pause | `deck1.play` | Perform | `Z` | `Z` |
| Cue | `deck1.cue` | Perform | `A` | `A` |
| Jump to the track start | `deck1.start` | Perform | `⇧A` | `Shift+A` |
| Beat sync | `deck1.sync` | Perform | `Q` | `Q` |
| Sync master | `deck1.master` | Perform | `⇧Q` | `Shift+Q` |
| Master tempo (key lock) | `deck1.keylock` | Perform | `⇧5` | `Shift+5` |
| Pitch bend + (hold) | `deck1.bend.up` | Perform | `E` | `E` |
| Pitch bend − (hold) | `deck1.bend.down` | Perform | `W` | `W` |
| Tempo up | `deck1.tempo.up` | Perform | `⇧E` | `Shift+E` |
| Tempo down | `deck1.tempo.down` | Perform | `⇧W` | `Shift+W` |
| Beat jump forward | `deck1.jump.fwd` | Perform | `⌘T` | `Ctrl+T` |
| Beat jump back | `deck1.jump.back` | Perform | `⌘R` | `Ctrl+R` |
| Hot cue A | `deck1.hotcue.a` | Perform | `1` | `1` |
| Hot cue B | `deck1.hotcue.b` | Perform | `2` | `2` |
| Hot cue C | `deck1.hotcue.c` | Perform | `3` | `3` |
| Hot cue D | `deck1.hotcue.d` | Perform | `4` | `4` |
| Hot cue E | `deck1.hotcue.e` | Perform | `5` | `5` |
| Hot cue F | `deck1.hotcue.f` | Perform | `⇧1` | `Shift+1` |
| Hot cue G | `deck1.hotcue.g` | Perform | `⇧2` | `Shift+2` |
| Hot cue H | `deck1.hotcue.h` | Perform | `⇧3` | `Shift+3` |
| Clear hot cue A | `deck1.hotcue.clear.a` | Perform | `⇧⌘1` | `Ctrl+Shift+1` |
| Clear hot cue B | `deck1.hotcue.clear.b` | Perform | `⇧⌘2` | `Ctrl+Shift+2` |
| Clear hot cue C | `deck1.hotcue.clear.c` | Perform | `⇧⌘3` | `Ctrl+Shift+3` |
| Clear hot cue D | `deck1.hotcue.clear.d` | Perform | `⇧⌘4` | `Ctrl+Shift+4` |
| Clear hot cue E | `deck1.hotcue.clear.e` | Perform | `⇧⌘5` | `Ctrl+Shift+5` |
| Clear hot cue F | `deck1.hotcue.clear.f` | Perform | `⌥⇧⌘1` | `Ctrl+Alt+Shift+1` |
| Clear hot cue G | `deck1.hotcue.clear.g` | Perform | `⌥⇧⌘2` | `Ctrl+Alt+Shift+2` |
| Clear hot cue H | `deck1.hotcue.clear.h` | Perform | `⌥⇧⌘3` | `Ctrl+Alt+Shift+3` |
| Loop in | `deck1.loop.in` | Perform | `D` | `D` |
| Loop out | `deck1.loop.out` | Perform | `F` | `F` |
| Exit / reloop | `deck1.loop.exit` | Perform | `G` | `G` |
| Auto beat loop | `deck1.loop.auto` | Perform | `⇧F` | `Shift+F` |
| Halve the loop | `deck1.loop.half` | Perform | `⇧D` | `Shift+D` |
| Double the loop | `deck1.loop.double` | Perform | `⇧G` | `Shift+G` |
| 1/2-beat loop | `deck1.loop.1_2` | Perform | `⌘1` | `Ctrl+1` |
| 1-beat loop | `deck1.loop.1` | Perform | `⌘2` | `Ctrl+2` |
| 2-beat loop | `deck1.loop.2` | Perform | `⌘3` | `Ctrl+3` |
| 4-beat loop | `deck1.loop.4` | Perform | `⌘4` | `Ctrl+4` |
| 8-beat loop | `deck1.loop.8` | Perform | `⌘5` | `Ctrl+5` |
| Next memory cue | `deck1.memory.next` | Perform | `T` | `T` |
| Previous memory cue | `deck1.memory.prev` | Perform | `R` | `R` |
| Next pad mode | `deck1.padmode.next` | Perform | `⇧⌘D` | `Ctrl+Shift+D` |
| Previous pad mode | `deck1.padmode.prev` | Perform | `⇧⌘S` | `Ctrl+Shift+S` |
| Next hot cue bank | `deck1.hotcue.bank` | Perform | `⇧⌘G` | `Ctrl+Shift+G` |
| Key sync on / off | `deck1.keysync` | Perform | `⇧⌘X` | `Ctrl+Shift+X` |
| Key shift up a semitone | `deck1.keyshift.up` | Perform | `⇧⌘V` | `Ctrl+Shift+V` |
| Key shift down a semitone | `deck1.keyshift.down` | Perform | `⇧⌘C` | `Ctrl+Shift+C` |
| **Deck 2** | | | | |
| Play / pause | `deck2.play` | Perform | `N` | `N` |
| Cue | `deck2.cue` | Perform | `H` | `H` |
| Jump to the track start | `deck2.start` | Perform | `⇧H` | `Shift+H` |
| Beat sync | `deck2.sync` | Perform | `Y` | `Y` |
| Sync master | `deck2.master` | Perform | `⇧Y` | `Shift+Y` |
| Master tempo (key lock) | `deck2.keylock` | Perform | `⇧0` | `Shift+0` |
| Pitch bend + (hold) | `deck2.bend.up` | Perform | `I` | `I` |
| Pitch bend − (hold) | `deck2.bend.down` | Perform | `U` | `U` |
| Tempo up | `deck2.tempo.up` | Perform | `⇧I` | `Shift+I` |
| Tempo down | `deck2.tempo.down` | Perform | `⇧U` | `Shift+U` |
| Beat jump forward | `deck2.jump.fwd` | Perform | `⌘P` | `Ctrl+P` |
| Beat jump back | `deck2.jump.back` | Perform | `⌘O` | `Ctrl+O` |
| Hot cue A | `deck2.hotcue.a` | Perform | `6` | `6` |
| Hot cue B | `deck2.hotcue.b` | Perform | `7` | `7` |
| Hot cue C | `deck2.hotcue.c` | Perform | `8` | `8` |
| Hot cue D | `deck2.hotcue.d` | Perform | `9` | `9` |
| Hot cue E | `deck2.hotcue.e` | Perform | `0` | `0` |
| Hot cue F | `deck2.hotcue.f` | Perform | `⇧6` | `Shift+6` |
| Hot cue G | `deck2.hotcue.g` | Perform | `⇧7` | `Shift+7` |
| Hot cue H | `deck2.hotcue.h` | Perform | `⇧8` | `Shift+8` |
| Clear hot cue A | `deck2.hotcue.clear.a` | Perform | `⇧⌘6` | `Ctrl+Shift+6` |
| Clear hot cue B | `deck2.hotcue.clear.b` | Perform | `⇧⌘7` | `Ctrl+Shift+7` |
| Clear hot cue C | `deck2.hotcue.clear.c` | Perform | `⇧⌘8` | `Ctrl+Shift+8` |
| Clear hot cue D | `deck2.hotcue.clear.d` | Perform | `⇧⌘9` | `Ctrl+Shift+9` |
| Clear hot cue E | `deck2.hotcue.clear.e` | Perform | `⇧⌘0` | `Ctrl+Shift+0` |
| Clear hot cue F | `deck2.hotcue.clear.f` | Perform | `⌥⇧⌘6` | `Ctrl+Alt+Shift+6` |
| Clear hot cue G | `deck2.hotcue.clear.g` | Perform | `⌥⇧⌘7` | `Ctrl+Alt+Shift+7` |
| Clear hot cue H | `deck2.hotcue.clear.h` | Perform | `⌥⇧⌘8` | `Ctrl+Alt+Shift+8` |
| Loop in | `deck2.loop.in` | Perform | `K` | `K` |
| Loop out | `deck2.loop.out` | Perform | `L` | `L` |
| Exit / reloop | `deck2.loop.exit` | Perform | `;` | `;` |
| Auto beat loop | `deck2.loop.auto` | Perform | `⇧L` | `Shift+L` |
| Halve the loop | `deck2.loop.half` | Perform | `⇧K` | `Shift+K` |
| Double the loop | `deck2.loop.double` | Perform | `⇧;` | `Shift+;` |
| 1/2-beat loop | `deck2.loop.1_2` | Perform | `⌘6` | `Ctrl+6` |
| 1-beat loop | `deck2.loop.1` | Perform | `⌘7` | `Ctrl+7` |
| 2-beat loop | `deck2.loop.2` | Perform | `⌘8` | `Ctrl+8` |
| 4-beat loop | `deck2.loop.4` | Perform | `⌘9` | `Ctrl+9` |
| 8-beat loop | `deck2.loop.8` | Perform | `⌘0` | `Ctrl+0` |
| Next memory cue | `deck2.memory.next` | Perform | `P` | `P` |
| Previous memory cue | `deck2.memory.prev` | Perform | `O` | `O` |
| Next pad mode | `deck2.padmode.next` | Perform | `⇧⌘;` | `Ctrl+Shift+;` |
| Previous pad mode | `deck2.padmode.prev` | Perform | `⇧⌘K` | `Ctrl+Shift+K` |
| Next hot cue bank | `deck2.hotcue.bank` | Perform | `⇧⌘J` | `Ctrl+Shift+J` |
| Key sync on / off | `deck2.keysync` | Perform | `⇧⌘B` | `Ctrl+Shift+B` |
| Key shift up a semitone | `deck2.keyshift.up` | Perform | `⇧⌘.` | `Ctrl+Shift+.` |
| Key shift down a semitone | `deck2.keyshift.down` | Perform | `⇧⌘,` | `Ctrl+Shift+,` |
| **Deck 3** | | | | |
| Cue | `deck3.cue` | Perform | `F1` | `F1` |
| Play / pause | `deck3.play` | Perform | `F2` | `F2` |
| Beat sync | `deck3.sync` | Perform | `F3` | `F3` |
| Hot cue A | `deck3.hotcue.a` | Perform | `F4` | `F4` |
| Hot cue B | `deck3.hotcue.b` | Perform | `F5` | `F5` |
| Hot cue C | `deck3.hotcue.c` | Perform | `F6` | `F6` |
| Hot cue D | `deck3.hotcue.d` | Perform | `F7` | `F7` |
| 4-beat loop | `deck3.loop.4` | Perform | `F8` | `F8` |
| Exit / reloop | `deck3.loop.exit` | Perform | `F9` | `F9` |
| Beat jump back | `deck3.jump.back` | Perform | `F10` | `F10` |
| Beat jump forward | `deck3.jump.fwd` | Perform | `F11` | `F11` |
| **Deck 4** | | | | |
| Cue | `deck4.cue` | Perform | `⇧F1` | `Shift+F1` |
| Play / pause | `deck4.play` | Perform | `⇧F2` | `Shift+F2` |
| Beat sync | `deck4.sync` | Perform | `⇧F3` | `Shift+F3` |
| Hot cue A | `deck4.hotcue.a` | Perform | `⇧F4` | `Shift+F4` |
| Hot cue B | `deck4.hotcue.b` | Perform | `⇧F5` | `Shift+F5` |
| Hot cue C | `deck4.hotcue.c` | Perform | `⇧F6` | `Shift+F6` |
| Hot cue D | `deck4.hotcue.d` | Perform | `⇧F7` | `Shift+F7` |
| 4-beat loop | `deck4.loop.4` | Perform | `⇧F8` | `Shift+F8` |
| Exit / reloop | `deck4.loop.exit` | Perform | `⇧F9` | `Shift+F9` |
| Beat jump back | `deck4.jump.back` | Perform | `⇧F10` | `Shift+F10` |
| Beat jump forward | `deck4.jump.fwd` | Perform | `⇧F11` | `Shift+F11` |
| **Effects** | | | | |
| Show / hide the Beat FX panel | `perform.fx.panel` | Perform | `⌥⌘F` | `Alt+F` |
| CDJ practice mode on / off | `perform.cdj` | Perform | `⌥⌘J` | `Alt+J` |
| Show / hide the lighting strip | `perform.light.panel` | Perform | `⌥⌘K` | `Alt+K` |
| FX 1 on / off | `fx1.on` | Perform | `⇧Z` | `Shift+Z` |
| FX 1: next effect | `fx1.next` | Perform | `⇧X` | `Shift+X` |
| FX 1: shorter beat | `fx1.beat.down` | Perform | `⇧C` | `Shift+C` |
| FX 1: longer beat | `fx1.beat.up` | Perform | `⇧V` | `Shift+V` |
| FX 1: tap the tempo | `fx1.tap` | Perform | `⇧B` | `Shift+B` |
| FX 1: vinyl brake (hold) | `fx1.release.brake` | Perform | `⇧⌘W` | `Ctrl+Shift+W` |
| FX 1: backspin (hold) | `fx1.release.backspin` | Perform | `⇧⌘E` | `Ctrl+Shift+E` |
| FX 1: echo out (hold) | `fx1.release.echo` | Perform | `⇧⌘R` | `Ctrl+Shift+R` |
| FX 2 on / off | `fx2.on` | Perform | `⇧N` | `Shift+N` |
| FX 2: next effect | `fx2.next` | Perform | `⇧M` | `Shift+M` |
| FX 2: shorter beat | `fx2.beat.down` | Perform | `⇧,` | `Shift+,` |
| FX 2: longer beat | `fx2.beat.up` | Perform | `⇧.` | `Shift+.` |
| FX 2: tap the tempo | `fx2.tap` | Perform | `⇧/` | `Shift+/` |
| FX 2: vinyl brake (hold) | `fx2.release.brake` | Perform | `⇧⌘Y` | `Ctrl+Shift+Y` |
| FX 2: backspin (hold) | `fx2.release.backspin` | Perform | `⇧⌘U` | `Ctrl+Shift+U` |
| FX 2: echo out (hold) | `fx2.release.echo` | Perform | `⇧⌘H` | `Ctrl+Shift+H` |
| Colour FX: next | `colorfx.next` | Perform | `⇧J` | `Shift+J` |
| Colour FX: previous | `colorfx.prev` | Perform | `⇧S` | `Shift+S` |
| **Sampler** | | | | |
| Show / hide the sampler panel | `perform.sampler.panel` | Perform | `⌥⌘S` | `Alt+S` |
| Sampler slot 1 | `sampler.slot1` | Perform | `X` | `X` |
| Sampler slot 2 | `sampler.slot2` | Perform | `C` | `C` |
| Sampler slot 3 | `sampler.slot3` | Perform | `V` | `V` |
| Sampler slot 4 | `sampler.slot4` | Perform | `B` | `B` |
| Sampler slot 5 | `sampler.slot5` | Perform | `M` | `M` |
| Sampler slot 6 | `sampler.slot6` | Perform | `,` | `,` |
| Sampler slot 7 | `sampler.slot7` | Perform | `.` | `.` |
| Sampler slot 8 | `sampler.slot8` | Perform | `/` | `/` |
| Previous sampler bank | `sampler.bank.prev` | Perform | `⇧R` | `Shift+R` |
| Next sampler bank | `sampler.bank.next` | Perform | `⇧T` | `Shift+T` |
| Start / stop the sequencer | `sampler.seq.play` | Perform | `⇧O` | `Shift+O` |
| Stop all samples | `sampler.stop.all` | Perform | `⇧P` | `Shift+P` |
