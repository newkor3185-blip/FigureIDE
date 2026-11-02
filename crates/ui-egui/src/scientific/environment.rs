use super::{
    BottomTab, append_console_code, cancel_rdata_import, clear_environment, compact_panel_style, environment_metadata_summary,
    environment_metadata_tooltip, import_rdata_dialog, panel_action, panel_caption, record_history, refresh_environment, remove_environment_object,
    request_help, request_packages, resolve_rdata_import, run_r, url_encode,
};

const ENV_HEADER_HEIGHT: f32 = 24.0;
const ENV_CATEGORY_ROW_HEIGHT: f32 = 22.0;
const ENV_ROW_HEIGHT: f32 = 24.0;
const ENV_ACTIONS_WIDTH: f32 = 68.0;
const ENV_SPLITTER_WIDTH: f32 = 8.0;
const ENV_DEFAULT_NAME_WIDTH: f32 = 116.0;
const ENV_ICON_BUTTON_SIZE: f32 = 20.0;
const ENV_ICON_BUTTON_GAP: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
struct EnvironmentTableColumns {
    total_width: f32,
    min_name_width: f32,
    max_name_width: f32,
    name_width: f32,
    name_left: f32,
    name_right: f32,
    splitter_left: f32,
    splitter_right: f32,
    type_left: f32,
    type_right: f32,
    actions_left: f32,
    actions_right: f32,
}

impl EnvironmentTableColumns {
    fn compute(left: f32, available_width: f32, preferred_name_width: f32) -> Self {
        let total_width = available_width.max(180.0);
        let actions_width = ENV_ACTIONS_WIDTH.min(total_width * 0.32);
        let flexible = (total_width - actions_width - ENV_SPLITTER_WIDTH).max(96.0);
        let min_name_width = 72.0_f32.min(flexible * 0.35);
        let min_type_width = 80.0_f32.min(flexible * 0.35);
        let max_name_width = (flexible - min_type_width).max(min_name_width);
        let base_preferred =
            if preferred_name_width.is_finite() && preferred_name_width > 0.0 { preferred_name_width } else { ENV_DEFAULT_NAME_WIDTH };
        let name_width = base_preferred.clamp(min_name_width, max_name_width);
        let name_left = left;
        let name_right = name_left + name_width;
        let splitter_left = name_right;
        let splitter_right = splitter_left + ENV_SPLITTER_WIDTH;
        let actions_right = left + total_width;
        let actions_left = (actions_right - actions_width).max(splitter_right);
        let type_left = splitter_right;
        let type_right = actions_left;
        Self {
            total_width,
            min_name_width,
            max_name_width,
            name_width,
            name_left,
            name_right,
            splitter_left,
            splitter_right,
            type_left,
            type_right,
            actions_left,
            actions_right,
        }
    }

    fn splitter_center_x(&self) -> f32 {
        (self.splitter_left + self.splitter_right) * 0.5
    }
}

fn environment_name_width_id() -> egui::Id {
    egui::Id::unique("r-environment-name-column-width")
}

fn load_environment_name_column_width(ctx: &egui::Context) -> f32 {
    ctx.data_mut(|d| d.get_persisted::<f32>(environment_name_width_id())).unwrap_or(ENV_DEFAULT_NAME_WIDTH)
}

fn save_environment_name_column_width(ctx: &egui::Context, width: f32) {
    ctx.data_mut(|d| d.insert_persisted(environment_name_width_id(), width));
}

fn category_open_id(category: &str) -> egui::Id {
    egui::Id::unique("r-environment-category-open").with(category)
}

fn is_category_open(ctx: &egui::Context, category: &str) -> bool {
    ctx.data_mut(|d| d.get_persisted::<bool>(category_open_id(category))).unwrap_or(true)
}

fn set_category_open(ctx: &egui::Context, category: &str, open: bool) {
    ctx.data_mut(|d| d.insert_persisted(category_open_id(category), open));
}

fn truncate_to_width(ui: &egui::Ui, text: &str, font: egui::FontId, max_width: f32) -> (String, bool) {
    if text.is_empty() || max_width <= 0.0 {
        return (String::new(), !text.is_empty());
    }
    let full = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE);
    if full.size().x <= max_width {
        return (text.to_owned(), false);
    }
    let char_boundaries: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    let mut low = 0usize;
    let mut high = char_boundaries.len();
    let mut best = String::from("…");
    while low < high {
        let mid = low + (high - low) / 2;
        let Some(&byte_end) = char_boundaries.get(mid) else { break };
        let Some(prefix) = text.get(..byte_end) else { break };
        let candidate = format!("{prefix}…");
        let width = ui.painter().layout_no_wrap(candidate.clone(), font.clone(), egui::Color32::WHITE).size().x;
        if width <= max_width {
            best = candidate;
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    (best, true)
}

fn format_type_structure(object: &crate::r_session::RObject) -> String {
    let summary = environment_metadata_summary(object);
    if !summary.trim().is_empty() {
        summary
    } else if let Some(first_class) = object.classes.first().filter(|c| !c.is_empty()) {
        first_class.clone()
    } else {
        object.data_type.clone()
    }
}

fn environment_row_tooltip(object: &crate::r_session::RObject) -> String {
    let structure = format_type_structure(object);
    format!("{}\nType / Structure: {structure}\n{}", object.variable, environment_metadata_tooltip(object))
}

pub(super) fn show_environment(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.scope(|ui| {
        compact_panel_style(ui);
        show_environment_entries(app, ui);
    });
}

fn show_environment_entries(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let query = app.scientific.environment_filter.to_lowercase();
    let objects: Vec<_> = app.scientific.objects.iter().filter(|item| item.variable.to_lowercase().contains(&query)).cloned().collect();
    ui.horizontal_wrapped(|ui| {
        ui.strong("Global Environment");
        panel_caption(
            ui,
            if query.is_empty() { format!("{} objects", objects.len()) } else { format!("{}/{}", objects.len(), app.scientific.objects.len()) },
        )
        .on_hover_text(format!("{} of {} objects shown", objects.len(), app.scientific.objects.len()));
        if panel_action(ui, "Refresh", !app.scientific.running, false).clicked() {
            refresh_environment(&mut app.scientific);
        }
        if panel_action(ui, "Save R Workspace", !app.scientific.running, false).clicked() {
            if let Err(err) = app.run("r.workspace.save", serde_json::json!({})) {
                app.status(format!("Save R Workspace failed: {err}"));
            }
        }
        if panel_action(ui, "Load R Workspace", !app.scientific.running, false).clicked() {
            if let Err(err) = app.run("r.workspace.load", serde_json::json!({})) {
                app.status(format!("Load R Workspace failed: {err}"));
            }
        }
        if app.scientific.resolve_project_rdata_path().is_some() {
            if panel_action(ui, "Restore R Workspace", !app.scientific.running, false).clicked() {
                super::restore_project_rdata_action(app);
            }
        }
        if panel_action(ui, "Import RData", !app.scientific.running, false).clicked() {
            crate::picks::button(app, import_rdata_dialog);
        }
        if panel_action(ui, "Check Dependencies", !app.scientific.running, false).clicked() {
            let _ = app.run("r.environment.diagnostics", serde_json::json!({}));
        }
        if panel_action(ui, "Clear All", !app.scientific.running, true).clicked() {
            app.scientific.confirm_environment_clear = true;
        }
    });
    ui.add(egui::TextEdit::singleline(&mut app.scientific.environment_filter).desired_width(ui.available_width()).hint_text("Filter objects"));
    if let Some(pending) = app.scientific.pending_rdata_import.clone() {
        ui.group(|ui| {
            let file_name = std::path::Path::new(&pending.file_path).file_name().and_then(|f| f.to_str()).unwrap_or(&pending.file_path);
            let n_total = pending.total_objects;
            let n_coll = pending.collisions.len();
            ui.horizontal_wrapped(|ui| {
                ui.strong("Import RData:");
                ui.label(format!("{file_name} ({n_total} objects, {n_coll} collision(s))"));
                if panel_action(ui, "Overwrite", !app.scientific.running, true).clicked() {
                    resolve_rdata_import(app, crate::r_session::ImportCollisionPolicy::Overwrite);
                }
                if panel_action(ui, "Skip", !app.scientific.running, false).clicked() {
                    resolve_rdata_import(app, crate::r_session::ImportCollisionPolicy::Skip);
                }
                if panel_action(ui, "Cancel", !app.scientific.running, false).clicked() {
                    cancel_rdata_import(&mut app.scientific);
                }
            });
            if !pending.collisions.is_empty() {
                let preview = pending.collisions.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                let suffix = if pending.collisions.len() > 5 { "..." } else { "" };
                panel_caption(ui, format!("Collisions: {preview}{suffix}"));
            }
        });
    }
    if app.scientific.confirm_environment_clear {
        ui.horizontal_wrapped(|ui| {
            ui.label("Clear all objects from .GlobalEnv?");
            if panel_action(ui, "Confirm", true, true).clicked() {
                app.scientific.confirm_environment_clear = false;
                clear_environment(&mut app.scientific);
            }
            if panel_action(ui, "Cancel", true, false).clicked() {
                app.scientific.confirm_environment_clear = false;
            }
        });
    }
    if let Some(name) = app.scientific.confirm_object_remove.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Remove {name} from R?"));
            if panel_action(ui, "Confirm Remove", true, true).clicked() {
                app.scientific.confirm_object_remove = None;
                remove_environment_object(&mut app.scientific, name);
            }
            if panel_action(ui, "Cancel", true, false).clicked() {
                app.scientific.confirm_object_remove = None;
            }
        });
    }
    if objects.is_empty() {
        panel_caption(ui, if query.is_empty() { "No objects in the global environment." } else { "No matching objects." });
    }

    let previous_item_spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;

    let table_left = ui.cursor().min.x;
    let available_width = ui.available_width();
    let preferred_name_width = load_environment_name_column_width(ui.ctx());
    let mut cols = EnvironmentTableColumns::compute(table_left, available_width, preferred_name_width);

    if let Some(resized_name_width) = show_environment_table_header(ui, &cols) {
        let clamped = resized_name_width.clamp(cols.min_name_width, cols.max_name_width);
        save_environment_name_column_width(ui.ctx(), clamped);
        cols = EnvironmentTableColumns::compute(table_left, available_width, clamped);
    }

    let mut global_row_index = 0usize;
    for category in ["Data", "Values", "Functions"] {
        let section: Vec<_> = objects.iter().filter(|object| object.category == category).collect();
        let open = show_environment_category_row(ui, &cols, category, section.len());
        if open {
            for object in section {
                show_environment_object_row(app, ui, &cols, object, global_row_index);
                global_row_index = global_row_index.saturating_add(1);
            }
        }
    }

    ui.spacing_mut().item_spacing.y = previous_item_spacing;
}

fn show_environment_table_header(ui: &mut egui::Ui, cols: &EnvironmentTableColumns) -> Option<f32> {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let (header_rect, _) = ui.allocate_exact_size(egui::vec2(cols.total_width, ENV_HEADER_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(header_rect, 0.0, tokens.panel_darker);
    ui.painter().hline(header_rect.x_range(), header_rect.top(), egui::Stroke::new(1.0, tokens.divider));
    ui.painter().hline(header_rect.x_range(), header_rect.bottom(), egui::Stroke::new(1.0, tokens.divider));
    ui.painter().vline(cols.actions_left, header_rect.y_range().shrink(4.0), egui::Stroke::new(1.0, tokens.divider));

    let header_font = crate::theme::semibold(tokens.font_size - 1.0);

    // Column 1: Name header
    let name_rect = egui::Rect::from_min_max(
        egui::pos2(cols.name_left + 6.0, header_rect.top() + 2.0),
        egui::pos2((cols.name_right - 4.0).max(cols.name_left + 8.0), header_rect.bottom() - 2.0),
    );
    let name_resp = ui.interact(name_rect, ui.make_persistent_id("r-env-col-header-name"), egui::Sense::hover());
    name_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Name"));
    let (name_text, _) = truncate_to_width(ui, "Name", header_font.clone(), name_rect.width());
    ui.painter().with_clip_rect(name_rect).text(
        egui::pos2(name_rect.left(), header_rect.center().y),
        egui::Align2::LEFT_CENTER,
        name_text,
        header_font.clone(),
        tokens.text_strong,
    );

    // Resizable boundary between Name and Type / Structure
    let splitter_rect = egui::Rect::from_min_max(
        egui::pos2(cols.splitter_left, header_rect.top() + 1.0),
        egui::pos2(cols.splitter_right, header_rect.bottom() - 1.0),
    );
    let splitter_resp = ui.interact(splitter_rect, ui.make_persistent_id("r-env-name-type-splitter"), egui::Sense::click_and_drag());
    if splitter_resp.clicked() || splitter_resp.drag_started() {
        splitter_resp.request_focus();
    }
    splitter_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Resize Name and Type columns"));
    let splitter_active = splitter_resp.hovered() || splitter_resp.dragged() || splitter_resp.has_focus();
    if splitter_active {
        ui.painter().rect_filled(splitter_rect.shrink2(egui::vec2(1.0, 2.0)), 2.0, tokens.hover);
    }
    ui.painter().vline(
        cols.splitter_center_x(),
        header_rect.y_range().shrink(3.0),
        egui::Stroke::new(if splitter_active { 1.5 } else { 1.0 }, if splitter_active { tokens.accent } else { tokens.divider }),
    );
    splitter_resp
        .clone()
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("Resize Name and Type / Structure columns: drag or use Left/Right arrow keys");

    let start_id = splitter_resp.id.with("drag-start");
    if splitter_resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, cols.name_width));
    }
    let mut updated_width = None;
    if splitter_resp.dragged() {
        let initial = ui.ctx().data(|d| d.get_temp::<f32>(start_id)).unwrap_or(cols.name_width);
        let delta = splitter_resp.total_drag_delta().unwrap_or_default();
        updated_width = Some(initial + delta.x);
    }
    if splitter_resp.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<f32>(start_id));
    }
    if splitter_resp.has_focus() {
        let change = ui.input_mut(|i| {
            let step = if i.modifiers.shift { 32.0 } else { 12.0 };
            if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft) {
                -step
            } else if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight) {
                step
            } else {
                0.0
            }
        });
        if change != 0.0 {
            updated_width = Some(cols.name_width + change);
        }
    }

    // Column 2: Type / Structure header
    let type_rect = egui::Rect::from_min_max(
        egui::pos2(cols.type_left + 4.0, header_rect.top() + 2.0),
        egui::pos2((cols.type_right - 4.0).max(cols.type_left + 8.0), header_rect.bottom() - 2.0),
    );
    let type_resp = ui.interact(type_rect, ui.make_persistent_id("r-env-col-header-type"), egui::Sense::hover());
    type_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Type / Structure"));
    let (type_text, _) = truncate_to_width(ui, "Type / Structure", header_font.clone(), type_rect.width());
    ui.painter().with_clip_rect(type_rect).text(
        egui::pos2(type_rect.left(), header_rect.center().y),
        egui::Align2::LEFT_CENTER,
        type_text,
        header_font.clone(),
        tokens.text_strong,
    );

    // Column 3: Actions header (right-aligned)
    let actions_rect = egui::Rect::from_min_max(
        egui::pos2(cols.actions_left + 4.0, header_rect.top() + 2.0),
        egui::pos2((cols.actions_right - 6.0).max(cols.actions_left + 8.0), header_rect.bottom() - 2.0),
    );
    let actions_resp = ui.interact(actions_rect, ui.make_persistent_id("r-env-col-header-actions"), egui::Sense::hover());
    actions_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Actions"));
    let (actions_text, _) = truncate_to_width(ui, "Actions", header_font.clone(), actions_rect.width());
    ui.painter().with_clip_rect(actions_rect).text(
        egui::pos2(actions_rect.right(), header_rect.center().y),
        egui::Align2::RIGHT_CENTER,
        actions_text,
        header_font,
        tokens.text_strong,
    );

    updated_width
}

fn show_environment_category_row(ui: &mut egui::Ui, cols: &EnvironmentTableColumns, category: &str, count: usize) -> bool {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let mut open = is_category_open(ui.ctx(), category);
    let label = format!("{category} ({count})");
    let (row_rect, _) = ui.allocate_exact_size(egui::vec2(cols.total_width, ENV_CATEGORY_ROW_HEIGHT), egui::Sense::hover());
    let button_rect = row_rect.shrink2(egui::vec2(0.0, 0.5));
    let resp = ui.interact(button_rect, ui.make_persistent_id(("r-environment-section", category)), egui::Sense::click());
    if resp.clicked() {
        open = !open;
        set_category_open(ui.ctx(), category, open);
    }
    let bg = if resp.hovered() { tokens.hover } else { tokens.panel_darker };
    ui.painter().rect_filled(row_rect, 0.0, bg);
    ui.painter().hline(row_rect.x_range(), row_rect.bottom(), egui::Stroke::new(1.0, tokens.divider));

    let chevron_rect = egui::Rect::from_center_size(egui::pos2(row_rect.left() + 10.0, row_rect.center().y), egui::vec2(11.0, 11.0));
    crate::icons::paint(
        ui,
        if open { "chevron-down" } else { "chevron-right" },
        chevron_rect,
        if resp.hovered() { tokens.text_strong } else { tokens.icon },
    );

    let text_rect = egui::Rect::from_min_max(
        egui::pos2(row_rect.left() + 20.0, row_rect.top() + 1.0),
        egui::pos2((row_rect.right() - 6.0).max(row_rect.left() + 24.0), row_rect.bottom() - 1.0),
    );
    let font = crate::theme::semibold(tokens.font_size - 0.5);
    let (display_text, _) = truncate_to_width(ui, &label, font.clone(), text_rect.width());
    ui.painter().with_clip_rect(text_rect).text(
        egui::pos2(text_rect.left(), row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_text,
        font,
        tokens.text,
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), open, &label));
    open
}

fn show_environment_object_row(
    app: &mut crate::VectorcraftApp,
    ui: &mut egui::Ui,
    cols: &EnvironmentTableColumns,
    object: &crate::r_session::RObject,
    row_index: usize,
) {
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let selected = app.scientific.selected_object.as_deref() == Some(object.variable.as_str());
    let (row_rect, _) = ui.allocate_exact_size(egui::vec2(cols.total_width, ENV_ROW_HEIGHT), egui::Sense::hover());

    let name_cell_rect = egui::Rect::from_min_max(
        egui::pos2(cols.name_left, row_rect.top() + 1.0),
        egui::pos2((cols.name_right - 1.0).max(cols.name_left + 4.0), row_rect.bottom() - 1.0),
    );
    let name_resp = ui.interact(name_cell_rect, ui.make_persistent_id(("r-env-row-name", &object.variable)), egui::Sense::click());

    let type_cell_rect = egui::Rect::from_min_max(
        egui::pos2(cols.type_left + 1.0, row_rect.top() + 1.0),
        egui::pos2((cols.type_right - 2.0).max(cols.type_left + 4.0), row_rect.bottom() - 1.0),
    );
    let type_resp = ui.interact(type_cell_rect, ui.make_persistent_id(("r-env-row-type", &object.variable)), egui::Sense::hover());

    let row_hovered = name_resp.hovered() || type_resp.hovered();
    let row_fill = if selected {
        tokens.row_selected
    } else if row_hovered {
        tokens.hover
    } else if row_index % 2 == 1 {
        tokens.panel_darker
    } else {
        tokens.panel
    };
    ui.painter().rect_filled(row_rect, 0.0, row_fill);
    if selected {
        ui.painter().rect_stroke(row_rect, 0.0, egui::Stroke::new(1.0, tokens.accent), egui::StrokeKind::Inside);
    } else {
        ui.painter().hline(row_rect.x_range(), row_rect.bottom(), egui::Stroke::new(1.0, tokens.divider));
    }
    ui.painter().vline(cols.splitter_center_x(), row_rect.y_range().shrink(2.0), egui::Stroke::new(1.0, tokens.divider));
    ui.painter().vline(cols.actions_left, row_rect.y_range().shrink(2.0), egui::Stroke::new(1.0, tokens.divider));

    let tooltip = environment_row_tooltip(object);

    // Column 1: Variable Name
    let name_font = crate::theme::semibold(tokens.font_size - 0.5);
    let name_text_max_w = (name_cell_rect.width() - 10.0).max(8.0);
    let (display_name, _) = truncate_to_width(ui, &object.variable, name_font.clone(), name_text_max_w);
    ui.painter().with_clip_rect(name_cell_rect.shrink2(egui::vec2(2.0, 0.0))).text(
        egui::pos2(name_cell_rect.left() + 6.0, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_name,
        name_font,
        if selected { tokens.text_strong } else { tokens.text },
    );
    let can_view = object.classes.iter().any(|class| class == "data.frame" || class == "tbl_df" || class == "matrix" || class == "table")
        || (object.category == "Data" && !object.classes.iter().any(|c| c == "function"));
    name_resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, &object.variable));
    if name_resp.double_clicked() && can_view {
        app.scientific.open_data_viewer(&object.variable);
    } else if name_resp.on_hover_text(&tooltip).clicked() || (type_resp.hovered() && ui.input(|i| i.pointer.primary_clicked())) {
        app.scientific.selected_object = Some(object.variable.clone());
        if !object.plot_kind.is_empty() {
            app.scientific.selected_plot = Some(object.variable.clone());
        }
    }

    // Column 2: Type / Structure
    let type_summary = format_type_structure(object);
    let type_font = egui::FontId::proportional(tokens.font_size - 1.0);
    let type_text_max_w = (type_cell_rect.width() - 6.0).max(8.0);
    let (display_type, _) = truncate_to_width(ui, &type_summary, type_font.clone(), type_text_max_w);
    ui.painter().with_clip_rect(type_cell_rect.shrink2(egui::vec2(1.0, 0.0))).text(
        egui::pos2(type_cell_rect.left() + 3.0, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        display_type,
        type_font,
        if selected { tokens.text } else { tokens.text_dim },
    );
    type_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &type_summary));
    type_resp.on_hover_text(&tooltip);

    // Column 3: Right-aligned Icon Actions
    let is_plot = !object.plot_kind.is_empty();

    struct RowAction {
        label: &'static str,
        tooltip: String,
        icon: &'static str,
        destructive: bool,
        kind: RowActionKind,
    }
    enum RowActionKind {
        ViewData,
        PlotRegistry,
        Remove,
    }

    let mut actions = Vec::with_capacity(3);
    if can_view {
        actions.push(RowAction {
            label: "View Data",
            tooltip: format!("View Data: inspect {} in Data Viewer", object.variable),
            icon: "eye",
            destructive: false,
            kind: RowActionKind::ViewData,
        });
    }
    if is_plot {
        actions.push(RowAction {
            label: "Plot Registry",
            tooltip: format!("Plot Registry: open {} in Plot Registry", object.variable),
            icon: "chart-column",
            destructive: false,
            kind: RowActionKind::PlotRegistry,
        });
    }
    actions.push(RowAction {
        label: "Remove",
        tooltip: format!("Remove {} from R environment (requires confirmation)", object.variable),
        icon: "trash-2",
        destructive: true,
        kind: RowActionKind::Remove,
    });

    let count = actions.len();
    let group_width = count as f32 * ENV_ICON_BUTTON_SIZE + (count.saturating_sub(1)) as f32 * ENV_ICON_BUTTON_GAP;
    let start_x = (cols.actions_right - 4.0 - group_width).max(cols.actions_left + 2.0);
    let btn_y = row_rect.center().y - ENV_ICON_BUTTON_SIZE * 0.5;

    for (idx, action) in actions.into_iter().enumerate() {
        let btn_x = start_x + idx as f32 * (ENV_ICON_BUTTON_SIZE + ENV_ICON_BUTTON_GAP);
        let btn_rect = egui::Rect::from_min_size(egui::pos2(btn_x, btn_y), egui::vec2(ENV_ICON_BUTTON_SIZE, ENV_ICON_BUTTON_SIZE));
        let btn_id = ui.make_persistent_id(("r-env-row-action", &object.variable, action.label));
        let resp = ui.interact(btn_rect, btn_id, egui::Sense::click());
        if resp.is_pointer_button_down_on() {
            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(4), tokens.tool_active);
        } else if resp.hovered() {
            ui.painter().rect_filled(btn_rect, egui::CornerRadius::same(4), tokens.hover);
        }
        let icon_color = if action.destructive {
            tokens.error
        } else if resp.hovered() {
            tokens.text_strong
        } else {
            tokens.icon
        };
        crate::icons::paint(ui, action.icon, btn_rect.shrink(3.5), icon_color);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), action.label));
        if resp.on_hover_text(action.tooltip).clicked() {
            match action.kind {
                RowActionKind::ViewData => {
                    app.scientific.selected_object = Some(object.variable.clone());
                    app.scientific.open_data_viewer(&object.variable);
                }
                RowActionKind::PlotRegistry => {
                    app.scientific.selected_object = Some(object.variable.clone());
                    app.scientific.selected_plot = Some(object.variable.clone());
                    super::focus_graph_tab(app);
                }
                RowActionKind::Remove => {
                    app.scientific.confirm_object_remove = Some(object.variable.clone());
                }
            }
        }
    }
}

pub(super) fn show_history(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("R History");
        ui.add(egui::TextEdit::singleline(&mut app.scientific.history_filter).hint_text("Search history"));
    });
    let filter = app.scientific.history_filter.to_lowercase();
    let history = app.scientific.console_history.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for command in history.into_iter().filter(|command| command.to_lowercase().contains(&filter)) {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&command).monospace());
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(command.clone());
                }
                if ui.button("Send to Editor").clicked()
                    && let Some(script) = app.scientific.active_script_mut()
                {
                    if !script.text.ends_with('\n') {
                        script.text.push('\n');
                    }
                    script.text.push_str(&command);
                }
                if ui.add_enabled(!app.scientific.running, egui::Button::new("Run")).clicked() {
                    record_history(&mut app.scientific, &command);
                    append_console_code(&mut app.scientific, &command);
                    run_r(&mut app.scientific, command.clone(), false);
                }
            });
        }
    });
}

pub(super) fn show_help(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("R Help");
        ui.add(egui::TextEdit::singleline(&mut app.scientific.help_query).hint_text("Function or topic"));
        if ui.add_enabled(!app.scientific.running, egui::Button::new("Search Help")).clicked() {
            request_help(&mut app.scientific);
        }
        if ui.button("Open in browser").clicked() && !app.scientific.help_query.trim().is_empty() {
            let topic = app.scientific.help_query.trim();
            let url = format!("https://rdrr.io/search?q={}", url_encode(topic));
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    });
    ui.label(&app.scientific.r_status);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(egui::RichText::new(&app.scientific.help_text).monospace());
    });
}

pub(super) fn show_packages(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("R Packages");
        ui.add(egui::TextEdit::singleline(&mut app.scientific.package_filter).hint_text("Filter packages"));
        if ui.add_enabled(!app.scientific.running, egui::Button::new("Refresh")).clicked() {
            request_packages(&mut app.scientific);
        }
    });
    let query = app.scientific.package_filter.to_lowercase();
    let packages = app.scientific.packages.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for package in packages.into_iter().filter(|package| package.name.to_lowercase().contains(&query)) {
            let name = package.name;
            let version = package.version;
            let loaded = package.loaded;
            ui.horizontal(|ui| {
                ui.label(&name);
                ui.label(version);
                ui.label(if loaded { "Loaded" } else { "Installed" });
                if !loaded && ui.add_enabled(!app.scientific.running, egui::Button::new("Load")).clicked() {
                    let command = format!("library({:?})", name);
                    record_history(&mut app.scientific, &command);
                    append_console_code(&mut app.scientific, &command);
                    run_r(&mut app.scientific, command, false);
                }
                if ui.button("Help").clicked() {
                    app.scientific.help_query = name.clone();
                    app.ui.r_studio_tab = BottomTab::Help.key().into();
                    request_help(&mut app.scientific);
                }
            });
        }
    });
    if app.scientific.packages.is_empty() && !app.scientific.running {
        ui.label("Package list has not been loaded.");
    }
}
