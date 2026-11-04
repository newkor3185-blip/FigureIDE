//! Create Project dialog for FigureIDE folder-based workspaces.
//!
//! Provides PyCharm-like experience:
//! - Project Name
//! - Location (with Browse button for parent folder)
//! - Final Project Path preview
//! - Inline validation and conflict detection
//! - Creation of new folder with standard FigureIDE structure

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use vectorcraft_format::fide::{DestinationConflict, check_destination_conflict, validate_parent_location, validate_project_name};

use super::DialogSpec;
use crate::VectorcraftApp;
use crate::state::Dialog;
use crate::theme::{self, Tokens};

pub const KIND: &str = "createProject";

pub(crate) const SPEC: DialogSpec = DialogSpec {
    heading,
    body,
    confirm,
    ok: Some("Create Project"),
    ok_label: Some(ok_label),
    ok_enabled: Some(ok_enabled),
    min_width: 500.0,
    max_width: Some(620.0),
    inputs: |_| &["name", "location", "mode", "save_r_workspace"],
    ..DialogSpec::FORM
};

fn heading(d: &Dialog) -> String {
    match d.str("mode").as_str() {
        "saveAs" => "Save FigureIDE Project As".into(),
        "save" => "Save FigureIDE Project".into(),
        _ => "New FigureIDE Project".into(),
    }
}

fn ok_label(app: &VectorcraftApp) -> &'static str {
    if let Some(ref d) = app.ui.dialog
        && (d.str("mode") == "saveAs" || d.str("mode") == "save")
    {
        return "Save Project";
    }
    "Create Project"
}

fn ok_enabled(_app: &VectorcraftApp, d: &Dialog) -> bool {
    let name = d.str("name");
    let loc = d.str("location");
    if name.is_empty() || loc.is_empty() {
        return false;
    }
    if validate_project_name(&name).is_err() {
        return false;
    }
    let parent = Path::new(&loc);
    if validate_parent_location(parent).is_err() {
        return false;
    }
    let conflict = check_destination_conflict(parent, &name);
    matches!(conflict, DestinationConflict::None)
}

fn body(app: &mut VectorcraftApp, ui: &mut egui::Ui, d: &mut Dialog) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut name = d.str("name");
    let mut loc = d.str("location");

    // Grid layout for fields
    egui::Grid::new("create_project_grid").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
        // Row 1: Project Name
        ui.label(egui::RichText::new("Project Name:").font(theme::semibold(13.0)));
        let name_response = ui.add(egui::TextEdit::singleline(&mut name).hint_text("e.g. SingleCell_Analysis").desired_width(360.0));
        if name_response.changed() {
            d.fields.insert("name".into(), json!(name));
        }
        ui.end_row();

        // Row 2: Location
        ui.label(egui::RichText::new("Location:").font(theme::semibold(13.0)));
        ui.horizontal(|ui| {
            let loc_response = ui.add(egui::TextEdit::singleline(&mut loc).hint_text("Parent folder").desired_width(280.0));
            if loc_response.changed() {
                d.fields.insert("location".into(), json!(loc));
            }
            if ui.button("Browse…").clicked()
                && let Some(picked) = crate::picks::folder(app)
            {
                d.fields.insert("location".into(), json!(picked));
                loc = picked;
            }
        });
        ui.end_row();

        // Row 3: Final Project Path Preview
        ui.label(egui::RichText::new("Final Project Path:").font(theme::semibold(13.0)));
        let parent_path = PathBuf::from(&loc);
        let final_path =
            if name.is_empty() { parent_path.to_string_lossy().into_owned() } else { parent_path.join(&name).to_string_lossy().into_owned() };
        ui.add(egui::Label::new(egui::RichText::new(&final_path).color(t.text_dim).font(theme::mono(12.0))).wrap());
        ui.end_row();
    });

    ui.add_space(8.0);

    // Save R Workspace (.RData) with Project option
    let mut save_r_workspace = if let Some(v) = d.fields.get("save_r_workspace").and_then(Value::as_bool) {
        v
    } else {
        app.scientific.save_r_workspace_preference.unwrap_or(app.scientific.session_active)
    };
    let r_active = app.scientific.session_active;
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

    ui.add_space(8.0);

    // Validation & Conflict messaging
    let name_err = validate_project_name(&name).err();
    let loc_err = validate_parent_location(Path::new(&loc)).err();
    let conflict =
        if name_err.is_none() && loc_err.is_none() { check_destination_conflict(Path::new(&loc), &name) } else { DestinationConflict::None };

    if let Some(err) = name_err {
        ui.label(egui::RichText::new(format!("⚠ {err}")).color(t.error));
    } else if let Some(err) = loc_err {
        ui.label(egui::RichText::new(format!("⚠ {err}")).color(t.error));
    } else {
        match conflict {
            DestinationConflict::ExistingFigureIDEProject(p) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⚠ A FigureIDE project already exists at this location.").color(t.warning));
                    if ui.button("Open Existing Project").clicked() {
                        let path_str = p.to_string_lossy().into_owned();
                        app.ui.dialog = None;
                        let _ = crate::scientific::project::open_project_from_path(app, &path_str);
                    }
                });
            }
            DestinationConflict::DirectoryNotEmpty(p) => {
                ui.label(
                    egui::RichText::new(format!(
                        "⚠ Directory '{}' already exists and is not empty. Please choose a different name or location.",
                        p.display()
                    ))
                    .color(t.error),
                );
            }
            DestinationConflict::FileExists(p) => {
                ui.label(
                    egui::RichText::new(format!("⚠ A file already exists at '{}'. Please choose a different name or location.", p.display()))
                        .color(t.error),
                );
            }
            DestinationConflict::None => {
                ui.label(egui::RichText::new("✓ Project folder will be created automatically.").color(t.text_dim));
            }
        }
    }

    false
}

fn confirm(app: &mut VectorcraftApp, d: &Dialog) -> Result<Value, String> {
    let name = d.str("name");
    let loc = d.str("location");
    let mode = d.str("mode");
    let save_r_workspace = if d.fields.contains_key("save_r_workspace") { Some(d.bool("save_r_workspace")) } else { None };

    validate_project_name(&name).map_err(|e| e.to_string())?;
    let parent = Path::new(&loc);
    validate_parent_location(parent).map_err(|e| e.to_string())?;

    let target_dir = parent.join(&name);
    match check_destination_conflict(parent, &name) {
        DestinationConflict::None => {}
        DestinationConflict::ExistingFigureIDEProject(_) => {
            return Err("A FigureIDE project already exists at this location.".to_string());
        }
        DestinationConflict::DirectoryNotEmpty(_) | DestinationConflict::FileExists(_) => {
            return Err("A file or non-empty directory already exists at this location.".to_string());
        }
    }

    crate::scientific::project::create_or_save_project(app, &target_dir, &name, &mode, save_r_workspace)?;
    app.ui.dialog = None;
    if mode == "saveAndExit" {
        vectorcraft_engine::cmd::recovery::forget_all(&mut app.session);
        app.ui.status = "quit".into();
    }
    Ok(Value::Null)
}
