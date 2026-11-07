//! Integration tests that need no network. Tests that need the model run
//! only if it was fetched (`od-cli stems fetch-model`), else they return
//! early.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use od_core::{Beatgrid, ContentHash, TrackId};
use od_stems::*;

fn write_wav(path: &Path, rate: u32, secs: f32, f: impl Fn(f32) -> (f32, f32)) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(secs * rate as f32) as usize {
        let (l, r) = f(i as f32 / rate as f32);
        w.write_sample((l * 32767.0) as i16).unwrap();
        w.write_sample((r * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

#[test]
fn load_track_data_attaches_cached_stems() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("t.wav");
    write_wav(&wav, 44_100, 2.0, |t| {
        let v = (t * 440.0 * std::f32::consts::TAU).sin() * 0.3;
        (v, v)
    });
    let cache = StemCache::new(dir.path().join("stems"));
    let hash = ContentHash("feedbeef".into());
    let grid = Beatgrid::default();

    // Not cached: plain track.
    let t = load_track_data(
        &cache,
        TrackId(1),
        &wav,
        Some(&hash),
        48_000,
        grid.clone(),
        &[],
        0.0,
    )
    .unwrap();
    assert!(!t.has_stems());

    // Cached stems (here: the whole mix as "vocals").
    let audio = od_decode::decode(&wav, od_decode::Layout::Stereo).unwrap();
    let n = audio.samples.len();
    let stems = Stems {
        sample_rate: 44_100,
        vocals: audio.samples.clone(),
        drums: vec![0.0; n],
        bass: vec![0.0; n],
    };
    cache.store(&hash, &stems).unwrap();
    let t = load_track_data(
        &cache,
        TrackId(1),
        &wav,
        Some(&hash),
        48_000,
        grid.clone(),
        &[],
        0.0,
    )
    .unwrap();
    let s = t.stems.as_ref().expect("stems attached");
    assert!((s.frames() as i64 - t.frames() as i64).abs() <= 1);
    println!("stem memory for 2 s at 48 kHz: {} bytes", s.memory_bytes());

    // Two-step load (decks): the mix alone, then the cached parts attached
    // to the same samples.
    let (mix, shape) = load_mix(TrackId(1), &wav, 48_000, grid.clone(), &[], 0.0).unwrap();
    assert!(!mix.has_stems());
    let full = attach_cached_stems(&cache, &mix, &hash, shape).expect("cached parts fit");
    assert!(full.has_stems());
    assert_eq!(full.samples, mix.samples);

    // Mismatching cache (other rate) is ignored, not an error.
    let bad = Stems {
        sample_rate: 22_050,
        ..stems
    };
    cache.store(&hash, &bad).unwrap();
    let t = load_track_data(
        &cache,
        TrackId(1),
        &wav,
        Some(&hash),
        48_000,
        grid,
        &[],
        0.0,
    )
    .unwrap();
    assert!(!t.has_stems());
    assert!(attach_cached_stems(&cache, &mix, &hash, shape).is_none());
}

#[test]
fn worker_reports_cached_and_missing_model_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let cache = StemCache::new(dir.path().join("stems"));
    let cached = ContentHash("aa".into());
    cache
        .store(
            &cached,
            &Stems {
                sample_rate: 44_100,
                vocals: vec![0.0; 20],
                drums: vec![0.0; 20],
                bass: vec![0.0; 20],
            },
        )
        .unwrap();
    // Point the model lookup at an empty data dir: no model, no download.
    // SAFETY: single-threaded at this point of this test binary's test; the
    // variable is only read by od_library::data_dir().
    unsafe { std::env::set_var("OPENDECKS_DATA_DIR", dir.path()) };
    let worker = StemWorker::spawn(
        cache,
        StemWorkerOptions {
            fetch_model: false,
            ..Default::default()
        },
    );
    worker.enqueue(StemJob {
        track: TrackId(1),
        path: "/nonexistent.wav".into(),
        hash: cached.clone(),
    });
    worker.enqueue(StemJob {
        track: TrackId(2),
        path: "/nonexistent.wav".into(),
        hash: ContentHash("bb".into()),
    });
    let ev = worker
        .events()
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        ev,
        StemEvent::AlreadyCached {
            track: TrackId(1),
            hash: cached
        }
    );
    match worker
        .events()
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
    {
        StemEvent::Failed { track, error } => {
            assert_eq!(track, TrackId(2));
            assert!(error.contains("fetch-model"), "{error}");
        }
        e => panic!("unexpected {e:?}"),
    }
    assert!(worker.is_idle());
}

#[test]
fn separates_a_synthetic_mix_if_the_model_is_present() {
    // Uses the real data dir's model only if a test did not redirect it.
    let model = dirs_model();
    if !model.is_file() {
        eprintln!("model not fetched; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("mix.wav");
    // 10 s: a kick-like click every 0.5 s plus a sustained bass tone.
    write_wav(&wav, 48_000, 10.0, |t| {
        let ph = t % 0.5;
        let kick = (-(ph * 40.0)).exp() * (ph * 60.0 * std::f32::consts::TAU).sin() * 0.6;
        let bass = (t * 55.0 * std::f32::consts::TAU).sin() * 0.2;
        (kick + bass, kick + bass)
    });
    let mut sep = Separator::new(&model, &SeparatorOptions::default()).unwrap();
    let mut last = 0.0;
    let s = sep
        .separate_file(&wav, &mut |f| last = f, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(last, 1.0);
    assert_eq!(s.sample_rate, 48_000);
    assert_eq!(s.vocals.len(), 10 * 48_000 * 2);
    let e = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let (v, d, b) = (e(&s.vocals), e(&s.drums), e(&s.bass));
    eprintln!("energy vocals {v:.2e} drums {d:.2e} bass {b:.2e}");
    assert!(
        d > 10.0 * v && b > 10.0 * v,
        "no vocals expected in kick + bass"
    );

    // Cancel aborts.
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        sep.separate_file(&wav, &mut |_| {}, &cancel),
        Err(Error::Cancelled)
    ));
}

fn dirs_model() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OpenDecks Session")
    } else {
        home.join(".local/share/opendecks-session")
    };
    base.join("models").join(MODEL.file_name)
}
