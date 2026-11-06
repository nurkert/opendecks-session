//! One-time rekordbox migration from the UI.

use crate::app::App;

pub(crate) fn run_rekordbox_import(app: &App) {
    app.ui.set_status_text("The rekordbox import is not available in this build yet.".into());
}
