//! Slint model helpers: persistent `VecModel`s updated row by row.

use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;

/// A one-off model (lists that are replaced as a whole).
pub fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// A model that stays bound to its Slint property; refreshes only touch the
/// rows that changed, so views keep their state and repaint less.
pub struct Rows<T: 'static> {
    inner: Rc<VecModel<T>>,
}

impl<T: Clone + 'static> Default for Rows<T> {
    fn default() -> Self {
        Self { inner: Rc::new(VecModel::default()) }
    }
}

impl<T: Clone + 'static> Rows<T> {
    /// The handle to bind to a Slint property once.
    pub fn handle(&self) -> ModelRc<T> {
        ModelRc::from(self.inner.clone())
    }

    pub fn len(&self) -> usize {
        self.inner.row_count()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn get(&self, i: usize) -> Option<T> {
        self.inner.row_data(i)
    }

    /// Replaces the content: a length change resets the model once, equal
    /// lengths update only the rows for which `same` is false. Returns the
    /// number of rows written.
    pub fn sync(&self, rows: Vec<T>, same: impl Fn(&T, &T) -> bool) -> usize {
        if rows.len() != self.inner.row_count() {
            let n = rows.len();
            self.inner.set_vec(rows);
            return n;
        }
        let mut written = 0;
        for (i, r) in rows.into_iter().enumerate() {
            let changed = self.inner.row_data(i).is_none_or(|old| !same(&old, &r));
            if changed {
                self.inner.set_row_data(i, r);
                written += 1;
            }
        }
        written
    }

    /// Writes one row.
    pub fn set(&self, i: usize, row: T) {
        if i < self.inner.row_count() {
            self.inner.set_row_data(i, row);
        }
    }
}

impl<T: Clone + PartialEq + 'static> Rows<T> {
    /// [`Rows::sync`] with plain equality.
    pub fn sync_eq(&self, rows: Vec<T>) -> usize {
        self.sync(rows, |a, b| a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_writes_only_changed_rows() {
        let rows = Rows::<i32>::default();
        assert_eq!(rows.sync_eq(vec![1, 2, 3]), 3);
        assert_eq!(rows.sync_eq(vec![1, 2, 3]), 0);
        assert_eq!(rows.sync_eq(vec![1, 5, 3]), 1);
        assert_eq!(rows.get(1), Some(5));
        assert_eq!(rows.sync_eq(vec![7]), 1);
        assert_eq!(rows.len(), 1);
        let h = rows.handle();
        assert_eq!(h.row_count(), 1);
    }

    #[test]
    fn custom_equality() {
        let rows = Rows::<(i32, &'static str)>::default();
        rows.sync_eq(vec![(1, "a")]);
        // Only the first field counts as identity here.
        assert_eq!(rows.sync(vec![(1, "b")], |a, b| a.0 == b.0), 0);
        assert_eq!(rows.get(0), Some((1, "a")));
    }
}
