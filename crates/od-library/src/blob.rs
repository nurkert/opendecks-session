//! Compact binary encodings for beatgrids, waveforms and energy curves.

use od_core::{Beat, Beatgrid, Waveform, WaveformColumn};

const BEAT_SIZE: usize = 17;

pub fn encode_grid(grid: &Beatgrid) -> Vec<u8> {
    let mut out = Vec::with_capacity(grid.beats.len() * BEAT_SIZE);
    for b in &grid.beats {
        out.extend_from_slice(&b.time.to_le_bytes());
        out.extend_from_slice(&b.bpm.to_le_bytes());
        out.push(b.bar_position);
    }
    out
}

pub fn decode_grid(bytes: &[u8], locked: bool) -> Beatgrid {
    let beats = bytes
        .chunks_exact(BEAT_SIZE)
        .map(|c| Beat {
            time: f64::from_le_bytes(c[0..8].try_into().unwrap()),
            bpm: f64::from_le_bytes(c[8..16].try_into().unwrap()),
            bar_position: c[16],
        })
        .collect();
    Beatgrid { beats, locked }
}

pub fn encode_waveform(w: &Waveform) -> Vec<u8> {
    w.columns.iter().flat_map(|c| [c.low, c.mid, c.high, c.peak]).collect()
}

pub fn decode_waveform(bytes: &[u8], rate: f64) -> Waveform {
    Waveform {
        columns_per_second: rate,
        columns: bytes
            .chunks_exact(4)
            .map(|c| WaveformColumn { low: c[0], mid: c[1], high: c[2], peak: c[3] })
            .collect(),
    }
}

pub fn encode_f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

pub fn decode_f32s(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips() {
        let g = Beatgrid::constant(127.3, 0.5, 2, 30.0);
        assert_eq!(decode_grid(&encode_grid(&g), false), g);
        let w = Waveform {
            columns_per_second: 150.0,
            columns: vec![WaveformColumn { low: 1, mid: 2, high: 3, peak: 4 }; 3],
        };
        assert_eq!(decode_waveform(&encode_waveform(&w), 150.0), w);
        assert_eq!(decode_f32s(&encode_f32s(&[0.5, 1.0])), vec![0.5, 1.0]);
    }
}
