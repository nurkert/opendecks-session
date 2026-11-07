//! Track table columns: which are shown, in which order, and the sort
//! (primary column plus secondary columns added with ⇧-click). The choice
//! and the secondary sort persist in the settings (`browser`).

use crate::app::App;
use crate::settings::SortSpec;
use crate::{ColumnChoice, ColumnView, models::model};

/// One column the table can show.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Column {
    /// Stable id (settings, accessible label `browser.header.<id>`).
    pub id: &'static str,
    /// Header text (English; translated with `od_i18n`).
    pub title: &'static str,
    /// Fixed width in px; 0 = shares the free width by `stretch`.
    pub width: f32,
    pub stretch: f32,
    pub right: bool,
    /// Sort column name (see `tracks::sort_key`); "" = not sortable.
    pub sort: &'static str,
}

const fn col(
    id: &'static str,
    title: &'static str,
    width: f32,
    stretch: f32,
    right: bool,
    sort: &'static str,
) -> Column {
    Column {
        id,
        title,
        width,
        stretch,
        right,
        sort,
    }
}

/// Every column, in the order of the column menu.
pub(crate) const COLUMNS: &[Column] = &[
    col("artwork", "ART", 34.0, 0.0, false, ""),
    col("preview", "PREVIEW", 116.0, 0.0, false, ""),
    col("title", "TITLE", 0.0, 3.0, false, "title"),
    col("artist", "ARTIST", 0.0, 2.0, false, "artist"),
    col("album", "ALBUM", 0.0, 2.0, false, "album"),
    col("genre", "GENRE", 0.0, 1.0, false, "genre"),
    col("label", "LABEL", 0.0, 1.0, false, "label"),
    col("comment", "COMMENT", 0.0, 2.0, false, "comment"),
    col("my-tag", "MY TAG", 0.0, 1.5, false, ""),
    col("colour", "COLOUR", 28.0, 0.0, false, "colour"),
    col("bpm", "BPM", 58.0, 0.0, true, "bpm"),
    col("key", "KEY", 46.0, 0.0, true, "key"),
    col("time", "TIME", 52.0, 0.0, true, "duration"),
    col("year", "YEAR", 44.0, 0.0, true, "year"),
    col("date-added", "ADDED", 86.0, 0.0, true, "date_added"),
    col("plays", "PLAYS", 46.0, 0.0, true, "play_count"),
    col("bitrate", "BITRATE", 62.0, 0.0, true, "bitrate"),
    col("rating", "RATING", 74.0, 0.0, true, "rating"),
];

/// The columns of a fresh install.
pub(crate) const DEFAULT_COLUMNS: &[&str] = &[
    "artwork", "preview", "title", "artist", "genre", "bpm", "key", "time", "rating",
];

/// The columns of an Explorer folder (files that may not be in the
/// collection: no genre, rating or play data).
pub(crate) const EXPLORER_COLUMNS: &[&str] = &[
    "artwork", "preview", "title", "bpm", "key", "artist", "time",
];

/// The column that cannot be hidden.
pub(crate) const FIXED: &str = "title";

pub(crate) fn column(id: &str) -> Option<&'static Column> {
    COLUMNS.iter().find(|c| c.id == id)
}

/// Known, unique ids with the title present; empty means the defaults.
pub(crate) fn sanitize(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if column(id).is_some() && !out.contains(id) {
            out.push(id.clone());
        }
    }
    if out.is_empty() {
        return DEFAULT_COLUMNS.iter().map(|s| s.to_string()).collect();
    }
    if !out.iter().any(|c| c == FIXED) {
        out.insert(0, FIXED.to_string());
    }
    out
}

/// Shows or hides `id` (the title stays). New columns go before the rating.
pub(crate) fn toggle(ids: &[String], id: &str) -> Vec<String> {
    let mut v = sanitize(ids);
    if id == FIXED || column(id).is_none() {
        return v;
    }
    if let Some(i) = v.iter().position(|c| c == id) {
        v.remove(i);
    } else {
        let at = v.iter().position(|c| c == "rating").unwrap_or(v.len());
        v.insert(at, id.to_string());
    }
    v
}

/// Moves `id` to position `to` (index among the shown columns before the
/// move; dropping on a column puts it in that column's place).
pub(crate) fn move_to(ids: &[String], id: &str, to: usize) -> Vec<String> {
    let mut v = sanitize(ids);
    let Some(from) = v.iter().position(|c| c == id) else {
        return v;
    };
    let c = v.remove(from);
    v.insert(to.min(v.len()), c);
    v
}

/// Secondary sort after ⇧-click on `column`: added (default direction),
/// reversed when already there; the primary column is never secondary.
pub(crate) fn shift_sort(
    then: &[SortSpec],
    primary: &str,
    column: &str,
    default_descending: bool,
) -> Vec<SortSpec> {
    let mut v: Vec<SortSpec> = then
        .iter()
        .filter(|s| s.column != primary)
        .cloned()
        .collect();
    if column == primary {
        return v;
    }
    match v.iter_mut().find(|s| s.column == column) {
        Some(s) => s.descending = !s.descending,
        None => v.push(SortSpec {
            column: column.to_string(),
            descending: default_descending,
        }),
    }
    v.truncate(3);
    v
}

/// Header models: the shown columns (with their sort rank) and the menu.
pub(crate) fn refresh(app: &App) {
    let st = app.state.borrow();
    // A folder of the Explorer: the reduced set that files on disk carry.
    // The touch screen leaves out TIME there (room for 44 px header keys).
    let ids: Vec<String> = if st.explorer.open.is_some() {
        EXPLORER_COLUMNS
            .iter()
            .filter(|c| !(st.config.kiosk && **c == "time"))
            .map(|s| s.to_string())
            .collect()
    } else {
        sanitize(&st.settings.browser.columns)
    };
    // The 800 px touch screen has no room for the ARTWORK column (the deck
    // headers show the cover).
    let ids: Vec<String> = ids.into_iter().filter(|c| !(st.config.kiosk && c == "artwork")).collect();
    let b = &st.browser;
    let kiosk = st.config.kiosk;
    let then = &st.settings.browser.sort_then;
    let views: Vec<ColumnView> = ids
        .iter()
        .filter_map(|id| column(id))
        .map(|c| {
            let (rank, descending) = if !c.sort.is_empty() && c.sort == b.sort {
                (1, b.descending)
            } else {
                then.iter()
                    .position(|s| s.column == c.sort && !c.sort.is_empty())
                    .map(|i| (i as i32 + 2, then[i].descending))
                    .unwrap_or((0, false))
            };
            ColumnView {
                id: c.id.into(),
                title: od_i18n::tr(c.title).into(),
                // The touch screen keeps the preview narrow (room for the
                // 44 px wide header keys).
                width: if kiosk && c.id == "preview" {
                    60.0
                } else {
                    c.width
                },
                stretch: c.stretch,
                right: c.right,
                sort: c.sort.into(),
                rank,
                descending,
            }
        })
        .collect();
    let choices: Vec<ColumnChoice> = COLUMNS
        .iter()
        .filter(|c| c.id != FIXED)
        .map(|c| ColumnChoice {
            id: c.id.into(),
            title: od_i18n::tr(c.title).into(),
            visible: ids.iter().any(|i| i == c.id),
        })
        .collect();
    app.ui.set_columns(model(views));
    app.ui.set_column_choices(model(choices));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn choice_keeps_title_and_known_ids() {
        assert_eq!(sanitize(&[]), ids(DEFAULT_COLUMNS));
        assert_eq!(
            sanitize(&ids(&["bpm", "nonsense", "bpm"])),
            ids(&["title", "bpm"])
        );
        let v = toggle(&ids(DEFAULT_COLUMNS), "album");
        assert_eq!(v[v.len() - 2], "album", "new columns go before the rating");
        assert!(!toggle(&v, "album").contains(&"album".to_string()));
        assert!(
            toggle(&v, "title").contains(&"title".to_string()),
            "the title cannot be hidden"
        );
        for c in COLUMNS {
            assert!(od_i18n::has_german(c.title), "no German for {}", c.title);
            assert!(
                c.sort.is_empty() || crate::browser::sort_key_known(c.sort),
                "{} sorts by a known key",
                c.id
            );
        }
    }

    #[test]
    fn reorder_and_secondary_sort() {
        let v = move_to(&ids(&["title", "artist", "bpm", "key"]), "key", 1);
        assert_eq!(v, ids(&["title", "key", "artist", "bpm"]));
        assert_eq!(
            move_to(&v, "title", 99),
            ids(&["key", "artist", "bpm", "title"])
        );
        let t = shift_sort(&[], "title", "bpm", true);
        assert_eq!(
            t,
            vec![SortSpec {
                column: "bpm".into(),
                descending: true
            }]
        );
        let t = shift_sort(&t, "title", "bpm", true);
        assert!(!t[0].descending, "⇧-click again reverses");
        let t = shift_sort(&t, "title", "key", false);
        assert_eq!(t.len(), 2);
        assert_eq!(
            shift_sort(&t, "bpm", "bpm", true).len(),
            1,
            "the primary column leaves the secondary list"
        );
    }
}
