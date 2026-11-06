//! Keylock (tempo change without pitch change) through the Rubber Band
//! library in real-time mode.
//!
//! [`Stretcher`] is a minimal safe wrapper over `rubberband-c.h`. It is created
//! off the real-time thread (construction allocates); `process`, `retrieve`,
//! `set_time_ratio` and `reset` are real-time safe once
//! `set_max_process_size` has been applied, which [`Stretcher::new`] does.
//!
//! Without the `rubberband` cargo feature, [`Stretcher::new`] returns `None`
//! and decks fall back to plain variable-rate playback.

use crate::config::Profile;

#[cfg(feature = "rubberband")]
mod ffi;

/// Largest number of input frames passed to one `process` call.
pub const MAX_FEED: usize = 4096;

pub struct Stretcher {
    #[cfg(feature = "rubberband")]
    state: ffi::RubberBandState,
    time_ratio: f64,
}

// SAFETY: a RubberBandState is only ever used from one thread at a time; it is
// moved, not shared, between the control and the audio thread.
unsafe impl Send for Stretcher {}

impl Stretcher {
    /// Whether keylock is compiled in.
    pub const AVAILABLE: bool = cfg!(feature = "rubberband");

    /// Creates a stereo real-time stretcher. `Full` uses the R3 ("finer")
    /// engine, `Lite` the cheaper R2 ("faster") engine with short windows.
    #[cfg(feature = "rubberband")]
    pub fn new(sample_rate: u32, profile: Profile) -> Option<Self> {
        use ffi::*;
        let common = OPTION_PROCESS_REAL_TIME | OPTION_THREADING_NEVER | OPTION_CHANNELS_TOGETHER;
        let options = match profile {
            Profile::Full => common | OPTION_ENGINE_FINER | OPTION_PITCH_HIGH_CONSISTENCY,
            Profile::Lite => {
                common | OPTION_ENGINE_FASTER | OPTION_TRANSIENTS_MIXED | OPTION_WINDOW_SHORT
            }
        };
        // SAFETY: plain constructor call with valid arguments.
        let state = unsafe {
            rubberband_set_default_debug_level(0);
            rubberband_new(sample_rate, 2, options, 1.0, 1.0)
        };
        if state.is_null() {
            return None;
        }
        // SAFETY: state is a valid, freshly created stretcher.
        unsafe { rubberband_set_max_process_size(state, MAX_FEED as u32) };
        Some(Self {
            state,
            time_ratio: 1.0,
        })
    }

    #[cfg(not(feature = "rubberband"))]
    pub fn new(_sample_rate: u32, _profile: Profile) -> Option<Self> {
        None
    }

    /// Output duration / input duration (1 / playback speed).
    pub fn set_time_ratio(&mut self, ratio: f64) {
        if (ratio - self.time_ratio).abs() < 1e-9 {
            return;
        }
        self.time_ratio = ratio;
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state; real-time safe in RT mode.
        unsafe {
            ffi::rubberband_set_time_ratio(self.state, ratio)
        };
    }

    pub fn time_ratio(&self) -> f64 {
        self.time_ratio
    }

    /// Pitch scale factor (1.0 = unchanged); used for key shifting.
    pub fn set_pitch_scale(&mut self, _scale: f64) {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        unsafe {
            ffi::rubberband_set_pitch_scale(self.state, _scale)
        };
    }

    pub fn reset(&mut self) {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        unsafe {
            ffi::rubberband_reset(self.state)
        };
    }

    /// Frames of input to feed before the first real input after a reset.
    pub fn preferred_start_pad(&self) -> usize {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        return unsafe { ffi::rubberband_get_preferred_start_pad(self.state) as usize };
        #[cfg(not(feature = "rubberband"))]
        0
    }

    /// Output frames to discard after a reset (when the start pad was fed).
    pub fn start_delay(&self) -> usize {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        return unsafe { ffi::rubberband_get_start_delay(self.state) as usize };
        #[cfg(not(feature = "rubberband"))]
        0
    }

    pub fn samples_required(&self) -> usize {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        return unsafe { ffi::rubberband_get_samples_required(self.state) as usize };
        #[cfg(not(feature = "rubberband"))]
        0
    }

    pub fn available(&self) -> usize {
        #[cfg(feature = "rubberband")]
        // SAFETY: valid state.
        return unsafe { ffi::rubberband_available(self.state).max(0) as usize };
        #[cfg(not(feature = "rubberband"))]
        0
    }

    /// Feeds de-interleaved input. `left.len()` must equal `right.len()` and
    /// be at most [`MAX_FEED`].
    pub fn process(&mut self, left: &[f32], right: &[f32]) {
        let n = left.len().min(right.len()).min(MAX_FEED);
        if n == 0 {
            return;
        }
        #[cfg(feature = "rubberband")]
        {
            let ptrs = [left.as_ptr(), right.as_ptr()];
            // SAFETY: two valid channel pointers with at least n frames each.
            unsafe { ffi::rubberband_process(self.state, ptrs.as_ptr(), n as u32, 0) };
        }
    }

    /// Retrieves up to `left.len()` output frames, returns the count.
    pub fn retrieve(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        let n = left.len().min(right.len());
        if n == 0 {
            return 0;
        }
        #[cfg(feature = "rubberband")]
        {
            let ptrs = [left.as_mut_ptr(), right.as_mut_ptr()];
            // SAFETY: two valid, writable channel pointers with n frames each.
            unsafe { ffi::rubberband_retrieve(self.state, ptrs.as_ptr(), n as u32) as usize }
        }
        #[cfg(not(feature = "rubberband"))]
        0
    }
}

impl Drop for Stretcher {
    fn drop(&mut self) {
        #[cfg(feature = "rubberband")]
        // SAFETY: state was created by rubberband_new and is dropped once.
        unsafe {
            ffi::rubberband_delete(self.state)
        };
    }
}

#[cfg(all(test, feature = "rubberband"))]
mod tests {
    use super::*;

    #[test]
    fn stretches_by_ratio() {
        let mut s = Stretcher::new(48_000, Profile::Full).unwrap();
        s.set_time_ratio(2.0);
        let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.05).sin()).collect();
        let mut out = vec![0f32; 4096];
        let mut out_r = vec![0f32; 4096];
        let mut produced = 0;
        for _ in 0..100 {
            s.process(&input, &input);
            produced += s.retrieve(&mut out, &mut out_r);
        }
        let expected = 100.0 * 1024.0 * 2.0;
        assert!((produced as f64) > expected * 0.8, "produced {produced}");
    }
}
