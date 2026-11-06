//! Explorer: folders on disk (placeholder until the Explorer lands).

use crate::app::App;
use od_core::TrackId;
use od_library::Library;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) struct ExplorerFile {
    pub track: Option<TrackId>,
}

#[derive(Default)]
pub(crate) struct ExplorerState {
    /// The folder shown in the track list, if any.
    pub open: Option<PathBuf>,
    pub files: Vec<ExplorerFile>,
}

impl ExplorerState {
    pub fn load(_lib: &Library) -> Self {
        Self::default()
    }
    pub fn close(&mut self) {
        self.open = None;
        self.files.clear();
    }
    pub fn track_at(&self, i: usize) -> Option<TrackId> {
        self.files.get(i).and_then(|f| f.track)
    }
    pub fn selected_ids(&self, selection: &BTreeSet<usize>) -> Vec<TrackId> {
        selection.iter().filter_map(|&i| self.track_at(i)).collect()
    }
    pub fn sort(&mut self, _column: &str, _descending: bool, _selection: &mut BTreeSet<usize>) {}
}

#[derive(Default)]
pub(crate) struct ExplorerRuntime;

pub(crate) fn wire(_app: &Rc<App>) {}
pub(crate) fn refresh_tree(_app: &App) {}
pub(crate) fn refresh_files(_app: &App) {}
pub(crate) fn activate(_app: &Rc<App>, _i: usize, _deck: Option<usize>) {}
pub(crate) fn poll(_app: &App) {}
