//! Stereo delay line with fractional (linear-interpolated) reads.

pub struct DelayLine {
    buf: Vec<[f32; 2]>,
    write: usize,
}

impl DelayLine {
    /// Allocates a delay line holding up to `max_frames` frames.
    pub fn new(max_frames: usize) -> Self {
        Self {
            buf: vec![[0.0; 2]; max_frames.max(4)],
            write: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len() - 2
    }

    #[inline]
    pub fn push(&mut self, frame: [f32; 2]) {
        self.buf[self.write] = frame;
        self.write += 1;
        if self.write == self.buf.len() {
            self.write = 0;
        }
    }

    /// Reads the frame written `delay` frames ago (1.0 = previous frame).
    #[inline]
    pub fn read(&self, delay: f32) -> [f32; 2] {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let di = d.floor();
        let frac = d - di;
        let i0 = (self.write + len - di as usize) % len;
        let i1 = (i0 + len - 1) % len;
        let a = self.buf[i0];
        let b = self.buf[i1];
        [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|f| *f = [0.0; 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_delay() {
        let mut d = DelayLine::new(16);
        for i in 0..10 {
            d.push([i as f32, -(i as f32)]);
        }
        assert_eq!(d.read(1.0), [9.0, -9.0]);
        assert_eq!(d.read(3.0), [7.0, -7.0]);
        assert_eq!(d.read(2.5), [7.5, -7.5]);
    }
}
