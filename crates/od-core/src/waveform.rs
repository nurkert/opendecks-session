use serde::{Deserialize, Serialize};

/// One column of a three-band waveform. Each value is a peak amplitude scaled
/// to 0..=255.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformColumn {
    pub low: u8,
    pub mid: u8,
    pub high: u8,
    /// Full-band peak.
    pub peak: u8,
}

/// A waveform at a fixed number of columns per second.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Waveform {
    pub columns_per_second: f64,
    pub columns: Vec<WaveformColumn>,
}

impl Waveform {
    /// Columns per second of the detail waveform (matches the 150 Hz
    /// resolution used by the Pioneer detail waveform).
    pub const DETAIL_RATE: f64 = 150.0;

    /// Downsamples into exactly `width` columns by taking the maximum of each
    /// covered range.
    pub fn overview(&self, width: usize) -> Vec<WaveformColumn> {
        if width == 0 || self.columns.is_empty() {
            return vec![WaveformColumn::default(); width];
        }
        let n = self.columns.len();
        (0..width)
            .map(|i| {
                let a = i * n / width;
                let b = ((i + 1) * n / width).max(a + 1).min(n);
                self.columns[a..b]
                    .iter()
                    .fold(WaveformColumn::default(), |acc, c| WaveformColumn {
                        low: acc.low.max(c.low),
                        mid: acc.mid.max(c.mid),
                        high: acc.high.max(c.high),
                        peak: acc.peak.max(c.peak),
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_takes_maxima() {
        let w = Waveform {
            columns_per_second: 1.0,
            columns: (0..10)
                .map(|i| WaveformColumn {
                    low: i,
                    mid: 0,
                    high: 0,
                    peak: i,
                })
                .collect(),
        };
        let o = w.overview(5);
        assert_eq!(
            o.iter().map(|c| c.peak).collect::<Vec<_>>(),
            vec![1, 3, 5, 7, 9]
        );
        assert_eq!(w.overview(20).len(), 20);
    }
}
