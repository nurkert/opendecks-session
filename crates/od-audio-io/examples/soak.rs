//! Soak test for SPEC §13 / M2: N seconds of 4 decks playing with keylock,
//! sync, tempo changes, two beat FX, colour FX and the record tap drained,
//! on the real audio device. Reports dropouts (backend xruns + callback
//! overruns) and processing load.
//!
//! cargo run --release -p od-audio-io --example soak -- <seconds> <buffer> <lite|full> <file>...
//!
//! The master level is set to zero so the run is silent; all processing
//! still happens.

use od_audio_io::{AudioConfig, AudioHost};
use od_core::TrackId;
use od_engine::*;
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3600);
    let buffer: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(128);
    let profile = if args.next().as_deref() == Some("lite") { Profile::Lite } else { Profile::Full };
    let files: Vec<std::path::PathBuf> = args.map(Into::into).collect();
    anyhow::ensure!(!files.is_empty(), "give at least one audio file");

    let cfg = AudioConfig { buffer_frames: Some(buffer), ..Default::default() };
    let neg = AudioHost::negotiate(&cfg)?;
    let (mut engine, processor) = Engine::new(EngineConfig {
        sample_rate: neg.sample_rate,
        max_block: 2048,
        decks: 4,
        profile,
    });
    let mut tap = engine.take_record_tap().expect("record tap");
    for d in 0..4 {
        let path = &files[d % files.len()];
        let analysis = od_analysis::analyze_file(path, &Default::default())?;
        let track = engine.prepare_track(TrackId(d as i64 + 1), path, analysis.beatgrid, &[], 0.0)?;
        engine.deck(d, DeckCommand::Load(track))?;
    }
    let host = AudioHost::start(&cfg, processor)?;
    println!(
        "device {} @ {} Hz, buffer {} frames, profile {:?}, {} s",
        host.info().device_name,
        host.info().sample_rate,
        buffer,
        profile,
        seconds
    );
    engine.send(Command::Mixer(MixerCommand::SetMasterLevel(0.0)))?;
    engine.send(Command::SetRecordTap(true))?;
    for d in 0..4 {
        engine.deck(d, DeckCommand::SetKeylock(true))?;
        engine.deck(d, DeckCommand::SetTempoFader(0.3 - d as f32 * 0.15))?;
        if d > 0 {
            engine.deck(d, DeckCommand::SetSync(true))?;
        }
        engine.send(Command::Channel(d, ChannelCommand::SetFader(1.0)))?;
        engine.send(Command::Channel(d, ChannelCommand::SetColorFxAmount(0.3)))?;
        engine.deck(d, DeckCommand::Play)?;
    }
    for (unit, kind, target) in [(0, BeatFxKind::Echo, FxTarget::Master), (1, BeatFxKind::Reverb, FxTarget::Channel(1))] {
        engine.send(Command::BeatFx(unit, BeatFxCommand::SetKind(kind)))?;
        engine.send(Command::BeatFx(unit, BeatFxCommand::SetTarget(target)))?;
        engine.send(Command::BeatFx(unit, BeatFxCommand::SetLevel(0.5)))?;
        engine.send(Command::BeatFx(unit, BeatFxCommand::SetOn(true)))?;
    }

    let start = Instant::now();
    let mut recorded = 0usize;
    let mut buf = Vec::with_capacity(1 << 16);
    let mut next_report = Duration::from_secs(1);
    let mut tempo_step = 0u32;
    while start.elapsed() < Duration::from_secs(seconds) {
        std::thread::sleep(Duration::from_millis(50));
        buf.clear();
        recorded += tap.drain_into(&mut buf);
        let st = *engine.state();
        for d in 0..4 {
            if st.decks[d].at_end || st.decks[d].position > st.decks[d].duration - 2.0 {
                engine.deck(d, DeckCommand::Seek { seconds: 0.0 })?;
                engine.deck(d, DeckCommand::Play)?;
            }
        }
        engine.collect_garbage();
        if start.elapsed() >= next_report {
            // Fine-grained during the first minute, then once a minute.
            next_report += if next_report < Duration::from_secs(60) { Duration::from_secs(5) } else { Duration::from_secs(300) };
            // Move the master tempo at every report to keep keylock and sync busy.
            tempo_step += 1;
            engine.deck(0, DeckCommand::SetTempoFader(((tempo_step % 7) as f32 - 3.0) / 10.0))?;
            let s = host.stats();
            println!(
                "{:>5.0} s | dropouts {} (xruns {}, overruns {}) | load {:4.1}% peak {:4.1}% | recorded {:.0} s | lost {}",
                start.elapsed().as_secs_f64(),
                s.xruns + s.overruns,
                s.xruns,
                s.overruns,
                s.load * 100.0,
                s.peak_load * 100.0,
                recorded as f64 / 2.0 / host.info().sample_rate as f64,
                s.device_lost
            );
        }
    }
    let s = host.stats();
    println!(
        "RESULT {} s: dropouts {} (xruns {}, overruns {}), peak load {:.1}%, callbacks {}, device lost {}",
        seconds,
        s.xruns + s.overruns,
        s.xruns,
        s.overruns,
        s.peak_load * 100.0,
        s.callbacks,
        s.device_lost
    );
    Ok(())
}
