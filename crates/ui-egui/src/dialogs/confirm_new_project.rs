//! Confirmation dialog before replacing an unsaved project workspace with a new project.

use serde_json::Value;

use super::DialogSpec;
use crate::VectorcraftApp;
use crate::state::Dialog;
use crate::theme::Tokens;

pub const KIND: &str = "confirmNewProject";

pub(crate) const SPEC: DialogSpec = DialogSpec {
    heading: |_| "Save Project Changes?".into(),
    body,
    confirm,
    ok: Some("Save"),
    discard: Some("Don't Save"),
    max_width: Some(440.0),
    ..DialogSpec::FORM
};

fn body(_app: &mut VectorcraftApp, ui: &mut egui::Ui, _d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new("Do you want to save changes to the current project before creating a new one?").font(crate::theme::semibold(14.0)));
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Your unsaved changes will be lost if you don't save them.").color(t.text_dim));
    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let discard = d.bool("discard");
    app.ui.dialog = None;
    if discard {
        crate::scientific::project::open_create_project_dialog(app, "new");
    } else if let Some(ref path) = app.scientific.project_path.clone() {
        crate::scientific::project::save_project_to_path(app, path, None)?;
        crate::scientific::project::open_create_project_dialog(app, "new");
    } else {
        crate::scientific::project::open_create_project_dialog(app, "save");
    }
    Ok(Value::Null)
}
