//! CPU rendering of waveforms and the ground texture. Waveforms use the
//! three-band convention DJs read at a glance: each column draws the low band
//! (blue), then the mids (amber) and the highs (bone) on top, each at its own
//! amplitude, so kicks, basslines, vocals and hats separate visually.

use od_core::{Beatgrid, Phrase, PhraseKind, Waveform, WaveformColumn};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

const GROUND: [u8; 3] = [0x1A, 0x1A, 0x19];
/// Band colours: low ≈ 450 nm blue, mid = 589 nm amber, high = bone.
const LOW: [u8; 3] = [0x2F, 0x6F, 0xF2];
const MID: [u8; 3] = [0xE8, 0xA1, 0x3A];
const HIGH: [u8; 3] = [0xF2, 0xEE, 0xE4];
/// Colour-blind-safe bands (Okabe–Ito blue, orange, white).
const LOW_CB: [u8; 3] = [0x00, 0x72, 0xB2];
const MID_CB: [u8; 3] = [0xE6, 0x9F, 0x00];
const HIGH_CB: [u8; 3] = [0xFF, 0xFF, 0xFF];

static COLORBLIND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switches waveform bands to the colour-blind-safe palette.
pub fn set_colorblind_safe(on: bool) {
    COLORBLIND.store(on, std::sync::atomic::Ordering::Relaxed);
}
/// Beatgrid: beats, bar starts and 16-bar starts, drawn behind the waveform.
const BEAT: [u8; 3] = [0x5E, 0x5B, 0x55];
const DOWNBEAT: [u8; 3] = [0xB7, 0xB1, 0xA4];
const DOWNBEAT_MARK: [u8; 3] = [0xE6, 0xE0, 0xD2];
const PHRASE_LINE: [u8; 3] = [0xD8, 0xB0, 0x3E];
const PHRASE_MARK: [u8; 3] = [0xF5, 0xCE, 0x4A];
/// 589 nm, dimmed: phrase ticks on the overview.
const TICK: [u8; 3] = [0xB8, 0x9B, 0x3A];

fn bands() -> [[u8; 3]; 3] {
    if COLORBLIND.load(std::sync::atomic::Ordering::Relaxed) { [LOW_CB, MID_CB, HIGH_CB] } else { [LOW, MID, HIGH] }
}

/// Colour of a phrase kind (shared by the phrase bar in the UI).
pub fn phrase_rgb(kind: PhraseKind) -> [u8; 3] {
    match kind {
        PhraseKind::Intro | PhraseKind::Outro => [0x7C, 0x83, 0xFF],
        PhraseKind::Up => [0x82, 0xD1, 0x6B],
        PhraseKind::Down => [0x4F, 0xB8, 0xF0],
        PhraseKind::Chorus => [0xF0, 0x8A, 0x3C],
        PhraseKind::Verse => [0xE6, 0xE0, 0xD2],
        PhraseKind::Bridge => [0xB0, 0x7C, 0xF0],
    }
}

/// Label of a phrase kind as shown on the phrase bar.
pub fn phrase_label(kind: PhraseKind) -> &'static str {
    match kind {
        PhraseKind::Intro => "INTRO",
        PhraseKind::Verse => "VERSE",
        PhraseKind::Chorus => "CHORUS",
        PhraseKind::Bridge => "BRIDGE",
        PhraseKind::Up => "UP",
        PhraseKind::Down => "DOWN",
        PhraseKind::Outro => "OUTRO",
    }
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

fn dim(rgb: [u8; 3]) -> [u8; 3] {
    rgb.map(|c| (c as u16 * 2 / 5) as u8)
}

/// Draws one three-band column centred on `mid`; `amp` is the half height of
/// the loudest band.
fn column(buf: &mut SharedPixelBuffer<Rgba8Pixel>, x: u32, mid: u32, amp: u32, c: &WaveformColumn, played: bool) {
    let [lo, mi, hi] = bands();
    let top = c.low.max(c.mid).max(c.high).max(1) as f32;
    for (v, rgb) in [(c.low, lo), (c.mid, mi), (c.high, hi)] {
        let a = (v as f32 / top * amp as f32).round() as u32;
        if a == 0 {
            continue;
        }
        let rgb = if played { dim(rgb) } else { rgb };
        vline(buf, x, mid.saturating_sub(a), mid + a, rgb);
    }
}

/// Paints phrase colours as a strip of `strip` pixels along the bottom.
fn phrase_strip(buf: &mut SharedPixelBuffer<Rgba8Pixel>, phrases: &[Phrase], grid: &Beatgrid, x_of: impl Fn(f64) -> Option<u32>, strip: u32) {
    let (w, h) = (buf.width(), buf.height());
    for p in phrases {
        let (Some(a), Some(b)) = (grid.beats.get(p.start_beat as usize), grid.beats.get((p.end_beat as usize).min(grid.beats.len().saturating_sub(1)))) else {
            continue;
        };
        let (Some(x0), Some(x1)) = (x_of(a.time), x_of(b.time)) else { continue };
        let rgb = phrase_rgb(p.kind);
        for x in x0..x1.min(w) {
            vline(buf, x, h.saturating_sub(strip), h, rgb);
        }
        // A dark gap marks the phrase boundary.
        vline(buf, x0, h.saturating_sub(strip), h, GROUND);
    }
}

/// Overview of a whole track, `width` × `height` physical pixels, with
/// beatgrid ticks every 16 bars.
pub fn overview(w: &Waveform, grid: &Beatgrid, duration: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    // Ticks every 16 bars sit in a margin above and below the waveform.
    let margin = (height / 10).max(3);
    if duration > 0.0 {
        for b in grid.beats.iter().step_by(64) {
            let x = (b.time / duration * width as f64) as u32;
            vline(&mut buf, x, 0, margin - 1, TICK);
            vline(&mut buf, x, height - margin + 1, height, TICK);
        }
    }
    if w.columns.is_empty() {
        return Image::from_rgba8(buf);
    }
    let cols = w.overview(width.max(1) as usize);
    let max = cols.iter().map(|c| c.peak).max().unwrap_or(1).max(1) as f32;
    let mid = height / 2;
    for (i, c) in cols.iter().enumerate() {
        let amp = (c.peak as f32 / max * (height as f32 / 2.0 - margin as f32)).max(1.0) as u32;
        column(&mut buf, i as u32, mid, amp, c, false);
    }
    Image::from_rgba8(buf)
}

/// Small overview for list rows (transparent background).
pub fn preview(w: &Waveform, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    for p in buf.make_mut_slice() {
        *p = Rgba8Pixel { r: 0, g: 0, b: 0, a: 0 };
    }
    let cols = w.overview(width.max(1) as usize);
    let max = cols.iter().map(|c| c.peak).max().unwrap_or(1).max(1) as f32;
    let mid = height / 2;
    for (i, c) in cols.iter().enumerate() {
        let amp = (c.peak as f32 / max * (height as f32 / 2.0 - 1.0)).max(1.0) as u32;
        column(&mut buf, i as u32, mid, amp, c, false);
    }
    Image::from_rgba8(buf)
}

/// Scrolling detail waveform centred on `position` seconds, showing
/// `span` seconds in total, with beat ticks from `grid` and a phrase strip
/// along the bottom. Audio already played is drawn dimmed.
#[allow(clippy::too_many_arguments)]
pub fn detail(w: &Waveform, grid: &Beatgrid, phrases: &[Phrase], position: f64, span: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, GROUND);
    let start = position - span / 2.0;
    let secs_per_px = span / width.max(1) as f64;
    let strip = (height / 18).max(3);
    let wave_h = height - strip - 1;
    // Waveform amplitude leaves a band free for the grid markers.
    let mark_h = (wave_h / 9).max(4);
    let x_of = |t: f64| {
        let x = (t - start) / secs_per_px;
        Some(x.clamp(0.0, width as f64) as u32)
    };
    phrase_strip(&mut buf, phrases, grid, x_of, strip);
    let mid = wave_h / 2;
    let rate = w.columns_per_second;
    for x in 0..width {
        let t = start + x as f64 * secs_per_px;
        if t < 0.0 {
            continue;
        }
        let a = (t * rate) as usize;
        let b = (((t + secs_per_px) * rate) as usize).max(a + 1);
        if a >= w.columns.len() {
            continue;
        }
        let c = w.columns[a..b.min(w.columns.len())].iter().fold(WaveformColumn::default(), |acc, c| WaveformColumn {
            low: acc.low.max(c.low),
            mid: acc.mid.max(c.mid),
            high: acc.high.max(c.high),
            peak: acc.peak.max(c.peak),
        });
        let amp = (c.peak as f32 / 255.0 * (wave_h as f32 / 2.0 - mark_h as f32 - 2.0)).max(1.0) as u32;
        column(&mut buf, x, mid, amp, &c, t < position);
    }
    // Beatgrid on top of the waveform, as thin lines DJs read at a glance:
    // beats, bar starts with markers top and bottom, 16-bar starts in amber.
    if !grid.beats.is_empty() {
        let first = grid.beat_index_at(start.max(0.0)).unwrap_or(0);
        for (i, b) in grid.beats.iter().enumerate().skip(first) {
            if b.time > start + span {
                break;
            }
            if b.time < start {
                continue;
            }
            let x = ((b.time - start) / secs_per_px) as u32;
            let downbeat = b.bar_position == 1;
            let phrase = downbeat && (i / 4) % 16 == 0;
            let line = if phrase { PHRASE_LINE } else if downbeat { DOWNBEAT } else { BEAT };
            vline(&mut buf, x, 0, wave_h, line);
            if downbeat {
                // Marker at the top: a 3 px wide block, strongest at 16 bars.
                let mark = if phrase { PHRASE_MARK } else { DOWNBEAT_MARK };
                for dx in 0..3u32 {
                    vline(&mut buf, (x + dx).saturating_sub(1), 0, mark_h, mark);
                    vline(&mut buf, (x + dx).saturating_sub(1), wave_h - mark_h, wave_h, mark);
                }
            }
        }
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
        let d = detail(&w, &g, &[], 5.0, 8.0, 600, 100);
        assert_eq!(d.size().height, 100);
        assert_eq!(overview(&Waveform::default(), &g, 0.0, 10, 10).size().width, 10);
        assert_eq!(ground_texture().size().width, 640);
    }

    #[test]
    fn three_bands_stack_by_amplitude() {
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(1, 41);
        fill(&mut buf, GROUND);
        column(&mut buf, 0, 20, 20, &WaveformColumn { low: 200, mid: 100, high: 50, peak: 200 }, false);
        let at = |y: usize| { let p = buf.as_slice()[y]; [p.r, p.g, p.b] };
        // Outermost: only the low band reaches; then mids; the centre is highs.
        assert_eq!(at(1), LOW);
        assert_eq!(at(14), MID);
        assert_eq!(at(20), HIGH);
    }

    #[test]
    fn beatgrid_lines_sit_on_the_beats() {
        let w = Waveform { columns_per_second: 150.0, columns: vec![WaveformColumn::default(); 1500] };
        // 120 BPM: a beat every 0.5 s; 8 s across 400 px = 25 px per beat.
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let img = detail(&w, &g, &[], 4.0, 8.0, 400, 90);
        let buf = img.to_rgba8().unwrap();
        let px = |x: u32, y: u32| { let p = buf.as_slice()[(y * 400 + x) as usize]; [p.r, p.g, p.b] };
        // Beat 0 (bar 1, a 16-bar start) at x = 0; beat 8 (bar 3) centred at x = 200.
        assert_eq!(px(0, 40), PHRASE_LINE);
        assert_eq!(px(0, 2), PHRASE_MARK);
        assert_eq!(px(200, 40), DOWNBEAT);
        assert_eq!(px(200, 2), DOWNBEAT_MARK);
        assert_eq!(px(200, 82), DOWNBEAT_MARK);
        // Next beat (not a downbeat) 25 px later, full height.
        assert_eq!(px(225, 40), BEAT);
        assert_eq!(px(225, 75), BEAT);
        // Bar start one bar (100 px) later.
        assert_eq!(px(300, 40), DOWNBEAT);
        assert_eq!(px(300, 2), DOWNBEAT_MARK);
    }

    #[test]
    fn phrase_strip_is_painted() {
        let w = Waveform { columns_per_second: 150.0, columns: vec![WaveformColumn::default(); 1500] };
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let phrases = [Phrase { start_beat: 0, end_beat: 16, kind: PhraseKind::Chorus }];
        let img = detail(&w, &g, &phrases, 4.0, 8.0, 200, 90);
        let buf = img.to_rgba8().unwrap();
        let p = buf.as_slice()[(89 * 200 + 150) as usize];
        assert_eq!([p.r, p.g, p.b], phrase_rgb(PhraseKind::Chorus));
    }
}
