//! Incremental sync of a registered device.
//!
//! The engine places audio files under `/Contents`, then lets every enabled
//! [`FormatWriter`] write its metadata for the tracks that changed. A sync
//! journal on the stick detects interrupted syncs, which force a full sync
//! the next time.

use crate::placement::{Placement, PlacementMap, contents_path};
use od_core::{DeviceId, TrackId};
use od_library::{Entry, Library, Node, NodeKind, TrackRecord};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Library(#[from] od_library::LibraryError),
    #[error(transparent)]
    Native(#[from] od_format_native::NativeError),
    #[error("not enough space on the stick: {needed} bytes needed, {free} bytes free")]
    NoSpace { needed: u64, free: u64 },
    #[error("sync cancelled")]
    Cancelled,
    #[error("{format} export failed: {message}")]
    Format {
        format: &'static str,
        message: String,
    },
}

pub type Result<T> = std::result::Result<T, SyncError>;

/// A track with its stick path.
#[derive(Clone, Debug)]
pub struct PlacedTrack {
    pub record: TrackRecord,
    /// Stick-relative path starting with `/Contents/`.
    pub stick_path: String,
}

/// Everything a format writer needs.
pub struct SyncContext<'a> {
    pub root: &'a Path,
    pub library: &'a Library,
    pub device: DeviceId,
    /// All tracks on the device after this sync.
    pub tracks: &'a [PlacedTrack],
    /// The device's node tree (smart playlists already converted to
    /// playlists). Parents precede children.
    pub tree: &'a [(Node, Vec<Entry>)],
    /// Tracks whose data must be (re)written.
    pub changed: &'a BTreeSet<TrackId>,
    /// Tracks removed from the device.
    pub removed: &'a [TrackId],
    pub playlists_changed: bool,
    pub full: bool,
}

/// One export format written to the stick.
pub trait FormatWriter {
    fn name(&self) -> &'static str;
    fn write(&mut self, ctx: &SyncContext) -> std::result::Result<(), String>;
    /// Changes found on the stick that need a user decision.
    fn proposals(&mut self) -> Vec<crate::pioneer::Proposal> {
        Vec::new()
    }
}

/// Progress reported during a sync.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Planning,
    Copying {
        done_bytes: u64,
        total_bytes: u64,
        current: String,
    },
    Writing {
        format: &'static str,
    },
    Finishing,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    pub tracks: usize,
    pub copied: usize,
    pub bytes_copied: u64,
    pub removed: usize,
    pub updated: usize,
    pub skipped_missing: Vec<TrackId>,
    pub full: bool,
    pub seconds: f64,
    pub proposals: Vec<crate::pioneer::Proposal>,
}

#[derive(Clone, Debug, Default)]
pub struct SyncOptions {
    /// Rewrite everything (Rebuild stick).
    pub force_full: bool,
}

/// The device's node tree for export: assigned nodes become roots, smart
/// playlists become static playlists, folders keep their children.
pub fn device_tree(
    lib: &Library,
    device: DeviceId,
    on_device: &HashSet<TrackId>,
) -> od_library::Result<Vec<(Node, Vec<Entry>)>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let assigned = lib.device_nodes(device)?;
    // Skip assigned nodes that are inside another assigned folder.
    let mut covered = HashSet::new();
    for n in &assigned {
        for d in lib.descendants(*n)?.into_iter().skip(1) {
            covered.insert(d);
        }
    }
    for (i, root) in assigned.iter().enumerate() {
        if covered.contains(root) {
            continue;
        }
        let mut stack = vec![(*root, true)];
        while let Some((id, is_root)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let mut node = lib.node(id)?;
            if is_root {
                node.parent = None;
                node.sort_order = i as i64;
            }
            let entries = match node.kind {
                NodeKind::Folder => vec![],
                NodeKind::Smart => {
                    node.kind = NodeKind::Playlist;
                    node.smart_rule = None;
                    lib.node_tracks(id)?
                        .into_iter()
                        .map(|t| Entry::new(t.id))
                        .collect()
                }
                _ => lib.entries(id)?,
            };
            let entries = entries
                .into_iter()
                .filter(|e| on_device.contains(&e.track))
                .collect();
            out.push((node, entries));
            let children = lib.children(Some(id))?;
            for c in children.into_iter().rev() {
                stack.push((c.id, false));
            }
        }
    }
    Ok(out)
}

fn copy_file(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    mut on_bytes: impl FnMut(u64),
) -> Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dst.with_file_name(format!(
        ".{}.part",
        dst.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    ));
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::File::create(&tmp)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(output);
            let _ = std::fs::remove_file(&tmp);
            return Err(SyncError::Cancelled);
        }
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        on_bytes(n as u64);
    }
    output.sync_all()?;
    drop(output);
    std::fs::rename(&tmp, dst)?;
    Ok(())
}

fn stick_file(root: &Path, stick_path: &str) -> PathBuf {
    root.join(stick_path.trim_start_matches('/'))
}

/// Removes empty directories from `dir` up to (not including) `stop`.
fn prune_empty_dirs(mut dir: PathBuf, stop: &Path) {
    while dir.starts_with(stop) && dir != stop {
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
        if !dir.pop() {
            break;
        }
    }
}

/// Syncs `device` mounted at `root`.
pub fn sync_device(
    lib: &mut Library,
    device: DeviceId,
    root: &Path,
    writers: &mut [Box<dyn FormatWriter + Send>],
    opts: &SyncOptions,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<SyncReport> {
    let started = Instant::now();
    progress(Progress::Planning);
    let pending = lib.pending_changes(device)?;
    let interrupted = od_format_native::interrupted_sync(root)?.is_some();
    let mut placement = PlacementMap::load(root);
    let full = opts.force_full
        || interrupted
        || lib.device(device)?.synced_seq == 0
        || placement.tracks.is_empty();
    let mut report = SyncReport {
        full,
        ..Default::default()
    };

    let wanted: Vec<TrackRecord> = lib.device_tracks(device)?;
    od_format_native::begin_sync(
        root,
        &od_format_native::SyncJournal {
            started_at: lib.now()?,
            full,
        },
    )?;

    // Decide placements and the files to copy.
    let wanted_ids: HashSet<TrackId> = wanted.iter().map(|t| t.id).collect();
    let removed: Vec<TrackId> = placement
        .tracks
        .keys()
        .filter(|id| !wanted_ids.contains(id))
        .copied()
        .collect();
    let mut taken: HashSet<String> = placement
        .tracks
        .iter()
        .filter(|(id, _)| wanted_ids.contains(id))
        .map(|(_, p)| p.path.to_lowercase())
        .collect();
    let mut placed = Vec::with_capacity(wanted.len());
    let mut to_copy: Vec<(usize, PathBuf)> = Vec::new();
    let mut changed: BTreeSet<TrackId> = if full {
        wanted_ids.iter().copied().collect()
    } else {
        pending.tracks.clone()
    };
    for t in wanted {
        if t.missing || !t.path.exists() {
            report.skipped_missing.push(t.id);
            if let Some(p) = placement.tracks.get(&t.id) {
                // Keep whatever is already on the stick.
                placed.push(PlacedTrack {
                    stick_path: p.path.clone(),
                    record: t,
                });
            }
            continue;
        }
        let existing = placement.tracks.get(&t.id);
        let stick_path = match existing {
            Some(p) if p.hash == t.content_hash => p.path.clone(),
            Some(p) => p.path.clone(), // same slot, new content
            None => {
                let p = contents_path(&t, &taken);
                taken.insert(p.to_lowercase());
                p
            }
        };
        let dst = stick_file(root, &stick_path);
        let up_to_date = existing
            .is_some_and(|p| p.hash == t.content_hash && p.size == t.file_size)
            && std::fs::metadata(&dst).is_ok_and(|m| m.len() == t.file_size);
        if !up_to_date {
            to_copy.push((placed.len(), t.path.clone()));
            changed.insert(t.id);
        }
        placed.push(PlacedTrack {
            record: t,
            stick_path,
        });
    }

    // Space check (files being replaced free their old size).
    let needed: u64 = to_copy
        .iter()
        .map(|(i, _)| {
            let t = &placed[*i];
            let old = std::fs::metadata(stick_file(root, &t.stick_path))
                .map(|m| m.len())
                .unwrap_or(0);
            t.record.file_size.saturating_sub(old)
        })
        .sum();
    if let Some((_, free)) = crate::volumes::space(root) {
        // Keep a small reserve for metadata.
        if needed + (8 << 20) > free {
            return Err(SyncError::NoSpace { needed, free });
        }
    }

    // Remove tracks that left the device.
    for id in &removed {
        if let Some(p) = placement.tracks.remove(id) {
            let f = stick_file(root, &p.path);
            if p.path.starts_with("/Contents/") && std::fs::remove_file(&f).is_ok() {
                report.removed += 1;
                if let Some(parent) = f.parent() {
                    prune_empty_dirs(parent.to_path_buf(), &root.join("Contents"));
                }
            }
        }
    }

    // Copy audio.
    let total_bytes: u64 = to_copy
        .iter()
        .map(|(i, _)| placed[*i].record.file_size)
        .sum();
    let mut done_bytes = 0;
    for (i, src) in &to_copy {
        let t = &placed[*i];
        let dst = stick_file(root, &t.stick_path);
        let name = t.record.tags.title.clone();
        copy_file(src, &dst, cancel, |n| {
            done_bytes += n;
            progress(Progress::Copying {
                done_bytes,
                total_bytes,
                current: name.clone(),
            });
        })?;
        report.copied += 1;
        report.bytes_copied += t.record.file_size;
        placement.tracks.insert(
            t.record.id,
            Placement {
                path: t.stick_path.clone(),
                hash: t.record.content_hash.clone(),
                size: t.record.file_size,
            },
        );
    }
    for t in &placed {
        placement
            .tracks
            .entry(t.record.id)
            .or_insert_with(|| Placement {
                path: t.stick_path.clone(),
                hash: t.record.content_hash.clone(),
                size: t.record.file_size,
            });
    }
    placement.save(root)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(SyncError::Cancelled);
    }

    // Metadata.
    let on_device: HashSet<TrackId> = placed.iter().map(|t| t.record.id).collect();
    changed.retain(|id| on_device.contains(id));
    let tree = device_tree(lib, device, &on_device)?;
    let ctx = SyncContext {
        root,
        library: lib,
        device,
        tracks: &placed,
        tree: &tree,
        changed: &changed,
        removed: &removed,
        playlists_changed: full || pending.playlists || !removed.is_empty(),
        full,
    };
    for w in writers.iter_mut() {
        progress(Progress::Writing { format: w.name() });
        w.write(&ctx).map_err(|message| SyncError::Format {
            format: w.name(),
            message,
        })?;
        report.proposals.extend(w.proposals());
    }
    report.updated = changed.len();
    report.tracks = placed.len();

    progress(Progress::Finishing);
    od_format_native::end_sync(root)?;
    lib.mark_device_synced(device, pending.head)?;
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

/// Writes the OpenDecks native format. Separated stems from the desktop's
/// stem cache travel along (`/OPENDECKS/stems/<hash>.odstem`), so a
/// Raspberry Pi without separation still plays the parts.
pub struct NativeWriter {
    /// The desktop's stem cache directory (`None` = don't carry stems).
    pub stems: Option<PathBuf>,
}

impl Default for NativeWriter {
    fn default() -> Self {
        Self {
            stems: Some(od_library::data_dir().join("stems")),
        }
    }
}

/// Stem file name for a content hash (as the stem cache names them).
fn stem_file(hash: &od_core::ContentHash) -> String {
    let name: String = hash
        .0
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    format!("{name}.odstem")
}

impl NativeWriter {
    /// Copies cached stems of the device's tracks and removes stale ones.
    fn carry_stems(&self, ctx: &SyncContext) -> std::io::Result<()> {
        let Some(src) = &self.stems else {
            return Ok(());
        };
        let dst = ctx.root.join("OPENDECKS").join("stems");
        let wanted: HashSet<String> = ctx
            .tracks
            .iter()
            .map(|t| stem_file(&t.record.content_hash))
            .collect();
        for name in &wanted {
            let from = src.join(name);
            let to = dst.join(name);
            if from.is_file() && !to.is_file() {
                std::fs::create_dir_all(&dst)?;
                let tmp = dst.join(format!("{name}.part"));
                std::fs::copy(&from, &tmp)?;
                std::fs::rename(&tmp, &to)?;
            }
        }
        if let Ok(entries) = std::fs::read_dir(&dst) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !wanted.contains(&name) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Ok(())
    }
}

impl FormatWriter for NativeWriter {
    fn name(&self) -> &'static str {
        "OpenDecks"
    }

    fn write(&mut self, ctx: &SyncContext) -> std::result::Result<(), String> {
        let run = || -> od_format_native::Result<()> {
            let mut stick = od_format_native::open_library(ctx.root)?;
            let present: HashSet<TrackId> = stick
                .tracks(od_library::SortKey::DateAdded, false)?
                .into_iter()
                .map(|t| t.id)
                .collect();
            let paths: HashMap<TrackId, &str> = ctx
                .tracks
                .iter()
                .map(|t| (t.record.id, t.stick_path.as_str()))
                .collect();
            for t in ctx.tracks {
                let id = t.record.id;
                if !ctx.full && present.contains(&id) && !ctx.changed.contains(&id) {
                    continue;
                }
                let mut data = ctx.library.mirror_track_data(id)?;
                data.record.path = PathBuf::from(paths[&id]);
                stick.mirror_put_track(&data)?;
            }
            stick.mirror_retain_tracks(&ctx.tracks.iter().map(|t| t.record.id).collect())?;
            if ctx.playlists_changed || ctx.full || !ctx.changed.is_empty() {
                stick.mirror_replace_tree(ctx.tree)?;
            }
            stick.set_meta("synced_at", &ctx.library.now()?)?;
            Ok(())
        };
        run().map_err(|e| e.to_string())?;
        self.carry_stems(ctx).map_err(|e| format!("stems: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{ContentHash, Cue, TrackTags};
    use od_library::{DeviceFormats, NewTrack};

    struct Fixture {
        _dir: tempfile::TempDir,
        music: PathBuf,
        stick: PathBuf,
        lib: Library,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        let stick = dir.path().join("stick");
        std::fs::create_dir_all(&music).unwrap();
        std::fs::create_dir_all(&stick).unwrap();
        let lib = Library::open(&dir.path().join("lib/library.sqlite")).unwrap();
        Fixture {
            _dir: dir,
            music,
            stick,
            lib,
        }
    }

    fn add(f: &mut Fixture, name: &str, artist: &str) -> TrackId {
        let path = f.music.join(format!("{name}.mp3"));
        std::fs::write(&path, format!("audio of {name}").repeat(100)).unwrap();
        f.lib
            .add_track(&NewTrack {
                content_hash: od_decode::content_hash(&path).unwrap(),
                file_size: std::fs::metadata(&path).unwrap().len(),
                path,
                format: None,
                duration: 200.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: None,
                bits_per_sample: None,
                tags: TrackTags {
                    title: name.into(),
                    artist: artist.into(),
                    ..Default::default()
                },
                artwork: None,
            })
            .unwrap()
    }

    fn run(f: &mut Fixture, dev: DeviceId) -> SyncReport {
        let mut writers: Vec<Box<dyn FormatWriter + Send>> =
            vec![Box::new(NativeWriter { stems: None })];
        sync_device(
            &mut f.lib,
            dev,
            &f.stick.clone(),
            &mut writers,
            &SyncOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap()
    }

    #[test]
    fn full_then_incremental_sync() {
        let mut f = fixture();
        let a = add(&mut f, "Alpha", "Artist One");
        let b = add(&mut f, "Beta", "Artist Two");
        let c = add(&mut f, "Gamma", "Artist Two");
        let pl = f.lib.create_node(None, NodeKind::Playlist, "Gig").unwrap();
        f.lib.add_to_node(pl, &[a, b], None).unwrap();
        let dev = f
            .lib
            .register_device("uuid", "Stick", DeviceFormats::default())
            .unwrap();
        f.lib.set_device_nodes(dev, &[pl]).unwrap();

        let r = run(&mut f, dev);
        assert!(r.full);
        assert_eq!((r.copied, r.tracks), (2, 2));
        assert!(
            f.stick
                .join("Contents/Artist One/Unknown Album/Alpha.mp3")
                .exists()
        );
        assert!(
            od_format_native::interrupted_sync(&f.stick)
                .unwrap()
                .is_none()
        );
        assert!(f.lib.pending_changes(dev).unwrap().is_empty());

        // A cue edit only rewrites that track's metadata.
        f.lib.set_cues(a, vec![Cue::hot(0, 12.5)]).unwrap();
        let r = run(&mut f, dev);
        assert!(!r.full);
        assert_eq!((r.copied, r.updated), (0, 1));
        let stick = od_format_native::Stick::open(&f.stick).unwrap().unwrap();
        assert_eq!(stick.library.cues(a).unwrap()[0].start, 12.5);
        drop(stick);

        // Swapping a track in the playlist copies the new one and removes the old one.
        f.lib.remove_from_node(pl, &[1]).unwrap();
        f.lib.add_to_node(pl, &[c], None).unwrap();
        let r = run(&mut f, dev);
        assert_eq!((r.copied, r.removed), (1, 1));
        assert!(
            !f.stick
                .join("Contents/Artist Two/Unknown Album/Beta.mp3")
                .exists()
        );
        let stick = od_format_native::Stick::open(&f.stick).unwrap().unwrap();
        let names: Vec<_> = stick
            .library
            .node_tracks(pl)
            .unwrap()
            .into_iter()
            .map(|t| t.tags.title)
            .collect();
        assert_eq!(names, vec!["Alpha", "Gamma"]);
        assert_eq!(stick.library.track_count().unwrap(), 2);
        let path = stick.library.track(c).unwrap().path;
        assert!(stick.resolve(&path).exists());
    }

    #[test]
    fn cached_stems_travel_with_the_tracks() {
        let mut f = fixture();
        let a = add(&mut f, "Alpha", "X");
        let b = add(&mut f, "Beta", "Y");
        let pl = f.lib.create_node(None, NodeKind::Playlist, "Gig").unwrap();
        f.lib.add_to_node(pl, &[a, b], None).unwrap();
        let dev = f
            .lib
            .register_device("uuid", "Stick", DeviceFormats::default())
            .unwrap();
        f.lib.set_device_nodes(dev, &[pl]).unwrap();
        // Only Alpha has separated stems on the desktop.
        let stems = f.music.join("stem-cache");
        std::fs::create_dir_all(&stems).unwrap();
        let ha = f.lib.track(a).unwrap().content_hash;
        std::fs::write(stems.join(stem_file(&ha)), b"stems of alpha").unwrap();
        let sync = |f: &mut Fixture| {
            let mut writers: Vec<Box<dyn FormatWriter + Send>> = vec![Box::new(NativeWriter {
                stems: Some(stems.clone()),
            })];
            sync_device(
                &mut f.lib,
                dev,
                &f.stick.clone(),
                &mut writers,
                &SyncOptions::default(),
                &AtomicBool::new(false),
                |_| {},
            )
            .unwrap();
        };
        sync(&mut f);
        let on_stick = f.stick.join("OPENDECKS/stems").join(stem_file(&ha));
        assert_eq!(std::fs::read(&on_stick).unwrap(), b"stems of alpha");
        assert_eq!(
            std::fs::read_dir(f.stick.join("OPENDECKS/stems"))
                .unwrap()
                .count(),
            1
        );
        // Alpha leaves the playlist: its stems leave the stick.
        f.lib.remove_from_node(pl, &[0]).unwrap();
        sync(&mut f);
        assert!(!on_stick.exists());
    }

    #[test]
    fn interrupted_sync_forces_full() {
        let mut f = fixture();
        let a = add(&mut f, "Alpha", "X");
        let pl = f.lib.create_node(None, NodeKind::Playlist, "P").unwrap();
        f.lib.add_to_node(pl, &[a], None).unwrap();
        let dev = f
            .lib
            .register_device("u", "S", DeviceFormats::default())
            .unwrap();
        f.lib.set_device_nodes(dev, &[pl]).unwrap();
        run(&mut f, dev);
        od_format_native::begin_sync(
            &f.stick,
            &od_format_native::SyncJournal {
                started_at: "x".into(),
                full: false,
            },
        )
        .unwrap();
        assert!(run(&mut f, dev).full);
    }

    #[test]
    fn missing_source_files_are_reported() {
        let mut f = fixture();
        let a = add(&mut f, "Alpha", "X");
        std::fs::remove_file(f.music.join("Alpha.mp3")).unwrap();
        let pl = f.lib.create_node(None, NodeKind::Playlist, "P").unwrap();
        f.lib.add_to_node(pl, &[a], None).unwrap();
        let dev = f
            .lib
            .register_device("u", "S", DeviceFormats::default())
            .unwrap();
        f.lib.set_device_nodes(dev, &[pl]).unwrap();
        let r = run(&mut f, dev);
        assert_eq!(r.skipped_missing, vec![a]);
        assert_eq!(r.copied, 0);
        let _ = ContentHash("unused".into());
    }

    #[test]
    fn smart_playlists_become_static() {
        let mut f = fixture();
        let a = add(&mut f, "Alpha", "X");
        let smart = f.lib.create_node(None, NodeKind::Smart, "All").unwrap();
        let dev = f
            .lib
            .register_device("u", "S", DeviceFormats::default())
            .unwrap();
        f.lib.set_device_nodes(dev, &[smart]).unwrap();
        let on: HashSet<TrackId> = [a].into_iter().collect();
        let tree = device_tree(&f.lib, dev, &on).unwrap();
        assert_eq!(tree[0].0.kind, NodeKind::Playlist);
        assert_eq!(tree[0].1.len(), 1);
    }
}
