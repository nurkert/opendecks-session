//! Headless OpenDecks Session tool. Drives the same crates as the app; used
//! for scripting, the Raspberry Pi and automated tests.

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use od_core::DataSource;
use od_devices::{FormatWriter, NativeWriter, PioneerWriter, SyncOptions};
use od_library::{DeviceFormats, Library, NewTrack, NodeKind};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

#[derive(Parser)]
#[command(name = "od-cli", version, about)]
struct Cli {
    /// Library database (default: the app's library).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Add all audio files below the given folders.
    Import { dirs: Vec<PathBuf> },
    /// Analyse tracks that have no (current) analysis.
    Analyze {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        lite: bool,
    },
    /// List tracks.
    List {
        #[arg(long)]
        search: Option<String>,
    },
    /// Show the playlist tree.
    Tree,
    /// Create a playlist (or set) with tracks matching a search.
    Playlist {
        name: String,
        #[arg(long)]
        set: bool,
        /// Tracks whose title/artist match this search (default: all).
        #[arg(long)]
        search: Option<String>,
    },
    /// Register a mounted stick with the given playlists (by name).
    Register {
        mount: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "playlist", required = true)]
        playlists: Vec<String>,
    },
    /// Sync a registered stick mounted at the given path.
    Sync {
        mount: PathBuf,
        #[arg(long)]
        full: bool,
    },
    /// Read a stick back and print what it contains.
    Verify { mount: PathBuf },
    /// Create a backup of the library now.
    Backup,
    /// Stem separation: precompute stems for library tracks (`stems 12 13`,
    /// `stems --all`), fetch the model, or test on a file.
    #[command(args_conflicts_with_subcommands = true)]
    Stems(StemsArgs),
}

#[derive(clap::Args)]
struct StemsArgs {
    #[command(subcommand)]
    cmd: Option<StemsCmd>,
    /// Library track ids to separate.
    tracks: Vec<i64>,
    /// Separate every track without cached stems.
    #[arg(long)]
    all: bool,
    /// Separate again even if stems are cached.
    #[arg(long)]
    force: bool,
}

#[derive(Subcommand)]
enum StemsCmd {
    /// Download the separation model (verifies SHA-256).
    FetchModel,
    /// Separate an audio file (outside the library) and print timing and
    /// per-section stem energies; `--store` also writes the stem cache.
    File {
        path: PathBuf,
        #[arg(long)]
        store: bool,
        /// Section length for the energy table, seconds.
        #[arg(long, default_value_t = 15.0)]
        section: f64,
        /// Segment overlap 0..0.5 (default: the library default).
        #[arg(long)]
        overlap: Option<f32>,
        /// ONNX Runtime threads (0 = all cores).
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into())).init();
    let cli = Cli::parse();
    if let Cmd::Stems(StemsArgs { cmd: Some(c), .. }) = &cli.command {
        return stems_cmd(c);
    }
    let path = cli.library.unwrap_or_else(Library::default_path);
    let mut lib = Library::open(&path).with_context(|| format!("opening {}", path.display()))?;
    match cli.command {
        Cmd::Import { dirs } => {
            let mut added = 0;
            for dir in dirs {
                let mut stack = vec![dir];
                while let Some(d) = stack.pop() {
                    for e in std::fs::read_dir(&d)?.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            stack.push(p);
                        } else if od_core::AudioFormat::is_supported_extension(&p) {
                            match od_decode::probe(&p) {
                                Ok(info) => {
                                    lib.add_track(&NewTrack {
                                        content_hash: od_decode::content_hash(&p)?,
                                        file_size: std::fs::metadata(&p)?.len(),
                                        path: p.clone(),
                                        format: info.format,
                                        duration: info.duration.unwrap_or(0.0),
                                        sample_rate: info.sample_rate,
                                        channels: info.channels,
                                        bit_rate: info.bit_rate_kbps,
                                        bits_per_sample: info.bits_per_sample,
                                        tags: info.tags,
                                        artwork: info.artwork,
                                    })?;
                                    added += 1;
                                }
                                Err(e) => eprintln!("skipped {}: {e}", p.display()),
                            }
                        }
                    }
                }
            }
            println!("{added} tracks added ({} in library)", lib.track_count()?);
        }
        Cmd::Analyze { all, lite } => {
            let version = if all { u32::MAX } else { od_analysis::ANALYSIS_VERSION };
            let todo = lib.tracks_needing_analysis(version)?;
            let opts = od_analysis::AnalysisOptions { lite, ..Default::default() };
            let paths: Vec<_> = todo.iter().filter_map(|id| lib.track(*id).ok().map(|t| (*id, t.path))).collect();
            let started = std::time::Instant::now();
            let results: Vec<_> = {
                use rayon::prelude::*;
                paths.par_iter().map(|(id, p)| (*id, od_analysis::analyze_file(p, &opts))).collect()
            };
            for (id, r) in results {
                match r {
                    Ok(a) => lib.store_analysis(id, &a, DataSource::Analysis)?,
                    Err(e) => eprintln!("track {id}: {e}"),
                }
            }
            println!("analysed {} tracks in {:.1} s", paths.len(), started.elapsed().as_secs_f64());
        }
        Cmd::List { search } => {
            let tracks = match search {
                Some(q) => lib.search(&q, 10_000)?,
                None => lib.tracks(od_library::SortKey::Artist, false)?,
            };
            for t in tracks {
                println!(
                    "{:>5}  {:>7}  {:>4}  {:>6}  {} — {}",
                    t.id,
                    t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default(),
                    t.key.map(|k| k.to_string()).unwrap_or_default(),
                    format!("{}:{:02}", t.duration as u64 / 60, t.duration as u64 % 60),
                    t.tags.artist,
                    t.tags.title
                );
            }
        }
        Cmd::Tree => {
            fn walk(lib: &Library, parent: Option<od_library::NodeId>, depth: usize) -> anyhow::Result<()> {
                for n in lib.children(parent)? {
                    let count = if n.kind == NodeKind::Folder { String::new() } else { format!(" ({})", lib.node_tracks(n.id)?.len()) };
                    println!("{}{} [{}]{count}", "  ".repeat(depth), n.name, n.kind.as_str());
                    walk(lib, Some(n.id), depth + 1)?;
                }
                Ok(())
            }
            walk(&lib, None, 0)?;
        }
        Cmd::Playlist { name, set, search } => {
            let tracks = match search {
                Some(q) => lib.search(&q, 10_000)?,
                None => lib.tracks(od_library::SortKey::Bpm, false)?,
            };
            let id = lib.create_node(None, if set { NodeKind::Set } else { NodeKind::Playlist }, &name)?;
            lib.add_to_node(id, &tracks.iter().map(|t| t.id).collect::<Vec<_>>(), None)?;
            println!("created {} with {} tracks", name, tracks.len());
        }
        Cmd::Register { mount, name, playlists } => {
            let all = lib.all_nodes()?;
            let mut nodes = Vec::new();
            for p in &playlists {
                let n = all.iter().find(|n| &n.name == p).with_context(|| format!("no playlist named {p}"))?;
                nodes.push(n.id);
            }
            let marker = match od_format_native::read_marker(&mount)? {
                Some(m) => m,
                None => {
                    let uuid = od_core::ContentHash(format!("{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos())).0;
                    let m = od_format_native::DeviceMarker {
                        format_version: od_format_native::FORMAT_VERSION,
                        device_uuid: uuid,
                        name: name.clone().unwrap_or_else(|| mount.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                        created_by: "od-cli".into(),
                    };
                    od_format_native::write_marker(&mount, &m)?;
                    m
                }
            };
            let id = lib.register_device(&marker.device_uuid, &marker.name, DeviceFormats::default())?;
            lib.set_device_nodes(id, &nodes)?;
            println!("registered {} as device {id}", marker.name);
        }
        Cmd::Sync { mount, full } => {
            let Some(marker) = od_format_native::read_marker(&mount)? else { bail!("{} is not a registered stick", mount.display()) };
            let Some(device) = lib.device_by_uuid(&marker.device_uuid)? else { bail!("stick not registered in this library") };
            let mut writers: Vec<Box<dyn FormatWriter + Send>> = Vec::new();
            if device.formats.opendecks {
                writers.push(Box::new(NativeWriter));
            }
            if device.formats.pioneer {
                writers.push(Box::new(PioneerWriter::default()));
            }
            let report = od_devices::sync::sync_device(
                &mut lib,
                device.id,
                &mount,
                &mut writers,
                &SyncOptions { force_full: full },
                &AtomicBool::new(false),
                |_| {},
            )?;
            println!(
                "synced {}: {} tracks, {} copied ({} MB), {} updated, {} removed, {} missing, {} proposals, {:.2} s{}",
                device.name,
                report.tracks,
                report.copied,
                report.bytes_copied / 1_000_000,
                report.updated,
                report.removed,
                report.skipped_missing.len(),
                report.proposals.len(),
                report.seconds,
                if report.full { " (full)" } else { "" }
            );
        }
        Cmd::Verify { mount } => {
            let pdb = mount.join("PIONEER/rekordbox/export.pdb");
            if pdb.exists() {
                let db = od_format_pioneer::pdb::ExportDatabase::parse(&std::fs::read(&pdb)?)?;
                println!("export.pdb: {} tracks, {} playlists", db.tracks.len(), db.playlists.len());
                let mut cues = 0;
                for t in &db.tracks {
                    let dat = mount.join(t.anlz_path.trim_start_matches('/'));
                    let ext = dat.with_extension("EXT");
                    let a = od_format_pioneer::anlz::AnlzFile::parse(&std::fs::read(&dat)?)?;
                    let e = od_format_pioneer::anlz::AnlzFile::parse(&std::fs::read(&ext)?)?;
                    cues += e.cues().len();
                    if a.beatgrid().is_none() {
                        println!("  no beatgrid: {}", t.tags.title);
                    }
                    if !mount.join(t.device_path.trim_start_matches('/')).exists() {
                        println!("  missing audio: {}", t.device_path);
                    }
                }
                println!("ANLZ: all files parse, {cues} cues");
            }
            if let Some(stick) = od_format_native::Stick::open(&mount)? {
                println!("OpenDecks: {} tracks, {} lists", stick.library.track_count()?, stick.library.all_nodes()?.len());
            }
        }
        Cmd::Backup => println!("{}", lib.backup_now("cli")?.display()),
        Cmd::Stems(a) => {
            let cache = od_stems::StemCache::open_default();
            let records = if a.all {
                lib.tracks(od_library::SortKey::Artist, false)?
            } else if a.tracks.is_empty() {
                bail!("give track ids or --all (see `od-cli stems --help`)");
            } else {
                a.tracks.iter().map(|id| lib.track(od_core::TrackId(*id))).collect::<Result<Vec<_>, _>>()?
            };
            let todo: Vec<_> = records.into_iter().filter(|t| !t.missing && (a.force || !cache.exists(&t.content_hash))).collect();
            if todo.is_empty() {
                println!("all stems cached in {}", cache.dir().display());
                return Ok(());
            }
            fetch_model_cli()?;
            let mut sep = od_stems::Separator::new(&od_stems::model_path(), &Default::default())?;
            let cancel = AtomicBool::new(false);
            let (mut ok, mut audio_s, started) = (0, 0.0, std::time::Instant::now());
            for (n, t) in todo.iter().enumerate() {
                let t0 = std::time::Instant::now();
                let label = format!("[{}/{}] {} — {}", n + 1, todo.len(), t.tags.artist, t.tags.title);
                let r = sep.separate_file(&t.path, &mut |f| eprint!("\r{label}: {:3.0} %", f * 100.0), &cancel);
                eprintln!();
                match r.and_then(|s| cache.store(&t.content_hash, &s)) {
                    Ok(p) => {
                        ok += 1;
                        audio_s += t.duration;
                        println!("{label}: {:.1} s → {}", t0.elapsed().as_secs_f64(), p.display());
                    }
                    Err(e) => eprintln!("{label}: {e}"),
                }
            }
            let secs = started.elapsed().as_secs_f64();
            println!("separated {ok}/{} tracks in {secs:.1} s ({:.2}× real time)", todo.len(), secs / audio_s.max(1e-9));
        }
    }
    Ok(())
}

fn fetch_model_cli() -> anyhow::Result<PathBuf> {
    if od_stems::model_available() {
        return Ok(od_stems::model_path());
    }
    let m = od_stems::MODEL;
    eprintln!("downloading {} ({} MB, {}) …", m.file_name, m.size / 1_000_000, m.license);
    let mut last = 0;
    let p = od_stems::fetch_model(
        &mut |done, total| {
            let pct = done * 100 / total.max(1);
            if pct != last {
                last = pct;
                eprint!("\r{pct:3} %");
            }
        },
        &AtomicBool::new(false),
    )?;
    eprintln!("\nmodel verified: {}", p.display());
    Ok(p)
}

fn stems_cmd(cmd: &StemsCmd) -> anyhow::Result<()> {
    match cmd {
        StemsCmd::FetchModel => {
            let p = fetch_model_cli()?;
            println!("{}", p.display());
        }
        StemsCmd::File { path, store, section, overlap, threads } => {
            fetch_model_cli()?;
            let t0 = std::time::Instant::now();
            let mut opts = od_stems::SeparatorOptions { threads: *threads, ..Default::default() };
            if let Some(o) = overlap {
                opts.overlap = *o;
            }
            let mut sep = od_stems::Separator::new(&od_stems::model_path(), &opts)?;
            let load_s = t0.elapsed().as_secs_f64();
            let t1 = std::time::Instant::now();
            let audio = od_decode::decode(path, od_decode::Layout::Stereo)?;
            let decode_s = t1.elapsed().as_secs_f64();
            let dur = audio.frames() as f64 / audio.sample_rate as f64;
            let t2 = std::time::Instant::now();
            let stems = sep.separate(&audio, &mut |f| eprint!("\rseparating {:3.0} %", f * 100.0), &AtomicBool::new(false))?;
            let sep_s = t2.elapsed().as_secs_f64();
            eprintln!();
            println!("{}: {:.1} s audio at {} Hz", path.display(), dur, audio.sample_rate);
            println!("model load {load_s:.2} s, decode {decode_s:.2} s, separation {sep_s:.1} s = {:.3}× real time", sep_s / dur);
            // Energy per section (dBFS RMS): mix, vocals, drums, bass, inst.
            let sr = audio.sample_rate as f64;
            let n = (section * sr) as usize * 2;
            let db = |x: &[f32]| 10.0 * ((x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64) + 1e-12).log10();
            println!("{:>7}  {:>6}  {:>6}  {:>6}  {:>6}  {:>6}", "t [s]", "mix", "vocals", "drums", "bass", "inst");
            for (k, m) in audio.samples.chunks(n).enumerate() {
                let r = k * n..k * n + m.len();
                let inst: Vec<f32> = r.clone().map(|i| m[i - k * n] - stems.vocals[i] - stems.drums[i] - stems.bass[i]).collect();
                println!(
                    "{:>7.0}  {:>6.1}  {:>6.1}  {:>6.1}  {:>6.1}  {:>6.1}",
                    k as f64 * section,
                    db(m),
                    db(&stems.vocals[r.clone()]),
                    db(&stems.drums[r.clone()]),
                    db(&stems.bass[r]),
                    db(&inst)
                );
            }
            if *store {
                let hash = od_decode::content_hash(path)?;
                let t3 = std::time::Instant::now();
                let p = od_stems::StemCache::open_default().store(&hash, &stems)?;
                println!("cached in {:.2} s: {} ({:.1} MB)", t3.elapsed().as_secs_f64(), p.display(), std::fs::metadata(&p)?.len() as f64 / 1e6);
                let cache = od_stems::StemCache::open_default();
                let t4 = std::time::Instant::now();
                let track = od_stems::load_track_data(&cache, od_core::TrackId(0), path, Some(&hash), 48_000, Default::default(), &[], 0.0)?;
                let s = track.stems.as_ref().context("stems not attached")?;
                println!(
                    "deck load with stems (decode + cache + resample to 48 kHz): {:.2} s; memory: mix {:.0} MB + stems {:.0} MB",
                    t4.elapsed().as_secs_f64(),
                    (track.samples.len() * 4) as f64 / 1e6,
                    s.memory_bytes() as f64 / 1e6
                );
            }
        }
    }
    Ok(())
}
