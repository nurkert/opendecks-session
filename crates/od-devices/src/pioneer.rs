//! Pioneer DJ legacy export (`export.pdb` + ANLZ files) as a format writer.
//!
//! Track ids on the stick equal library track ids. Analysis files are only
//! rewritten for changed tracks. Before rewriting, files that a player
//! modified (cues set on a CDJ) and history playlists written by players are
//! detected and turned into [`Proposal`]s instead of being overwritten.

use crate::sync::{FormatWriter, SyncContext};
use od_core::{Cue, CueKind, DataSource, TrackId};
use od_format_pioneer::anlz::{AnlzFile, AnlzInput, AnlzSlots, build_anlz};
use od_format_pioneer::pdb::{ExportDatabase, ExportHistoryPlaylist, ExportPlaylist, ExportTrack};
use od_library::NodeKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const PDB_PATH: &str = "PIONEER/rekordbox/export.pdb";
const MYSETTING_PATH: &str = "PIONEER/MYSETTING.DAT";
/// Library meta key with the player preferences for sticks (JSON of
/// [`StickPlayerSettings`]), kept up to date by the app.
pub const MYSETTING_META: &str = "pioneer.mysetting";

/// Player preferences written to sticks (`MYSETTING.DAT`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickPlayerSettings {
    pub quantize: bool,
    pub auto_cue: bool,
    pub master_tempo: bool,
    /// "six", "ten", "sixteen" or "wide".
    pub tempo_range: String,
    pub jog_vinyl: bool,
    /// "en" or "de".
    pub language: String,
}

impl Default for StickPlayerSettings {
    fn default() -> Self {
        Self {
            quantize: true,
            auto_cue: true,
            master_tempo: false,
            tempo_range: "ten".into(),
            jog_vinyl: true,
            language: "en".into(),
        }
    }
}

/// The stored preferences (or the defaults) as `MYSETTING.DAT` values.
pub fn player_settings(meta: Option<&str>) -> od_format_pioneer::mysetting::PlayerSettings {
    use od_format_pioneer::mysetting::{Language, PlayerSettings, TempoRange};
    let s: StickPlayerSettings = meta
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    PlayerSettings {
        quantize: s.quantize,
        auto_cue: s.auto_cue,
        master_tempo: s.master_tempo,
        tempo_range: match s.tempo_range.as_str() {
            "six" => TempoRange::Six,
            "sixteen" => TempoRange::Sixteen,
            "wide" => TempoRange::Wide,
            _ => TempoRange::Ten,
        },
        jog_vinyl: s.jog_vinyl,
        language: if s.language.starts_with("de") {
            Language::German
        } else {
            Language::English
        },
        ..PlayerSettings::default()
    }
}

/// A change made on the stick outside OpenDecks, waiting for the user.
#[derive(Clone, Debug, PartialEq)]
pub enum Proposal {
    /// A player changed the cues of a track.
    Cues {
        track: TrackId,
        stick: Vec<Cue>,
        library: Vec<Cue>,
    },
    /// A player wrote a history playlist.
    History { name: String, tracks: Vec<TrackId> },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct WrittenTrack {
    dat: String,
    /// BLAKE3 of the .EXT we wrote (players store cues there).
    ext_hash: String,
    dat_hash: String,
}

/// What we wrote last time, stored in `/OPENDECKS/pioneer.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PioneerState {
    tracks: BTreeMap<TrackId, WrittenTrack>,
    /// History playlists already offered to the user (name + track list).
    seen_history: BTreeSet<String>,
    /// Tracks whose stick cues are waiting for a decision; not overwritten.
    held: BTreeSet<TrackId>,
}

impl PioneerState {
    fn file(root: &Path) -> PathBuf {
        od_format_native::dir(root).join("pioneer.json")
    }

    fn load(root: &Path) -> Self {
        std::fs::read(Self::file(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, root: &Path) -> std::io::Result<()> {
        od_format_native::write_atomic(
            &Self::file(root),
            &serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )
    }
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn stick_file(root: &Path, p: &str) -> PathBuf {
    root.join(p.trim_start_matches('/'))
}

fn ext_path(dat: &str) -> String {
    dat.strip_suffix(".DAT")
        .map(|s| format!("{s}.EXT"))
        .unwrap_or_else(|| dat.to_string())
}

fn ex2_path(dat: &str) -> String {
    dat.strip_suffix(".DAT")
        .map(|s| format!("{s}.2EX"))
        .unwrap_or_else(|| dat.to_string())
}

fn cue_key(c: &Cue) -> (u8, i64, Option<i64>) {
    let slot = match c.kind {
        CueKind::Hot(s) => s,
        CueKind::Memory => 255,
    };
    // Millisecond precision, as stored by players.
    (
        slot,
        (c.start * 1000.0).round() as i64,
        c.loop_end.map(|e| (e * 1000.0).round() as i64),
    )
}

fn same_cues(a: &[Cue], b: &[Cue]) -> bool {
    let mut x: Vec<_> = a.iter().map(cue_key).collect();
    let mut y: Vec<_> = b.iter().map(cue_key).collect();
    x.sort();
    y.sort();
    x == y
}

/// Writes the Pioneer legacy export.
#[derive(Default)]
pub struct PioneerWriter {
    proposals: Vec<Proposal>,
}

impl PioneerWriter {
    /// Proposals found during the last write.
    pub fn take_proposals(&mut self) -> Vec<Proposal> {
        std::mem::take(&mut self.proposals)
    }

    /// Releases a held track so that the next sync overwrites the stick's
    /// cues with the library's (used after the user decided).
    pub fn release(root: &Path, track: TrackId) -> std::io::Result<()> {
        let mut state = PioneerState::load(root);
        state.held.remove(&track);
        // The player's current file counts as seen: it is overwritten on the
        // next sync without being proposed again.
        if let Some(w) = state.tracks.get_mut(&track) {
            if let Ok(bytes) = std::fs::read(stick_file(root, &ext_path(&w.dat))) {
                w.ext_hash = hash(&bytes);
            }
        }
        state.save(root)
    }

    fn run(&mut self, ctx: &SyncContext) -> Result<(), String> {
        let mut state = PioneerState::load(ctx.root);
        let lib = ctx.library;

        // 1. Detect what players changed since our last write.
        let existing_pdb = std::fs::read(stick_file(ctx.root, PDB_PATH)).ok();
        let mut history: Vec<ExportHistoryPlaylist> = Vec::new();
        if let Some(bytes) = &existing_pdb {
            if let Ok(db) = ExportDatabase::parse(bytes) {
                for h in db.history {
                    let key = format!("{}:{:?}", h.name, h.track_ids);
                    if state.seen_history.insert(key) {
                        let tracks = h.track_ids.iter().map(|&id| TrackId(id as i64)).collect();
                        self.proposals.push(Proposal::History {
                            name: h.name.clone(),
                            tracks,
                        });
                    }
                    history.push(h);
                }
            }
        }
        for (id, written) in &state.tracks {
            if state.held.contains(id) || !ctx.tracks.iter().any(|t| t.record.id == *id) {
                continue;
            }
            let ext = stick_file(ctx.root, &ext_path(&written.dat));
            let Ok(bytes) = std::fs::read(&ext) else {
                continue;
            };
            if hash(&bytes) == written.ext_hash {
                continue;
            }
            let stick_cues = AnlzFile::parse(&bytes)
                .map(|f| f.cues())
                .unwrap_or_default();
            let library = lib.cues(*id).map_err(|e| e.to_string())?;
            if !same_cues(&stick_cues, &library) {
                self.proposals.push(Proposal::Cues {
                    track: *id,
                    stick: stick_cues,
                    library,
                });
            }
        }
        for p in &self.proposals {
            if let Proposal::Cues { track, .. } = p {
                state.held.insert(*track);
            }
        }

        // 2. Analysis files. Slots are assigned in id order so they are stable.
        let mut sorted: Vec<_> = ctx.tracks.iter().collect();
        sorted.sort_by_key(|t| t.record.id);
        let mut slots = AnlzSlots::new();
        let mut export_tracks = Vec::with_capacity(sorted.len());
        let mut new_state_tracks = BTreeMap::new();
        for t in sorted {
            let id = t.record.id;
            let dat = slots.assign(&t.stick_path);
            let previous = state.tracks.get(&id);
            let path_moved = previous.is_none_or(|p| p.dat != dat);
            let files_missing = !stick_file(ctx.root, &dat).exists();
            let must_write = ctx.full || ctx.changed.contains(&id) || path_moved || files_missing;
            if state.held.contains(&id) {
                // Keep the player's version until the user decides.
                if let Some(p) = previous {
                    new_state_tracks.insert(id, p.clone());
                }
            } else if must_write {
                let analysis = lib.analysis(id).map_err(|e| e.to_string())?;
                let cues = lib.cues(id).map_err(|e| e.to_string())?;
                let set = build_anlz(&AnlzInput {
                    device_path: &t.stick_path,
                    duration: t.record.duration,
                    beatgrid: &analysis.beatgrid,
                    cues: &cues,
                    waveform: &analysis.waveform,
                    phrases: &analysis.phrases,
                    phrase_mood: analysis.phrase_mood,
                    is_mp3_vbr_index: None,
                });
                for (path, bytes) in [
                    (dat.clone(), &set.dat),
                    (ext_path(&dat), &set.ext),
                    (ex2_path(&dat), &set.ex2),
                ] {
                    od_format_native::write_atomic(&stick_file(ctx.root, &path), bytes)
                        .map_err(|e| e.to_string())?;
                }
                new_state_tracks.insert(
                    id,
                    WrittenTrack {
                        dat: dat.clone(),
                        ext_hash: hash(&set.ext),
                        dat_hash: hash(&set.dat),
                    },
                );
            } else if let Some(p) = previous {
                new_state_tracks.insert(id, p.clone());
            }
            let r = &t.record;
            let Some(format) = r.format.filter(|f| f.pioneer_playable()) else {
                continue;
            };
            export_tracks.push(ExportTrack {
                id: id.0 as u32,
                device_path: t.stick_path.clone(),
                tags: r.tags.clone(),
                duration: r.duration,
                sample_rate: r.sample_rate,
                bit_rate_kbps: r.bit_rate.unwrap_or(0),
                bits_per_sample: r.bits_per_sample.unwrap_or(16) as u16,
                file_size: r.file_size,
                format,
                bpm: lib
                    .beatgrid(id)
                    .ok()
                    .flatten()
                    .and_then(|g| g.first_bpm())
                    .or(r.bpm),
                key: r.key,
                artwork_path: None,
                anlz_path: dat,
                date_added: r.date_added.get(..10).unwrap_or("2026-01-01").to_string(),
                play_count: r.play_count,
            });
        }
        // Delete analysis files of tracks that left the stick.
        for (id, w) in &state.tracks {
            if !new_state_tracks.contains_key(id) || new_state_tracks[id].dat != w.dat {
                for p in [w.dat.clone(), ext_path(&w.dat), ex2_path(&w.dat)] {
                    let _ = std::fs::remove_file(stick_file(ctx.root, &p));
                }
            }
        }

        // 3. export.pdb: always regenerated (small), history preserved.
        let exported: std::collections::HashSet<u32> = export_tracks.iter().map(|t| t.id).collect();
        let mut node_ids: HashMap<i64, u32> = HashMap::new();
        let mut playlists = Vec::new();
        for (n, entries) in ctx.tree {
            let id = node_ids.len() as u32 + 1;
            node_ids.insert(n.id.0, id);
            playlists.push(ExportPlaylist {
                id,
                parent_id: n
                    .parent
                    .and_then(|p| node_ids.get(&p.0).copied())
                    .unwrap_or(0),
                name: n.name.clone(),
                is_folder: n.kind == NodeKind::Folder,
                sort_order: n.sort_order.max(0) as u32,
                track_ids: entries
                    .iter()
                    .map(|e| e.track.0 as u32)
                    .filter(|id| exported.contains(id))
                    .collect(),
            });
        }
        let today = lib
            .now()
            .map_err(|e| e.to_string())?
            .get(..10)
            .unwrap_or_default()
            .to_string();
        let db = ExportDatabase {
            tracks: export_tracks,
            playlists,
            history,
            export_date: today,
        };
        od_format_native::write_atomic(&stick_file(ctx.root, PDB_PATH), &db.to_pdb_bytes())
            .map_err(|e| e.to_string())?;
        // 4. Player preferences (`MYSETTING.DAT`) from the library's settings.
        let my = player_settings(lib.meta(MYSETTING_META).ok().flatten().as_deref());
        od_format_native::write_atomic(
            &stick_file(ctx.root, MYSETTING_PATH),
            &od_format_pioneer::mysetting::to_bytes(&my),
        )
        .map_err(|e| e.to_string())?;

        state.tracks = new_state_tracks;
        state.save(ctx.root).map_err(|e| e.to_string())
    }
}

impl FormatWriter for PioneerWriter {
    fn name(&self) -> &'static str {
        "Pioneer"
    }

    fn write(&mut self, ctx: &SyncContext) -> Result<(), String> {
        self.run(ctx)
    }

    fn proposals(&mut self) -> Vec<Proposal> {
        self.take_proposals()
    }
}

/// Converts stick cues for storing in the library after the user accepted.
pub fn accepted_cues(stick: &[Cue]) -> Vec<Cue> {
    stick
        .iter()
        .cloned()
        .map(|c| Cue {
            source: DataSource::Manual,
            ..c
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{NativeWriter, SyncOptions, sync_device};
    use od_core::{AudioFormat, Beatgrid, TrackTags};
    use od_library::{DeviceFormats, Library, NewTrack};
    use std::sync::atomic::AtomicBool;

    fn setup() -> (
        tempfile::TempDir,
        Library,
        PathBuf,
        TrackId,
        od_core::DeviceId,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let stick = dir.path().join("stick");
        std::fs::create_dir_all(&stick).unwrap();
        let mut lib = Library::open(&dir.path().join("lib.sqlite")).unwrap();
        let file = dir.path().join("track.mp3");
        std::fs::write(&file, vec![7u8; 10_000]).unwrap();
        let id = lib
            .add_track(&NewTrack {
                content_hash: od_decode::content_hash(&file).unwrap(),
                file_size: 10_000,
                path: file,
                format: Some(AudioFormat::Mp3),
                duration: 240.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: Some(320),
                bits_per_sample: None,
                tags: TrackTags {
                    title: "Night Drive".into(),
                    artist: "Someone".into(),
                    ..Default::default()
                },
                artwork: None,
            })
            .unwrap();
        lib.set_beatgrid(id, Beatgrid::constant(124.0, 0.05, 1, 240.0))
            .unwrap();
        lib.set_cues(id, vec![Cue::hot(0, 16.0)]).unwrap();
        let pl = lib.create_node(None, NodeKind::Playlist, "Gig").unwrap();
        lib.add_to_node(pl, &[id], None).unwrap();
        let dev = lib
            .register_device("u", "S", DeviceFormats::default())
            .unwrap();
        lib.set_device_nodes(dev, &[pl]).unwrap();
        (dir, lib, stick, id, dev)
    }

    fn sync(lib: &mut Library, dev: od_core::DeviceId, stick: &Path) -> Vec<Proposal> {
        let mut writers: Vec<Box<dyn FormatWriter + Send>> = vec![
            Box::new(NativeWriter { stems: None }),
            Box::new(PioneerWriter::default()),
        ];
        sync_device(
            lib,
            dev,
            stick,
            &mut writers,
            &SyncOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap()
        .proposals
    }

    /// Sync writes `MYSETTING.DAT` from the library's player settings.
    #[test]
    fn writes_mysetting_from_the_library_settings() {
        let (_dir, mut lib, stick, _id, dev) = setup();
        sync(&mut lib, dev, &stick);
        let b = std::fs::read(stick.join(MYSETTING_PATH)).unwrap();
        assert_eq!(
            b,
            od_format_pioneer::mysetting::to_bytes(&player_settings(None)),
            "defaults without stored settings"
        );
        let s = StickPlayerSettings {
            quantize: false,
            tempo_range: "wide".into(),
            language: "de".into(),
            ..Default::default()
        };
        lib.set_meta(MYSETTING_META, &serde_json::to_string(&s).unwrap())
            .unwrap();
        sync(&mut lib, dev, &stick);
        let b = std::fs::read(stick.join(MYSETTING_PATH)).unwrap();
        // Data bytes are 1-based from offset 104: quantize 11, language 13, tempo range 37.
        assert_eq!((b[104 + 10], b[104 + 12], b[104 + 36]), (0x80, 0x83, 0x83));
        assert_eq!(
            u16::from_le_bytes([b[144], b[145]]),
            od_format_pioneer::mysetting::crc16_xmodem(&b[..144])
        );
    }

    #[test]
    fn writes_pdb_and_anlz_and_detects_player_cues() {
        let (_dir, mut lib, stick, id, dev) = setup();
        assert!(sync(&mut lib, dev, &stick).is_empty());
        let pdb = ExportDatabase::parse(&std::fs::read(stick.join(PDB_PATH)).unwrap()).unwrap();
        assert_eq!(pdb.tracks.len(), 1);
        assert_eq!(pdb.tracks[0].tags.title, "Night Drive");
        assert_eq!(pdb.playlists[0].name, "Gig");
        assert_eq!(pdb.playlists[0].track_ids, vec![id.0 as u32]);
        let dat = stick.join(pdb.tracks[0].anlz_path.trim_start_matches('/'));
        let anlz = AnlzFile::parse(&std::fs::read(&dat).unwrap()).unwrap();
        assert!((anlz.beatgrid().unwrap().beats[0].time - 0.05).abs() < 0.002);
        let ext = std::fs::read(dat.with_extension("EXT")).unwrap();
        assert_eq!(AnlzFile::parse(&ext).unwrap().cues().len(), 1);

        // Library cue edit: rewritten on the next sync.
        lib.set_cues(id, vec![Cue::hot(0, 20.0), Cue::hot(1, 40.0)])
            .unwrap();
        assert!(sync(&mut lib, dev, &stick).is_empty());
        let ext_bytes = std::fs::read(dat.with_extension("EXT")).unwrap();
        assert_eq!(AnlzFile::parse(&ext_bytes).unwrap().cues().len(), 2);

        // A "player" rewrites the EXT with a different cue set.
        let player = build_anlz(&AnlzInput {
            device_path: &pdb.tracks[0].device_path,
            duration: 240.0,
            beatgrid: &Beatgrid::constant(124.0, 0.05, 1, 240.0),
            cues: &[Cue::hot(2, 64.0)],
            waveform: &Default::default(),
            phrases: &[],
            phrase_mood: Default::default(),
            is_mp3_vbr_index: None,
        });
        std::fs::write(dat.with_extension("EXT"), &player.ext).unwrap();
        lib.set_cues(id, vec![Cue::hot(0, 21.0)]).unwrap();
        let proposals = sync(&mut lib, dev, &stick);
        assert!(
            matches!(&proposals[..], [Proposal::Cues { stick, .. }] if stick.len() == 1 && stick[0].kind == CueKind::Hot(2)),
            "{proposals:?}"
        );
        // The player's file was kept.
        assert_eq!(
            std::fs::read(dat.with_extension("EXT")).unwrap(),
            player.ext
        );

        // The user keeps the library version: release and resync.
        PioneerWriter::release(&stick, id).unwrap();
        lib.set_cues(id, vec![Cue::hot(0, 22.0)]).unwrap();
        assert!(sync(&mut lib, dev, &stick).is_empty());
        let cues = AnlzFile::parse(&std::fs::read(dat.with_extension("EXT")).unwrap())
            .unwrap()
            .cues();
        assert!((cues[0].start - 22.0).abs() < 0.002);
    }

    #[test]
    fn same_cues_ignores_order_and_sub_ms() {
        let a = vec![Cue::hot(0, 1.0), Cue::memory(2.0)];
        let b = vec![Cue::memory(2.0002), Cue::hot(0, 1.0)];
        assert!(same_cues(&a, &b));
        assert!(!same_cues(&a, &[Cue::hot(0, 1.0)]));
    }
}
