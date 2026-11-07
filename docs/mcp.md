# Preparing sets with an LLM: the OpenDecks MCP server

`opendecks-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server (protocol revision 2025-06-18) that gives an MCP client, for example
Claude Desktop or Claude Code, access to your OpenDecks Session library
(SPEC §5.6). You talk about the set you want to play; the model searches the
library, checks keys, tempo and energy, builds the set and writes transition
notes, and you see every change in the app as it happens. Every change can be
undone in the app.

## Where the binary is

| Install | Path |
| --- | --- |
| macOS app | `/Applications/OpenDecks Session.app/Contents/MacOS/opendecks-mcp` |
| Debian/Ubuntu package | `/usr/bin/opendecks-mcp` |
| Linux tar.gz | `opendecks-mcp` next to `opendecks` in the unpacked folder |
| Raspberry Pi bundle | `/opt/opendecks/opendecks-mcp` |
| From source | `cargo build --release -p od-mcp` → `target/release/opendecks-mcp` |

It uses the same library as the app: `--library <file>` if given, else the
`library` entry of `config.toml` in the OpenDecks data directory, else the
default (`~/Library/Application Support/OpenDecks Session/library.sqlite` on
macOS, `$XDG_DATA_HOME/opendecks-session/library.sqlite` on Linux).

## Setup

### Claude Desktop

Edit `claude_desktop_config.json` (Settings → Developer → Edit Config; on
macOS it is `~/Library/Application Support/Claude/claude_desktop_config.json`)
and restart Claude Desktop:

```json
{
  "mcpServers": {
    "opendecks": {
      "command": "/Applications/OpenDecks Session.app/Contents/MacOS/opendecks-mcp",
      "args": []
    }
  }
}
```

For another library add `"args": ["--library", "/path/to/library.sqlite"]`.

### Claude Code

```sh
claude mcp add opendecks -- "/Applications/OpenDecks Session.app/Contents/MacOS/opendecks-mcp"
# another library:
claude mcp add opendecks -- /path/to/opendecks-mcp --library /path/to/library.sqlite
```

### Other clients, HTTP

Any MCP client that starts a server over stdio works the same way. Clients
that connect over HTTP can use the local Streamable HTTP transport:

```sh
opendecks-mcp --http 127.0.0.1:8765        # serves http://127.0.0.1:8765/mcp
claude mcp add --transport http opendecks http://127.0.0.1:8765/mcp
```

`--http` only accepts loopback addresses (`127.0.0.1`, `[::1]`), only answers
requests whose `Host` is a loopback name, and rejects browser requests from
non-local origins (protection against DNS rebinding).

Options: `--library <file>`, `--http <addr>`, `--version`. Logging goes to
stderr; set `OPENDECKS_MCP_LOG=info` (or `debug`) for more.

## What the model can do

Ids come from the tools themselves (`library_overview`, `search_tracks`,
`list_nodes`). Positions in playlists and sets are 0-based. Keys are shown in
Camelot notation (8A = A minor); every key input also accepts Open Key (1m)
and musical names (Am, F#).

### Reading

| Tool | What it returns |
| --- | --- |
| `library_overview` | Track counts, analysed / missing / unplayed, total hours, top genres, BPM buckets, key and rating distribution, colours, energy buckets, the My Tag catalog, the whole playlist/set tree with ids, history sessions. |
| `search_tracks` | Text search (title, artist, album, genre, label, remixer, comment) plus filters: BPM range (optionally half/double time), exact key, `compatible_with` a key (same, Camelot ±1, relative major/minor), genres, rating range, colour, My Tag values, energy range, duration, played / never played, added within N days, inside a list, excluded ids; sorting and paging. |
| `get_tracks` | Every field of up to 50 tracks: tags, rating, colour, comment, My Tag, BPM and grid (source, locked), key in all notations with its source and compatible keys, duration, loudness (LUFS) and peak, mean energy and a 16-step energy curve, phrases (kind, start, end, bars), hot and memory cues and loops, play count, last played, date added, file details. |
| `list_nodes` | The tree of folders, playlists, smart lists and sets with ids, track counts and smart rules. |
| `get_node` | One list. Sets and playlists: ordered entries with start times, transition notes and backup tracks; smart lists: rule and current matches; folders: children. |
| `harmonic_neighbours` | Tracks that mix after (or before) a track: within ±N % tempo (half/double time counted), up to a Camelot distance, optional energy direction (up, down, same), excluding a list being built. Scored 0–100 with tempo change and key relation (same key, ±1, relative, +2 energy boost). |
| `set_flow` | A set or playlist in order: start time, BPM, key, energy per position; each transition with tempo change, key relation, fit ratings and energy change; warnings for tempo or key clashes, energy jumps or drops, repeated tracks, the same artist twice in a row, unanalysed or missing files. |
| `history` | History sessions and plays (own performances and imported player histories). |

### Writing

Every write tool is **one undo step** in the app, labelled `MCP: …` (for
example `MCP: Create set`), and is recorded in the change log, so USB sticks
that carry the changed lists are marked out of date and sync as usual.

| Tool | Change |
| --- | --- |
| `create_set` | New set, optionally in a folder, filled in order, with planned start time and notes. |
| `create_playlist` | New playlist in an existing folder or a new folder, optionally filled. |
| `add_tracks` | Insert tracks at a position (default: end). |
| `remove_entries` | Remove entries by position and/or track id. Tracks stay in the collection. |
| `reorder` | New order as a permutation, or move positions to an index. Notes and backups move with their entries. |
| `set_transition_note` | Note for the mix from a position into the next one, optionally backup tracks. |
| `set_track_tags` | Rating, colour, comment, My Tag values (replace, add, remove) for up to 500 tracks at once. |
| `create_smart_list` | Live smart list from rule text, e.g. `bpm 122-128, key 8A~, genre house, -genre techno, rating >= 3, added < 30d`. |
| `rename_node` | Rename a folder, playlist, set or smart list. |
| `delete_node` | Delete a list or a folder with its contents. Requires `confirm: true`; without it the tool only reports what would be deleted. |

### Light show

The DMX rig is part of what the model can see and change. Rig edits are
written to `lighting.json` next to the library; a running app picks them up
within a tenth of a second and redraws the LIGHTING view. Live actions are
dropped into the rig's inbox (`lighting-inbox/` next to it), which the app
empties several times a second; they need the app to be running. Rig edits
are not part of the library's undo history.

| Tool | What it does |
| --- | --- |
| `lighting_overview` | On / off, mode, master, blackout, outputs per universe, the patched fixtures (address, group, role), the look of each group, the cue stack, the lighting cues stored with tracks (by bar), and the fixture types, programs and effects the other tools accept. |
| `lighting_patch` | Add a built-in fixture type at the next free DMX address (or a given universe and address), change one (name, group, address, mirrored pan) or remove one. |
| `lighting_settings` | Lighting on or off; mode `auto` (follows the music), `easy` (one look) or `pro` (looks per group and the cue stack); master; blackout; the easy-mode program and colour. |
| `lighting_group` | The pro-mode look of a group: program, colour (`#RRGGBB`), intensity, pan / tilt, speed and a beat-synced effect (dimmer, pan or tilt; sine, saw, square or random; length in beats, depth, spread). `null` hands a field back to the automatic show. |
| `lighting_output` | A universe to Art-Net (IP), sACN (universe) or an Enttec USB-DMX Pro (serial device), or no output. |
| `lighting_cue` | Add, change, remove or move cues of the stack (name, fade in beats, looks per group). Track cues follow moved and removed cues. |
| `lighting_track_cues` | The lighting cues stored with a track: which cue fires at which bar. |
| `lighting_live` | Now: cue go / back, fire a cue, flash, strobe, fog, blackout, tap tempo. |

Example: *"Patch two RGB pars as Front and a moving head, send universe 1 to
sACN 1, and make a cue stack Intro (blue, slow), Build (chase), Drop
(strobe) — then put Drop on bar 65 of Deep Night."*

### Resources and prompts

- Resources: `opendecks://track/{id}` (same as `get_tracks`) and
  `opendecks://node/{id}` (same as `get_node`); every playlist, set and smart
  list is also listed as a resource.
- Prompts: `build_set` (theme or feeling, duration, energy arc, BPM range)
  walks the model through overview → search → harmonic chaining → your OK →
  `create_set` → `set_flow` → transition notes. `fix_transitions` (set id)
  checks a set with `set_flow` and proposes fixes before applying them.

Example requests: *"Build a 90-minute sunset set, deep and warm, 118–124 BPM,
slow build to a peak at the end"*, *"Which tracks fit after Deep Night with
a bit more energy?"*, *"Tag everything I played at the club last Friday
with Situation: Peak"*.

## Running next to the app

The app and the server open the same SQLite file at the same time. The
library uses WAL mode with a busy timeout, and every write takes the write
lock up front in one short transaction, so neither side sees half an edit or
fails on a lock. The app polls for outside changes and redraws the tree,
track list and undo state within a fraction of a second, so MCP edits
appear live. Undo in the app (Edit → Undo) undoes MCP edits like your own,
in order.

## Safety

- **Local only.** Over stdio the server talks only to the client that started
  it; `--http` listens on loopback only. The server itself makes no network
  connections.
- **What it can change:** the playlist/set tree (lists, entries, notes,
  smart rules) and track tags (rating, colour, comment, My Tag). It cannot
  touch audio files, cues, beatgrids, keys, analysis, devices, recordings or
  settings, and it cannot remove tracks from the collection.
- **Everything is undoable** in the app, one step per tool call, including
  deletions. Deleting requires `confirm: true`; well-behaved clients ask
  you first, and most clients ask before every write tool unless you allow
  it.
- **Privacy:** what the tools return (titles, artists, comments, file paths,
  play history) goes to the model your client uses. Use a client and model
  you are comfortable sharing your library metadata with.
- **Untrusted text:** tags and comments of downloaded files are returned as
  data. A comment that reads like an instruction is still only a comment;
  review proposed changes before you approve them.
- The app's automatic database backups (SPEC §5.3) cover MCP edits as well.

## Troubleshooting

- *"library schema version N is newer than this application supports"*: the
  server is older than the app that last opened the library; use the
  `opendecks-mcp` shipped with the same release.
- Nothing appears in the client: check that the path in the configuration
  points to the binary and run it by hand (`opendecks-mcp --version`).
  Over stdio it waits silently for a client; stop it with Ctrl+C.
- Wrong library: pass `--library` explicitly.
