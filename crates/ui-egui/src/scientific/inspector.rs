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
    panel_caption(ui, "R plot size");
    let units = app.session.general_unit();
    let field_width = ((ui.available_width() - 48.0) / 2.0).max(36.0);
    let mut width_changed = false;
    let mut height_changed = false;
    let origin = app.ruler_origin();
    let bounds = app
        .session
        .doc()
        .ok()
        .and_then(|state| state.doc.bounds_of(&[vectorcraft_doc::NodeId(panel.panel_id)], false))
        .map(|bounds| (bounds.x0 - origin.x, bounds.y0 - origin.y));
    egui::Grid::new(("graph-transform-grid", panel.panel_id)).num_columns(4).spacing([4.0, 6.0]).min_col_width(0.0).show(ui, |ui| {
        if auto_apply {
            crate::widgets::dim_label(ui, "X:");
            if let Some(value) = crate::widgets::num_field_named(
                ui,
                (panel.panel_id, "graph-transform-x"),
                Some("X position"),
                bounds.map(|b| b.0),
                units,
                field_width,
            ) && bounds.is_some_and(|current| (value - current.0).abs() > f64::EPSILON)
            {
                crate::panels::transform::run_transform(app, "object.setBounds", serde_json::json!({"x": value + origin.x}));
            }
        }
        crate::widgets::dim_label(ui, "W:").on_hover_text("Output width");
        let width_points = app.scientific.properties_width_mm * DOC_POINTS_PER_MM;
        if let Some(value) =
            crate::widgets::size_field_named(ui, (panel.panel_id, "graph-size-width"), Some("Output width"), Some(width_points), units, field_width)
        {
            let width_mm = (value / DOC_POINTS_PER_MM).clamp(1.0, 1000.0);
            width_changed = (width_mm - app.scientific.properties_width_mm).abs() > f64::EPSILON;
            app.scientific.properties_width_mm = width_mm;
        }
        if auto_apply {
            ui.end_row();
            crate::widgets::dim_label(ui, "Y:");
            if let Some(value) = crate::widgets::num_field_named(
                ui,
                (panel.panel_id, "graph-transform-y"),
                Some("Y position"),
                bounds.map(|b| b.1),
                units,
                field_width,
            ) && bounds.is_some_and(|current| (value - current.1).abs() > f64::EPSILON)
            {
                crate::panels::transform::run_transform(app, "object.setBounds", serde_json::json!({"y": value + origin.y}));
            }
        }
        crate::widgets::dim_label(ui, "H:").on_hover_text("Output height");
        let height_points = app.scientific.properties_height_mm * DOC_POINTS_PER_MM;
        if let Some(value) = crate::widgets::size_field_named(
            ui,
            (panel.panel_id, "graph-size-height"),
            Some("Output height"),
            Some(height_points),
            units,
            field_width,
        ) {
            let height_mm = (value / DOC_POINTS_PER_MM).clamp(1.0, 1000.0);
            height_changed = (height_mm - app.scientific.properties_height_mm).abs() > f64::EPSILON;
            app.scientific.properties_height_mm = height_mm;
        }
        ui.end_row();
    });
    ui.checkbox(&mut app.scientific.properties_aspect_lock, "Lock aspect ratio");
    if app.scientific.properties_aspect_lock && app.scientific.properties_aspect_ratio.is_finite() && app.scientific.properties_aspect_ratio > 0.0 {
        if width_changed {
            app.scientific.properties_height_mm = app.scientific.properties_width_mm / app.scientific.properties_aspect_ratio;
        } else if height_changed {
            app.scientific.properties_width_mm = app.scientific.properties_height_mm * app.scientific.properties_aspect_ratio;
        }
    }
    if auto_apply && (width_changed || height_changed) {
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
