//! Performance benchmark (ignored by default). Run with
//! `cargo test --release -p od-engine --test perf -- --ignored --nocapture`.

mod common;

use std::time::Instant;

use common::*;
use od_engine::*;

fn run(profile: Profile, keylock: bool, block: usize) -> (f64, f64, f64) {
    let mut rig = Rig::with(EngineConfig {
        sample_rate: SR,
        max_block: block,
        decks: 4,
        profile,
    });
    for i in 0..4 {
        // Broadband-ish material: a few partials so the stretcher has work.
        let n = 120 * SR as usize;
        let f = 110.0 * (i + 1) as f64;
        let s: Vec<f32> = (0..n)
            .flat_map(|k| {
                let t = k as f64 / SR as f64;
                let v = ((std::f64::consts::TAU * f * t).sin() * 0.2
                    + (std::f64::consts::TAU * f * 3.01 * t).sin() * 0.1
                    + (std::f64::consts::TAU * f * 7.3 * t).sin() * 0.05)
                    as f32;
                [v, v * 0.9]
            })
            .collect();
        rig.deck(
            i,
            DeckCommand::Load(track_from(i as i64, s, 124.0 + i as f64, 0.0)),
        );
        rig.deck(i, DeckCommand::SetKeylock(keylock));
        rig.deck(i, DeckCommand::SetTempoRange(TempoRange::Sixteen));
        rig.deck(i, DeckCommand::SetTempoFader(0.1 * i as f32 - 0.15));
        rig.deck(i, DeckCommand::Play);
        rig.send(Command::Channel(
            i,
            ChannelCommand::SetEq {
                band: 0,
                value: -0.3,
            },
        ));
        rig.send(Command::Channel(i, ChannelCommand::SetColorFxAmount(0.2)));
    }
    rig.deck(1, DeckCommand::SetSync(true));
    rig.send(Command::BeatFx(0, BeatFxCommand::SetKind(BeatFxKind::Echo)));
    rig.send(Command::BeatFx(0, BeatFxCommand::SetOn(true)));
    rig.send(Command::BeatFx(
        1,
        BeatFxCommand::SetKind(BeatFxKind::Reverb),
    ));
    rig.send(Command::BeatFx(
        1,
        BeatFxCommand::SetTarget(FxTarget::Channel(2)),
    ));
    rig.send(Command::BeatFx(1, BeatFxCommand::SetOn(true)));
    rig.send(Command::SetRecordTap(true));
    let mut tap = rig.engine.take_record_tap().unwrap();
    let mut sink = Vec::with_capacity(1 << 22);

    // Warm up (keylock priming).
    for _ in 0..200 {
        rig.block(block);
    }
    tap.drain_into(&mut sink);
    sink.clear();
    let seconds = 60.0;
    let blocks = (seconds * SR as f64 / block as f64) as usize;
    let mut worst = 0f64;
    let start = Instant::now();
    for b in 0..blocks {
        let t0 = Instant::now();
        rig.block(block);
        worst = worst.max(t0.elapsed().as_secs_f64());
        if b % 256 == 0 {
            tap.drain_into(&mut sink);
            sink.clear();
        }
    }
    let per_block = start.elapsed().as_secs_f64() / blocks as f64;
    let budget = block as f64 / SR as f64;
    (
        per_block * 1e6,
        per_block / budget * 100.0,
        worst / budget * 100.0,
    )
}

#[test]
#[ignore]
fn perf_4_decks_keylock_2_fx() {
    println!("\n4 decks + 2 Beat FX + EQ + colour FX + record tap @ 48 kHz");
    println!(
        "{:<34} {:>12} {:>12} {:>12}",
        "configuration", "µs/block", "% budget", "worst %"
    );
    for (name, profile, kl, block) in [
        ("Full, keylock, 128 frames", Profile::Full, true, 128),
        ("Lite, keylock, 128 frames", Profile::Lite, true, 128),
        ("Lite, keylock, 256 frames", Profile::Lite, true, 256),
        ("no keylock, 128 frames", Profile::Full, false, 128),
    ] {
        let (us, pct, worst) = run(profile, kl, block);
        println!("{name:<34} {us:>12.1} {pct:>11.1}% {worst:>11.1}%");
    }
}
