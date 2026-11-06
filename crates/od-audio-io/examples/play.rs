//! Plays an audio file through the engine on the default output for 10 s.
//!
//! ```text
//! cargo run --release -p od-audio-io --example play -- path/to/track.mp3 [device-name] [buffer-frames]
//! ```
//!
//! Prints the available devices, the negotiated stream and live statistics.
//! On a 4-output controller (DDJ-FLX10) the headphones get the cue (PFL)
//! signal of the deck on outputs 3/4.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use od_audio_io::{AudioConfig, AudioHost, Backend, backends, list_devices};
use od_core::{Beatgrid, TrackId};
use od_engine::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path: PathBuf = args
        .next()
        .ok_or("usage: play <audio file> [device] [buffer frames]")?
        .into();
    let device = args.next();
    let buffer: Option<u32> = args.next().and_then(|b| b.parse().ok());

    println!("Backends: {:?}", backends());
    for d in list_devices(Backend::Default)? {
        println!(
            "  {}{} — out {} ch, in {} ch, rates {:?}, default {:?} Hz, buffer {:?}",
            if d.is_default_output { "* " } else { "  " },
            d.name,
            d.max_output_channels,
            d.max_input_channels,
            d.sample_rates,
            d.default_sample_rate,
            d.buffer_range
        );
    }

    let cfg = AudioConfig {
        device,
        buffer_frames: Some(buffer.unwrap_or(256)),
        ..Default::default()
    };
    let neg = AudioHost::negotiate(&cfg)?;
    println!("Negotiated: {neg:#?}");

    let t0 = Instant::now();
    let (mut engine, processor) = Engine::new(EngineConfig {
        sample_rate: neg.sample_rate,
        max_block: neg.buffer_frames.unwrap_or(512) as usize,
        decks: 2,
        profile: Profile::Full,
    });
    let track = engine.prepare_track(TrackId(1), &path, Beatgrid::default(), &[], 0.0)?;
    println!(
        "Loaded {} ({:.1} s) in {:?}",
        path.display(),
        track.duration(),
        t0.elapsed()
    );

    let host = AudioHost::start(&cfg, processor)?;
    println!(
        "Stream: {} @ {} Hz, {} ch, {}, routing {:?}, nominal latency {:?}",
        host.info().device_name,
        host.info().sample_rate,
        host.info().channels,
        host.info().sample_format,
        host.info().routing,
        host.info().nominal_latency()
    );

    engine.deck(0, DeckCommand::Load(track))?;
    engine.send(Command::Channel(0, ChannelCommand::SetCue(true)))?;
    engine.deck(0, DeckCommand::Play)?;

    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_secs(1));
        let st = *engine.state();
        let s = host.stats();
        println!(
            "pos {:6.2} s | master peak {:.2} rms {:.2} | callbacks {} frames {} | load {:4.1}% (peak {:4.1}%) | xruns {} | latency {:?}",
            st.decks[0].position,
            st.master_meter.peak,
            st.master_meter.rms,
            s.callbacks,
            s.callback_frames,
            s.load * 100.0,
            s.peak_load * 100.0,
            s.xruns + s.overruns,
            s.output_latency
        );
        engine.collect_garbage();
    }
    engine.deck(0, DeckCommand::Pause)?;
    std::thread::sleep(Duration::from_millis(100));
    drop(host);
    Ok(())
}
