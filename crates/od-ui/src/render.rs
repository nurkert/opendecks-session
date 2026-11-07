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

/// Waveform style: 0 = three bands, 1 = blue, 2 = RGB.
static STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Waveform style by its settings name ("3band", "blue", "rgb").
pub fn set_waveform_style(name: &str) {
    let v = match name {
        "blue" => 1,
        "rgb" => 2,
        _ => 0,
    };
    STYLE.store(v, std::sync::atomic::Ordering::Relaxed);
}

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

/// Colours of one appearance (dark, or light on paper).
struct Pal {
    ground: [u8; 3],
    beat: [u8; 3],
    downbeat: [u8; 3],
    downbeat_mark: [u8; 3],
    phrase_line: [u8; 3],
    phrase_mark: [u8; 3],
    tick: [u8; 3],
    bands: [[u8; 3]; 3],
    bands_cb: [[u8; 3]; 3],
}

const DARK: Pal = Pal {
    ground: GROUND,
    beat: BEAT,
    downbeat: DOWNBEAT,
    downbeat_mark: DOWNBEAT_MARK,
    phrase_line: PHRASE_LINE,
    phrase_mark: PHRASE_MARK,
    tick: TICK,
    bands: [LOW, MID, HIGH],
    bands_cb: [LOW_CB, MID_CB, HIGH_CB],
};

/// Light appearance: paper ground, darker spectral bands, ink highs.
const LIGHT: Pal = Pal {
    ground: [0xE9, 0xE5, 0xDA],
    beat: [0xC2, 0xBD, 0xB0],
    downbeat: [0x6E, 0x6A, 0x62],
    downbeat_mark: [0x1D, 0x1C, 0x1A],
    phrase_line: [0xA0, 0x7A, 0x10],
    phrase_mark: [0x85, 0x63, 0x00],
    tick: [0xB0, 0x8A, 0x2A],
    bands: [[0x1F, 0x5F, 0xD0], [0xC2, 0x7A, 0x12], [0x4A, 0x47, 0x40]],
    bands_cb: [[0x00, 0x72, 0xB2], [0xB3, 0x78, 0x00], [0x1D, 0x1C, 0x1A]],
};

fn pal() -> &'static Pal {
    if crate::appearance::is_light() {
        &LIGHT
    } else {
        &DARK
    }
}

fn bands() -> [[u8; 3]; 3] {
    let p = pal();
    if COLORBLIND.load(std::sync::atomic::Ordering::Relaxed) {
        p.bands_cb
    } else {
        p.bands
    }
}

/// Colour of a phrase kind (shared by the phrase bar in the UI).
pub fn phrase_rgb(kind: PhraseKind) -> [u8; 3] {
    if crate::appearance::is_light() {
        return match kind {
            PhraseKind::Intro | PhraseKind::Outro => [0x4F, 0x56, 0xD6],
            PhraseKind::Up => [0x2E, 0x7A, 0x1F],
            PhraseKind::Down => [0x1A, 0x72, 0xA6],
            PhraseKind::Chorus => [0xA5, 0x4E, 0x0E],
            PhraseKind::Verse => [0x4A, 0x47, 0x40],
            PhraseKind::Bridge => [0x6A, 0x4B, 0xD8],
        };
    }
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
        *p = Rgba8Pixel {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
            a: 255,
        };
    }
}

fn vline(buf: &mut SharedPixelBuffer<Rgba8Pixel>, x: u32, y0: u32, y1: u32, rgb: [u8; 3]) {
    let (w, h) = (buf.width(), buf.height());
    if x >= w {
        return;
    }
    let px = buf.make_mut_slice();
    for y in y0.min(h)..y1.min(h) {
        px[(y * w + x) as usize] = Rgba8Pixel {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
            a: 255,
        };
    }
}

fn dim(rgb: [u8; 3]) -> [u8; 3] {
    rgb.map(|c| (c as u16 * 2 / 5) as u8)
}

/// Draws one three-band column centred on `mid`; `amp` is the half height of
/// the loudest band.
fn column(
    buf: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: u32,
    mid: u32,
    amp: u32,
    c: &WaveformColumn,
    played: bool,
) {
    styled_column(
        STYLE.load(std::sync::atomic::Ordering::Relaxed),
        buf,
        x,
        mid,
        amp,
        c,
        played,
    )
}

fn styled_column(
    style: u8,
    buf: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: u32,
    mid: u32,
    amp: u32,
    c: &WaveformColumn,
    played: bool,
) {
    match style {
        1 => return mono_column(buf, x, mid, amp, c, played),
        2 => return rgb_column(buf, x, mid, amp, c, played),
        _ => {}
    }
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

/// Blue style: one envelope in blue, the highs lighter on top.
fn mono_column(
    buf: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: u32,
    mid: u32,
    amp: u32,
    c: &WaveformColumn,
    played: bool,
) {
    let top = c.low.max(c.mid).max(c.high).max(1) as f32;
    let body = bands()[0];
    let light = body.map(|v| v / 2 + 120);
    for (v, rgb) in [(c.low.max(c.mid), body), (c.high, light)] {
        let a = (v as f32 / top * amp as f32).round() as u32;
        if a > 0 {
            vline(
                buf,
                x,
                mid.saturating_sub(a),
                mid + a,
                if played { dim(rgb) } else { rgb },
            );
        }
    }
}

/// RGB style: one envelope whose colour mixes the bands (lows red, mids
/// green, highs blue), so the timbre shows as hue.
fn rgb_column(
    buf: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: u32,
    mid: u32,
    amp: u32,
    c: &WaveformColumn,
    played: bool,
) {
    let top = c.low.max(c.mid).max(c.high);
    if top == 0 || amp == 0 {
        return;
    }
    // The loudest band is full intensity; the others in proportion.
    let rgb = [c.low, c.mid, c.high].map(|v| ((v as u32 * 255) / top as u32).max(40) as u8);
    vline(
        buf,
        x,
        mid.saturating_sub(amp),
        mid + amp,
        if played { dim(rgb) } else { rgb },
    );
}

/// Paints phrase colours as a strip of `strip` pixels along the bottom.
fn phrase_strip(
    buf: &mut SharedPixelBuffer<Rgba8Pixel>,
    phrases: &[Phrase],
    grid: &Beatgrid,
    x_of: impl Fn(f64) -> Option<u32>,
    strip: u32,
) {
    let (w, h) = (buf.width(), buf.height());
    for p in phrases {
        let (Some(a), Some(b)) = (
            grid.beats.get(p.start_beat as usize),
            grid.beats
                .get((p.end_beat as usize).min(grid.beats.len().saturating_sub(1))),
        ) else {
            continue;
        };
        let (Some(x0), Some(x1)) = (x_of(a.time), x_of(b.time)) else {
            continue;
        };
        let rgb = phrase_rgb(p.kind);
        for x in x0..x1.min(w) {
            vline(buf, x, h.saturating_sub(strip), h, rgb);
        }
        // A dark gap marks the phrase boundary.
        vline(buf, x0, h.saturating_sub(strip), h, pal().ground);
    }
}

/// Overview of a whole track, `width` × `height` physical pixels, with
/// beatgrid ticks every 16 bars.
pub fn overview(w: &Waveform, grid: &Beatgrid, duration: f64, width: u32, height: u32) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, pal().ground);
    // Ticks every 16 bars sit in a margin above and below the waveform.
    let margin = (height / 10).max(3);
    if duration > 0.0 {
        for b in grid.beats.iter().step_by(64) {
            let x = (b.time / duration * width as f64) as u32;
            vline(&mut buf, x, 0, margin - 1, pal().tick);
            vline(&mut buf, x, height - margin + 1, height, pal().tick);
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
        *p = Rgba8Pixel {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
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
pub fn detail(
    w: &Waveform,
    grid: &Beatgrid,
    phrases: &[Phrase],
    position: f64,
    span: f64,
    width: u32,
    height: u32,
) -> Image {
    draw_detail(
        w,
        grid,
        phrases,
        position - span / 2.0,
        span,
        Some(position),
        width,
        height,
    )
}

/// One tile of the scrolling waveform: `span` seconds from `start`, nothing
/// dimmed. The UI scrolls tiles by translating them, so the CPU renders a
/// tile only when the playhead enters a new one.
pub fn detail_tile(
    w: &Waveform,
    grid: &Beatgrid,
    phrases: &[Phrase],
    start: f64,
    span: f64,
    width: u32,
    height: u32,
) -> Image {
    draw_detail(w, grid, phrases, start, span, None, width, height)
}

/// A tile turned for the vertical lanes: time runs top to bottom, the
/// waveform's amplitude spreads left and right.
pub fn transpose(img: &Image) -> Image {
    let Some(src) = img.to_rgba8() else {
        return img.clone();
    };
    let (w, h) = (src.width(), src.height());
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(h, w);
    let from = src.as_slice();
    let to = out.make_mut_slice();
    for y in 0..h {
        for x in 0..w {
            to[(x * h + (h - 1 - y)) as usize] = from[(y * w + x) as usize];
        }
    }
    Image::from_rgba8(out)
}

#[allow(clippy::too_many_arguments)]
fn draw_detail(
    w: &Waveform,
    grid: &Beatgrid,
    phrases: &[Phrase],
    start: f64,
    span: f64,
    dim_before: Option<f64>,
    width: u32,
    height: u32,
) -> Image {
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(width.max(1), height.max(1));
    fill(&mut buf, pal().ground);
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
        let c = w.columns[a..b.min(w.columns.len())].iter().fold(
            WaveformColumn::default(),
            |acc, c| WaveformColumn {
                low: acc.low.max(c.low),
                mid: acc.mid.max(c.mid),
                high: acc.high.max(c.high),
                peak: acc.peak.max(c.peak),
            },
        );
        let amp =
            (c.peak as f32 / 255.0 * (wave_h as f32 / 2.0 - mark_h as f32 - 2.0)).max(1.0) as u32;
        column(&mut buf, x, mid, amp, &c, dim_before.is_some_and(|p| t < p));
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
            let line = if phrase {
                pal().phrase_line
            } else if downbeat {
                pal().downbeat
            } else {
                pal().beat
            };
            vline(&mut buf, x, 0, wave_h, line);
            if downbeat {
                // Marker at the top: a 3 px wide block, strongest at 16 bars.
                let mark = if phrase {
                    pal().phrase_mark
                } else {
                    pal().downbeat_mark
                };
                for dx in 0..3u32 {
                    vline(&mut buf, (x + dx).saturating_sub(1), 0, mark_h, mark);
                    vline(
                        &mut buf,
                        (x + dx).saturating_sub(1),
                        wave_h - mark_h,
                        wave_h,
                        mark,
                    );
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
            px[(y * w + x) as usize] = Rgba8Pixel {
                r: v(0x15),
                g: v(0x15),
                b: v(0x14),
                a: 255,
            };
        }
    }
    Image::from_rgba8(buf)
}

/// Cover art scaled to a square thumbnail of `size` pixels (PNG / JPEG).
pub fn artwork_thumbnail(data: &[u8], size: u32) -> Option<Image> {
    artwork_pixels(data, size).map(Image::from_rgba8)
}

/// [`artwork_thumbnail`] as a pixel buffer (can be made on any thread).
pub fn artwork_pixels(data: &[u8], size: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = image::load_from_memory(data).ok()?;
    let thumb = img
        .resize_to_fill(
            size.max(1),
            size.max(1),
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        thumb.as_raw(),
        thumb.width(),
        thumb.height(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sizes() {
        let w = Waveform {
            columns_per_second: 150.0,
            columns: vec![
                WaveformColumn {
                    low: 200,
                    mid: 20,
                    high: 10,
                    peak: 200
                };
                1500
            ],
        };
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let img = overview(&w, &g, 10.0, 400, 80);
        assert_eq!(img.size().width, 400);
        let d = detail(&w, &g, &[], 5.0, 8.0, 600, 100);
        assert_eq!(d.size().height, 100);
        assert_eq!(
            overview(&Waveform::default(), &g, 0.0, 10, 10).size().width,
            10
        );
        assert_eq!(ground_texture().size().width, 640);
    }

    #[test]
    fn three_bands_stack_by_amplitude() {
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(1, 41);
        fill(&mut buf, GROUND);
        column(
            &mut buf,
            0,
            20,
            20,
            &WaveformColumn {
                low: 200,
                mid: 100,
                high: 50,
                peak: 200,
            },
            false,
        );
        let at = |y: usize| {
            let p = buf.as_slice()[y];
            [p.r, p.g, p.b]
        };
        // Outermost: only the low band reaches; then mids; the centre is highs.
        assert_eq!(at(1), LOW);
        assert_eq!(at(14), MID);
        assert_eq!(at(20), HIGH);
    }

    #[test]
    fn beatgrid_lines_sit_on_the_beats() {
        let w = Waveform {
            columns_per_second: 150.0,
            columns: vec![WaveformColumn::default(); 1500],
        };
        // 120 BPM: a beat every 0.5 s; 8 s across 400 px = 25 px per beat.
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let img = detail(&w, &g, &[], 4.0, 8.0, 400, 90);
        let buf = img.to_rgba8().unwrap();
        let px = |x: u32, y: u32| {
            let p = buf.as_slice()[(y * 400 + x) as usize];
            [p.r, p.g, p.b]
        };
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
    fn tiles_join_seamlessly() {
        let w = Waveform {
            columns_per_second: 150.0,
            columns: (0..1500)
                .map(|i| WaveformColumn {
                    low: (i % 200) as u8,
                    mid: 40,
                    high: 20,
                    peak: (i % 200) as u8,
                })
                .collect(),
        };
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        // Two 4 s tiles side by side equal one 8 s detail without dimming.
        let a = detail_tile(&w, &g, &[], 0.0, 4.0, 200, 60)
            .to_rgba8()
            .unwrap();
        let b = detail_tile(&w, &g, &[], 4.0, 4.0, 200, 60)
            .to_rgba8()
            .unwrap();
        let whole = detail_tile(&w, &g, &[], 0.0, 8.0, 400, 60)
            .to_rgba8()
            .unwrap();
        for y in [10u32, 30, 50] {
            for x in [0u32, 57, 199] {
                assert_eq!(
                    a.as_slice()[(y * 200 + x) as usize],
                    whole.as_slice()[(y * 400 + x) as usize]
                );
                assert_eq!(
                    b.as_slice()[(y * 200 + x) as usize],
                    whole.as_slice()[(y * 400 + 200 + x) as usize]
                );
            }
        }
    }

    #[test]
    fn phrase_strip_is_painted() {
        let w = Waveform {
            columns_per_second: 150.0,
            columns: vec![WaveformColumn::default(); 1500],
        };
        let g = Beatgrid::constant(120.0, 0.0, 1, 10.0);
        let phrases = [Phrase {
            start_beat: 0,
            end_beat: 16,
            kind: PhraseKind::Chorus,
        }];
        let img = detail(&w, &g, &phrases, 4.0, 8.0, 200, 90);
        let buf = img.to_rgba8().unwrap();
        let p = buf.as_slice()[(89 * 200 + 150) as usize];
        assert_eq!([p.r, p.g, p.b], phrase_rgb(PhraseKind::Chorus));
    }

    #[test]
    fn artwork_thumbnails_are_square() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 10, 10, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let t = artwork_thumbnail(&png, 16).unwrap();
        assert_eq!((t.size().width, t.size().height), (16, 16));
        assert!(artwork_thumbnail(b"not an image", 16).is_none());
    }

    #[test]
    fn waveform_styles() {
        let c = WaveformColumn {
            low: 200,
            mid: 50,
            high: 10,
            peak: 200,
        };
        // Styles by number (the global switch is shared by parallel tests).
        let px = |style: u8| {
            let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(1, 21);
            styled_column(style, &mut buf, 0, 10, 8, &c, false);
            let p = buf.as_slice()[10];
            [p.r, p.g, p.b]
        };
        let rgb = px(2);
        assert!(
            rgb[0] == 255 && rgb[1] < 100 && rgb[2] < 100,
            "bass-heavy column is red in RGB: {rgb:?}"
        );
        let blue = px(1);
        assert!(blue[2] > blue[0], "blue style: {blue:?}");
        assert_ne!(px(0), rgb);
    }

    #[test]
    fn transposed_tiles_run_top_to_bottom() {
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(3, 2);
        // Mark the first column (time 0) red.
        for y in 0..2 {
            buf.make_mut_slice()[(y * 3) as usize] = Rgba8Pixel {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            };
        }
        let t = transpose(&Image::from_rgba8(buf)).to_rgba8().unwrap();
        assert_eq!((t.width(), t.height()), (2, 3));
        assert!(
            t.as_slice()[..2].iter().all(|p| p.r == 255),
            "time 0 is the top row"
        );
        assert!(t.as_slice()[2..].iter().all(|p| p.r == 0));
    }
}
