//! Real-time safety: `EngineProcessor::process` must not allocate or free.
//!
//! A counting global allocator records (de)allocations made on the current
//! thread while a guard flag is set. (Allocations inside the Rubber Band C++
//! library bypass the Rust allocator and are not seen here; Rubber Band is
//! real-time safe in RT mode once `set_max_process_size` was applied.)

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use od_engine::*;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static GUARD: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if GUARD.try_with(|g| g.get()).unwrap_or(false) {
        ALLOCS.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        note();
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn guarded<R>(f: impl FnOnce() -> R) -> R {
    GUARD.with(|g| g.set(true));
    let r = f();
    GUARD.with(|g| g.set(false));
    r
}

#[test]
fn process_never_allocates() {
    // The guard itself works.
    let seen = guarded(|| {
        let v: Vec<u8> = Vec::with_capacity(16);
        drop(std::hint::black_box(v));
        ALLOCS.swap(0, Ordering::SeqCst)
    });
    assert_eq!(seen, 2);

    let mut rig = Rig::new(4);
    let sample = SampleData::new(
        od_decode::DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples: vec![0.2; 20_000],
        },
        SR,
    );
    let tracks: Vec<_> = (0..4)
        .map(|i| sine_track(i, 30.0, 300.0 + 50.0 * i as f64, 0.3, 120.0 + i as f64))
        .collect();
    let replacement = sine_track(9, 5.0, 500.0, 0.3, 125.0);
    let mut sender = rig.engine.new_sender().unwrap();
    let mut tap = rig.engine.take_record_tap().unwrap();
    let mut drained = Vec::with_capacity(1 << 20);

    for (i, t) in tracks.into_iter().enumerate() {
        rig.deck(i, DeckCommand::Load(t));
        rig.deck(i, DeckCommand::Play);
    }
    rig.deck(1, DeckCommand::SetSync(true));
    rig.deck(2, DeckCommand::SetKeylock(true));
    rig.deck(3, DeckCommand::SetKeylock(true));
    rig.deck(3, DeckCommand::SetTempoFader(0.4));
    rig.send(Command::SetRecordTap(true));
    rig.send(Command::Sampler(SamplerCommand::Load { slot: 0, sample }));
    rig.send(Command::Sampler(SamplerCommand::SetMode {
        slot: 0,
        mode: SampleMode::Loop,
    }));
    rig.send(Command::Sampler(SamplerCommand::Trigger { slot: 0 }));
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
    rig.send(Command::Channel(0, ChannelCommand::SetColorFxAmount(-0.5)));
    rig.send(Command::Channel(
        1,
        ChannelCommand::SetEq {
            band: 0,
            value: -1.0,
        },
    ));

    // Warm-up block outside the guard (lazy statics in std, e.g. thread ids).
    rig.block(BLOCK);

    let script: Vec<Vec<Command>> = vec![
        vec![Command::Deck(0, DeckCommand::AutoLoop { beats: 1.0 / 8.0 })],
        vec![
            Command::Deck(0, DeckCommand::SetSlip(true)),
            Command::Deck(0, DeckCommand::LoopDouble),
        ],
        vec![Command::Deck(0, DeckCommand::LoopExit)],
        vec![Command::Deck(
            2,
            DeckCommand::LoopRoll {
                beats: 0.25,
                on: true,
            },
        )],
        vec![Command::Deck(
            2,
            DeckCommand::LoopRoll {
                beats: 0.25,
                on: false,
            },
        )],
        vec![
            Command::Deck(1, DeckCommand::JogTouch(true)),
            Command::Deck(1, DeckCommand::JogTurn(0.1)),
        ],
        vec![Command::Deck(1, DeckCommand::JogTouch(false))],
        vec![
            Command::Deck(3, DeckCommand::SetHotCue { slot: 2 }),
            Command::Deck(3, DeckCommand::BeatJump { beats: 8.0 }),
        ],
        vec![Command::Deck(
            3,
            DeckCommand::HotCue {
                slot: 2,
                pressed: true,
            },
        )],
        vec![Command::Deck(0, DeckCommand::SetReverse(true))],
        vec![
            Command::Deck(0, DeckCommand::SetReverse(false)),
            Command::Deck(0, DeckCommand::Load(replacement)),
        ],
        vec![
            Command::Deck(0, DeckCommand::Play),
            Command::Channel(2, ChannelCommand::SetColorFx(ColorFxKind::Space)),
        ],
        vec![
            Command::Channel(2, ChannelCommand::SetColorFxAmount(0.6)),
            Command::Mixer(MixerCommand::SetCrossfader(0.3)),
        ],
        vec![Command::BeatFx(0, BeatFxCommand::SetKind(BeatFxKind::Roll))],
        vec![
            Command::Deck(2, DeckCommand::SetKeyShift(2.0)),
            Command::Deck(1, DeckCommand::Cue(true)),
        ],
    ];
    let mut out_m = vec![0f32; 4096];
    let mut out_h = vec![0f32; 4096];
    for cmds in script {
        for c in cmds {
            sender.send(c).unwrap();
        }
        let n = guarded(|| {
            for _ in 0..40 {
                rig.proc.process(
                    BLOCK,
                    &mut Outputs {
                        master: &mut out_m[..BLOCK * 2],
                        headphones: Some(&mut out_h[..BLOCK * 2]),
                    },
                );
            }
            ALLOCS.swap(0, Ordering::SeqCst)
        });
        assert_eq!(n, 0, "process allocated or freed memory");
        tap.drain_into(&mut drained);
        if drained.len() > (1 << 19) {
            drained.clear();
        }
        rig.engine.collect_garbage();
    }
    // Odd block sizes and blocks larger than max_block.
    let n = guarded(|| {
        rig.proc.process(
            77,
            &mut Outputs {
                master: &mut out_m[..154],
                headphones: None,
            },
        );
        rig.proc.process(
            1500,
            &mut Outputs {
                master: &mut out_m[..3000],
                headphones: None,
            },
        );
        ALLOCS.swap(0, Ordering::SeqCst)
    });
    assert_eq!(n, 0);
    assert_eq!(rig.state().garbage_leaked, 0);
}
