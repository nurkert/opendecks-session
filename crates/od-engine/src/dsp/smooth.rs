//! Parameter smoothing to avoid zipper noise.

/// A linearly ramped parameter. Each call to [`Smoothed::next`] moves the
/// current value one step towards the target; the ramp length is fixed in
/// samples so that a full-scale change takes `ramp` samples.
#[derive(Clone, Copy, Debug)]
pub struct Smoothed {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp: u32,
}

impl Smoothed {
    pub fn new(value: f32, ramp_samples: u32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp: ramp_samples.max(1),
        }
    }

    pub fn set(&mut self, target: f32) {
        if target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp;
        self.step = (target - self.current) / self.ramp as f32;
    }

    /// Jumps to `value` without ramping.
    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 {
                self.target
            } else {
                self.current + self.step
            };
        }
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_linearly_to_target() {
        let mut s = Smoothed::new(0.0, 4);
        s.set(1.0);
        let v: Vec<f32> = (0..6).map(|_| s.next()).collect();
        assert_eq!(v, vec![0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
        assert!(s.is_settled());
    }
}
