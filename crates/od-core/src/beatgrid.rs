use serde::{Deserialize, Serialize};

/// One beat of a beatgrid.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    /// Position in seconds.
    pub time: f64,
    /// Tempo from this beat to the next one.
    pub bpm: f64,
    /// Position inside the bar, 1..=4 (1 is the downbeat).
    pub bar_position: u8,
}

/// A beatgrid as an explicit list of beats. Supports variable tempo.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Beatgrid {
    pub beats: Vec<Beat>,
    /// A locked grid is never replaced by analysis.
    pub locked: bool,
}

impl Beatgrid {
    /// Builds a constant-tempo grid. `first_beat` is the time of a beat with
    /// bar position `first_bar_position`; beats are extended backwards to the
    /// start of the track and forwards to `duration`.
    pub fn constant(bpm: f64, first_beat: f64, first_bar_position: u8, duration: f64) -> Self {
        assert!(bpm > 0.0, "bpm must be positive");
        let period = 60.0 / bpm;
        let steps_back = (first_beat / period).floor() as i64;
        let start = first_beat - steps_back as f64 * period;
        let start_bar = (first_bar_position as i64 - 1 - steps_back).rem_euclid(4);
        let mut beats = Vec::new();
        let mut i: i64 = 0;
        loop {
            let time = start + i as f64 * period;
            if time > duration {
                break;
            }
            beats.push(Beat {
                time,
                bpm,
                bar_position: ((start_bar + i).rem_euclid(4) + 1) as u8,
            });
            i += 1;
        }
        Self { beats, locked: false }
    }

    pub fn is_empty(&self) -> bool {
        self.beats.is_empty()
    }

    /// Tempo at the first beat, or `None` for an empty grid.
    pub fn first_bpm(&self) -> Option<f64> {
        self.beats.first().map(|b| b.bpm)
    }

    /// Most common tempo of the grid weighted by duration.
    pub fn dominant_bpm(&self) -> Option<f64> {
        if self.beats.len() < 2 {
            return self.first_bpm();
        }
        let mut buckets: Vec<(i64, f64)> = Vec::new();
        for w in self.beats.windows(2) {
            let key = (w[0].bpm * 100.0).round() as i64;
            let dur = w[1].time - w[0].time;
            match buckets.iter_mut().find(|(k, _)| *k == key) {
                Some((_, d)) => *d += dur,
                None => buckets.push((key, dur)),
            }
        }
        buckets
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k as f64 / 100.0)
    }

    /// Index of the last beat at or before `time`.
    pub fn beat_index_at(&self, time: f64) -> Option<usize> {
        match self.beats.partition_point(|b| b.time <= time) {
            0 => None,
            n => Some(n - 1),
        }
    }

    /// Fractional beat number at `time` (0.0 at the first beat). Extrapolates
    /// outside the grid with the nearest tempo.
    pub fn beat_position(&self, time: f64) -> Option<f64> {
        let first = self.beats.first()?;
        match self.beat_index_at(time) {
            None => Some((time - first.time) * first.bpm / 60.0),
            Some(i) => {
                let b = &self.beats[i];
                let period = match self.beats.get(i + 1) {
                    Some(next) => next.time - b.time,
                    None => 60.0 / b.bpm,
                };
                Some(i as f64 + (time - b.time) / period)
            }
        }
    }

    /// Time of a fractional beat number, inverse of [`Self::beat_position`].
    pub fn time_of_beat(&self, beat: f64) -> Option<f64> {
        let first = self.beats.first()?;
        if beat < 0.0 {
            return Some(first.time + beat * 60.0 / first.bpm);
        }
        let i = beat.floor() as usize;
        match self.beats.get(i) {
            Some(b) => {
                let period = match self.beats.get(i + 1) {
                    Some(next) => next.time - b.time,
                    None => 60.0 / b.bpm,
                };
                Some(b.time + beat.fract() * period)
            }
            None => {
                let last = self.beats.last()?;
                let extra = beat - (self.beats.len() - 1) as f64;
                Some(last.time + extra * 60.0 / last.bpm)
            }
        }
    }

    /// Moves every beat by `offset` seconds.
    pub fn shift(&mut self, offset: f64) {
        for b in &mut self.beats {
            b.time += offset;
        }
    }

    /// Makes the beat at index `i` a downbeat and renumbers all bars.
    pub fn set_downbeat(&mut self, i: usize) {
        for (j, b) in self.beats.iter_mut().enumerate() {
            b.bar_position = ((j as i64 - i as i64).rem_euclid(4) + 1) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_grid_extends_backwards() {
        let g = Beatgrid::constant(120.0, 1.25, 1, 10.0);
        assert!((g.beats[0].time - 0.25).abs() < 1e-9);
        assert_eq!(g.beats[0].bar_position, 3);
        assert_eq!(g.beats[2].bar_position, 1);
        assert!((g.beats[2].time - 1.25).abs() < 1e-9);
        assert_eq!(g.beats.len(), 20);
    }

    #[test]
    fn beat_position_roundtrip() {
        let g = Beatgrid::constant(128.0, 0.1, 1, 60.0);
        for t in [0.0, 0.1, 3.3, 59.0, 70.0] {
            let b = g.beat_position(t).unwrap();
            assert!((g.time_of_beat(b).unwrap() - t).abs() < 1e-9, "t={t}");
        }
        assert_eq!(g.dominant_bpm(), Some(128.0));
    }

    #[test]
    fn set_downbeat_renumbers() {
        let mut g = Beatgrid::constant(120.0, 0.0, 1, 4.0);
        g.set_downbeat(1);
        let bars: Vec<u8> = g.beats.iter().map(|b| b.bar_position).collect();
        assert_eq!(&bars[..6], &[4, 1, 2, 3, 4, 1]);
    }
}
