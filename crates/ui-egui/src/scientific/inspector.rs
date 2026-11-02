use super::{
    DOC_POINTS_PER_MM, PanelAction, RLinkedPanel, linked_panel_metadata, panel_action, panel_caption, render_plot, selected_linked_panel,
    workspace_status,
};

/// Linked-plot dimensions in the native Properties dock and Graph settings.
pub(crate) fn show_inspector_plot_properties(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) -> bool {
    let Some(panel) = selected_linked_panel(app) else { return false };
    let tokens = crate::theme::Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(format!("R-linked plot · Panel {}", panel.label)).strong().color(tokens.accent));
    show_plot_properties_body(app, ui, &panel, false);
    true
}

pub(super) fn show_graph_plot_properties(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    if let Some(panel) = selected_linked_panel(app) {
        show_plot_properties_body(app, ui, &panel, true);
    }
}

fn show_plot_properties_body(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui, panel: &RLinkedPanel, auto_apply: bool) {
    // A committed Graph size may still be rendering. Keep that panel's pending size visible
    // when selection moves to another panel and back, so a later edit cannot revert it.
    let source = if auto_apply {
        app.scientific
            .graph_pending
            .get(&(panel.document_uid, panel.panel_id))
            .map(|request| (request.width_mm, request.height_mm))
            .unwrap_or((panel.width_mm, panel.height_mm))
    } else {
        (panel.width_mm, panel.height_mm)
    };
    let newly_selected = app.scientific.selected_properties_panel != Some(panel.panel_id);
    let unchanged_draft = app
        .scientific
        .properties_source
        .is_some_and(|old| (app.scientific.properties_width_mm - old.0).abs() < 0.01 && (app.scientific.properties_height_mm - old.1).abs() < 0.01);
    if newly_selected || (app.scientific.properties_source != Some(source) && unchanged_draft) {
        app.scientific.selected_properties_panel = Some(panel.panel_id);
        app.scientific.properties_width_mm = source.0;
        app.scientific.properties_height_mm = source.1;
        app.scientific.properties_aspect_ratio = if source.1 > 0.0 { source.0 / source.1 } else { 1.0 };
    }
    app.scientific.properties_source = Some(source);
    let tokens = crate::theme::Tokens::get(ui.ctx());
    let connected = app.scientific.plots.iter().any(|plot| plot.variable == panel.r_variable);
    ui.label(egui::RichText::new(&panel.r_variable).strong()).on_hover_text(linked_panel_metadata(&panel.r_variable, panel.panel_id));
    ui.label(
        egui::RichText::new(if connected { "Linked to R · SVG vectors" } else { "R variable unavailable · SVG vectors retained" })
            .size(tokens.font_size - 1.0)
            .color(if connected { tokens.text_dim } else { tokens.warning }),
    );
    ui.add_space(4.0);
    panel_caption(ui, "R plot size · mm");
    let width_response = ui
        .horizontal(|ui| {
            ui.label("W").on_hover_text("Output width in millimeters");
            ui.add_sized(
                [ui.available_width().max(80.0), 22.0],
                egui::DragValue::new(&mut app.scientific.properties_width_mm)
                    .range(1.0..=1000.0)
                    .speed(0.5)
                    .suffix(" mm")
                    .update_while_editing(false),
            )
        })
        .inner;
    let width_changed = width_response.changed();
    let height_response = ui
        .horizontal(|ui| {
            ui.label("H").on_hover_text("Output height in millimeters");
            ui.add_sized(
                [ui.available_width().max(80.0), 22.0],
                egui::DragValue::new(&mut app.scientific.properties_height_mm)
                    .range(1.0..=1000.0)
                    .speed(0.5)
                    .suffix(" mm")
                    .update_while_editing(false),
            )
        })
        .inner;
    let height_changed = height_response.changed();
    ui.checkbox(&mut app.scientific.properties_aspect_lock, "Lock aspect ratio");
    if app.scientific.properties_aspect_lock && app.scientific.properties_aspect_ratio.is_finite() && app.scientific.properties_aspect_ratio > 0.0 {
        if width_changed {
            app.scientific.properties_height_mm = app.scientific.properties_width_mm / app.scientific.properties_aspect_ratio;
        } else if height_changed {
            app.scientific.properties_width_mm = app.scientific.properties_height_mm * app.scientific.properties_aspect_ratio;
        }
    }
    let commit_size = |response: &egui::Response| {
        response.lost_focus() || response.drag_stopped() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)))
    };
    if auto_apply && (commit_size(&width_response) || commit_size(&height_response)) {
        let key = (panel.document_uid, panel.panel_id);
        let style = app.scientific.graph_pending.get(&key).map(|request| request.style.clone()).unwrap_or_else(|| panel.graph_style.clone());
        let width_mm = app.scientific.properties_width_mm;
        let height_mm = app.scientific.properties_height_mm;
        if let Err(error) = super::request_graph_apply(&mut app.scientific, panel, style, width_mm, height_mm) {
            app.scientific.r_status = format!("Could not apply plot size: {error}");
        }
    }
    panel_caption(ui, "Resizing renders the linked R plot at the new size.");
    ui.horizontal_wrapped(|ui| {
        if panel_action(ui, "Update from R", !app.scientific.running && connected, false)
            .on_hover_text("Refresh this linked SVG using the current R object and stored dimensions.")
            .clicked()
        {
            render_plot(
                &mut app.scientific,
                panel.document_uid,
                panel.r_variable.clone(),
                PanelAction::Refresh {
                    panel_id: panel.panel_id,
                    generated_id: panel.generated_id,
                    width_mm: panel.width_mm,
                    height_mm: panel.height_mm,
                },
            );
        }
        let can_apply = !app.scientific.running && connected;
        if !auto_apply && panel_action(ui, "Apply Resize", can_apply, false).clicked() {
            let width_mm = app.scientific.properties_width_mm;
            let height_mm = app.scientific.properties_height_mm;
            if !width_mm.is_finite() || !height_mm.is_finite() || !(1.0..=1000.0).contains(&width_mm) || !(1.0..=1000.0).contains(&height_mm) {
                app.scientific.r_status = "Plot dimensions must be between 1 and 1000 mm".into();
            } else {
                render_plot(
                    &mut app.scientific,
                    panel.document_uid,
                    panel.r_variable.clone(),
                    PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm, height_mm },
                );
            }
        }
    });
    let status = if app.scientific.r_status.starts_with("Updated linked panel") {
        "Plot updated from R"
    } else if app.scientific.r_status.starts_with("Reflowed ") {
        "Plot reflowed from R"
    } else if app.scientific.r_status.starts_with("Could not ") {
        "Plot action failed"
    } else {
        workspace_status(&app.scientific, ui).0
    };
    ui.add(egui::Label::new(egui::RichText::new(status).size(tokens.font_size - 1.0).color(tokens.text_dim)).truncate())
        .on_hover_text(&app.scientific.r_status);
}

fn selected_plot_size_mm(app: &crate::VectorcraftApp, generated_id: u64) -> Option<(f64, f64)> {
    let bounds = app.session.doc().ok()?.doc.oriented_bounds(&[vectorcraft_doc::NodeId(generated_id)], false)?;
    let width = bounds.rect.width() / DOC_POINTS_PER_MM;
    let height = bounds.rect.height() / DOC_POINTS_PER_MM;
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some((width, height))
}

pub(super) fn observe_selected_panel_resize(app: &mut crate::VectorcraftApp, ui: &egui::Ui) {
    let Some(panel) = selected_linked_panel(app) else { return };
    let Some(size) = selected_plot_size_mm(app, panel.generated_id) else { return };
    let key = (panel.document_uid, panel.panel_id);
    let pointer_down = ui.input(|input| input.pointer.primary_down());
    let previous = app.scientific.observed_panel_sizes.insert(key, size);
    let changed = previous.is_some_and(|old| (old.0 - size.0).abs() > 0.01 || (old.1 - size.1).abs() > 0.01);
    if changed && pointer_down {
        app.scientific.pending_mouse_resize.insert(key, size);
    }
    if changed {
        app.scientific.properties_width_mm = size.0;
        app.scientific.properties_height_mm = size.1;
        app.scientific.properties_aspect_ratio = size.0 / size.1;
    }
    if pointer_down {
        return;
    }
    let pending = app.scientific.pending_mouse_resize.remove(&key);
    if pending.is_none() && !changed {
        return;
    }
    let (width_mm, height_mm) = pending.unwrap_or(size);
    render_plot(
        &mut app.scientific,
        panel.document_uid,
        panel.r_variable.clone(),
        PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm, height_mm },
    );
}
