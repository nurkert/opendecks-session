//! Spawns `opendecks-mcp` against a temporary library and drives it over
//! stdio JSON-RPC like an MCP client, while a second connection plays the
//! running app: it sees every write, and can undo each one as one step.

use od_core::{AudioFormat, Beatgrid, ContentHash, DataSource, MusicalKey, Phrase, PhraseKind, TrackAnalysis, TrackId, TrackTags};
use od_library::{Library, NewTrack, NodeId, NodeKind};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: i64,
}

impl Client {
    fn spawn(library: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_opendecks-mcp"))
            .arg("--library")
            .arg(library)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn opendecks-mcp");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take().unwrap();
        let mut c = Self { child, stdin, lines, next_id: 1 };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "od-mcp-test", "version": "1"}
            }),
        );
        assert_eq!(init["protocolVersion"], "2025-06-18", "{init}");
        assert_eq!(init["serverInfo"]["name"], "opendecks-session");
        assert!(init["capabilities"]["tools"].is_object());
        assert!(init["capabilities"]["resources"].is_object());
        assert!(init["capabilities"]["prompts"].is_object());
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and returns the full response message.
    fn raw(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self.lines.recv_timeout(Duration::from_secs(30)).expect("response from server");
            let msg: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON on stdout: {line} ({e})"));
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let msg = self.raw(method, params);
        assert!(msg.get("error").is_none(), "{method} failed: {msg}");
        msg["result"].clone()
    }

    /// Calls a tool and returns (structured result, is_error, text).
    fn tool(&mut self, name: &str, args: Value) -> (Value, bool, String) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let text: Vec<String> =
            r["content"].as_array().unwrap().iter().map(|c| c["text"].as_str().unwrap_or("").to_string()).collect();
        (r["structuredContent"].clone(), r["isError"] == json!(true), text.join("\n"))
    }

    /// Calls a tool that must succeed.
    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (v, err, text) = self.tool(name, args);
        assert!(!err, "{name} failed: {text}");
        // Text content carries a summary and the same JSON for clients that
        // ignore structured content.
        assert!(text.contains('\n'), "{text}");
        v
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Seed {
    _dir: tempfile::TempDir,
    path: PathBuf,
    /// Deep Night 122 8A, Sunrise 124 9A, Warehouse 132 3B, Groove 123 8B, Afterglow 121 7A.
    t: Vec<i64>,
}

fn seed() -> Seed {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let mut lib = Library::open(&path).unwrap();
    let specs = [
        ("Deep Night", "Ann", "Deep House", 122.0, "8A", 0.40),
        ("Sunrise", "Ben", "Deep House", 124.0, "9A", 0.50),
        ("Warehouse", "Cid", "Techno", 132.0, "3B", 0.90),
        ("Groove", "Dee", "House", 123.0, "8B", 0.55),
        ("Afterglow", "Ann", "House", 121.0, "7A", 0.35),
    ];
    let mut t = Vec::new();
    for (title, artist, genre, bpm, key, energy) in specs {
        let id = lib
            .add_track(&NewTrack {
                path: PathBuf::from(format!("/music/{title}.flac")),
                content_hash: ContentHash(format!("h-{title}")),
                file_size: 1000,
                format: Some(AudioFormat::Flac),
                duration: 300.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: None,
                bits_per_sample: Some(16),
                tags: TrackTags { title: title.into(), artist: artist.into(), genre: genre.into(), ..Default::default() },
                artwork: None,
            })
            .unwrap();
        let analysis = TrackAnalysis {
            analysis_version: 1,
            beatgrid: Beatgrid::constant(bpm, 0.1, 1, 300.0),
            key: MusicalKey::parse(key),
            phrases: vec![
                Phrase { start_beat: 0, end_beat: 64, kind: PhraseKind::Intro },
                Phrase { start_beat: 64, end_beat: 320, kind: PhraseKind::Chorus },
                Phrase { start_beat: 320, end_beat: 384, kind: PhraseKind::Outro },
            ],
            loudness_lufs: Some(-8.5),
            peak: 0.98,
            energy: vec![energy as f32; 300],
            ..Default::default()
        };
        lib.store_analysis(id, &analysis, DataSource::Analysis).unwrap();
        t.push(id.0);
    }
    lib.record_play(TrackId(t[0]), "2026-11-01 Club", "opendecks").unwrap();
    lib.record_play(TrackId(t[1]), "2026-11-01 Club", "opendecks").unwrap();
    Seed { _dir: dir, path, t }
}

#[test]
fn handshake_lists_tools_resources_prompts() {
    let s = seed();
    let mut c = Client::spawn(&s.path);
    let tools = c.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "library_overview", "search_tracks", "get_tracks", "list_nodes", "get_node", "harmonic_neighbours", "set_flow",
        "history", "create_set", "create_playlist", "add_tracks", "remove_entries", "reorder", "set_transition_note",
        "set_track_tags", "create_smart_list", "rename_node", "delete_node", "lighting_overview", "lighting_patch",
        "lighting_settings", "lighting_group", "lighting_output", "lighting_cue", "lighting_track_cues", "lighting_live",
    ] {
        assert!(names.contains(&want), "missing tool {want}");
    }
    for t in tools["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().unwrap().len() > 20);
        let ro = t["annotations"]["readOnlyHint"] == json!(true);
        let name = t["name"].as_str().unwrap();
        let write = ["create", "add_", "remove", "reorder", "set_t", "rename", "delete"].iter().any(|p| name.starts_with(p))
            || (name.starts_with("lighting_") && name != "lighting_overview");
        assert_eq!(ro, !write, "{}", t["name"]);
    }
    let delete = tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == "delete_node").unwrap();
    assert_eq!(delete["annotations"]["destructiveHint"], true);
    assert!(delete["inputSchema"]["properties"]["confirm"].is_object());

    let templates = c.request("resources/templates/list", json!({}));
    let uris: Vec<&str> =
        templates["resourceTemplates"].as_array().unwrap().iter().map(|t| t["uriTemplate"].as_str().unwrap()).collect();
    assert_eq!(uris, ["opendecks://track/{id}", "opendecks://node/{id}"]);
    let r = c.request("resources/read", json!({"uri": format!("opendecks://track/{}", s.t[0])}));
    let track: Value = serde_json::from_str(r["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(track["title"], "Deep Night");
    let missing = c.raw("resources/read", json!({"uri": "opendecks://track/999"}));
    assert!(missing["error"].is_object());

    let prompts = c.request("prompts/list", json!({}));
    let names: Vec<&str> = prompts["prompts"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["build_set", "fix_transitions"]);
    let p = c.request("prompts/get", json!({"name": "build_set", "arguments": {"theme": "sunset", "duration_minutes": "90", "bpm_range": "118-124"}}));
    let text = p["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("sunset") && text.contains("90 minutes") && text.contains("118-124"));
    let p = c.request("prompts/get", json!({"name": "fix_transitions", "arguments": {"node_id": "7"}}));
    assert!(p["messages"][0]["content"]["text"].as_str().unwrap().contains("id 7"));
    assert!(c.raw("prompts/get", json!({"name": "fix_transitions", "arguments": {}}))["error"].is_object());
}

#[test]
fn read_tools() {
    let s = seed();
    let t = &s.t;
    let mut c = Client::spawn(&s.path);

    let o = c.ok("library_overview", json!({}));
    assert_eq!(o["tracks"], 5);
    assert_eq!(o["analysed"], 5);
    assert_eq!(o["unplayed"], 3);
    assert_eq!(o["keys"]["8A"], 1);
    assert_eq!(o["genres"][0], json!(["Deep House", 2]));
    assert_eq!(o["history_sessions"], 1);

    // Harmonic: compatible with 8A = 8A, 9A, 7A, 8B.
    let r = c.ok("search_tracks", json!({"compatible_with": "Am", "sort": "bpm"}));
    let ids: Vec<i64> = r["tracks"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    assert_eq!(ids, [t[4], t[0], t[3], t[1]]);
    let r = c.ok("search_tracks", json!({"bpm_min": 121.5, "bpm_max": 124, "genres": ["deep"]}));
    assert_eq!(r["total"], 2);
    let r = c.ok("search_tracks", json!({"query": "ware", "energy_min": 0.8}));
    assert_eq!(r["tracks"][0]["title"], "Warehouse");
    assert_eq!(r["tracks"][0]["energy"], 0.9);
    let r = c.ok("search_tracks", json!({"played": false, "added_within_days": 1}));
    assert_eq!(r["total"], 3);
    let r = c.ok("search_tracks", json!({"bpm_min": 61, "bpm_max": 61.8, "bpm_half_double": true}));
    assert_eq!(r["total"], 2, "{r}");
    let (_, err, text) = c.tool("search_tracks", json!({"key": "H#"}));
    assert!(err && text.contains("cannot read key"));

    let g = c.ok("get_tracks", json!({"ids": [t[0], 999]}));
    let d = &g["tracks"][0];
    assert_eq!(d["key"]["camelot"], "8A");
    assert_eq!(d["key"]["open_key"], "1m");
    assert_eq!(d["key"]["musical"], "Am");
    assert_eq!(d["bpm"], 122.0);
    assert_eq!(d["energy"], 0.4);
    assert_eq!(d["loudness_lufs"], -8.5);
    assert_eq!(d["phrases"].as_array().unwrap().len(), 3);
    assert_eq!(d["phrases"][0]["kind"], "intro");
    assert_eq!(d["phrases"][0]["bars"], 16);
    assert_eq!(d["play_count"], 1);
    assert!(d["date_added"].as_str().unwrap().starts_with("20"));
    assert_eq!(g["unknown_ids"], json!([999]));

    let n = c.ok("harmonic_neighbours", json!({"track_id": t[0]}));
    let ids: Vec<i64> = n["candidates"].as_array().unwrap().iter().map(|t| t["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&t[2]), "Warehouse clashes in tempo and key");
    // Afterglow: -1 on the wheel, 0.8 % slower, closest energy.
    assert_eq!(n["candidates"][0]["id"], t[4]);
    assert_eq!(n["candidates"][0]["key_relation"], "-1 (down a fifth)");
    assert_eq!(n["candidates"][0]["bpm_change_pct"], -0.8);
    let groove = n["candidates"].as_array().unwrap().iter().find(|c| c["id"] == t[3]).unwrap();
    assert_eq!(groove["key_relation"], "relative major/minor");
    let up = c.ok("harmonic_neighbours", json!({"track_id": t[0], "energy": "up"}));
    assert!(up["candidates"].as_array().unwrap().iter().all(|c| c["energy_delta"].as_f64().unwrap() > 0.0));

    let set = c.ok("create_set", json!({"name": "Check", "track_ids": [t[0], t[2], t[0]]}))["id"].as_i64().unwrap();
    let f = c.ok("set_flow", json!({"node_id": set}));
    assert_eq!(f["transitions"][0]["tempo_fit"], "clash");
    assert_eq!(f["transitions"][0]["key_fit"], "clash");
    let warnings = f["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect::<Vec<_>>().join("|");
    assert!(warnings.contains("already played at pos 0"), "{warnings}");
    assert!(warnings.contains("energy jump"), "{warnings}");
    assert_eq!(f["points"][2]["start"], "10:00");

    let h = c.ok("history", json!({}));
    assert_eq!(h["sessions"][0]["plays"], 2);
    let h = c.ok("history", json!({"session": "2026-11-01 Club"}));
    assert_eq!(h["plays"][0]["title"], "Deep Night");

    let l = c.ok("list_nodes", json!({}));
    assert_eq!(l["nodes"][0]["name"], "Check");
    let node = c.ok("get_node", json!({"id": set}));
    assert_eq!(node["entries"].as_array().unwrap().len(), 3);
    assert_eq!(node["entries"][1]["pos"], 1);
}

/// Every write tool: visible to the app's connection at once, one journal
/// entry labelled "MCP: …", a change-log row (stick sync), and undo/redo in
/// the app restores the previous / new state.
#[test]
fn every_write_is_one_undo_step() {
    let s = seed();
    let t = &s.t;
    let mut c = Client::spawn(&s.path);
    let mut app = Library::open(&s.path).unwrap();
    let step = |app: &mut Library, label: &str, seq_before: i64| {
        assert!(app.external_change().unwrap(), "{label}: app must notice the external write");
        assert!(app.change_seq().unwrap() > seq_before, "{label}: change log");
        let j = app.journal(1).unwrap();
        assert_eq!(j[0].label, label);
        assert!(!j[0].undone);
    };

    // create_set
    let seq = app.change_seq().unwrap();
    let set = NodeId(c.ok("create_set", json!({"name": "Friday", "track_ids": [t[0], t[1], t[2]], "planned_start": "23:30"}))["id"].as_i64().unwrap());
    step(&mut app, "MCP: Create set", seq);
    assert_eq!(app.node(set).unwrap().kind, NodeKind::Set);
    assert_eq!(app.node(set).unwrap().set_meta.unwrap().planned_start.as_deref(), Some("23:30"));
    assert_eq!(app.entries(set).unwrap().len(), 3);
    assert_eq!(app.undo().unwrap().as_deref(), Some("MCP: Create set"));
    assert!(app.node(set).is_err());
    app.redo().unwrap();
    assert_eq!(app.entries(set).unwrap().len(), 3);

    // create_playlist in a new folder: one step for both nodes.
    let seq = app.change_seq().unwrap();
    let r = c.ok("create_playlist", json!({"name": "Warmup", "new_folder": "Gigs", "track_ids": [t[4]]}));
    step(&mut app, "MCP: Create playlist", seq);
    let (pl, folder) = (NodeId(r["id"].as_i64().unwrap()), NodeId(r["folder_id"].as_i64().unwrap()));
    assert_eq!(app.node(pl).unwrap().parent, Some(folder));
    app.undo().unwrap();
    assert!(app.node(folder).is_err() && app.node(pl).is_err());
    app.redo().unwrap();
    assert_eq!(app.node_tracks(folder).unwrap().len(), 1);
    // create_playlist inside an existing folder.
    let inner = c.ok("create_playlist", json!({"name": "Late", "folder_id": folder.0}))["id"].as_i64().unwrap();
    assert_eq!(app.node(NodeId(inner)).unwrap().parent, Some(folder));
    let order = |app: &Library| app.entries(set).unwrap().iter().map(|e| e.track.0).collect::<Vec<_>>();

    // add_tracks
    let seq = app.change_seq().unwrap();
    c.ok("add_tracks", json!({"node_id": set.0, "track_ids": [t[3]], "position": 1}));
    step(&mut app, "MCP: Add tracks", seq);
    assert_eq!(order(&app), [t[0], t[3], t[1], t[2]]);
    app.undo().unwrap();
    assert_eq!(order(&app), [t[0], t[1], t[2]]);
    app.redo().unwrap();

    // reorder (permutation and from/to)
    let seq = app.change_seq().unwrap();
    c.ok("reorder", json!({"node_id": set.0, "order": [3, 2, 1, 0]}));
    step(&mut app, "MCP: Reorder", seq);
    assert_eq!(order(&app), [t[2], t[1], t[3], t[0]]);
    app.undo().unwrap();
    assert_eq!(order(&app), [t[0], t[3], t[1], t[2]]);
    app.redo().unwrap();
    c.ok("reorder", json!({"node_id": set.0, "from": [0], "to": 4}));
    assert_eq!(order(&app), [t[1], t[3], t[0], t[2]]);
    let (_, err, text) = c.tool("reorder", json!({"node_id": set.0, "order": [0, 0, 1, 2]}));
    assert!(err && text.contains("permutation"), "{text}");

    // remove_entries
    let seq = app.change_seq().unwrap();
    c.ok("remove_entries", json!({"node_id": set.0, "positions": [3]}));
    step(&mut app, "MCP: Remove entries", seq);
    assert_eq!(order(&app), [t[1], t[3], t[0]]);
    app.undo().unwrap();
    assert_eq!(order(&app).len(), 4);
    app.redo().unwrap();
    let (_, err, _) = c.tool("remove_entries", json!({"node_id": set.0, "positions": [9]}));
    assert!(err);

    // set_transition_note with backups
    let seq = app.change_seq().unwrap();
    c.ok("set_transition_note", json!({"node_id": set.0, "position": 0, "note": "long blend over the outro", "backup_track_ids": [t[4]]}));
    step(&mut app, "MCP: Transition note", seq);
    assert_eq!(app.entries(set).unwrap()[0].note, "long blend over the outro");
    assert_eq!(app.entries(set).unwrap()[0].backups, [TrackId(t[4])]);
    app.undo().unwrap();
    assert_eq!(app.entries(set).unwrap()[0].note, "");
    app.redo().unwrap();

    // set_track_tags: two tracks, tags and My Tag, one step.
    let seq = app.change_seq().unwrap();
    c.ok("set_track_tags", json!({"track_ids": [t[0], t[1]], "rating": 5, "color": "red", "comment": "opener", "add_my_tags": [{"category": "Mood", "value": "Dark"}]}));
    step(&mut app, "MCP: Edit tags", seq);
    let a = app.track(TrackId(t[0])).unwrap();
    assert_eq!((a.tags.rating, a.tags.comment.as_str()), (5, "opener"));
    assert_eq!(a.tags.color.unwrap().to_hex(), "#f82c30");
    assert_eq!(app.my_tags(TrackId(t[1])).unwrap()[0].value, "Dark");
    let r = c.ok("search_tracks", json!({"my_tags": ["Mood: dark"], "color": "red", "rating_min": 5}));
    assert_eq!(r["total"], 2);
    app.undo().unwrap();
    assert_eq!(app.track(TrackId(t[1])).unwrap().tags.rating, 0);
    assert!(app.my_tags(TrackId(t[0])).unwrap().is_empty());
    app.redo().unwrap();
    c.ok("set_track_tags", json!({"track_ids": [t[0]], "remove_my_tags": [{"category": "Mood", "value": "Dark"}], "color": "none"}));
    assert!(app.my_tags(TrackId(t[0])).unwrap().is_empty());
    assert_eq!(app.track(TrackId(t[0])).unwrap().tags.color, None);

    // create_smart_list
    let seq = app.change_seq().unwrap();
    let r = c.ok("create_smart_list", json!({"name": "Deep 120s", "rule": "genre deep, bpm 120-125", "folder_id": folder.0}));
    step(&mut app, "MCP: Create smart list", seq);
    assert_eq!(r["matches"], 2);
    let smart = NodeId(r["id"].as_i64().unwrap());
    assert_eq!(app.node_tracks(smart).unwrap().len(), 2);
    app.undo().unwrap();
    assert!(app.node(smart).is_err());
    app.redo().unwrap();
    let (_, err, text) = c.tool("create_smart_list", json!({"name": "x", "rule": "tempo 120"}));
    assert!(err && text.contains("Unknown field"), "{text}");

    // rename_node
    let seq = app.change_seq().unwrap();
    c.ok("rename_node", json!({"node_id": set.0, "name": "Saturday"}));
    step(&mut app, "MCP: Rename", seq);
    assert_eq!(app.node(set).unwrap().name, "Saturday");
    app.undo().unwrap();
    assert_eq!(app.node(set).unwrap().name, "Friday");
    app.redo().unwrap();

    // delete_node needs confirm; the confirmed delete is undoable with notes.
    let before = app.journal(1).unwrap()[0].id;
    let (_, err, text) = c.tool("delete_node", json!({"node_id": folder.0}));
    assert!(err && text.contains("confirm=true") && text.contains("3 nested lists"), "{text}");
    assert_eq!(app.journal(1).unwrap()[0].id, before, "nothing written without confirm");
    let seq = app.change_seq().unwrap();
    c.ok("delete_node", json!({"node_id": set.0, "confirm": true}));
    step(&mut app, "MCP: Delete", seq);
    assert!(app.node(set).is_err());
    assert_eq!(app.undo().unwrap().as_deref(), Some("MCP: Delete"));
    assert_eq!(app.entries(set).unwrap()[0].note, "long blend over the outro");

    // Writes into the wrong kind of node are refused without a journal entry.
    let before = app.journal(1).unwrap()[0].id;
    let (_, err, text) = c.tool("add_tracks", json!({"node_id": folder.0, "track_ids": [t[0]]}));
    assert!(err && text.contains("only playlists and sets"), "{text}");
    let (_, err, _) = c.tool("add_tracks", json!({"node_id": set.0, "track_ids": [999]}));
    assert!(err);
    assert_eq!(app.journal(1).unwrap()[0].id, before);
}

/// The app and the server write the same file at the same time.
#[test]
fn concurrent_app_writes() {
    let s = seed();
    let mut c = Client::spawn(&s.path);
    let mut app = Library::open(&s.path).unwrap();
    let set = c.ok("create_set", json!({"name": "Shared"}))["id"].as_i64().unwrap();
    for i in 0..20 {
        app.add_to_node(NodeId(set), &[TrackId(s.t[i % 5])], None).unwrap();
        c.ok("add_tracks", json!({"node_id": set, "track_ids": [s.t[(i + 1) % 5]]}));
    }
    assert_eq!(app.entries(NodeId(set)).unwrap().len(), 40);
    let node = c.ok("get_node", json!({"id": set}));
    assert_eq!(node["entries"].as_array().unwrap().len(), 40);
}

#[test]
fn http_refuses_non_loopback() {
    let s = seed();
    let out = Command::new(env!("CARGO_BIN_EXE_opendecks-mcp"))
        .arg("--library")
        .arg(&s.path)
        .args(["--http", "0.0.0.0:0"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("loopback"));
}

#[test]
fn http_transport_initializes() {
    use std::io::Read;
    use std::net::TcpStream;
    let s = seed();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_opendecks-mcp"))
        .arg("--library")
        .arg(&s.path)
        .args(["--http", &format!("127.0.0.1:{port}")])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stream = None;
    for _ in 0..100 {
        if let Ok(st) = TcpStream::connect(("127.0.0.1", port)) {
            stream = Some(st);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut stream = stream.expect("server listening");
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}})
    .to_string();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut resp = Vec::new();
    let mut buf = [0u8; 4096];
    // Read until the initialize result arrived (the SSE stream may stay open).
    while !String::from_utf8_lossy(&resp).contains("protocolVersion") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => resp.extend_from_slice(&buf[..n]),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.contains("\"protocolVersion\":\"2025-06-18\""), "{text}");
}

/// The light show from the chat: patch, looks, outputs, cue stack, track
/// cues and live actions; the rig file next to the library follows (the app
/// reloads it) and live actions wait in the rig's inbox for the app.
#[test]
fn lighting_rig_and_live_control() {
    let s = seed();
    let mut c = Client::spawn(&s.path);
    let rig = s.path.with_file_name("lighting.json");
    let o = c.ok("lighting_overview", json!({}));
    assert_eq!(o["enabled"], false);
    assert!(o["accepts"]["fixture_types"].as_array().unwrap().iter().any(|t| t == "moving-head"));

    c.ok("lighting_patch", json!({"action": "add", "type": "rgb-par", "name": "Front L"}));
    c.ok("lighting_patch", json!({"action": "add", "type": "moving-head", "group": "Movers"}));
    c.ok("lighting_patch", json!({"action": "update", "fixture": "Front L", "group": "Front"}));
    c.ok("lighting_settings", json!({"enabled": true, "mode": "pro", "master": 0.8, "easy_program": "chase", "easy_color": "#FF8800"}));
    c.ok("lighting_group", json!({"group": "Front", "program": "pulse", "color": "#0044FF", "intensity": 0.7}));
    c.ok("lighting_group", json!({"group": "Movers", "effect": {"attr": "pan", "wave": "sine", "beats": 8, "depth": 0.5, "spread": 0.25}}));
    c.ok("lighting_output", json!({"universe": 1, "kind": "sacn", "target": "3"}));
    c.ok("lighting_cue", json!({"action": "add", "name": "Intro", "groups": {"Front": {"color": "#00FF00"}}}));
    c.ok("lighting_cue", json!({"action": "add", "name": "Drop", "fade_beats": 2, "groups": {"front": {"program": "strobe"}}}));
    c.ok("lighting_track_cues", json!({"track_id": s.t[0], "cues": [{"bar": 1, "cue": "Intro"}, {"bar": 17, "cue": 1}]}));
    c.ok("lighting_cue", json!({"action": "move", "index": 1, "to": 0}));

    let cfg = od_lighting::RigConfig::load(&rig).unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.settings.mode, od_lighting::Mode::Pro);
    assert_eq!(cfg.patch.fixtures.len(), 2);
    assert_eq!(cfg.patch.fixtures[0].group, "Front");
    assert_eq!(cfg.settings.groups["Front"].program, Some(od_lighting::Program::Pulse));
    assert_eq!(cfg.settings.groups["Movers"].effect.unwrap().beats, 8.0);
    assert_eq!(cfg.outputs, vec![(0, od_lighting::OutputKind::Sacn { universe: 3 })]);
    assert_eq!(cfg.settings.cues[0].name, "Drop");
    assert_eq!(cfg.settings.cues[0].groups["Front"].program, Some(od_lighting::Program::Strobe));
    // Track cues follow the move: bar 1 → Intro (now 1), bar 17 → Drop (now 0).
    let tc = &cfg.settings.track_cues[&s.t[0]];
    assert_eq!(tc.iter().map(|t| (t.beat, t.cue)).collect::<Vec<_>>(), vec![(0, 1), (64, 0)]);

    let o = c.ok("lighting_overview", json!({}));
    assert_eq!(o["track_cues"][0]["cues"][1]["cue_name"], "Drop");
    assert_eq!(o["groups"]["Front"]["color"], "#0044FF");

    // Bad input is reported, not stored.
    let (_, err, text) = c.tool("lighting_group", json!({"group": "Front", "color": "blue"}));
    assert!(err && text.contains("#RRGGBB"), "{text}");
    let (_, err, _) = c.tool("lighting_track_cues", json!({"track_id": s.t[0], "cues": [{"bar": 1, "cue": "Nope"}]}));
    assert!(err);

    c.ok("lighting_live", json!({"action": "blackout"}));
    c.ok("lighting_live", json!({"action": "cue", "index": 1}));
    assert_eq!(
        od_lighting::rig::drain(&rig),
        vec![od_lighting::LiveCommand::Blackout { on: true }, od_lighting::LiveCommand::Cue { index: 1 }]
    );

    c.ok("lighting_patch", json!({"action": "remove", "fixture": 1}));
    assert_eq!(od_lighting::RigConfig::load(&rig).unwrap().patch.fixtures.len(), 1);
}
