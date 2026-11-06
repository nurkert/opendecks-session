//! The command core shared by all modules: status messages, error
//! reporting, the refresh after a library edit, and undo / redo.

use crate::app::App;
use crate::{browser, player, shell};

impl App {
    /// Shows a status message in the current language.
    pub(crate) fn status(&self, text: impl Into<String>) {
        self.ui.set_status_text(od_i18n::translate_message(&text.into()).into());
    }

    /// Shows a library error and turns the result into an option.
    pub(crate) fn report<T>(&self, r: od_library::Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.status(format!("{e}"));
                None
            }
        }
    }

    /// Reloads the current list and every view that shows library data.
    pub(crate) fn refresh_library(&self) {
        self.state.borrow_mut().load_rows();
        browser::refresh_tree(self);
        browser::refresh_tracks(self);
        player::refresh_detail(self);
        shell::refresh_status(self);
    }

    /// After a library edit: refresh and show `label` (if any).
    pub(crate) fn after_edit(&self, label: &str) {
        self.refresh_library();
        if !label.is_empty() {
            self.status(label.to_string());
        }
    }

    /// Tells the user that stick tracks cannot be edited; returns true when
    /// the current view is read-only.
    pub(crate) fn guard_read_only(&self) -> bool {
        if self.state.borrow().read_only() {
            self.status("Tracks on a stick are read-only here — edit them in your library and the stick follows");
            true
        } else {
            false
        }
    }

    pub(crate) fn undo(&self) {
        let r = self.state.borrow_mut().lib.undo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Undid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to undo".into()));
        }
    }

    pub(crate) fn redo(&self) {
        let r = self.state.borrow_mut().lib.redo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Redid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to redo".into()));
        }
    }
}
