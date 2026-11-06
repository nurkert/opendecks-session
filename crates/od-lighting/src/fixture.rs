//! Fixtures: what each DMX channel of a light does, the built-in generic
//! types, a subset of the Open Fixture Library (OFL) JSON format, and the
//! patch that places fixtures on universes.

use serde::{Deserialize, Serialize};

/// What one DMX channel controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelKind {
    Dimmer,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Pan,
    PanFine,
    Tilt,
    TiltFine,
    /// Shutter / strobe: written "open" (255) at rest, the strobe rate
    /// scaled into the range above 64 (the common layout).
    Strobe,
    /// Pan/tilt speed (0 = fastest on most movers).
    Speed,
    ColorWheel,
    Gobo,
    /// Pixel `n` of a multi-cell fixture (light bars): red, green, blue.
    PixelRed(u8),
    PixelGreen(u8),
    PixelBlue(u8),
    /// Anything else; held at the given value.
    Fixed(u8),
}

/// A fixture type in one mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FixtureDef {
    pub manufacturer: String,
    pub model: String,
    pub mode: String,
    pub channels: Vec<ChannelKind>,
}

/// Rough role used by the automatic show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FixtureRole {
    /// Static wash / par.
    Wash,
    /// Moving head (has pan/tilt).
    Mover,
    /// Multi-cell bar.
    Bar,
    /// Plain dimmer / strobe.
    Effect,
}

impl FixtureDef {
    pub fn has(&self, kind: ChannelKind) -> bool {
        self.channels.contains(&kind)
    }

    pub fn pixels(&self) -> u8 {
        self.channels
            .iter()
            .filter_map(|c| match c {
                ChannelKind::PixelRed(n) => Some(n + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    pub fn role(&self) -> FixtureRole {
        if self.has(ChannelKind::Pan) || self.has(ChannelKind::Tilt) {
            FixtureRole::Mover
        } else if self.pixels() > 1 {
            FixtureRole::Bar
        } else if self.has(ChannelKind::Red) || self.pixels() == 1 {
            FixtureRole::Wash
        } else {
            FixtureRole::Effect
        }
    }

    /// Generic built-in types (no library file needed).
    pub fn builtin(name: &str) -> Option<Self> {
        use ChannelKind::*;
        let (model, mode, channels): (&str, &str, Vec<ChannelKind>) = match name {
            "dimmer" => ("Dimmer", "1ch", vec![Dimmer]),
            "rgb-par" => ("RGB par", "3ch", vec![Red, Green, Blue]),
            "rgbw-par" => (
                "RGBW par",
                "6ch",
                vec![Dimmer, Red, Green, Blue, White, Strobe],
            ),
            "moving-head" => (
                "Moving head",
                "14ch",
                vec![
                    Pan,
                    PanFine,
                    Tilt,
                    TiltFine,
                    Speed,
                    Dimmer,
                    Strobe,
                    Red,
                    Green,
                    Blue,
                    White,
                    ColorWheel,
                    Gobo,
                    Fixed(0),
                ],
            ),
            "light-bar-8" => {
                let mut ch = vec![Dimmer, Strobe];
                for n in 0..8 {
                    ch.extend([PixelRed(n), PixelGreen(n), PixelBlue(n)]);
                }
                ("Light bar 8 × RGB", "26ch", ch)
            }
            "strobe" => ("Strobe", "2ch", vec![Dimmer, Strobe]),
            _ => return None,
        };
        Some(Self {
            manufacturer: "Generic".into(),
            model: model.into(),
            mode: mode.into(),
            channels,
        })
    }

    pub const BUILTIN: [&'static str; 6] = [
        "dimmer",
        "rgb-par",
        "rgbw-par",
        "moving-head",
        "light-bar-8",
        "strobe",
    ];

    /// Reads one mode of an Open Fixture Library fixture (`.json`). Channels
    /// whose capability is not understood become `Fixed(0)`, so the address
    /// footprint stays right.
    pub fn from_ofl(
        json: &str,
        manufacturer: &str,
        mode: Option<&str>,
    ) -> Result<Self, crate::Error> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| crate::Error::Fixture(e.to_string()))?;
        let model = v["name"].as_str().unwrap_or("Fixture").to_string();
        let modes = v["modes"]
            .as_array()
            .ok_or_else(|| crate::Error::Fixture("no modes".into()))?;
        let m = match mode {
            Some(name) => modes
                .iter()
                .find(|m| m["name"].as_str() == Some(name) || m["shortName"].as_str() == Some(name))
                .ok_or_else(|| crate::Error::Fixture(format!("no mode {name}")))?,
            None => modes
                .first()
                .ok_or_else(|| crate::Error::Fixture("no modes".into()))?,
        };
        let available = &v["availableChannels"];
        let mut channels = Vec::new();
        for key in m["channels"].as_array().into_iter().flatten() {
            let Some(key) = key.as_str() else {
                channels.push(ChannelKind::Fixed(0)); // null = unused slot
                continue;
            };
            channels.push(ofl_channel(key, available));
        }
        Ok(Self {
            manufacturer: manufacturer.into(),
            model,
            mode: m["name"].as_str().unwrap_or("").into(),
            channels,
        })
    }
}

fn ofl_channel(key: &str, available: &serde_json::Value) -> ChannelKind {
    use ChannelKind::*;
    // Fine channels are listed as aliases ("Pan fine") of the coarse one.
    for (name, ch) in available.as_object().into_iter().flatten() {
        let aliases = ch["fineChannelAliases"].as_array();
        if aliases.is_some_and(|a| a.iter().any(|x| x.as_str() == Some(key))) {
            return match ofl_channel(name, available) {
                Pan => PanFine,
                Tilt => TiltFine,
                _ => Fixed(0),
            };
        }
    }
    let ch = &available[key];
    let cap = if ch["capability"].is_object() {
        &ch["capability"]
    } else {
        &ch["capabilities"][0]
    };
    let default = ch["defaultValue"]
        .as_u64()
        .map(|v| v.min(255) as u8)
        .unwrap_or(0);
    match cap["type"].as_str().unwrap_or("") {
        "Intensity" => Dimmer,
        "ColorIntensity" => match cap["color"].as_str().unwrap_or("") {
            "Red" => Red,
            "Green" => Green,
            "Blue" => Blue,
            "White" | "Warm White" | "Cold White" => White,
            "Amber" => Amber,
            "UV" => Uv,
            _ => Fixed(0),
        },
        "Pan" => Pan,
        "Tilt" => Tilt,
        "ShutterStrobe" => Strobe,
        "PanTiltSpeed" => Speed,
        "WheelSlot" | "ColorPreset" if key.to_lowercase().contains("color") => ColorWheel,
        "WheelSlot" if key.to_lowercase().contains("gobo") => Gobo,
        _ => Fixed(default),
    }
}

/// One fixture placed on the rig.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Patched {
    pub name: String,
    pub def: FixtureDef,
    /// Universe index (0-based).
    pub universe: u16,
    /// First channel, 1..=512.
    pub address: u16,
    /// Group the show addresses ("Front wash", "Movers", …).
    pub group: String,
    /// Mirror pan (for movers hung on the opposite side).
    #[serde(default)]
    pub invert_pan: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub fixtures: Vec<Patched>,
}

impl Patch {
    /// Adds a fixture after checking its footprint fits and does not overlap.
    pub fn add(&mut self, fixture: Patched) -> Result<usize, crate::Error> {
        let len = fixture.def.channels.len() as u16;
        if fixture.address == 0 || fixture.address + len - 1 > 512 {
            return Err(crate::Error::Patch(format!(
                "{} does not fit at {}",
                fixture.name, fixture.address
            )));
        }
        let range = fixture.address..fixture.address + len;
        if let Some(other) = self.fixtures.iter().find(|f| {
            f.universe == fixture.universe
                && f.address < range.end
                && range.start < f.address + f.def.channels.len() as u16
        }) {
            return Err(crate::Error::Patch(format!(
                "{} overlaps {}",
                fixture.name, other.name
            )));
        }
        self.fixtures.push(fixture);
        Ok(self.fixtures.len() - 1)
    }

    /// Next free address on a universe for a footprint of `len` channels.
    pub fn next_free(&self, universe: u16, len: u16) -> Option<u16> {
        let mut start = 1;
        loop {
            if start + len - 1 > 512 {
                return None;
            }
            match self.fixtures.iter().find(|f| {
                f.universe == universe
                    && f.address < start + len
                    && start < f.address + f.def.channels.len() as u16
            }) {
                Some(f) => start = f.address + f.def.channels.len() as u16,
                None => return Some(start),
            }
        }
    }

    pub fn universes(&self) -> u16 {
        self.fixtures
            .iter()
            .map(|f| f.universe + 1)
            .max()
            .unwrap_or(0)
    }

    pub fn groups(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.fixtures {
            if !out.contains(&f.group) {
                out.push(f.group.clone());
            }
        }
        out
    }

    pub fn load(path: &std::path::Path) -> Result<Self, crate::Error> {
        let text = std::fs::read_to_string(path).map_err(|e| crate::Error::Patch(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| crate::Error::Patch(e.to_string()))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), crate::Error> {
        let text =
            serde_json::to_string_pretty(self).map_err(|e| crate::Error::Patch(e.to_string()))?;
        std::fs::write(path, text).map_err(|e| crate::Error::Patch(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFL: &str = r#"{
      "name": "Spot 60",
      "availableChannels": {
        "Pan": { "fineChannelAliases": ["Pan fine"], "capability": { "type": "Pan", "angleStart": "0deg", "angleEnd": "540deg" } },
        "Tilt": { "capability": { "type": "Tilt" } },
        "Dimmer": { "capability": { "type": "Intensity" } },
        "Shutter": { "capabilities": [ { "dmxRange": [0, 7], "type": "ShutterStrobe", "shutterEffect": "Closed" } ] },
        "Color Wheel": { "capabilities": [ { "dmxRange": [0, 9], "type": "WheelSlot" } ] },
        "Red": { "capability": { "type": "ColorIntensity", "color": "Red" } },
        "Macro": { "defaultValue": 12, "capability": { "type": "Effect" } }
      },
      "modes": [
        { "name": "Basic", "channels": ["Pan", "Tilt", "Dimmer"] },
        { "name": "Extended", "channels": ["Pan", "Pan fine", "Tilt", "Dimmer", "Shutter", "Color Wheel", "Red", null, "Macro"] }
      ]
    }"#;

    #[test]
    fn reads_an_ofl_mode() {
        use ChannelKind::*;
        let def = FixtureDef::from_ofl(OFL, "Acme", Some("Extended")).unwrap();
        assert_eq!(
            def.channels,
            vec![
                Pan,
                PanFine,
                Tilt,
                Dimmer,
                Strobe,
                ColorWheel,
                Red,
                Fixed(0),
                Fixed(12)
            ]
        );
        assert_eq!(def.role(), FixtureRole::Mover);
        assert_eq!(
            FixtureDef::from_ofl(OFL, "Acme", None)
                .unwrap()
                .channels
                .len(),
            3
        );
        assert!(FixtureDef::from_ofl(OFL, "Acme", Some("Nope")).is_err());
    }

    #[test]
    fn builtins_have_roles() {
        let role = |n| FixtureDef::builtin(n).unwrap().role();
        assert_eq!(role("moving-head"), FixtureRole::Mover);
        assert_eq!(role("light-bar-8"), FixtureRole::Bar);
        assert_eq!(role("rgbw-par"), FixtureRole::Wash);
        assert_eq!(role("strobe"), FixtureRole::Effect);
        assert_eq!(
            FixtureDef::builtin("light-bar-8").unwrap().channels.len(),
            26
        );
    }

    #[test]
    fn patch_rejects_overlaps_and_finds_space() {
        let mut patch = Patch::default();
        let par = FixtureDef::builtin("rgbw-par").unwrap();
        let fixture = |name: &str, address| Patched {
            name: name.into(),
            def: par.clone(),
            universe: 0,
            address,
            group: "Wash".into(),
            invert_pan: false,
        };
        patch.add(fixture("A", 1)).unwrap();
        assert!(patch.add(fixture("B", 6)).is_err());
        assert!(patch.add(fixture("C", 510)).is_err());
        assert_eq!(patch.next_free(0, 6), Some(7));
        patch.add(fixture("D", 7)).unwrap();
        assert_eq!(patch.groups(), vec!["Wash".to_string()]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rig.json");
        patch.save(&path).unwrap();
        assert_eq!(Patch::load(&path).unwrap(), patch);
    }
}
