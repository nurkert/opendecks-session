//! CPU rendering of waveforms into images, in the emission-line style:
//! each column is a hairline whose colour mixes the three band lines and
//! whose height is the peak. Gaps between columns keep the line character.

use od_core::{Beatgrid, Waveform, WaveformColumn};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const GROUND: [u8; 3] = [0x18, 0x19, 0x1C];
const LOW: [f32; 3] = [243.0, 144.0, 71.0]; // 615 nm
const MID: [f32; 3] = [130.0, 209.0, 107.0]; // 546 nm
const HIGH: [f32; 3] = [79.0, 184.0, 240.0]; // 486 nm
const BEAT: [u8; 3] = [0x4A, 0x4D, 0x53];
const DOWNBEAT: [u8; 3] = [0x8B, 0x86, 0x7B];

fn mix(c: &WaveformColumn) -> [u8; 3] {
    let (l, m, h) = (c.low as f32, c.mid as f32, c.high as f32);
    let sum = (l + m + h).max(1.0);
    let mut out = [0u8; 3];
    for i in 0..3 {
        // Lift towards bone so that quiet mixes stay readable.
        let v = (LOW[i] * l + MID[i] * m + HIGH[i] * h) / sum;
        out[i] = (v * 0.85 + 230.0 * 0.15).min(255.0) as u8;
    }
    out
}

fn fill(buf: &mut SharedPixelBuffer<Rgba8Pixel>, rgb: [u8; 3]) {
    for p in buf.make_mut_slice() {
        *p = Rgba8Pixel { r: rgb[0], g: rgb[1], b: rgb[2], a: 255 };
    }
}

fn vline(buf: &mut SharedPixelBuffer<Rgba8Pixel>, x: u32, y0: u32, y1: u32, rgb: [u8; 3], alpha: f32) {
    let (w, h) = (buf.width(), buf.height());
    if x >= w {
        return;
    }
    let px = buf.make_mut_slice();
    for y in y0.min(h)..y1.min(h) {
        let p = &mut px[(y * w + x) as usize];
        p.r = (p.r as f32 * (1.0 - alpha) + rgb[0] as f32 * alpha) as u8;
        p.g = (p.g as f32 * (1.0 - alpha) + rgb[1] as f32 * alpha) as u8;
        p.b = (p.b as f32 * (1.0 - alpha) + rgb[2] as f32 * alpha) as u8;
    }
}

/// Overview of a whole track, `width` × `height` physical pixels.
pub fn overview(w: &Waveform, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    if w.columns.is_empty() {
        return Image::from_rgba8(buf);
    }
    // One emission line every 2 px.
    let lines = (width / 2).max(1) as usize;
    let cols = w.overview(lines);
    let max = cols.iter().map(|c| c.peak).max().unwrap_or(1).max(1) as f32;
    let mid = height / 2;
    for (i, c) in cols.iter().enumerate() {
        let amp = (c.peak as f32 / max * (height as f32 / 2.0 - 1.0)).max(1.0) as u32;
        vline(&mut buf, i as u32 * 2, mid.saturating_sub(amp), mid + amp, mix(c), 1.0);
    }
    Image::from_rgba8(buf)
}

/// Scrolling detail waveform centred on `position` seconds, showing
/// `span` seconds in total, with beat ticks from `grid`.
pub fn detail(w: &Waveform, grid: &Beatgrid, position: f64, span: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    let start = position - span / 2.0;
    let secs_per_px = span / width.max(1) as f64;
    // Beat ticks first, so the waveform draws over them.
    if let (Some(first), Some(_)) = (grid.beat_index_at(start.max(0.0)).or(Some(0)), grid.beats.first()) {
        for b in grid.beats.iter().skip(first) {
            if b.time > start + span {
                break;
            }
            if b.time < start {
                continue;
            }
            let x = ((b.time - start) / secs_per_px) as u32;
            let (rgb, len) = if b.bar_position == 1 { (DOWNBEAT, height) } else { (BEAT, height / 5) };
            vline(&mut buf, x, 0, len, rgb, 1.0);
            vline(&mut buf, x, height - len, height, rgb, 1.0);
        }
    }
    if w.columns.is_empty() {
        return Image::from_rgba8(buf);
    }
    let mid = height / 2;
    let rate = w.columns_per_second;
    let mut x = 0;
    while x < width {
        let t = start + x as f64 * secs_per_px;
        if t >= 0.0 {
            let a = (t * rate) as usize;
            let b = (((t + secs_per_px * 2.0) * rate) as usize).max(a + 1);
            if a < w.columns.len() {
                let c = w.columns[a..b.min(w.columns.len())].iter().fold(WaveformColumn::default(), |acc, c| WaveformColumn {
                    low: acc.low.max(c.low),
                    mid: acc.mid.max(c.mid),
                    high: acc.high.max(c.high),
                    peak: acc.peak.max(c.peak),
                });
                let amp = (c.peak as f32 / 255.0 * (height as f32 / 2.0 - 1.0)).max(1.0) as u32;
                vline(&mut buf, x, mid.saturating_sub(amp), mid + amp, mix(&c), 1.0);
            }
        }
        x += 2;
    }
    Image::from_rgba8(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sizes() {
        let w = Waveform {
            columns_per_second: 150.0,
            columns: vec![WaveformColumn { low: 200, mid: 100, high: 50, peak: 200 }; 1500],
        };
        let img = overview(&w, 400, 80);
        assert_eq!(img.size().width, 400);
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let d = detail(&w, &g, 5.0, 8.0, 600, 100);
        assert_eq!(d.size().height, 100);
        let empty = overview(&Waveform::default(), 10, 10);
        assert_eq!(empty.size().width, 10);
    }
}
