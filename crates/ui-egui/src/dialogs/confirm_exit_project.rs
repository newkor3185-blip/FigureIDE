//! Confirmation dialog before exiting FigureIDE with unsaved project changes.

use serde_json::{Value, json};

use super::DialogSpec;
use crate::VectorcraftApp;
use crate::state::Dialog;
use crate::theme::{self, Tokens};

pub const KIND: &str = "confirmExitProject";

pub(crate) const SPEC: DialogSpec = DialogSpec {
    heading: |_| "Save FigureIDE Project?".into(),
    body,
    confirm,
    ok: Some("Save & Exit"),
    discard: Some("Don't Save"),
    max_width: Some(480.0),
    inputs: |_| &["save_r_workspace", "discard"],
    ..DialogSpec::FORM
};

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.label(
        egui::RichText::new("You have unsaved changes in your project. Would you like to save your project before closing FigureIDE?")
            .font(theme::semibold(13.0)),
    );
    ui.add_space(8.0);

    let r_active = app.scientific.session_active;
    let mut save_r_workspace = if r_active {
        if let Some(v) = d.fields.get("save_r_workspace").and_then(Value::as_bool) {
            v
        } else {
            app.scientific.save_r_workspace_preference.unwrap_or(true)
        }
    } else {
        false
    };

    ui.horizontal(|ui| {
        let cb = egui::Checkbox::new(&mut save_r_workspace, "Save R Workspace (.RData) with Project");
        let cb_resp = ui.add_enabled(r_active, cb);
        if cb_resp.changed() {
            d.fields.insert("save_r_workspace".into(), json!(save_r_workspace));
        }
        if !r_active {
            ui.label(egui::RichText::new("(R session is not active — workspace cannot be saved)").color(t.text_dim));
        }
    });

    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let discard = d.bool("discard");
    if discard {
        app.ui.dialog = None;
        vectorcraft_engine::cmd::recovery::forget_all(&mut app.session);
        app.ui.status = "quit".into();
        return Ok(Value::Null);
    }

    let save_r_workspace = if !app.scientific.session_active {
        Some(false)
    } else if d.fields.contains_key("save_r_workspace") {
        Some(d.bool("save_r_workspace"))
    } else {
        app.scientific.save_r_workspace_preference.or(Some(true))
    };

    if let Some(ref proj_path) = app.scientific.project_path.clone() {
        crate::scientific::project::save_project_to_path(app, proj_path, save_r_workspace)?;
        app.ui.dialog = None;
        vectorcraft_engine::cmd::recovery::forget_all(&mut app.session);
        app.ui.status = "quit".into();
        Ok(Value::Null)
    } else {
        app.ui.dialog = None;
        crate::scientific::project::open_create_project_dialog(app, "saveAndExit");
        Ok(Value::Null)
    }
}
