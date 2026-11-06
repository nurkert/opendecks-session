//! Raw bindings to the subset of `rubberband-c.h` (API 3.x) we use.
#![allow(dead_code)]

use std::os::raw::{c_double, c_float, c_int, c_uint, c_void};

pub type RubberBandState = *mut c_void;

pub const OPTION_PROCESS_REAL_TIME: c_int = 0x0000_0001;
pub const OPTION_TRANSIENTS_MIXED: c_int = 0x0000_0100;
pub const OPTION_THREADING_NEVER: c_int = 0x0001_0000;
pub const OPTION_WINDOW_SHORT: c_int = 0x0010_0000;
pub const OPTION_PITCH_HIGH_QUALITY: c_int = 0x0200_0000;
pub const OPTION_PITCH_HIGH_CONSISTENCY: c_int = 0x0400_0000;
pub const OPTION_CHANNELS_TOGETHER: c_int = 0x1000_0000;
pub const OPTION_ENGINE_FASTER: c_int = 0x0000_0000;
pub const OPTION_ENGINE_FINER: c_int = 0x2000_0000;

unsafe extern "C" {
    pub fn rubberband_new(
        sample_rate: c_uint,
        channels: c_uint,
        options: c_int,
        initial_time_ratio: c_double,
        initial_pitch_scale: c_double,
    ) -> RubberBandState;
    pub fn rubberband_delete(state: RubberBandState);
    pub fn rubberband_reset(state: RubberBandState);
    pub fn rubberband_set_time_ratio(state: RubberBandState, ratio: c_double);
    pub fn rubberband_set_pitch_scale(state: RubberBandState, scale: c_double);
    pub fn rubberband_get_preferred_start_pad(state: RubberBandState) -> c_uint;
    pub fn rubberband_get_start_delay(state: RubberBandState) -> c_uint;
    pub fn rubberband_get_latency(state: RubberBandState) -> c_uint;
    pub fn rubberband_set_max_process_size(state: RubberBandState, samples: c_uint);
    pub fn rubberband_get_samples_required(state: RubberBandState) -> c_uint;
    pub fn rubberband_process(
        state: RubberBandState,
        input: *const *const c_float,
        samples: c_uint,
        final_: c_int,
    );
    pub fn rubberband_available(state: RubberBandState) -> c_int;
    pub fn rubberband_retrieve(
        state: RubberBandState,
        output: *const *mut c_float,
        samples: c_uint,
    ) -> c_uint;
    pub fn rubberband_set_default_debug_level(level: c_int);
}
