//! Attribute model: what a fixture should do (intensity, colour, position,
//! strobe, per-pixel colour), rendered to DMX values through its channel
//! layout.

use crate::fixture::{ChannelKind, Patch, Patched};
use crate::output::Universe;

/// Desired state of one fixture, all values 0..1.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub intensity: f32,
    pub color: [f32; 3],
    pub pan: f32,
    pub tilt: f32,
    /// 0 = no strobe, 1 = fastest.
    pub strobe: f32,
    /// Per-cell colour × intensity for bars (empty = use `color`).
    pub pixels: Vec<[f32; 3]>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            intensity: 0.0,
            color: [1.0, 1.0, 1.0],
            pan: 0.5,
            tilt: 0.5,
            strobe: 0.0,
            pixels: Vec::new(),
        }
    }
}

impl Look {
    /// Linear blend towards `other` (`t` = 0 → self, 1 → other).
    pub fn blend(&self, other: &Look, t: f32) -> Look {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let n = self.pixels.len().max(other.pixels.len());
        let px = |l: &Look, i: usize| {
            l.pixels.get(i).copied().unwrap_or(
                l.color
                    .map(|c| c * if l.pixels.is_empty() { 1.0 } else { 0.0 }),
            )
        };
        Look {
            intensity: mix(self.intensity, other.intensity),
            color: [0, 1, 2].map(|i| mix(self.color[i], other.color[i])),
            pan: mix(self.pan, other.pan),
            tilt: mix(self.tilt, other.tilt),
            strobe: if t < 0.5 { self.strobe } else { other.strobe },
            pixels: (0..n)
                .map(|i| {
                    let (a, b) = (px(self, i), px(other, i));
                    [0, 1, 2].map(|c| mix(a[c], b[c]))
                })
                .collect(),
        }
    }
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Writes `look` for `fixture` into its universe.
pub fn render_fixture(fixture: &Patched, look: &Look, universe: &mut Universe) {
    let def = &fixture.def;
    let has_dimmer = def.has(ChannelKind::Dimmer);
    let has_white = def.has(ChannelKind::White);
    // Without a dimmer channel the intensity scales the colour.
    let scale = if has_dimmer { 1.0 } else { look.intensity };
    let mut rgb = look.color.map(|c| c * scale);
    let mut white = 0.0;
    if has_white {
        white = rgb.iter().copied().fold(f32::MAX, f32::min).max(0.0);
        rgb = rgb.map(|c| c - white);
    }
    let pan = if fixture.invert_pan {
        1.0 - look.pan
    } else {
        look.pan
    };
    let pan16 = (pan.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let tilt16 = (look.tilt.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let pixels = def.pixels() as usize;
    let pixel = |n: u8| -> [f32; 3] {
        if look.pixels.is_empty() {
            look.color.map(|c| c * scale)
        } else {
            let i = (n as usize * look.pixels.len()) / pixels.max(1);
            look.pixels
                .get(i)
                .copied()
                .unwrap_or([0.0; 3])
                .map(|c| c * scale)
        }
    };
    for (i, kind) in def.channels.iter().enumerate() {
        let slot = fixture.address as usize - 1 + i;
        if slot >= 512 {
            break;
        }
        universe[slot] = match *kind {
            ChannelKind::Dimmer => byte(look.intensity),
            ChannelKind::Red => byte(rgb[0]),
            ChannelKind::Green => byte(rgb[1]),
            ChannelKind::Blue => byte(rgb[2]),
            ChannelKind::White => byte(white),
            ChannelKind::Amber | ChannelKind::Uv => 0,
            ChannelKind::Pan => (pan16 >> 8) as u8,
            ChannelKind::PanFine => pan16 as u8,
            ChannelKind::Tilt => (tilt16 >> 8) as u8,
            ChannelKind::TiltFine => tilt16 as u8,
            // Open at rest; strobe speeds in the upper range.
            ChannelKind::Strobe => {
                if look.strobe <= 0.0 {
                    255
                } else {
                    64 + byte(look.strobe * 0.74)
                }
            }
            ChannelKind::Speed => 0,
            ChannelKind::ColorWheel | ChannelKind::Gobo => 0,
            ChannelKind::PixelRed(n) => byte(pixel(n)[0]),
            ChannelKind::PixelGreen(n) => byte(pixel(n)[1]),
            ChannelKind::PixelBlue(n) => byte(pixel(n)[2]),
            ChannelKind::Fixed(v) => v,
        };
    }
}

/// Renders a whole rig: one look per patched fixture (same order).
pub fn render(patch: &Patch, looks: &[Look]) -> Vec<Universe> {
    let mut out = vec![[0u8; 512]; patch.universes() as usize];
    for (fixture, look) in patch.fixtures.iter().zip(looks) {
        render_fixture(fixture, look, &mut out[fixture.universe as usize]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::FixtureDef;

    fn patched(kind: &str, address: u16) -> Patched {
        Patched {
            name: kind.into(),
            def: FixtureDef::builtin(kind).unwrap(),
            universe: 0,
            address,
            group: "G".into(),
            invert_pan: false,
        }
    }

    #[test]
    fn rgbw_extracts_white_and_keeps_strobe_open() {
        let mut u = [0u8; 512];
        let look = Look {
            intensity: 1.0,
            color: [1.0, 0.5, 0.5],
            ..Look::default()
        };
        render_fixture(&patched("rgbw-par", 10), &look, &mut u);
        assert_eq!(&u[9..15], &[255, 128, 0, 0, 128, 255]);
    }

    #[test]
    fn rgb_without_dimmer_scales_colour() {
        let mut u = [0u8; 512];
        let look = Look {
            intensity: 0.5,
            color: [1.0, 0.0, 1.0],
            ..Look::default()
        };
        render_fixture(&patched("rgb-par", 1), &look, &mut u);
        assert_eq!(&u[0..3], &[128, 0, 128]);
    }

    #[test]
    fn mover_writes_sixteen_bit_position_and_mirrors() {
        let mut u = [0u8; 512];
        let look = Look {
            intensity: 1.0,
            pan: 0.25,
            tilt: 1.0,
            strobe: 1.0,
            ..Look::default()
        };
        let mut mover = patched("moving-head", 1);
        render_fixture(&mover, &look, &mut u);
        assert_eq!(u16::from_be_bytes([u[0], u[1]]), 16384);
        assert_eq!(u16::from_be_bytes([u[2], u[3]]), 65535);
        assert!(u[6] > 200 && u[6] < 255);
        mover.invert_pan = true;
        render_fixture(&mover, &look, &mut u);
        assert_eq!(u16::from_be_bytes([u[0], u[1]]), 49151);
    }

    #[test]
    fn bar_pixels_spread_over_cells() {
        let mut u = [0u8; 512];
        let look = Look {
            intensity: 1.0,
            pixels: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            ..Look::default()
        };
        render_fixture(&patched("light-bar-8", 1), &look, &mut u);
        assert_eq!(&u[2..5], &[255, 0, 0]); // cell 0
        assert_eq!(&u[23..26], &[0, 0, 255]); // cell 7
    }

    #[test]
    fn blend_halfway() {
        let a = Look {
            intensity: 0.0,
            color: [1.0, 0.0, 0.0],
            ..Look::default()
        };
        let b = Look {
            intensity: 1.0,
            color: [0.0, 0.0, 1.0],
            ..Look::default()
        };
        let m = a.blend(&b, 0.5);
        assert_eq!((m.intensity, m.color), (0.5, [0.5, 0.0, 0.5]));
    }
}
