//! CPU rendering of waveforms and the ground texture, in the emission-line
//! style: amplitude is drawn as bone hairline columns with charcoal gaps;
//! colour appears only as a pure-wavelength 1px mark on columns where one
//! frequency band clearly dominates.

use od_core::{Beatgrid, Waveform, WaveformColumn};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const GROUND: [u8; 3] = [0x1A, 0x1A, 0x19];
const BONE: [u8; 3] = [0xB7, 0xB1, 0xA4];
const BONE_DIM: [u8; 3] = [0x6E, 0x6A, 0x62];
/// Band marks: low = 656 nm, mid = 546 nm, high = 486 nm.
const LOW: [u8; 3] = [0xEF, 0x5A, 0x5A];
const MID: [u8; 3] = [0x82, 0xD1, 0x6B];
const HIGH: [u8; 3] = [0x4F, 0xB8, 0xF0];
const BEAT: [u8; 3] = [0x3A, 0x39, 0x36];
const DOWNBEAT: [u8; 3] = [0x8B, 0x86, 0x7B];

/// The pure-wavelength mark of a column whose energy is dominated by one band.
fn band_mark(c: &WaveformColumn) -> Option<[u8; 3]> {
    let (l, m, h) = (c.low as f32, c.mid as f32, c.high as f32);
    let sum = l + m + h;
    if sum < 30.0 {
        return None;
    }
    let (best, share) = [(LOW, l), (MID, m), (HIGH, h)]
        .into_iter()
        .map(|(rgb, v)| (rgb, v / sum))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (share > 0.55).then_some(best)
}

fn fill(buf: &mut SharedPixelBuffer<Rgba8Pixel>, rgb: [u8; 3]) {
    for p in buf.make_mut_slice() {
        *p = Rgba8Pixel { r: rgb[0], g: rgb[1], b: rgb[2], a: 255 };
    }
}

fn vline(buf: &mut SharedPixelBuffer<Rgba8Pixel>, x: u32, y0: u32, y1: u32, rgb: [u8; 3]) {
    let (w, h) = (buf.width(), buf.height());
    if x >= w {
        return;
    }
    let px = buf.make_mut_slice();
    for y in y0.min(h)..y1.min(h) {
        px[(y * w + x) as usize] = Rgba8Pixel { r: rgb[0], g: rgb[1], b: rgb[2], a: 255 };
    }
}

/// Draws one amplitude column centred on `mid` plus its band mark at the
/// top of the column.
fn column(buf: &mut SharedPixelBuffer<Rgba8Pixel>, x: u32, mid: u32, amp: u32, c: &WaveformColumn, dim: bool) {
    let top = mid.saturating_sub(amp);
    vline(buf, x, top, mid + amp, if dim { BONE_DIM } else { BONE });
    if let Some(mark) = band_mark(c) {
        vline(buf, x, top, (top + 2).min(mid + amp), mark);
    }
}

/// Overview of a whole track, `width` × `height` physical pixels, with
/// beatgrid ticks every 16 bars.
pub fn overview(w: &Waveform, grid: &Beatgrid, duration: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    if duration > 0.0 {
        for b in grid.beats.iter().step_by(64) {
            let x = (b.time / duration * width as f64) as u32;
            vline(&mut buf, x, 0, height / 8, DOWNBEAT);
            vline(&mut buf, x, height - height / 8, height, DOWNBEAT);
        }
    }
    if w.columns.is_empty() {
        return Image::from_rgba8(buf);
    }
    let lines = (width / 2).max(1) as usize;
    let cols = w.overview(lines);
    let max = cols.iter().map(|c| c.peak).max().unwrap_or(1).max(1) as f32;
    let mid = height / 2;
    for (i, c) in cols.iter().enumerate() {
        let amp = (c.peak as f32 / max * (height as f32 / 2.0 - 2.0)).max(1.0) as u32;
        column(&mut buf, i as u32 * 2, mid, amp, c, false);
    }
    Image::from_rgba8(buf)
}

/// Scrolling detail waveform centred on `position` seconds, showing
/// `span` seconds in total, with beat ticks from `grid`. Audio already
/// played is drawn dimmed.
pub fn detail(w: &Waveform, grid: &Beatgrid, position: f64, span: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    let start = position - span / 2.0;
    let secs_per_px = span / width.max(1) as f64;
    if !grid.beats.is_empty() {
        let first = grid.beat_index_at(start.max(0.0)).unwrap_or(0);
        for b in grid.beats.iter().skip(first) {
            if b.time > start + span {
                break;
            }
            if b.time < start {
                continue;
            }
            let x = ((b.time - start) / secs_per_px) as u32;
            let (rgb, len) = if b.bar_position == 1 { (DOWNBEAT, height / 4) } else { (BEAT, height / 8) };
            vline(&mut buf, x, 0, len, rgb);
            vline(&mut buf, x, height - len, height, rgb);
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
                let amp = (c.peak as f32 / 255.0 * (height as f32 / 2.0 - 2.0)).max(1.0) as u32;
                column(&mut buf, x, mid, amp, &c, t < position);
            }
        }
        x += 2;
    }
    Image::from_rgba8(buf)
}

/// The charcoal continuum: a faint, deterministic vertical banding drawn
/// once at start-up and stretched over the window.
pub fn ground_texture() -> Image {
    let (w, h) = (640u32, 4u32);
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let px = buf.make_mut_slice();
    let mut seed: u32 = 0x9E37_79B9;
    let mut level = 0i32;
    for x in 0..w {
        // Slow random walk in ±3 levels around the ground value.
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        if seed % 9 == 0 {
            level = (level + if seed & 0x100 == 0 { 1 } else { -1 }).clamp(-3, 3);
        }
        let v = |base: i32| (base + level).clamp(0, 255) as u8;
        for y in 0..h {
            px[(y * w + x) as usize] = Rgba8Pixel { r: v(0x15), g: v(0x15), b: v(0x14), a: 255 };
        }
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
            columns: vec![WaveformColumn { low: 200, mid: 20, high: 10, peak: 200 }; 1500],
        };
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let img = overview(&w, &g, 10.0, 400, 80);
        assert_eq!(img.size().width, 400);
        let d = detail(&w, &g, 5.0, 8.0, 600, 100);
        assert_eq!(d.size().height, 100);
        assert_eq!(overview(&Waveform::default(), &g, 0.0, 10, 10).size().width, 10);
        assert_eq!(ground_texture().size().width, 640);
    }

    #[test]
    fn band_marks_only_for_dominant_bands() {
        assert_eq!(band_mark(&WaveformColumn { low: 200, mid: 20, high: 10, peak: 0 }), Some(LOW));
        assert_eq!(band_mark(&WaveformColumn { low: 100, mid: 100, high: 100, peak: 0 }), None);
        assert_eq!(band_mark(&WaveformColumn { low: 5, mid: 5, high: 5, peak: 0 }), None);
    }
}
