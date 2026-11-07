//! Registered USB devices, their assigned content and sync state.

use crate::nodes::{NodeId, NodeKind, descendants};
use crate::{Library, LibraryError, NOW, Result, TrackRecord};
use od_core::{DeviceId, TrackId};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// Export formats enabled for a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceFormats {
    /// Legacy Pioneer export (`export.pdb` + ANLZ).
    pub pioneer: bool,
    /// OpenDecks native format.
    pub opendecks: bool,
}

impl Default for DeviceFormats {
    fn default() -> Self {
        Self {
            pioneer: true,
            opendecks: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: DeviceId,
    pub volume_uuid: String,
    pub name: String,
    pub formats: DeviceFormats,
    /// Change-log position the device was last synced to.
    pub synced_seq: i64,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
}

/// What changed since a device was last synced.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PendingChanges {
    /// Tracks on the device whose data (tags, cues, grid, analysis, file)
    /// changed.
    pub tracks: BTreeSet<TrackId>,
    /// Playlist structure or membership changed.
    pub playlists: bool,
    /// Number of change-log entries that affect this device.
    pub count: usize,
    /// Head of the change log when this was computed; pass it to
    /// [`Library::mark_device_synced`] after a successful sync.
    pub head: i64,
}

impl PendingChanges {
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

fn device_from_row(r: &rusqlite::Row) -> rusqlite::Result<Device> {
    let formats: String = r.get(3)?;
    Ok(Device {
        id: DeviceId(r.get(0)?),
        volume_uuid: r.get(1)?,
        name: r.get(2)?,
        formats: serde_json::from_str(&formats).unwrap_or_default(),
        synced_seq: r.get(4)?,
        last_sync: r.get(5)?,
        last_error: r.get(6)?,
    })
}

const DEVICE_COLUMNS: &str = "id, volume_uuid, name, formats, synced_seq, last_sync, last_error";

impl Library {
    /// Registers a device. Returns the existing device if the volume is
    /// already known.
    pub fn register_device(
        &mut self,
        volume_uuid: &str,
        name: &str,
        formats: DeviceFormats,
    ) -> Result<DeviceId> {
        if let Some(d) = self.device_by_uuid(volume_uuid)? {
            return Ok(d.id);
        }
        self.conn.execute(
            "INSERT INTO device (volume_uuid, name, formats) VALUES (?1, ?2, ?3)",
            params![volume_uuid, name, serde_json::to_string(&formats)?],
        )?;
        Ok(DeviceId(self.conn.last_insert_rowid()))
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DEVICE_COLUMNS} FROM device ORDER BY name, id"
        ))?;
        Ok(stmt
            .query_map([], device_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn device(&self, id: DeviceId) -> Result<Device> {
        self.conn
            .query_row(
                &format!("SELECT {DEVICE_COLUMNS} FROM device WHERE id = ?1"),
                [id.0],
                device_from_row,
            )
            .optional()?
            .ok_or_else(|| LibraryError::NotFound(format!("device {id}")))
    }

    pub fn device_by_uuid(&self, uuid: &str) -> Result<Option<Device>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {DEVICE_COLUMNS} FROM device WHERE volume_uuid = ?1"),
                [uuid],
                device_from_row,
            )
            .optional()?)
    }

    pub fn update_device(
        &mut self,
        id: DeviceId,
        name: &str,
        formats: DeviceFormats,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE device SET name = ?2, formats = ?3, synced_seq = 0 WHERE id = ?1",
            params![id.0, name, serde_json::to_string(&formats)?],
        )?;
        Ok(())
    }

    pub fn forget_device(&mut self, id: DeviceId) -> Result<()> {
        self.conn
            .execute("DELETE FROM device WHERE id = ?1", [id.0])?;
        Ok(())
    }

    /// Nodes assigned to the device (top-level selections).
    pub fn device_nodes(&self, id: DeviceId) -> Result<Vec<NodeId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT node_id FROM device_node WHERE device_id = ?1 ORDER BY node_id")?;
        Ok(stmt
            .query_map([id.0], |r| Ok(NodeId(r.get(0)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces the device's assigned nodes. Forces a playlist rewrite.
    pub fn set_device_nodes(&mut self, id: DeviceId, nodes: &[NodeId]) -> Result<()> {
        self.tx(|tx| {
            tx.execute("DELETE FROM device_node WHERE device_id = ?1", [id.0])?;
            for n in nodes {
                tx.execute("INSERT INTO device_node (device_id, node_id) VALUES (?1, ?2)", params![id.0, n.0])?;
            }
            crate::record_change(tx, "device_content", None, None)?;
            // Reset so that the next sync rewrites playlists.
            tx.execute("UPDATE device SET synced_seq = min(synced_seq, (SELECT coalesce(max(seq),1) - 1 FROM change)) WHERE id = ?1", [id.0])?;
            Ok(())
        })
    }

    /// All nodes whose content ends up on the device (assigned nodes and
    /// their descendants).
    pub fn device_node_closure(&self, id: DeviceId) -> Result<BTreeSet<NodeId>> {
        let mut out = BTreeSet::new();
        for n in self.device_nodes(id)? {
            out.extend(descendants(&self.conn, n)?);
        }
        Ok(out)
    }

    /// The distinct tracks that belong on the device.
    pub fn device_tracks(&self, id: DeviceId) -> Result<Vec<TrackRecord>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for n in self.device_node_closure(id)? {
            if self.node(n)?.kind == NodeKind::Folder {
                continue;
            }
            for t in self.node_tracks(n)? {
                if seen.insert(t.id) {
                    out.push(t);
                }
            }
        }
        Ok(out)
    }

    /// Computes what changed for the device since its last sync.
    pub fn pending_changes(&self, id: DeviceId) -> Result<PendingChanges> {
        let device = self.device(id)?;
        let head = self.change_seq()?;
        let nodes = self.device_node_closure(id)?;
        let tracks: HashSet<TrackId> = self.device_tracks(id)?.into_iter().map(|t| t.id).collect();
        let has_smart = nodes
            .iter()
            .any(|n| self.node(*n).is_ok_and(|n| n.kind == NodeKind::Smart));
        let mut pending = PendingChanges {
            head,
            ..Default::default()
        };
        if device.synced_seq == 0 {
            // Never synced: everything is pending.
            pending.playlists = true;
            pending.tracks = tracks.into_iter().collect();
            pending.count = pending.tracks.len().max(1);
            return Ok(pending);
        }
        let mut stmt = self.conn.prepare(
            "SELECT kind, track_id, node_id FROM change WHERE seq > ?1 AND seq <= ?2 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![device.synced_seq, head], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for row in rows {
            let (kind, track, node) = row?;
            let relevant = match (track, node) {
                (_, Some(n)) => nodes.contains(&NodeId(n)) || kind == "node",
                (Some(t), None) => {
                    let t = TrackId(t);
                    if tracks.contains(&t) {
                        pending.tracks.insert(t);
                        true
                    } else {
                        // A removed track or a tag change can alter smart playlists.
                        kind == "track_removed" || (has_smart && kind != "analysis")
                    }
                }
                (None, None) => kind == "device_content",
            };
            if relevant {
                pending.count += 1;
                if node.is_some()
                    || track.is_none()
                    || !tracks.contains(&TrackId(track.unwrap_or(0)))
                {
                    pending.playlists = true;
                }
            }
        }
        Ok(pending)
    }

    /// Records a successful sync up to change-log position `seq`.
    pub fn mark_device_synced(&mut self, id: DeviceId, seq: i64) -> Result<()> {
        self.conn.execute(
            &format!("UPDATE device SET synced_seq = ?2, last_sync = {NOW}, last_error = NULL WHERE id = ?1"),
            params![id.0, seq.max(1)],
        )?;
        Ok(())
    }

    pub fn mark_device_error(&mut self, id: DeviceId, error: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE device SET last_error = ?2 WHERE id = ?1",
            params![id.0, error],
        )?;
        Ok(())
    }

    /// Forces a full rewrite on the next sync.
    pub fn reset_device_sync(&mut self, id: DeviceId) -> Result<()> {
        self.conn
            .execute("UPDATE device SET synced_seq = 0 WHERE id = ?1", [id.0])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;
    use od_core::Cue;

    #[test]
    fn cue_edit_marks_device_out_of_date() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", None, None);
        let b = track(&mut lib, "B", None, None);
        let other = track(&mut lib, "Other", None, None);
        let pl = lib.create_node(None, NodeKind::Playlist, "Gig").unwrap();
        lib.add_to_node(pl, &[a, b], None).unwrap();
        let dev = lib
            .register_device("UUID-1", "Red stick", DeviceFormats::default())
            .unwrap();
        assert_eq!(
            lib.register_device("UUID-1", "x", DeviceFormats::default())
                .unwrap(),
            dev
        );
        lib.set_device_nodes(dev, &[pl]).unwrap();

        let p = lib.pending_changes(dev).unwrap();
        assert!(p.playlists);
        assert_eq!(p.tracks.len(), 2);
        lib.mark_device_synced(dev, p.head).unwrap();
        assert!(lib.pending_changes(dev).unwrap().is_empty());

        // Editing a track that is not on the stick does not matter.
        lib.set_cues(other, vec![Cue::hot(0, 1.0)]).unwrap();
        assert!(lib.pending_changes(dev).unwrap().is_empty());

        // Editing a cue of a track on the stick does.
        lib.set_cues(a, vec![Cue::hot(0, 1.0)]).unwrap();
        let p = lib.pending_changes(dev).unwrap();
        assert_eq!(p.tracks.iter().copied().collect::<Vec<_>>(), vec![a]);
        assert!(!p.playlists);
        lib.mark_device_synced(dev, p.head).unwrap();

        // Reordering the playlist changes the playlists only.
        lib.move_entries(pl, &[1], 0).unwrap();
        let p = lib.pending_changes(dev).unwrap();
        assert!(p.playlists);
        assert!(p.tracks.is_empty());
    }

    #[test]
    fn device_tracks_include_folder_children() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", None, None);
        let f = lib.create_node(None, NodeKind::Folder, "F").unwrap();
        let pl = lib.create_node(Some(f), NodeKind::Playlist, "P").unwrap();
        lib.add_to_node(pl, &[a, a], None).unwrap();
        let dev = lib
            .register_device("U", "S", DeviceFormats::default())
            .unwrap();
        lib.set_device_nodes(dev, &[f]).unwrap();
        assert_eq!(lib.device_tracks(dev).unwrap().len(), 1);
        assert!(lib.device_node_closure(dev).unwrap().contains(&pl));
    }
}
