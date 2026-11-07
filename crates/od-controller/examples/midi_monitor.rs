//! MIDI monitor: prints every raw MIDI message from connected controllers
//! together with the actions the bundled mappings decode from it.
//!
//! ```text
//! cargo run -p od-controller --example midi_monitor
//! cargo run -p od-controller --example midi_monitor -- --mapping mappings/pioneer/ddj-flx10.toml
//! cargo run -p od-controller --example midi_monitor -- --learn deck.play_pause
//! cargo run -p od-controller --example midi_monitor -- --leds
//! ```
//!
//! Options:
//! - `--mapping <file>`: use only this mapping file (default: all in `mappings/`).
//! - `--mapped-only`: do not open ports without a mapping.
//! - `--learn <action>`: after 5 s of input print a suggested `[[input]]` entry.
//! - `--leds`: cycle the Play/Cue/Sync LEDs and VU meters of every deck so
//!   the output side of the mapping can be checked.
//!
//! Use it on real hardware to confirm entries marked `verify = true`.

use od_controller::io::{ControllerEvent, ControllerManager, ManagerConfig, list_ports};
use od_controller::{ControlFeedback, DeckElement, Mapping};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut mapping_file = None;
    let mut mapped_only = false;
    let mut learn: Option<String> = None;
    let mut leds = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mapping" => mapping_file = args.next(),
            "--mapped-only" => mapped_only = true,
            "--learn" => learn = args.next(),
            "--leds" => leds = true,
            "-h" | "--help" => {
                println!(
                    "usage: midi_monitor [--mapping FILE] [--mapped-only] [--learn ACTION] [--leds]"
                );
                return;
            }
            other => eprintln!("ignoring unknown argument {other}"),
        }
    }

    let mut mappings = Vec::new();
    let loaded = match &mapping_file {
        Some(f) => vec![(f.into(), Mapping::load(std::path::Path::new(f)))],
        None => Mapping::load_dir(&od_controller::bundled_mappings_dir()),
    };
    for (path, r) in loaded {
        match r {
            Ok(m) => {
                println!(
                    "mapping {} ({}): {} inputs, {} outputs, {} entries to verify on hardware",
                    m.device().id,
                    path.display(),
                    m.inputs.len(),
                    m.outputs.len(),
                    m.unverified().len()
                );
                mappings.push(Arc::new(m));
            }
            Err(e) => eprintln!("mapping {}: {e}", path.display()),
        }
    }

    match list_ports() {
        Ok(p) => {
            println!("MIDI inputs:  {:?}", p.inputs);
            println!("MIDI outputs: {:?}", p.outputs);
        }
        Err(e) => eprintln!("cannot list MIDI ports: {e}"),
    }
    for d in od_controller::usb::list_usb_devices() {
        if mappings
            .iter()
            .any(|m| m.matches_usb(d.vendor_id, d.product_id))
        {
            println!(
                "USB {:04X}:{:04X} {} matches a mapping",
                d.vendor_id,
                d.product_id,
                d.product.as_deref().unwrap_or("")
            );
        }
    }

    let decks: Vec<u8> = mappings.iter().map(|m| m.device().decks).collect();
    let mgr = ControllerManager::start(
        mappings,
        ManagerConfig {
            monitor_unmapped_ports: !mapped_only,
            ..Default::default()
        },
    );
    let monitor = mgr.monitor().subscribe();
    if learn.is_some() {
        mgr.start_learn();
        println!("MIDI learn: move or press the control now (5 s)...");
    }
    let started = Instant::now();
    let mut led_step = 0u32;
    let mut last_led = Instant::now();
    println!("listening — press Ctrl-C to quit");
    loop {
        crossbeam_select(&mgr, &monitor);
        if let Some(action) = &learn
            && started.elapsed() > Duration::from_secs(5)
        {
            match mgr.finish_learn().and_then(|l| l.suggest()) {
                Some(s) => println!("\nsuggested entry:\n{}", s.to_toml(action, action)),
                None => println!("\nno MIDI received during learn"),
            }
            learn = None;
        }
        if leds && last_led.elapsed() > Duration::from_millis(400) {
            last_led = Instant::now();
            led_step += 1;
            let max_decks = decks.iter().copied().max().unwrap_or(2);
            for deck in 1..=max_decks {
                let el = [DeckElement::Play, DeckElement::Cue, DeckElement::Sync]
                    [(led_step % 3) as usize];
                for e in [DeckElement::Play, DeckElement::Cue, DeckElement::Sync] {
                    mgr.send_feedback(None, ControlFeedback::deck_led(deck, e, e == el));
                }
                let level = ((led_step % 8) as f32) / 7.0;
                mgr.send_feedback(None, ControlFeedback::channel_vu(deck, level));
            }
        }
    }
}

fn crossbeam_select(
    mgr: &ControllerManager,
    monitor: &crossbeam_channel::Receiver<od_controller::monitor::MonitorEvent>,
) {
    crossbeam_channel::select! {
        recv(monitor) -> ev => if let Ok(ev) = ev
            && ev.direction == od_controller::monitor::Direction::In {
                println!("{ev}");
            },
        recv(mgr.events()) -> ev => match ev {
            Ok(ControllerEvent::Action { .. }) | Err(_) => {}
            Ok(other) => println!("== {other:?}"),
        },
        default(Duration::from_millis(50)) => {}
    }
}
