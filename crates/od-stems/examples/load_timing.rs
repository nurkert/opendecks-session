//! Times the phases of loading a track onto a deck:
//! `cargo run -p od-stems --example load_timing -- <audio> [hash] [engine-rate]`.
use od_core::{Beatgrid, ContentHash, TrackId};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let hash = args.get(2).map(|h| ContentHash(h.clone()));
    let rate: u32 = args.get(3).and_then(|r| r.parse().ok()).unwrap_or(48_000);
    let cache = od_stems::StemCache::open_default();
    let t = Instant::now();
    let stems = hash.as_ref().and_then(|h| cache.load(h).ok().flatten());
    println!("stem cache read: {:?} (stems: {})", t.elapsed(), stems.is_some());
    let t = Instant::now();
    let audio = od_decode::decode(path, od_decode::Layout::Stereo).unwrap();
    println!("decode: {:?} ({} Hz, {:.1} s)", t.elapsed(), audio.sample_rate, audio.frames() as f64 / audio.sample_rate as f64);
    let t = Instant::now();
    let _ = od_engine::TrackData::new_with_stems(TrackId(1), audio.clone(), rate, Beatgrid::default(), &[], 0.0, None);
    println!("track data (mix only): {:?}", t.elapsed());
    if let Some(s) = stems.as_ref() {
        let t = Instant::now();
        let _ = od_engine::TrackData::new_with_stems(TrackId(1), audio, rate, Beatgrid::default(), &[], 0.0, Some(s));
        println!("track data (with stems): {:?}", t.elapsed());
    }
}
