//! DMX lighting for OpenDecks Session (SPEC §10.5).
//!
//! * [`output`]: Art-Net, sACN and Enttec DMX USB Pro outputs.
//! * [`fixture`]: fixture types (built-in generics and Open Fixture Library
//!   files) and the patch.
//! * [`render`]: the attribute model ([`Look`]) rendered to DMX values.
//! * [`show`]: automatic, easy and pro modes driven by the music.
//! * [`Lighting`]: patch + show + outputs, ticked by the app.

pub mod fixture;
pub mod output;
pub mod render;
pub mod show;

pub use fixture::{ChannelKind, FixtureDef, FixtureRole, Patch, Patched};
pub use output::{Output, OutputKind, Universe};
pub use render::{Look, render, render_fixture};
pub use show::{
    Attr, DeckMusic, Effect, GroupSettings, LightCue, Mode, Momentary, Program, Show, ShowSettings,
    TrackCue, Wave, hsv, hue_for_key,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fixture: {0}")]
    Fixture(String),
    #[error("patch: {0}")]
    Patch(String),
    #[error("output: {0}")]
    Output(#[from] std::io::Error),
}

/// Everything about a rig the app keeps between sessions.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RigConfig {
    pub patch: Patch,
    pub settings: ShowSettings,
    /// Universe → output.
    pub outputs: Vec<(u16, OutputKind)>,
    /// Lighting on at all (outputs only send while enabled).
    #[serde(default)]
    pub enabled: bool,
}

impl RigConfig {
    /// Reads a rig; a missing file is the empty default.
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Patch(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), Error> {
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::Patch(e.to_string()))?;
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

/// A complete lighting rig: patch, show and the outputs per universe.
pub struct Lighting {
    pub patch: Patch,
    pub show: Show,
    outputs: Vec<(u16, Output)>,
    /// Last rendered universes (for the UI's channel monitor).
    pub frame: Vec<Universe>,
    /// Last looks (for the UI's rig preview).
    pub looks: Vec<Look>,
}

impl Lighting {
    pub fn new(patch: Patch, show: Show) -> Self {
        Self {
            patch,
            show,
            outputs: Vec::new(),
            frame: Vec::new(),
            looks: Vec::new(),
        }
    }

    /// Sends `universe` through `kind` (one output per universe and kind).
    pub fn add_output(&mut self, universe: u16, kind: OutputKind) -> Result<(), Error> {
        self.outputs.push((universe, Output::open(kind)?));
        Ok(())
    }

    pub fn clear_outputs(&mut self) {
        self.outputs.clear();
    }

    /// Renders one frame and sends it. Output errors are logged, not fatal:
    /// a missing interface must not stop the music.
    pub fn tick(&mut self, decks: &[DeckMusic], dt: f32) {
        self.looks = self.show.frame(&self.patch, decks, dt);
        self.frame = render(&self.patch, &self.looks);
        for (universe, output) in &mut self.outputs {
            if let Some(data) = self.frame.get(*universe as usize)
                && let Err(e) = output.send(data)
            {
                tracing::debug!(universe, "dmx output failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rig_config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lighting.json");
        assert_eq!(RigConfig::load(&path).unwrap(), RigConfig::default());
        let mut rig = RigConfig {
            enabled: true,
            ..Default::default()
        };
        rig.outputs.push((0, OutputKind::Sacn { universe: 1 }));
        rig.settings.mode = Mode::Pro;
        rig.settings.groups.insert(
            "Movers".into(),
            GroupSettings {
                program: Some(Program::Sweep),
                ..Default::default()
            },
        );
        rig.save(&path).unwrap();
        assert_eq!(RigConfig::load(&path).unwrap(), rig);
    }

    #[test]
    fn tick_renders_and_sends() {
        let rx = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut patch = Patch::default();
        patch
            .add(Patched {
                name: "Par".into(),
                def: FixtureDef::builtin("rgbw-par").unwrap(),
                universe: 0,
                address: 1,
                group: "Wash".into(),
                invert_pan: false,
            })
            .unwrap();
        let mut rig = Lighting::new(patch, Show::default());
        rig.add_output(
            0,
            OutputKind::ArtNet {
                target: rx.local_addr().unwrap(),
                universe: 0,
            },
        )
        .unwrap();
        rig.tick(&[], 0.025);
        let mut buf = [0u8; 600];
        let n = rx.recv(&mut buf).unwrap();
        assert_eq!(n, 530);
        assert_eq!(buf[18], rig.frame[0][0]);
        assert!(rig.frame[0][0] > 0, "idle wash is on");
    }
}
