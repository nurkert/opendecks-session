//! Devices: registered sticks, their sync state, new volumes, changes made
//! on players (proposals) and the browsable stick libraries.

use crate::app::{App, handler};
use crate::browser;
use crate::models::model;
use crate::{DeviceRow, NodeChoice, ProposalRow, VolumeRow, format};
use od_core::{DeviceId, TrackId};
use od_devices::{Command, DeviceState, DeviceStatus, Event, Proposal, Volume};
use od_library::{DeviceFormats, Library, NodeId, NodeKind};
use slint::SharedString;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct DevicesState {
    pub selected: Option<DeviceId>,
    pub statuses: Vec<DeviceStatus>,
    pub volumes: Vec<Volume>,
    pub proposals: Vec<(DeviceId, Proposal)>,
}

/// Selects the first registered stick when none is selected.
pub(crate) fn select_default(app: &App) {
    let first = app
        .state
        .borrow()
        .lib
        .devices()
        .ok()
        .and_then(|d| d.first().map(|d| d.id));
    let mut st = app.state.borrow_mut();
    if st.devices.selected.is_none() {
        st.devices.selected = first;
    }
}

/// State, detail line and progress of a stick (detail untranslated).
pub(crate) fn device_line(s: &DeviceStatus) -> (&'static str, String, f32) {
    match &s.state {
        DeviceState::Disconnected => ("disconnected", "Not connected".to_string(), 0.0),
        DeviceState::InSync => (
            "in-sync",
            match s.device.last_sync.as_deref() {
                Some(t) => format!("In sync · {}", t.get(11..16).unwrap_or(t)),
                None => "In sync".to_string(),
            },
            1.0,
        ),
        DeviceState::OutOfDate { pending } => (
            "pending",
            format!(
                "{pending} change{} waiting",
                if *pending == 1 { "" } else { "s" }
            ),
            0.0,
        ),
        DeviceState::Syncing(p) => match p {
            od_devices::Progress::Copying {
                done_bytes,
                total_bytes,
                current,
            } => (
                "syncing",
                format!("Copying {current}"),
                if *total_bytes > 0 {
                    *done_bytes as f32 / *total_bytes as f32
                } else {
                    0.0
                },
            ),
            od_devices::Progress::Writing { format } => {
                ("syncing", format!("Writing {format} data"), 0.95)
            }
            _ => ("syncing", "Preparing".to_string(), 0.02),
        },
        DeviceState::Error(e) => ("error", e.clone(), 0.0),
    }
}

fn space(free: u64, total: u64) -> String {
    od_i18n::trf(
        "{} FREE OF {}",
        &[&format::bytes(free), &format::bytes(total)],
    )
}

pub(crate) fn refresh(app: &App) {
    let st = app.state.borrow();
    let ui = &app.ui;
    let mut rows: Vec<DeviceRow> = st
        .devices
        .statuses
        .iter()
        .map(|s| {
            let (state, detail, progress) = device_line(s);
            let mut formats = Vec::new();
            if s.device.formats.pioneer {
                formats.push("PIONEER");
            }
            if s.device.formats.opendecks {
                formats.push("OPENDECKS");
            }
            DeviceRow {
                id: s.device.id.0 as i32,
                name: s.device.name.clone().into(),
                state: state.into(),
                detail: od_i18n::translate_message(&detail).into(),
                progress,
                space: s
                    .volume
                    .as_ref()
                    .map(|v| space(v.free_bytes, v.total_bytes))
                    .unwrap_or_default()
                    .into(),
                formats: formats.join(" + ").into(),
                connected: s.volume.is_some(),
            }
        })
        .collect();
    // OPENDECKS_DEMO_SYNC=0.4 shows the first connected stick mid-sync
    // (documentation screenshots and design review).
    if let Some(p) = std::env::var("OPENDECKS_DEMO_SYNC")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        if let Some(r) = rows.iter_mut().find(|r| r.connected) {
            r.state = "syncing".into();
            r.progress = p.clamp(0.0, 1.0);
            r.detail = od_i18n::translate_message("Copying {}")
                .replace("{}", "…")
                .into();
        }
    }
    ui.set_devices(model(rows));
    let vols: Vec<VolumeRow> = st
        .devices
        .volumes
        .iter()
        .enumerate()
        .map(|(i, v)| VolumeRow {
            index: i as i32,
            name: v.name.clone().into(),
            space: space(v.free_bytes, v.total_bytes).into(),
        })
        .collect();
    ui.set_volumes(model(vols));

    // Device detail.
    let Some(id) = st.devices.selected else {
        ui.set_device_selected(-1);
        return;
    };
    let Ok(device) = st.lib.device(id) else {
        ui.set_device_selected(-1);
        return;
    };
    ui.set_device_selected(id.0 as i32);
    ui.set_device_name(device.name.clone().into());
    ui.set_device_pioneer(device.formats.pioneer);
    ui.set_device_opendecks(device.formats.opendecks);
    let assigned: HashSet<NodeId> = st
        .lib
        .device_nodes(id)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut choices = Vec::new();
    fn walk(
        lib: &Library,
        parent: Option<NodeId>,
        depth: i32,
        assigned: &HashSet<NodeId>,
        out: &mut Vec<NodeChoice>,
    ) {
        for n in lib.children(parent).unwrap_or_default() {
            out.push(NodeChoice {
                id: n.id.0 as i32,
                name: n.name.clone().into(),
                depth,
                checked: assigned.contains(&n.id),
            });
            if n.kind == NodeKind::Folder {
                walk(lib, Some(n.id), depth + 1, assigned, out);
            }
        }
    }
    walk(&st.lib, None, 0, &assigned, &mut choices);
    ui.set_device_content(model(choices));
    let props: Vec<ProposalRow> = st
        .devices
        .proposals
        .iter()
        .enumerate()
        .filter(|(_, (d, _))| *d == id)
        .map(|(i, (_, p))| match p {
            Proposal::Cues {
                track,
                stick,
                library,
            } => {
                let title = st
                    .lib
                    .track(*track)
                    .map(|t| t.tags.title)
                    .unwrap_or_else(|_| format!("Track {track}"));
                ProposalRow {
                    index: i as i32,
                    title: title.into(),
                    detail: format!(
                        "Player set {} cue(s); library has {}",
                        stick.len(),
                        library.len()
                    )
                    .into(),
                    kind: "cues".into(),
                }
            }
            Proposal::History { name, tracks } => ProposalRow {
                index: i as i32,
                title: format!("History: {name}").into(),
                detail: format!("{} tracks played", tracks.len()).into(),
                kind: "history".into(),
            },
        })
        .collect();
    ui.set_proposals(model(props));
}

/// Mounted sticks with an OpenDecks library, sorted and de-duplicated.
fn browsable_sticks(statuses: &[DeviceStatus], volumes: &[Volume]) -> Vec<(String, PathBuf)> {
    let mut sticks: Vec<(String, PathBuf)> = statuses
        .iter()
        .filter_map(|s| {
            s.volume
                .as_ref()
                .map(|v| (s.device.name.clone(), v.mount_point.clone()))
        })
        .chain(volumes.iter().map(|v| {
            let name = od_format_native::read_marker(&v.mount_point)
                .ok()
                .flatten()
                .map(|m| m.name)
                .unwrap_or_else(|| v.name.clone());
            (name, v.mount_point.clone())
        }))
        .filter(|(_, root)| od_format_native::library_path(root).exists())
        .collect();
    sticks.sort();
    sticks.dedup_by(|a, b| a.1 == b.1);
    sticks
}

/// Recomputes the browsable sticks from connected volumes.
fn update_sticks(app: &App) {
    let changed = {
        let mut st = app.state.borrow_mut();
        let sticks = browsable_sticks(&st.devices.statuses, &st.devices.volumes);
        if sticks == st.browser.sticks {
            false
        } else {
            // Leave a stick view whose stick went away.
            let b = &mut st.browser;
            let active_root = b
                .stick
                .as_ref()
                .map(|(i, _)| b.sticks.get(*i).map(|s| s.1.clone()));
            b.sticks = sticks;
            if let Some(root) = active_root.flatten() {
                match b.sticks.iter().position(|s| s.1 == root) {
                    Some(i) => {
                        if let Some(s) = b.stick.as_mut() {
                            s.0 = i;
                        }
                    }
                    None => {
                        b.stick = None;
                        b.node = None;
                        b.selection.clear();
                        st.player.current = None;
                    }
                }
            }
            true
        }
    };
    if changed {
        // Standalone: a stick becomes the source as soon as it is plugged in.
        let auto = {
            let st = app.state.borrow();
            st.browser.stick.is_none()
                && !st.browser.sticks.is_empty()
                && (st.config.kiosk || st.lib.track_count().unwrap_or(0) == 0)
        };
        if auto {
            app.ui.invoke_select_node(browser::stick_root_id(0));
        }
        app.refresh_library();
    }
}

/// Device manager events; returns true when the library needs a refresh.
pub(crate) fn poll(app: &App) -> bool {
    let mut need_refresh = false;
    let mut changed = false;
    for e in app.device_manager.events().try_iter() {
        match e {
            Event::Status(s) => {
                app.state.borrow_mut().devices.statuses = s;
                changed = true;
            }
            Event::UnregisteredVolumes(v) => {
                app.state.borrow_mut().devices.volumes = v;
                changed = true;
            }
            Event::SyncFinished { device, report } => {
                let name = app
                    .state
                    .borrow()
                    .lib
                    .device(device)
                    .map(|d| d.name)
                    .unwrap_or_default();
                app.status(format!(
                    "{name} synced: {} copied, {} updated, {} removed{}",
                    report.copied,
                    report.updated,
                    report.removed,
                    if report.skipped_missing.is_empty() {
                        String::new()
                    } else {
                        format!(", {} missing files skipped", report.skipped_missing.len())
                    }
                ));
            }
            Event::SyncFailed { error, .. } => app.status(format!("Sync failed: {error}")),
            Event::Proposals { device, proposals } => {
                {
                    let mut st = app.state.borrow_mut();
                    let p = &mut st.devices.proposals;
                    p.retain(|(d, p)| *d != device || proposals.iter().all(|q| !same_target(p, q)));
                    p.extend(proposals.into_iter().map(|p| (device, p)));
                }
                changed = true;
                app.status(
                    "A player changed cues or wrote history on a stick — review it under Devices",
                );
            }
            Event::Ejected { device } => {
                let name = app
                    .state
                    .borrow()
                    .lib
                    .device(device)
                    .map(|d| d.name)
                    .unwrap_or_default();
                app.status(format!("{name} can be removed safely"));
            }
            Event::EjectFailed { error, .. } => app.status(format!("Eject failed: {error}")),
            Event::Registered { .. } => need_refresh = true,
        }
    }
    if changed {
        update_sticks(app);
        refresh(app);
    }
    need_refresh
}

fn same_target(a: &Proposal, b: &Proposal) -> bool {
    match (a, b) {
        (Proposal::Cues { track: x, .. }, Proposal::Cues { track: y, .. }) => x == y,
        (
            Proposal::History {
                name: x,
                tracks: tx,
            },
            Proposal::History {
                name: y,
                tracks: ty,
            },
        ) => x == y && tx == ty,
        _ => false,
    }
}

fn resolve_proposal(app: &App, index: usize, accept: bool) {
    let entry = {
        let mut st = app.state.borrow_mut();
        if index >= st.devices.proposals.len() {
            return;
        }
        st.devices.proposals.remove(index)
    };
    let (device, proposal) = entry;
    let root = app
        .state
        .borrow()
        .devices
        .statuses
        .iter()
        .find(|s| s.device.id == device)
        .and_then(|s| s.volume.as_ref().map(|v| v.mount_point.clone()));
    match proposal {
        Proposal::Cues { track, stick, .. } => {
            if let Some(root) = &root {
                let _ = od_devices::PioneerWriter::release(root, track);
            }
            let cues = if accept {
                od_devices::pioneer::accepted_cues(&stick)
            } else {
                app.state.borrow().lib.cues(track).unwrap_or_default()
            };
            // Rejecting rewrites the library's cues to the stick.
            let r = app.state.borrow_mut().lib.set_cues(track, cues);
            app.report(r);
        }
        Proposal::History { name, tracks } => {
            if accept {
                let existing: Vec<TrackId> = tracks
                    .into_iter()
                    .filter(|t| app.state.borrow().lib.track(*t).is_ok())
                    .collect();
                let folder = {
                    let st = app.state.borrow();
                    st.lib
                        .children(None)
                        .unwrap_or_default()
                        .into_iter()
                        .find(|n| n.kind == NodeKind::Folder && n.name == "History")
                        .map(|n| n.id)
                };
                let folder = match folder {
                    Some(f) => Some(f),
                    None => {
                        let r = app.state.borrow_mut().lib.create_node(
                            None,
                            NodeKind::Folder,
                            "History",
                        );
                        app.report(r)
                    }
                };
                if let Some(f) = folder {
                    let r =
                        app.state
                            .borrow_mut()
                            .lib
                            .create_node(Some(f), NodeKind::Playlist, &name);
                    if let Some(pl) = app.report(r) {
                        let _ = app.state.borrow_mut().lib.add_to_node(pl, &existing, None);
                    }
                }
            }
        }
    }
    app.after_edit(if accept {
        "Change taken over"
    } else {
        "Kept the library version"
    });
    refresh(app);
    app.device_manager.send(Command::SyncNow(device));
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.on_sync_device(handler!(app, |a, id: i32| a
        .device_manager
        .send(Command::SyncNow(DeviceId(id as i64)))));
    // DEVICES heading: sync every registered stick now (sticks that are not
    // plugged in are skipped by the device manager).
    ui.on_sync_all_devices(handler!(app, |a| {
        let ids: Vec<DeviceId> = a
            .state
            .borrow()
            .lib
            .devices()
            .unwrap_or_default()
            .iter()
            .map(|d| d.id)
            .collect();
        for id in ids {
            a.device_manager.send(Command::SyncNow(id));
        }
        a.status("Syncing sticks…");
    }));
    ui.on_rebuild_device(handler!(app, |a, id: i32| {
        a.device_manager.send(Command::Rebuild(DeviceId(id as i64)));
        a.status("Rebuilding stick…");
    }));
    ui.on_eject_device(handler!(app, |a, id: i32| {
        a.device_manager.send(Command::Eject(DeviceId(id as i64)));
        a.status("Ejecting…");
    }));
    ui.on_open_device(handler!(app, |a, id: i32| {
        a.state.borrow_mut().devices.selected = Some(DeviceId(id as i64));
        a.ui.set_section("devices".into());
        refresh(&a);
    }));
    ui.on_forget_device(handler!(app, |a, id: i32| {
        let r = a.state.borrow_mut().lib.forget_device(DeviceId(id as i64));
        if a.report(r).is_some() {
            a.state.borrow_mut().devices.selected = None;
            a.device_manager.send(Command::Refresh);
            a.status("Stick forgotten. Its files were left untouched.");
        }
    }));
    ui.on_register_volume(handler!(app, |a, i: i32| {
        let (volume, node, name) = {
            let st = a.state.borrow();
            let node = st.browser.node.filter(|_| st.explorer.open.is_none());
            let name = node.and_then(|n| st.lib.node(n).ok()).map(|n| n.name);
            (st.devices.volumes.get(i as usize).cloned(), node, name)
        };
        let (Some(v), Some(n)) = (volume, node) else {
            a.status("Select the playlist, set or folder that should go on the stick first");
            return;
        };
        a.device_manager.send(Command::Register {
            mount_point: v.mount_point.clone(),
            name: v.name.clone(),
            formats: DeviceFormats::default(),
            nodes: vec![n],
        });
        a.status(format!(
            "Registering {} with {} — the first sync starts now",
            v.name,
            name.unwrap_or_default()
        ));
    }));
    ui.on_set_device_formats(handler!(app, |a, pioneer: bool, opendecks: bool| {
        let id = a.state.borrow().devices.selected;
        if let Some(id) = id {
            let name = a
                .state
                .borrow()
                .lib
                .device(id)
                .map(|d| d.name)
                .unwrap_or_default();
            let r = a.state.borrow_mut().lib.update_device(
                id,
                &name,
                DeviceFormats { pioneer, opendecks },
            );
            if a.report(r).is_some() {
                refresh(&a);
            }
        }
    }));
    ui.on_set_device_node(handler!(app, |a, node: i32, on: bool| {
        let id = a.state.borrow().devices.selected;
        if let Some(id) = id {
            let mut nodes = a.state.borrow().lib.device_nodes(id).unwrap_or_default();
            let n = NodeId(node as i64);
            nodes.retain(|x| *x != n);
            if on {
                nodes.push(n);
            }
            let r = a.state.borrow_mut().lib.set_device_nodes(id, &nodes);
            if a.report(r).is_some() {
                refresh(&a);
                browser::refresh_tree(&a);
            }
        }
    }));
    ui.on_rename_device(handler!(app, |a, name: SharedString| {
        let id = a.state.borrow().devices.selected;
        if let Some(id) = id {
            let formats = a
                .state
                .borrow()
                .lib
                .device(id)
                .map(|d| d.formats)
                .unwrap_or_default();
            let r = a
                .state
                .borrow_mut()
                .lib
                .update_device(id, name.trim(), formats);
            if a.report(r).is_some() {
                a.device_manager.send(Command::Refresh);
            }
        }
    }));
    ui.on_accept_proposal(handler!(app, |a, i: i32| resolve_proposal(
        &a, i as usize, true
    )));
    ui.on_reject_proposal(handler!(app, |a, i: i32| resolve_proposal(
        &a, i as usize, false
    )));
}
