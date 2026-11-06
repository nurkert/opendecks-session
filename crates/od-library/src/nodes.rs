//! The tree of folders, playlists, smart playlists and sets.

use crate::journal::{EntityState, apply_edit};
use crate::smart::Rule;
use crate::tracks::{TRACK_COLUMNS, TrackRecord, track_from_row};
use crate::{Library, LibraryError, NOW, Result};
use od_core::{DeviceId, TrackId};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub i64);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Folder,
    Playlist,
    Smart,
    Set,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Folder => "folder",
            NodeKind::Playlist => "playlist",
            NodeKind::Smart => "smart",
            NodeKind::Set => "set",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "folder" => NodeKind::Folder,
            "smart" => NodeKind::Smart,
            "set" => NodeKind::Set,
            _ => NodeKind::Playlist,
        }
    }

    /// Whether the node holds an explicit, ordered track list.
    pub fn has_entries(self) -> bool {
        matches!(self, NodeKind::Playlist | NodeKind::Set)
    }
}

/// Extra data of a set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SetMeta {
    /// Planned start time, free-form local time such as "23:30".
    pub planned_start: Option<String>,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: NodeKind,
    pub name: String,
    pub sort_order: i64,
    pub smart_rule: Option<Rule>,
    pub set_meta: Option<SetMeta>,
    pub created: String,
}

/// One position of a playlist or set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub track: TrackId,
    /// Transition note (sets).
    #[serde(default)]
    pub note: String,
    /// Alternative tracks for this position (sets).
    #[serde(default)]
    pub backups: Vec<TrackId>,
}

impl Entry {
    pub fn new(track: TrackId) -> Self {
        Self { track, note: String::new(), backups: Vec::new() }
    }
}

/// A node with all of its descendants, for undoable deletion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subtree {
    /// Parents come before children.
    pub nodes: Vec<Node>,
    pub entries: Vec<(NodeId, Vec<Entry>)>,
    pub device_links: Vec<(DeviceId, NodeId)>,
}

fn node_from_row(r: &rusqlite::Row) -> rusqlite::Result<Node> {
    let rule: Option<String> = r.get(5)?;
    let meta: Option<String> = r.get(6)?;
    Ok(Node {
        id: NodeId(r.get(0)?),
        parent: r.get::<_, Option<i64>>(1)?.map(NodeId),
        kind: NodeKind::parse(&r.get::<_, String>(2)?),
        name: r.get(3)?,
        sort_order: r.get(4)?,
        smart_rule: rule.and_then(|s| serde_json::from_str(&s).ok()),
        set_meta: meta.and_then(|s| serde_json::from_str(&s).ok()),
        created: r.get(7)?,
    })
}

const NODE_COLUMNS: &str = "id, parent_id, kind, name, sort_order, smart_rule, set_meta, created";

pub(crate) fn read_node(conn: &Connection, id: NodeId) -> Result<Node> {
    conn.query_row(&format!("SELECT {NODE_COLUMNS} FROM node WHERE id = ?1"), [id.0], node_from_row)
        .optional()?
        .ok_or_else(|| LibraryError::NotFound(format!("node {id}")))
}

fn insert_node(conn: &Connection, n: &Node) -> Result<()> {
    conn.execute(
        "INSERT INTO node (id, parent_id, kind, name, sort_order, smart_rule, set_meta, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            n.id.0,
            n.parent.map(|p| p.0),
            n.kind.as_str(),
            n.name,
            n.sort_order,
            n.smart_rule.as_ref().map(serde_json::to_string).transpose()?,
            n.set_meta.as_ref().map(serde_json::to_string).transpose()?,
            n.created
        ],
    )?;
    Ok(())
}

pub(crate) fn write_node(conn: &Connection, n: &Node) -> Result<()> {
    conn.execute(
        "UPDATE node SET parent_id = ?2, kind = ?3, name = ?4, sort_order = ?5, smart_rule = ?6,
            set_meta = ?7 WHERE id = ?1",
        params![
            n.id.0,
            n.parent.map(|p| p.0),
            n.kind.as_str(),
            n.name,
            n.sort_order,
            n.smart_rule.as_ref().map(serde_json::to_string).transpose()?,
            n.set_meta.as_ref().map(serde_json::to_string).transpose()?
        ],
    )?;
    Ok(())
}

pub(crate) fn read_entries(conn: &Connection, node: NodeId) -> Result<Vec<Entry>> {
    let mut stmt = conn.prepare_cached(
        "SELECT track_id, note, backups FROM entry WHERE node_id = ?1 ORDER BY position",
    )?;
    let rows = stmt.query_map([node.0], |r| {
        let backups: String = r.get(2)?;
        Ok(Entry {
            track: TrackId(r.get(0)?),
            note: r.get(1)?,
            backups: serde_json::from_str(&backups).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn write_entries(conn: &Connection, node: NodeId, entries: &[Entry]) -> Result<()> {
    conn.execute("DELETE FROM entry WHERE node_id = ?1", [node.0])?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO entry (node_id, position, track_id, note, backups) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (i, e) in entries.iter().enumerate() {
        stmt.execute(params![node.0, i as i64, e.track.0, e.note, serde_json::to_string(&e.backups)?])?;
    }
    Ok(())
}

pub(crate) fn descendants(conn: &Connection, root: NodeId) -> Result<Vec<NodeId>> {
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE sub(id) AS (SELECT ?1 UNION ALL SELECT n.id FROM node n JOIN sub ON n.parent_id = sub.id)
         SELECT id FROM sub",
    )?;
    Ok(stmt.query_map([root.0], |r| Ok(NodeId(r.get(0)?)))?.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn capture_subtree(conn: &Connection, root: NodeId) -> Result<Option<Subtree>> {
    let exists: bool = conn.query_row("SELECT count(*) > 0 FROM node WHERE id = ?1", [root.0], |r| r.get(0))?;
    if !exists {
        return Ok(None);
    }
    let ids = descendants(conn, root)?;
    let mut nodes = Vec::with_capacity(ids.len());
    let mut entries = Vec::new();
    let mut device_links = Vec::new();
    for id in &ids {
        nodes.push(read_node(conn, *id)?);
        entries.push((*id, read_entries(conn, *id)?));
        let mut stmt = conn.prepare_cached("SELECT device_id FROM device_node WHERE node_id = ?1")?;
        for d in stmt.query_map([id.0], |r| r.get::<_, i64>(0))? {
            device_links.push((DeviceId(d?), *id));
        }
    }
    Ok(Some(Subtree { nodes, entries, device_links }))
}

pub(crate) fn delete_subtree(conn: &Connection, root: NodeId) -> Result<()> {
    conn.execute("DELETE FROM node WHERE id = ?1", [root.0])?;
    Ok(())
}

pub(crate) fn restore_subtree(conn: &Connection, t: &Subtree) -> Result<()> {
    for n in &t.nodes {
        insert_node(conn, n)?;
    }
    for (node, entries) in &t.entries {
        write_entries(conn, *node, entries)?;
    }
    for (device, node) in &t.device_links {
        conn.execute(
            "INSERT OR IGNORE INTO device_node (device_id, node_id) SELECT ?1, ?2 WHERE EXISTS
               (SELECT 1 FROM device WHERE id = ?1)",
            params![device.0, node.0],
        )?;
    }
    Ok(())
}

impl Library {
    /// Creates a node as the last child of `parent`.
    pub fn create_node(&mut self, parent: Option<NodeId>, kind: NodeKind, name: &str) -> Result<NodeId> {
        let name = name.to_string();
        self.tx(|tx| {
            if let Some(p) = parent {
                if read_node(tx, p)?.kind != NodeKind::Folder {
                    return Err(LibraryError::Invalid("only folders can contain other nodes".into()));
                }
            }
            let id = NodeId(tx.query_row("SELECT coalesce(max(id), 0) + 1 FROM node", [], |r| r.get(0))?);
            let sort_order: i64 = tx.query_row(
                "SELECT coalesce(max(sort_order), -1) + 1 FROM node WHERE parent_id IS ?1",
                [parent.map(|p| p.0)],
                |r| r.get(0),
            )?;
            let created: String =
                tx.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0))?;
            let node = Node {
                id,
                parent,
                kind,
                name,
                sort_order,
                smart_rule: (kind == NodeKind::Smart).then(|| Rule::All(vec![])),
                set_meta: (kind == NodeKind::Set).then(SetMeta::default),
                created,
            };
            let label = format!("Create {}", kind.as_str());
            apply_edit(
                tx,
                &label,
                vec![EntityState::Subtree {
                    root: id,
                    tree: Some(Subtree { nodes: vec![node], entries: vec![], device_links: vec![] }),
                }],
            )?;
            Ok(id)
        })
    }

    /// A new node as it would be created as the last child of `parent`
    /// (fresh id, sort order and timestamp), without writing it. Callers put
    /// it into an [`EntityState::Subtree`] together with its entries so that
    /// creating and filling a list is one undo step; call it inside
    /// [`Library::edit_with`] so that the id stays free until the write.
    pub fn new_node(&self, parent: Option<NodeId>, kind: NodeKind, name: &str) -> Result<Node> {
        let conn = &self.conn;
        if let Some(p) = parent {
            if read_node(conn, p)?.kind != NodeKind::Folder {
                return Err(LibraryError::Invalid("only folders can contain other nodes".into()));
            }
        }
        let id = NodeId(conn.query_row("SELECT coalesce(max(id), 0) + 1 FROM node", [], |r| r.get(0))?);
        let sort_order: i64 = conn.query_row(
            "SELECT coalesce(max(sort_order), -1) + 1 FROM node WHERE parent_id IS ?1",
            [parent.map(|p| p.0)],
            |r| r.get(0),
        )?;
        let created: String = conn.query_row(&format!("SELECT {NOW}"), [], |r| r.get(0))?;
        Ok(Node {
            id,
            parent,
            kind,
            name: name.to_string(),
            sort_order,
            smart_rule: (kind == NodeKind::Smart).then(|| Rule::All(vec![])),
            set_meta: (kind == NodeKind::Set).then(SetMeta::default),
            created,
        })
    }

    pub fn node(&self, id: NodeId) -> Result<Node> {
        read_node(&self.conn, id)
    }

    /// Children of `parent` (root level for `None`), in display order.
    pub fn children(&self, parent: Option<NodeId>) -> Result<Vec<Node>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {NODE_COLUMNS} FROM node WHERE parent_id IS ?1 ORDER BY sort_order, id"
        ))?;
        Ok(stmt.query_map([parent.map(|p| p.0)], node_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    /// All nodes.
    pub fn all_nodes(&self) -> Result<Vec<Node>> {
        let mut stmt =
            self.conn.prepare(&format!("SELECT {NODE_COLUMNS} FROM node ORDER BY parent_id, sort_order, id"))?;
        Ok(stmt.query_map([], node_from_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub fn descendants(&self, root: NodeId) -> Result<Vec<NodeId>> {
        descendants(&self.conn, root)
    }

    pub fn rename_node(&mut self, id: NodeId, name: &str) -> Result<()> {
        let mut node = self.node(id)?;
        node.name = name.to_string();
        self.edit("Rename", vec![EntityState::Node { node }])
    }

    /// Moves a node under `parent` at position `index` among its siblings.
    pub fn move_node(&mut self, id: NodeId, parent: Option<NodeId>, index: usize) -> Result<()> {
        if let Some(p) = parent {
            if self.descendants(id)?.contains(&p) {
                return Err(LibraryError::Invalid("cannot move a folder into itself".into()));
            }
            if self.node(p)?.kind != NodeKind::Folder {
                return Err(LibraryError::Invalid("only folders can contain other nodes".into()));
            }
        }
        let mut siblings: Vec<Node> = self.children(parent)?.into_iter().filter(|n| n.id != id).collect();
        let mut node = self.node(id)?;
        node.parent = parent;
        siblings.insert(index.min(siblings.len()), node);
        let states = siblings
            .into_iter()
            .enumerate()
            .map(|(i, mut n)| {
                n.sort_order = i as i64;
                EntityState::Node { node: n }
            })
            .collect();
        self.edit("Move", states)
    }

    pub fn delete_node(&mut self, id: NodeId) -> Result<()> {
        self.node(id)?;
        self.edit("Delete", vec![EntityState::Subtree { root: id, tree: None }])
    }

    pub fn entries(&self, node: NodeId) -> Result<Vec<Entry>> {
        read_entries(&self.conn, node)
    }

    fn require_entries(&self, node: NodeId) -> Result<()> {
        if self.node(node)?.kind.has_entries() {
            Ok(())
        } else {
            Err(LibraryError::Invalid("this node has no track list".into()))
        }
    }

    /// Inserts tracks at `index` (append for `None`).
    pub fn add_to_node(&mut self, node: NodeId, tracks: &[TrackId], index: Option<usize>) -> Result<()> {
        self.require_entries(node)?;
        let mut entries = self.entries(node)?;
        let at = index.unwrap_or(entries.len()).min(entries.len());
        entries.splice(at..at, tracks.iter().map(|&t| Entry::new(t)));
        self.edit("Add tracks", vec![EntityState::Entries { node, entries }])
    }

    /// Removes the entries at the given positions.
    pub fn remove_from_node(&mut self, node: NodeId, positions: &[usize]) -> Result<()> {
        let entries: Vec<Entry> = self
            .entries(node)?
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !positions.contains(i))
            .map(|(_, e)| e)
            .collect();
        self.edit("Remove tracks", vec![EntityState::Entries { node, entries }])
    }

    /// Moves the entries at `positions` so that they start at `to` (index in
    /// the list before the move).
    pub fn move_entries(&mut self, node: NodeId, positions: &[usize], to: usize) -> Result<()> {
        let all = self.entries(node)?;
        let mut moving = Vec::new();
        let mut rest = Vec::new();
        let mut insert_at = 0;
        for (i, e) in all.into_iter().enumerate() {
            if positions.contains(&i) {
                moving.push(e);
            } else {
                if i < to {
                    insert_at += 1;
                }
                rest.push(e);
            }
        }
        rest.splice(insert_at..insert_at, moving);
        self.edit("Reorder", vec![EntityState::Entries { node, entries: rest }])
    }

    pub fn set_entry_note(&mut self, node: NodeId, position: usize, note: &str) -> Result<()> {
        let mut entries = self.entries(node)?;
        let e = entries.get_mut(position).ok_or_else(|| LibraryError::NotFound("entry".into()))?;
        e.note = note.to_string();
        self.edit("Edit transition note", vec![EntityState::Entries { node, entries }])
    }

    pub fn set_entry_backups(&mut self, node: NodeId, position: usize, backups: Vec<TrackId>) -> Result<()> {
        let mut entries = self.entries(node)?;
        let e = entries.get_mut(position).ok_or_else(|| LibraryError::NotFound("entry".into()))?;
        e.backups = backups;
        self.edit("Edit backup tracks", vec![EntityState::Entries { node, entries }])
    }

    pub fn set_smart_rule(&mut self, id: NodeId, rule: Rule) -> Result<()> {
        let mut node = self.node(id)?;
        if node.kind != NodeKind::Smart {
            return Err(LibraryError::Invalid("not a smart playlist".into()));
        }
        node.smart_rule = Some(rule);
        self.edit("Edit smart rules", vec![EntityState::Node { node }])
    }

    pub fn set_set_meta(&mut self, id: NodeId, meta: SetMeta) -> Result<()> {
        let mut node = self.node(id)?;
        if node.kind != NodeKind::Set {
            return Err(LibraryError::Invalid("not a set".into()));
        }
        node.set_meta = Some(meta);
        self.edit("Edit set", vec![EntityState::Node { node }])
    }

    /// The tracks of a playlist, set or smart playlist, in order. Folders
    /// return the tracks of all descendants (deduplicated, in tree order).
    pub fn node_tracks(&self, id: NodeId) -> Result<Vec<TrackRecord>> {
        let node = self.node(id)?;
        match node.kind {
            NodeKind::Playlist | NodeKind::Set => {
                let mut stmt = self.conn.prepare_cached(&format!(
                    "SELECT {} FROM entry e JOIN track t ON t.id = e.track_id WHERE e.node_id = ?1
                     ORDER BY e.position",
                    TRACK_COLUMNS.split(", ").map(|c| format!("t.{c}")).collect::<Vec<_>>().join(", ")
                ))?;
                Ok(stmt.query_map([id.0], track_from_row)?.collect::<rusqlite::Result<_>>()?)
            }
            NodeKind::Smart => {
                let rule = node.smart_rule.unwrap_or(Rule::All(vec![]));
                let all = self.tracks(crate::SortKey::DateAdded, true)?;
                Ok(all.into_iter().filter(|t| rule.matches(t)).collect())
            }
            NodeKind::Folder => {
                let mut seen = std::collections::HashSet::new();
                let mut out = Vec::new();
                for child in self.children(Some(id))? {
                    for t in self.node_tracks(child.id)? {
                        if seen.insert(t.id) {
                            out.push(t);
                        }
                    }
                }
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;

    #[test]
    fn tree_and_entries() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", None, None);
        let b = track(&mut lib, "B", None, None);
        let c = track(&mut lib, "C", None, None);
        let folder = lib.create_node(None, NodeKind::Folder, "Gigs").unwrap();
        let pl = lib.create_node(Some(folder), NodeKind::Playlist, "Warmup").unwrap();
        assert!(lib.create_node(Some(pl), NodeKind::Playlist, "x").is_err());
        lib.add_to_node(pl, &[a, b, c], None).unwrap();
        lib.move_entries(pl, &[2], 0).unwrap();
        let order: Vec<_> = lib.entries(pl).unwrap().iter().map(|e| e.track).collect();
        assert_eq!(order, vec![c, a, b]);
        lib.remove_from_node(pl, &[1]).unwrap();
        assert_eq!(lib.node_tracks(pl).unwrap().len(), 2);
        assert_eq!(lib.node_tracks(folder).unwrap().len(), 2);
        assert!(lib.move_node(folder, Some(folder), 0).is_err());
    }

    #[test]
    fn delete_folder_is_undoable() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", None, None);
        let folder = lib.create_node(None, NodeKind::Folder, "F").unwrap();
        let pl = lib.create_node(Some(folder), NodeKind::Set, "Set").unwrap();
        lib.add_to_node(pl, &[a], None).unwrap();
        lib.set_entry_note(pl, 0, "long blend").unwrap();
        lib.delete_node(folder).unwrap();
        assert!(lib.node(pl).is_err());
        lib.undo().unwrap();
        assert_eq!(lib.node(pl).unwrap().parent, Some(folder));
        assert_eq!(lib.entries(pl).unwrap()[0].note, "long blend");
        // Undo the creation of everything.
        while lib.undo().unwrap().is_some() {}
        assert!(lib.all_nodes().unwrap().is_empty());
    }

    #[test]
    fn move_node_reorders_siblings() {
        let mut lib = Library::open_in_memory().unwrap();
        let x = lib.create_node(None, NodeKind::Playlist, "X").unwrap();
        let y = lib.create_node(None, NodeKind::Playlist, "Y").unwrap();
        let f = lib.create_node(None, NodeKind::Folder, "F").unwrap();
        lib.move_node(f, None, 0).unwrap();
        let names: Vec<_> = lib.children(None).unwrap().into_iter().map(|n| n.name).collect();
        assert_eq!(names, vec!["F", "X", "Y"]);
        lib.move_node(y, Some(f), 0).unwrap();
        assert_eq!(lib.children(Some(f)).unwrap()[0].id, y);
        let _ = x;
    }
}
