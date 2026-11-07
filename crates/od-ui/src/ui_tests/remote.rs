//! The running app follows a lighting rig edited by another program (the
//! MCP server) and runs its live commands.

use super::harness::*;

#[test]
fn app_follows_rig_edits_and_live_commands_from_outside() {
    let ui = Ui::open((1440, 900));
    let rig = crate::lighting::rig_path(&ui.fx.library);
    // Another program patches a fixture and saves the rig.
    let mut config = od_lighting::RigConfig::load(&rig).unwrap();
    od_lighting::add_fixture(&mut config.patch, 1, "Front Par", "").unwrap();
    config.settings.mode = od_lighting::Mode::Pro;
    config.settings.cues.push(od_lighting::LightCue {
        name: "Drop".into(),
        groups: Default::default(),
        fade_beats: 0.0,
    });
    config.save(&rig).unwrap();
    assert!(
        ui.wait_until(5.0, |u| u.app.lighting.borrow().config.patch.fixtures.len()
            == 1),
        "the app took the edited rig"
    );
    assert_eq!(
        ui.app.lighting.borrow().config.settings.mode,
        od_lighting::Mode::Pro
    );

    // Live: black out, then fire the first cue.
    od_lighting::rig::send(&rig, &od_lighting::LiveCommand::Blackout { on: true }).unwrap();
    od_lighting::rig::send(&rig, &od_lighting::LiveCommand::Cue { index: 0 }).unwrap();
    assert!(
        ui.wait_until(5.0, |u| u.app.lighting.borrow().config.settings.blackout),
        "blackout from outside"
    );
    assert!(
        ui.wait_until(5.0, |u| slint::ComponentHandle::global::<crate::Lighting>(
            &u.app.ui
        )
        .get_active_cue()
            == 0),
        "cue fired from outside"
    );
    assert!(
        od_lighting::rig::drain(&rig).is_empty(),
        "the inbox was emptied"
    );
    // The app's own save does not count as an outside edit.
    assert!(
        od_lighting::RigConfig::load(&rig)
            .unwrap()
            .settings
            .blackout
    );
}
