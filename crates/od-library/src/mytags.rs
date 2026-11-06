//! My Tag: user-defined tag categories (such as "Mood" or "Situation") with
//! free values (such as "Peak" or "Warm-up") attached to tracks.

use crate::journal::EntityState;
use crate::{Library, Result};
use od_core::TrackId;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MyTag {
    pub category: String,
    pub value: String,
}

impl MyTag {
    pub fn new(category: &str, value: &str) -> Self {
        Self { category: category.trim().to_string(), value: value.trim().to_string() }
    }
}

pub(crate) fn read(conn: &Connection, track: TrackId) -> Result<Vec<MyTag>> {
    let mut stmt =
        conn.prepare_cached("SELECT category, value FROM my_tag WHERE track_id = ?1 ORDER BY category, value")?;
    let rows = stmt.query_map([track.0], |r| Ok(MyTag { category: r.get(0)?, value: r.get(1)? }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn write(conn: &Connection, track: TrackId, tags: &[MyTag]) -> Result<()> {
    conn.execute("DELETE FROM my_tag WHERE track_id = ?1", [track.0])?;
    let mut stmt =
        conn.prepare_cached("INSERT OR IGNORE INTO my_tag (track_id, category, value) VALUES (?1, ?2, ?3)")?;
    for t in tags.iter().filter(|t| !t.category.is_empty() && !t.value.is_empty()) {
        stmt.execute(params![track.0, t.category, t.value])?;
    }
    Ok(())
}

impl Library {
    /// The My Tag values of one track.
    pub fn my_tags(&self, track: TrackId) -> Result<Vec<MyTag>> {
        read(&self.conn, track)
    }

    /// My Tag values of every tagged track.
    pub fn all_my_tags(&self) -> Result<HashMap<TrackId, Vec<MyTag>>> {
        let mut stmt = self.conn.prepare_cached("SELECT track_id, category, value FROM my_tag ORDER BY category, value")?;
        let mut out: HashMap<TrackId, Vec<MyTag>> = HashMap::new();
        for row in stmt.query_map([], |r| {
            Ok((TrackId(r.get(0)?), MyTag { category: r.get(1)?, value: r.get(2)? }))
        })? {
            let (t, tag) = row?;
            out.entry(t).or_default().push(tag);
        }
        Ok(out)
    }

    /// Every category with its values and how many tracks carry each value.
    pub fn my_tag_catalog(&self) -> Result<BTreeMap<String, BTreeMap<String, u32>>> {
        let mut stmt = self.conn.prepare_cached("SELECT category, value, count(*) FROM my_tag GROUP BY category, value")?;
        let mut out: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, u32>(2)?)))? {
            let (c, v, n) = row?;
            out.entry(c).or_default().insert(v, n);
        }
        Ok(out)
    }

    /// User edit: replace the My Tag values of a track.
    pub fn set_my_tags(&mut self, track: TrackId, tags: Vec<MyTag>) -> Result<()> {
        self.track(track)?;
        self.edit("Edit My Tag", vec![EntityState::MyTags { track, tags }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;

    #[test]
    fn my_tags_are_undoable() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", None, None);
        lib.set_my_tags(a, vec![MyTag::new("Mood", "Dark"), MyTag::new("Situation", "Peak")]).unwrap();
        assert_eq!(lib.my_tags(a).unwrap().len(), 2);
        assert_eq!(lib.my_tag_catalog().unwrap()["Mood"]["Dark"], 1);
        lib.set_my_tags(a, vec![MyTag::new("Mood", "Warm")]).unwrap();
        assert_eq!(lib.all_my_tags().unwrap()[&a], vec![MyTag::new("Mood", "Warm")]);
        lib.undo().unwrap();
        assert_eq!(lib.my_tags(a).unwrap().len(), 2);
    }
}
