//! Engine configuration.

/// Performance profile (SPEC §3.2). Only the keylock quality and the size of
/// the FX set differ; the signal path is otherwise identical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Profile {
    /// Raspberry Pi / weak hardware: Rubber Band "faster" engine.
    Lite,
    /// Desktop: Rubber Band R3 "finer" engine.
    #[default]
    Full,
}

/// Static engine configuration, fixed for the lifetime of an engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    /// Engine (and device) sample rate in Hz. Tracks are resampled to it at
    /// load time.
    pub sample_rate: u32,
    /// Largest block the processor is asked to render at once. Larger
    /// requests are split internally.
    pub max_block: usize,
    /// Number of decks / mixer channels: 2 or 4.
    pub decks: usize,
    pub profile: Profile,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            max_block: 512,
            decks: 4,
            profile: Profile::Full,
        }
    }
}

impl EngineConfig {
    pub(crate) fn validated(mut self) -> Self {
        self.decks = if self.decks <= 2 { 2 } else { MAX_DECKS };
        self.max_block = self.max_block.clamp(16, 8192);
        self.sample_rate = self.sample_rate.clamp(8_000, 192_000);
        self
    }
}

/// Upper bound of decks/channels; state arrays are always this long.
pub const MAX_DECKS: usize = 4;
/// Number of hot cue slots (A–H) per bank.
pub const HOT_CUES: usize = 8;
/// Number of hot cue banks per deck (bank `b`, slot `s` is library hot cue
/// index `b * HOT_CUES + s`).
pub const HOT_CUE_BANKS: usize = 4;
/// Number of sampler slots per bank (one pad page).
pub const SAMPLER_SLOTS: usize = 16;
/// Number of sampler banks.
pub const SAMPLER_BANKS: usize = 4;
/// Total sampler slots addressable by [`crate::SamplerCommand`]: slot
/// `bank * SAMPLER_SLOTS + pad`.
pub const SAMPLER_TOTAL: usize = SAMPLER_SLOTS * SAMPLER_BANKS;
/// Steps of the sampler step sequencer (one bar of 16th notes).
pub const SEQ_STEPS: usize = 16;
/// Slices of the slicer domain (one per pad).
pub const SLICER_SLICES: usize = 8;
/// Number of Beat FX units.
pub const BEAT_FX_UNITS: usize = 2;
/// Effect slots of a Beat FX unit in multi mode (slot 0 is the unit itself).
pub const BEAT_FX_SLOTS: usize = 3;
/// Maximum number of additional command senders (see [`crate::Engine::new_sender`]).
pub const MAX_SENDERS: usize = 8;
