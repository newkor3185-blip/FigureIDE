use super::{
    DOC_POINTS_PER_MM, PanelAction, RLinkedPanel, RenderedPlot, ResizeMode, compact_panel_style, go_to_plot_source, linked_panel_metadata,
    linked_panel_status, panel_action, panel_caption, panel_entry, persistent_panel_name, selected_linked_panel, update_all_panels,
};

pub(super) fn show_linked_panel_entries(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.scope(|ui| {
        compact_panel_style(ui);
        linked_panel_entries(app, ui);
    });
}

fn linked_panel_entries(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let document_uid = app.session.active().map(|doc| doc.uid).unwrap_or_default();
    let linked: Vec<_> = app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).cloned().collect();
    ui.horizontal_wrapped(|ui| {
        if panel_action(ui, "Update All", !app.scientific.running && !linked.is_empty(), false).clicked() {
            update_all_panels(app);
        }
    });
    if linked.is_empty() {
        panel_caption(ui, "No linked plot panels in this document.");
        return;
    }
    let plot_names: Vec<_> = app.scientific.plots.iter().map(|plot| plot.variable.clone()).collect();
    let object_names: Vec<_> = app.scientific.objects.iter().map(|object| object.variable.clone()).collect();
    let selected_id = selected_linked_panel(app).map(|panel| panel.panel_id);
    for panel in linked {
        let exists = plot_names.iter().any(|name| name == &panel.r_variable);
        let variable_exists = object_names.iter().any(|name| name == &panel.r_variable);
        panel_entry(ui, selected_id == Some(panel.panel_id), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("Panel {}", panel.label));
                let tokens = crate::theme::Tokens::get(ui.ctx());
                let conn_state = app.scientific.panel_connection_state(&panel);
                let (status_text, status_color) = match conn_state {
                    super::LinkedPanelState::Offline => ("Offline", tokens.text_dim),
                    super::LinkedPanelState::MissingObject => ("Missing R Object", tokens.warning),
                    super::LinkedPanelState::IncompatibleObject => ("Incompatible Object", tokens.warning),
                    super::LinkedPanelState::ReadyToUpdate => ("Ready to Update", tokens.accent),
                    super::LinkedPanelState::Updating => ("Updating...", tokens.accent),
                    super::LinkedPanelState::Connected => ("Connected", tokens.text_dim),
                    super::LinkedPanelState::Error => ("Update Failed", tokens.error),
                };
                ui.label(egui::RichText::new(status_text).size(tokens.font_size - 1.0).color(status_color))
                    .on_hover_text(linked_panel_status(exists, variable_exists));
            });
            ui.label(&panel.r_variable).on_hover_text(linked_panel_metadata(&panel.r_variable, panel.panel_id));
            panel_caption(ui, format!("{:.1} x {:.1} mm | {}", panel.width_mm, panel.height_mm, panel.resize_mode.as_str()));
            ui.horizontal_wrapped(|ui| {
                if panel_action(ui, "Select / Focus", true, false).clicked() {
                    let _ = app.run("select.set", serde_json::json!({"ids": [panel.panel_id]}));
                    app.scientific.selected_plot = Some(panel.r_variable.clone());
                }
                if panel_action(ui, "Go to Source", true, false).clicked() {
                    app.scientific.selected_plot = Some(panel.r_variable.clone());
                    go_to_plot_source(&mut app.scientific, &panel.r_variable);
                }
                let conn_state = app.scientific.panel_connection_state(&panel);
                let can_update = !app.scientific.running
                    && matches!(
                        conn_state,
                        super::LinkedPanelState::ReadyToUpdate | super::LinkedPanelState::Connected | super::LinkedPanelState::Error
                    );
                if panel_action(ui, "Update", can_update, false).clicked() {
                    let _ = super::update_panel_async(app, panel.panel_id);
                }
                if panel_action(ui, "Relink", !app.scientific.running && !plot_names.is_empty(), false).clicked() {
                    app.scientific.panel_relink_open =
                        if app.scientific.panel_relink_open == Some(panel.panel_id) { None } else { Some(panel.panel_id) };
                }
            });
            if app.scientific.panel_relink_open == Some(panel.panel_id) {
                ui.group(|ui| {
                    ui.label(format!("Current: {}", panel.r_variable));
                    let mut variable = app.scientific.panel_relink_drafts.get(&panel.panel_id).cloned().unwrap_or_else(|| {
                        plot_names.iter().find(|name| *name != &panel.r_variable).cloned().unwrap_or_else(|| panel.r_variable.clone())
                    });
                    egui::ComboBox::from_id_salt(("r-panel-variable", panel.panel_id))
                        .width(ui.available_width().min(260.0))
                        .selected_text(&variable)
                        .show_ui(ui, |ui| {
                            for name in &plot_names {
                                ui.selectable_value(&mut variable, name.clone(), name);
                            }
                        });
                    app.scientific.panel_relink_drafts.insert(panel.panel_id, variable.clone());
                    ui.horizontal_wrapped(|ui| {
                        let can_relink = variable != panel.r_variable && !app.scientific.running;
                        if panel_action(ui, "Confirm Relink", can_relink, true).clicked() {
                            let _ = super::relink_panel_async(app, panel.panel_id, variable.clone());
                            app.scientific.panel_relink_drafts.remove(&panel.panel_id);
                            app.scientific.panel_relink_open = None;
                        }
                        if panel_action(ui, "Cancel", true, false).clicked() {
                            app.scientific.panel_relink_drafts.remove(&panel.panel_id);
                            app.scientific.panel_relink_open = None;
                        }
                    });
                });
            }
            ui.horizontal_wrapped(|ui| {
                if panel_action(ui, "Unlink", true, false).on_hover_text("Remove the R association and keep the vector artwork.").clicked()
                    && let Err(error) = app.run("ui.unlinkRPanel", serde_json::json!({"panel": panel.panel_id}))
                {
                    app.scientific.r_status = format!("Could not unlink panel: {error}");
                }
                if app.scientific.delete_artwork_confirmation == Some(panel.panel_id) {
                    ui.label("Delete this panel's vector artwork?");
                    if panel_action(ui, "Confirm Delete", true, true).clicked() {
                        match app.run("edit.clear", serde_json::json!({"ids": [panel.panel_id]})) {
                            Ok(_) => {
                                app.scientific.linked_panels.retain(|link| link.panel_id != panel.panel_id);
                                app.scientific.panel_relink_drafts.remove(&panel.panel_id);
                                if app.scientific.panel_relink_open == Some(panel.panel_id) {
                                    app.scientific.panel_relink_open = None;
                                }
                            }
                            Err(error) => app.scientific.r_status = format!("Could not delete panel artwork: {error}"),
                        }
                        app.scientific.delete_artwork_confirmation = None;
                    }
                    if panel_action(ui, "Cancel", true, false).clicked() {
                        app.scientific.delete_artwork_confirmation = None;
                    }
                } else if panel_action(ui, "Delete Artwork", true, true)
                    .on_hover_text("Delete this linked panel's vector artwork from the document.")
                    .clicked()
                {
                    app.scientific.delete_artwork_confirmation = Some(panel.panel_id);
                }
            });
        });
    }
}

pub(super) fn apply_rendered_plot(app: &mut crate::VectorcraftApp, rendered: RenderedPlot) {
    let RenderedPlot { variable, svg, plots, action } = rendered;
    app.scientific.objects = plots;
    app.scientific.plots = app.scientific.objects.iter().filter(|object| !object.plot_kind.is_empty()).cloned().collect();
    if let PanelAction::Create { label, width_mm, height_mm } = &action {
        if let Err(error) = app.run("edit.undoGroup.begin", serde_json::json!({"label": format!("Add R plot {variable}")})) {
            app.scientific.r_status = format!("Could not begin plot insertion: {error}");
            return;
        }
        let result = create_rendered_plot(app, &variable, &svg, label, *width_mm, *height_mm);
        let ended = app.run("edit.undoGroup.end", serde_json::json!({"cancel": result.is_err()}));
        app.scientific.r_status = match (result, ended) {
            (Ok(true), Ok(_)) => format!("Added {variable} as Panel {label}"),
            (Ok(false), Ok(_)) => format!(
                "Added {variable} as Panel {label}; the artboard has no free space, so the graph was placed beyond its right edge for manual placement."
            ),
            (Err(error), _) => format!("Could not add {variable}: {error}"),
            (Ok(_), Err(error)) => format!("Plot was inserted, but its undo group could not close: {error}"),
        };
        return;
    }
    let (target_id, width_mm, height_mm) = match &action {
        PanelAction::Refresh { generated_id, width_mm, height_mm, .. }
        | PanelAction::Reflow { generated_id, width_mm, height_mm, .. }
        | PanelAction::Graph { generated_id, width_mm, height_mm, .. }
        | PanelAction::Relink { generated_id, width_mm, height_mm, .. } => (*generated_id, *width_mm, *height_mm),
        PanelAction::Create { .. } => return,
    };
    let hybrid =
        app.session.doc().ok().and_then(|state| state.doc.node(vectorcraft_doc::NodeId(target_id))).and_then(|node| node.children()).is_some_and(
            |children| {
                children.iter().any(|child| matches!(&child.kind, vectorcraft_doc::NodeKind::PlacedDocument(p) if p.key.starts_with("rvs-svg-")))
            },
        );
    let generated_id = match if hybrid { import_hybrid_group(app, &svg, width_mm, height_mm) } else { import_svg_group(app, &svg) } {
        Ok(id) => id,
        Err(error) => {
            app.scientific.r_status = format!("Could not import {variable} SVG: {error}");
            return;
        }
    };
    match action {
        PanelAction::Refresh { panel_id, generated_id: target_generated_id, width_mm, height_mm } => {
            let result = app.run(
                "object.replaceGroupContents",
                serde_json::json!({"target": target_generated_id, "source": generated_id, "preserveRUserEdits": true}),
            );
            let replaced = match result {
                Ok(result) => result,
                Err(error) => {
                    let _ = app.run("edit.clear", serde_json::json!({"ids": [generated_id]}));
                    app.scientific.panel_states.insert(panel_id, super::LinkedPanelState::Error);
                    app.scientific.r_status = format!("Could not refresh {variable}; previous plot was kept: {error}");
                    return;
                }
            };
            if let Some(panel) = app.scientific.linked_panels.iter_mut().find(|panel| panel.panel_id == panel_id) {
                panel.generated_id = target_generated_id;
                panel.r_variable = variable.clone();
                panel.width_mm = width_mm;
                panel.height_mm = height_mm;
            }
            app.scientific.panel_states.insert(panel_id, super::LinkedPanelState::Connected);
            let retained = replaced["preservedText"].as_u64().unwrap_or(0);
            app.scientific.r_status = format!("Updated linked panel {panel_id} from {variable}; preserved {retained} edited text label(s)");
        }
        PanelAction::Relink { panel_id, generated_id: target_generated_id, target_variable, width_mm, height_mm } => {
            let Some(existing_panel) = app.scientific.linked_panels.iter().find(|p| p.panel_id == panel_id).cloned() else {
                let _ = app.run("edit.clear", serde_json::json!({"ids": [generated_id]}));
                app.scientific.r_status = "Linked panel no longer exists; previous artwork was kept".into();
                return;
            };

            let undo_begin = app.run("edit.undoGroup.begin", serde_json::json!({"label": format!("Relink Panel to {target_variable}")}));
            if let Err(error) = undo_begin {
                let _ = app.run("edit.clear", serde_json::json!({"ids": [generated_id]}));
                app.scientific.panel_states.insert(panel_id, super::LinkedPanelState::Error);
                app.scientific.r_status = format!("Could not begin undo group for relink: {error}");
                return;
            }

            let result = app.run(
                "object.replaceGroupContents",
                serde_json::json!({"target": target_generated_id, "source": generated_id, "preserveRUserEdits": true}),
            );

            if let Err(error) = result {
                let _ = app.run("edit.clear", serde_json::json!({"ids": [generated_id]}));
                let _ = app.run("edit.undoGroup.end", serde_json::json!({"cancel": true}));
                app.scientific.panel_states.insert(panel_id, super::LinkedPanelState::Error);
                app.scientific.r_status = format!("Could not relink {target_variable}; previous artwork was kept: {error}");
                return;
            }

            let mut updated_panel = existing_panel;
            updated_panel.r_variable = target_variable.clone();
            updated_panel.width_mm = width_mm;
            updated_panel.height_mm = height_mm;
            let new_panel_name = persistent_panel_name(&updated_panel, width_mm, height_mm, ResizeMode::Reflow);

            let _ = app.run("object.setName", serde_json::json!({"id": panel_id, "name": new_panel_name}));
            let _ = app.run("edit.undoGroup.end", serde_json::json!({"cancel": false}));

            if let Some(panel) = app.scientific.linked_panels.iter_mut().find(|p| p.panel_id == panel_id) {
                panel.r_variable = target_variable.clone();
                panel.width_mm = width_mm;
                panel.height_mm = height_mm;
            }
            app.scientific.panel_states.insert(panel_id, super::LinkedPanelState::Connected);
            app.scientific.r_status = format!("Relinked panel {panel_id} to {target_variable}");
        }
        PanelAction::Reflow { panel_id, generated_id: target_generated_id, width_mm, height_mm } => {
            apply_reflow(app, panel_id, target_generated_id, generated_id, width_mm, height_mm, None);
        }
        PanelAction::Graph { panel_id, generated_id: target_generated_id, width_mm, height_mm, style } => {
            apply_reflow(app, panel_id, target_generated_id, generated_id, width_mm, height_mm, Some(*style));
        }
        PanelAction::Create { .. } => {}
    }
}

fn apply_reflow(
    app: &mut crate::VectorcraftApp,
    panel_id: u64,
    target_generated_id: u64,
    imported_id: u64,
    width_mm: f64,
    height_mm: f64,
    graph_style: Option<super::graph_style::GraphStyle>,
) {
    let Some(mut panel) = app.scientific.linked_panels.iter().find(|panel| panel.panel_id == panel_id).cloned() else {
        let _ = app.run("edit.clear", serde_json::json!({"ids": [imported_id]}));
        app.scientific.r_status = "Linked panel no longer exists; the previous artwork was kept".into();
        return;
    };
    let variable = panel.r_variable.clone();
    if let Some(style) = graph_style {
        panel.graph_style = style;
    }
    let panel_name = persistent_panel_name(&panel, width_mm, height_mm, ResizeMode::Reflow);
    let result = app.run(
        "object.resizeRPanel",
        serde_json::json!({
            "panel": panel_id, "target": target_generated_id, "source": imported_id,
            "width": width_mm * DOC_POINTS_PER_MM, "height": height_mm * DOC_POINTS_PER_MM,
            "panelName": panel_name, "preserveRUserEdits": true
        }),
    );
    if let Err(error) = result {
        let _ = app.run("edit.clear", serde_json::json!({"ids": [imported_id]}));
        app.scientific.r_status = format!("Could not reflow {variable}; previous artwork was kept: {error}");
        return;
    }
    if let Some(link) = app.scientific.linked_panels.iter_mut().find(|link| link.panel_id == panel_id) {
        link.width_mm = width_mm;
        link.height_mm = height_mm;
        link.r_variable = variable.to_owned();
        link.resize_mode = ResizeMode::Reflow;
        link.graph_style = panel.graph_style;
    }
    app.scientific.observed_panel_sizes.insert((panel.document_uid, panel_id), (width_mm, height_mm));
    app.scientific.pending_mouse_resize.remove(&(panel.document_uid, panel_id));
    app.scientific.r_status = format!("Reflowed {variable} to {width_mm:.1} x {height_mm:.1} mm");
}

fn import_svg_group(app: &mut crate::VectorcraftApp, svg: &str) -> Result<u64, String> {
    app.run("clipboard.importSvg", serde_json::json!({"svg": svg}))?;
    paste_svg_group(app)
}

fn import_hybrid_group(app: &mut crate::VectorcraftApp, svg: &str, width_mm: f64, height_mm: f64) -> Result<u64, String> {
    let result = app.run(
        "object.importRPlotSource",
        serde_json::json!({"svg": svg, "width": width_mm * DOC_POINTS_PER_MM, "height": height_mm * DOC_POINTS_PER_MM}),
    )?;
    result["id"].as_u64().ok_or_else(|| "VectorCraft returned no embedded SVG group ID".into())
}

fn paste_svg_group(app: &mut crate::VectorcraftApp) -> Result<u64, String> {
    #[cfg(test)]
    let started = std::time::Instant::now();
    let pasted = app.paste_internal_clipboard_in_place()?;
    #[cfg(test)]
    let pasted_ms = started.elapsed().as_secs_f64() * 1000.0;
    let ids: Vec<u64> = pasted["ids"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_u64).collect();
    if ids.is_empty() {
        return Err("SVG import produced no document objects".into());
    }
    let group = app.run("object.group", serde_json::json!({"ids": ids}))?;
    #[cfg(test)]
    if std::env::var_os("RVS_UMAP_PROFILE_STAGES").is_some() {
        eprintln!("umap_stage paste_ms={pasted_ms:.1} group_ms={:.1}", started.elapsed().as_secs_f64() * 1000.0 - pasted_ms);
    }
    group["id"].as_u64().ok_or_else(|| "VectorCraft returned no imported group ID".into())
}

fn ensure_plot_artboard(app: &mut crate::VectorcraftApp) -> Result<vectorcraft_geom::Rect, String> {
    if app.session.active().is_none() {
        app.run(
            "file.new",
            serde_json::json!({
                "width": "210 mm", "height": "297 mm", "units": "Millimeters", "backgroundContents": "white"
            }),
        )?;
    }
    if app.session.doc().map_err(|error| error.to_string())?.doc.artboards.is_empty() {
        let created = app.run(
            "artboard.new",
            serde_json::json!({
                "x": 0.0, "y": 0.0, "width": 210.0 * DOC_POINTS_PER_MM,
                "height": 297.0 * DOC_POINTS_PER_MM, "color": "#ffffff"
            }),
        )?;
        let index = created["index"].as_u64().ok_or_else(|| "VectorCraft returned no artboard index".to_owned())? as usize;
        crate::panels::artboards::select(app, index);
    }
    let state = app.session.doc().map_err(|error| error.to_string())?;
    let index = crate::panels::artboards::selected(app, state.doc.artboards.len());
    state.doc.artboards.get(index).map(|artboard| artboard.rect).ok_or_else(|| "No usable artboard exists for the plot".to_owned())
}

fn create_rendered_plot(
    app: &mut crate::VectorcraftApp,
    variable: &str,
    svg: &str,
    label: &str,
    width_mm: f64,
    height_mm: f64,
) -> Result<bool, String> {
    #[cfg(test)]
    let started = std::time::Instant::now();
    // Import before creating a document or artboard, so invalid SVG leaves the
    // workspace untouched. Reuse that clipboard for the paste instead of parsing twice.
    // A large linked plot stays one placed source in the document. The renderer
    // rasterizes it on a worker; vector import is deferred until export or unlink.
    let hybrid = svg.len() >= 1_000_000;
    if !hybrid {
        app.run("clipboard.importSvg", serde_json::json!({"svg": svg, "preserveOriginal": true}))?;
    }
    #[cfg(test)]
    let imported_ms = started.elapsed().as_secs_f64() * 1000.0;
    let board = ensure_plot_artboard(app)?;
    let generated_id = if hybrid { import_hybrid_group(app, svg, width_mm, height_mm)? } else { paste_svg_group(app)? };
    #[cfg(test)]
    let pasted_ms = started.elapsed().as_secs_f64() * 1000.0;
    let bounds = bounds_of(app, generated_id).ok_or_else(|| format!("Could not determine bounds for {variable}"))?;
    let label_id = app.run("text.create", serde_json::json!({"x": bounds.x0, "y": bounds.y0 - 6.0, "text": label, "size": 9}))?["id"]
        .as_u64()
        .ok_or_else(|| "VectorCraft returned no panel label ID".to_owned())?;
    let label_bounds = bounds_of(app, label_id).ok_or_else(|| "Could not measure the new plot label".to_owned())?;
    app.run("object.setName", serde_json::json!({"id": generated_id, "name": "RVS_GENERATED"}))?;
    app.run("object.setName", serde_json::json!({"id": label_id, "name": "RVS_LABEL"}))?;
    let panel_id = app.run("object.group", serde_json::json!({"ids": [generated_id, label_id]}))?["id"]
        .as_u64()
        .ok_or_else(|| "VectorCraft returned no panel ID".to_owned())?;
    let document_uid = app.session.active().map(|doc| doc.uid).ok_or_else(|| "No document exists after plot import".to_owned())?;
    let metadata = RLinkedPanel {
        panel_id,
        generated_id,
        label: label.to_owned(),
        r_variable: variable.to_owned(),
        width_mm,
        height_mm,
        resize_mode: ResizeMode::Reflow,
        graph_style: Default::default(),
        small_figure_typography: true,
        document_uid,
    };
    app.run(
        "object.setName",
        serde_json::json!({
            "id": panel_id, "name": persistent_panel_name(&metadata, width_mm, height_mm, ResizeMode::Reflow)
        }),
    )?;
    // The panel contains exactly the generated group and its label. Their
    // bounds were already measured before grouping; avoid walking every point
    // in the new panel a second time.
    let panel_bounds = bounds.union(label_bounds);
    let occupied: Vec<_> = app
        .scientific
        .linked_panels
        .iter()
        .filter(|panel| panel.document_uid == document_uid)
        .filter_map(|panel| bounds_of(app, panel.panel_id))
        .filter(|bounds| rectangles_overlap(*bounds, board))
        .collect();
    let placement = find_graph_placement(board, panel_bounds.width(), panel_bounds.height(), &occupied);
    let (target_x, target_y, fits_artboard) = placement.map_or_else(
        || {
            let rightmost = occupied.iter().map(|rect| rect.x1).fold(board.x1, f64::max);
            (rightmost + GRAPH_PLACEMENT_GAP, board.y0 + GRAPH_PLACEMENT_MARGIN, false)
        },
        |(x, y)| (x, y, true),
    );
    app.run(
        "object.move",
        serde_json::json!({
            "ids": [panel_id], "dx": target_x - panel_bounds.x0, "dy": target_y - panel_bounds.y0
        }),
    )?;
    #[cfg(test)]
    let positioned_ms = started.elapsed().as_secs_f64() * 1000.0;
    app.run("object.markRGenerated", serde_json::json!({"group": generated_id}))?;
    #[cfg(test)]
    let marked_ms = started.elapsed().as_secs_f64() * 1000.0;
    app.run("select.set", serde_json::json!({"ids": [panel_id]}))?;
    app.scientific.observed_panel_sizes.insert((document_uid, panel_id), (width_mm, height_mm));
    app.scientific.linked_panels.push(metadata);
    #[cfg(test)]
    if std::env::var_os("RVS_UMAP_PROFILE_STAGES").is_some() {
        eprintln!(
            "umap_stage import_ms={imported_ms:.1} paste_total_ms={:.1} names_move_ms={:.1} mark_ms={:.1} select_ms={:.1}",
            pasted_ms - imported_ms,
            positioned_ms - pasted_ms,
            marked_ms - positioned_ms,
            started.elapsed().as_secs_f64() * 1000.0 - marked_ms
        );
    }
    Ok(fits_artboard)
}

const GRAPH_PLACEMENT_MARGIN: f64 = 18.0;
const GRAPH_PLACEMENT_GAP: f64 = 18.0;

/// Find the first non-overlapping top-left position, searching rows from top to bottom and each
/// row from left to right. Candidate coordinates come from artboard margins and occupied bounds,
/// so a free rectangle can be found without sampling or depending on screen zoom.
fn find_graph_placement(board: vectorcraft_geom::Rect, width: f64, height: f64, occupied: &[vectorcraft_geom::Rect]) -> Option<(f64, f64)> {
    if !board.x0.is_finite()
        || !board.y0.is_finite()
        || !board.x1.is_finite()
        || !board.y1.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }
    let min_x = board.x0 + GRAPH_PLACEMENT_MARGIN;
    let min_y = board.y0 + GRAPH_PLACEMENT_MARGIN;
    let max_x = board.x1 - GRAPH_PLACEMENT_MARGIN - width;
    let max_y = board.y1 - GRAPH_PLACEMENT_MARGIN - height;
    if min_x > max_x || min_y > max_y {
        return None;
    }
    let mut xs = vec![min_x];
    let mut ys = vec![min_y];
    for rect in occupied.iter().copied().filter(|rect| rectangles_overlap(*rect, board)) {
        if rect.x1.is_finite() {
            xs.push(rect.x1 + GRAPH_PLACEMENT_GAP);
        }
        if rect.y1.is_finite() {
            ys.push(rect.y1 + GRAPH_PLACEMENT_GAP);
        }
    }
    xs.retain(|x| x.is_finite() && *x >= min_x && *x <= max_x);
    ys.retain(|y| y.is_finite() && *y >= min_y && *y <= max_y);
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < f64::EPSILON);
    for y in ys {
        for &x in &xs {
            let candidate = vectorcraft_geom::Rect::new(x, y, x + width, y + height);
            if !occupied.iter().any(|rect| rectangles_overlap_with_gap(candidate, *rect)) {
                return Some((x, y));
            }
        }
    }
    None
}

fn rectangles_overlap_with_gap(a: vectorcraft_geom::Rect, b: vectorcraft_geom::Rect) -> bool {
    a.x0 < b.x1 + GRAPH_PLACEMENT_GAP && a.x1 + GRAPH_PLACEMENT_GAP > b.x0 && a.y0 < b.y1 + GRAPH_PLACEMENT_GAP && a.y1 + GRAPH_PLACEMENT_GAP > b.y0
}

fn rectangles_overlap(a: vectorcraft_geom::Rect, b: vectorcraft_geom::Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn bounds_of(app: &crate::VectorcraftApp, id: u64) -> Option<vectorcraft_geom::Rect> {
    app.session.doc().ok()?.doc.bounds_of(&[vectorcraft_doc::NodeId(id)], false)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_color::Paint;
    use vectorcraft_doc::{AppearanceItem, Node, NodeId, NodeKind};

    use super::apply_rendered_plot;
    use super::{GRAPH_PLACEMENT_GAP, GRAPH_PLACEMENT_MARGIN, bounds_of, find_graph_placement};
    use crate::scientific::{PanelAction, RenderedPlot};

    #[test]
    fn automatic_graph_placement_is_row_major_and_uses_actual_obstacle_bounds() {
        use vectorcraft_geom::Rect;

        let board = Rect::new(0.0, 0.0, 500.0, 400.0);
        assert_eq!(find_graph_placement(board, 100.0, 80.0, &[]), Some((GRAPH_PLACEMENT_MARGIN, GRAPH_PLACEMENT_MARGIN)));

        let first = Rect::new(18.0, 18.0, 118.0, 98.0);
        assert_eq!(find_graph_placement(board, 100.0, 80.0, &[first]), Some((136.0, 18.0)));

        let narrow_board = Rect::new(0.0, 0.0, 260.0, 190.0);
        let wide_first = Rect::new(18.0, 18.0, 218.0, 98.0);
        assert_eq!(find_graph_placement(narrow_board, 100.0, 50.0, &[wide_first]), Some((18.0, 116.0)));

        // The first graph is narrower than the incoming one. Its right edge, not a fixed column
        // width, determines the earliest valid position before the second graph.
        let narrow = Rect::new(18.0, 18.0, 68.0, 78.0);
        let second = Rect::new(100.0, 18.0, 180.0, 78.0);
        assert_eq!(find_graph_placement(board, 100.0, 50.0, &[narrow, second]), Some((198.0, 18.0)));

        // A rotated graph's axis-aligned document bounds are a conservative occupied region.
        let rotated_bounds = Rect::new(18.0, 18.0, 150.0, 110.0);
        assert_eq!(find_graph_placement(board, 100.0, 70.0, &[rotated_bounds]), Some((168.0, 18.0)));

        let moved = Rect::new(300.0, 250.0, 420.0, 320.0);
        assert_eq!(find_graph_placement(board, 100.0, 80.0, &[moved]), Some((18.0, 18.0)));
        assert_eq!(find_graph_placement(Rect::new(0.0, 0.0, 120.0, 100.0), 100.0, 80.0, &[]), None);

        let fully_occupied =
            Rect::new(GRAPH_PLACEMENT_MARGIN, GRAPH_PLACEMENT_MARGIN, board.x1 - GRAPH_PLACEMENT_MARGIN, board.y1 - GRAPH_PLACEMENT_MARGIN);
        assert_eq!(find_graph_placement(board, 100.0, 80.0, &[fully_occupied]), None);
    }

    #[test]
    fn automatic_graph_placement_uses_open_space_after_manual_move() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 400.0, "height": 250.0})).unwrap();
        let insert = |app: &mut crate::VectorcraftApp, variable: &str, label: &str, width: u32| {
            apply_rendered_plot(
                app,
                RenderedPlot {
                    variable: variable.into(),
                    svg: format!(
                        "<svg xmlns='http://www.w3.org/2000/svg' width='{width}pt' height='60pt'><rect width='{width}' height='60' fill='white'/></svg>"
                    ),
                    plots: Vec::new(),
                    action: PanelAction::Create { label: label.into(), width_mm: 60.0, height_mm: 25.0 },
                },
            );
        };
        insert(&mut app, "plot_A", "A", 100);
        assert!(app.scientific.r_status.starts_with("Added plot_A"), "{}", app.scientific.r_status);
        let first = app.scientific.linked_panels[0].clone();
        let initial_first = bounds_of(&app, first.panel_id).unwrap();
        assert_eq!((initial_first.x0, initial_first.y0), (GRAPH_PLACEMENT_MARGIN, GRAPH_PLACEMENT_MARGIN));
        app.run("object.move", json!({"ids": [first.panel_id], "dx": 220.0, "dy": 100.0})).unwrap();
        let moved_first = bounds_of(&app, first.panel_id).unwrap();

        insert(&mut app, "plot_B", "B", 120);
        assert!(app.scientific.r_status.starts_with("Added plot_B"), "{}", app.scientific.r_status);
        let second = app.scientific.linked_panels[1].clone();
        let second_bounds = bounds_of(&app, second.panel_id).unwrap();
        assert_eq!((second_bounds.x0, second_bounds.y0), (GRAPH_PLACEMENT_MARGIN, GRAPH_PLACEMENT_MARGIN));
        assert_eq!(bounds_of(&app, first.panel_id), Some(moved_first), "existing graph was repositioned");
        assert!(second_bounds.x1 + GRAPH_PLACEMENT_GAP <= moved_first.x0 || second_bounds.x0 >= moved_first.x1 + GRAPH_PLACEMENT_GAP);
        assert!(second_bounds.y1 + GRAPH_PLACEMENT_GAP <= moved_first.y0 || second_bounds.y0 >= moved_first.y1 + GRAPH_PLACEMENT_GAP);

        let expected_after_redo = second_bounds;
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.doc().unwrap().doc.node(NodeId(second.panel_id)).is_none());
        assert_eq!(bounds_of(&app, first.panel_id), Some(moved_first));
        app.run("edit.redo", json!({})).unwrap();
        let redone = bounds_of(&app, second.panel_id).unwrap();
        assert_eq!(redone, expected_after_redo);
        assert_eq!(bounds_of(&app, first.panel_id), Some(moved_first));

        // Keep the test's document positions in artboard coordinates and independent of view zoom.
        let board = app.session.doc().unwrap().doc.artboards[0].rect;
        assert!(second_bounds.x0 >= board.x0 && second_bounds.y0 >= board.y0);
        assert!(second_bounds.x1 <= board.x1 && second_bounds.y1 <= board.y1);
    }

    #[test]
    fn automatic_graph_placement_keeps_oversized_graph_with_manual_placement_notice() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 100.0, "height": 80.0})).unwrap();
        apply_rendered_plot(
            &mut app,
            RenderedPlot {
                variable: "plot_large".into(),
                svg: "<svg xmlns='http://www.w3.org/2000/svg' width='300pt' height='200pt'><rect width='300' height='200' fill='white'/></svg>"
                    .into(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "Large".into(), width_mm: 100.0, height_mm: 70.0 },
            },
        );
        assert!(app.scientific.r_status.contains("no free space"), "{}", app.scientific.r_status);
        assert!(app.scientific.r_status.contains("manual placement"), "{}", app.scientific.r_status);
        assert_eq!(app.scientific.linked_panels.len(), 1, "overflow must preserve the generated graph");
        let panel = &app.scientific.linked_panels[0];
        let bounds = bounds_of(&app, panel.panel_id).unwrap();
        let board = app.session.doc().unwrap().doc.artboards[0].rect;
        assert!(bounds.x0 >= board.x1, "overflow graph should be placed outside the artboard: {bounds:?}, {board:?}");
    }

    #[test]
    #[ignore = "requires RVS_UMAP_SVG to point to a generated local benchmark SVG"]
    fn profile_large_linked_plot_insertion() {
        let path = std::env::var("RVS_UMAP_SVG").expect("RVS_UMAP_SVG must name a local benchmark SVG");
        let svg = std::fs::read_to_string(path).unwrap();
        let reflow_svg = (std::env::var_os("RVS_UMAP_PROFILE_REFLOW").is_some()).then(|| svg.clone());
        let second_svg = (std::env::var_os("RVS_UMAP_PROFILE_SECOND").is_some()).then(|| svg.clone());
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let started = std::time::Instant::now();
        apply_rendered_plot(
            &mut app,
            RenderedPlot {
                variable: "umap_plot".into(),
                svg,
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        println!("linked_insert_ms={:.1} nodes={}", started.elapsed().as_secs_f64() * 1000.0, app.session.doc().unwrap().doc.layers[0].count());
        if std::env::var_os("RVS_UMAP_PROFILE_ZOOM").is_some() {
            let doc = &app.session.doc().unwrap().doc;
            let mut renderer = vectorcraft_render::Renderer::new();
            let view = vectorcraft_geom::Affine::scale(2.0);
            let exact = vectorcraft_render::RenderOptions::default();
            if std::env::var_os("RVS_UMAP_PROFILE_EXACT").is_some() {
                let at = std::time::Instant::now();
                let _ = renderer.render(doc, 1440, 900, view, &exact);
                println!("zoom_exact_cold_ms={:.1}", at.elapsed().as_secs_f64() * 1000.0);
                let at = std::time::Instant::now();
                let _ = renderer.render(doc, 1440, 900, view, &exact);
                println!("zoom_exact_warm_ms={:.1}", at.elapsed().as_secs_f64() * 1000.0);
            }
            let cached = vectorcraft_render::RenderOptions { progressive_placed: true, ..exact };
            let _ = renderer.render(doc, 1440, 900, view, &cached);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while vectorcraft_render::placed_document::busy() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(!vectorcraft_render::placed_document::busy(), "hybrid preview timed out");
            let at = std::time::Instant::now();
            let _ = renderer.render(doc, 1440, 900, view, &cached);
            println!("zoom_cached_ms={:.1}", at.elapsed().as_secs_f64() * 1000.0);
        }
        if std::env::var_os("RVS_UMAP_PROFILE_HIT").is_some() {
            let doc = &app.session.doc().unwrap().doc;
            let panel_id = app.scientific.linked_panels[0].panel_id;
            let point = doc.bounds_of(&[NodeId(panel_id)], false).unwrap().center();
            for fast in [false, true] {
                let started = std::time::Instant::now();
                for _ in 0..20 {
                    let hit = vectorcraft_doc::hit::hit_test(
                        doc,
                        point,
                        vectorcraft_doc::hit::HitOptions { linked_panel_as_unit: fast, ..Default::default() },
                    );
                    assert_eq!(hit.map(|h| h.top_object(None).0), Some(panel_id));
                }
                println!("linked_hit_ms_per_click fast={fast} {:.2}", started.elapsed().as_secs_f64() * 50.0);
            }
        }
        let second_panel = second_svg.map(|svg| {
            apply_rendered_plot(
                &mut app,
                RenderedPlot {
                    variable: "umap_plot_two".into(),
                    svg,
                    plots: Vec::new(),
                    action: PanelAction::Create { label: "B".into(), width_mm: 70.0, height_mm: 50.0 },
                },
            );
            assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
            app.scientific.linked_panels[1].clone()
        });
        if let Some(svg) = reflow_svg {
            let panel = app.scientific.linked_panels[0].clone();
            let started = std::time::Instant::now();
            apply_rendered_plot(
                &mut app,
                RenderedPlot {
                    variable: "umap_plot".into(),
                    svg,
                    plots: Vec::new(),
                    action: PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 100.0, height_mm: 80.0 },
                },
            );
            assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
            println!("linked_reflow_ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(app.scientific.linked_panels[0].panel_id, panel.panel_id);
            assert_eq!(app.scientific.linked_panels[0].width_mm, 100.0);
            if let Some(second) = &second_panel {
                assert_eq!(app.scientific.linked_panels[1].panel_id, second.panel_id);
                assert_eq!(app.scientific.linked_panels[1].width_mm, second.width_mm);
                assert_eq!(app.scientific.linked_panels[1].height_mm, second.height_mm);
            }
            let undo_started = std::time::Instant::now();
            app.run("edit.undo", json!({})).unwrap();
            println!("linked_undo_ms={:.1}", undo_started.elapsed().as_secs_f64() * 1000.0);
            let redo_started = std::time::Instant::now();
            app.run("edit.redo", json!({})).unwrap();
            println!("linked_redo_ms={:.1}", redo_started.elapsed().as_secs_f64() * 1000.0);
            let saved = vectorcraft_format::save_file(&app.session.doc().unwrap().doc);
            let reopened = vectorcraft_format::load(&saved).unwrap();
            assert!(
                reopened
                    .node(vectorcraft_doc::NodeId(panel.panel_id))
                    .and_then(|node| node.name.as_deref())
                    .is_some_and(|name| name.starts_with("RVS_PANEL|"))
            );
            println!("linked_save_bytes={}", saved.len());
            let revision = app.session.active().unwrap().revision;
            apply_rendered_plot(
                &mut app,
                RenderedPlot {
                    variable: "umap_plot".into(),
                    svg: "<svg><broken".into(),
                    plots: Vec::new(),
                    action: PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 100.0, height_mm: 80.0 },
                },
            );
            assert!(app.scientific.r_status.starts_with("Could not import umap_plot SVG:"));
            assert_eq!(app.session.active().unwrap().revision, revision);
            if let Some(path) = std::env::var_os("RVS_UMAP_OUTPUT_PROJECT") {
                std::fs::write(path, &saved).unwrap();
            }
        }
        if std::env::var_os("RVS_UMAP_PROFILE_UNLINK").is_some() {
            let panel = app.scientific.linked_panels[0].clone();
            app.run("ui.unlinkRPanel", json!({"panel": panel.panel_id})).unwrap();
            let at = std::time::Instant::now();
            super::super::linked_plot_protection::confirm(&mut app).unwrap();
            println!("linked_unlink_ms={:.1}", at.elapsed().as_secs_f64() * 1000.0);
            let doc = &app.session.doc().unwrap().doc;
            assert!(doc.node(NodeId(panel.panel_id)).is_some());
            assert!(!doc.node(NodeId(panel.panel_id)).unwrap().name.as_deref().unwrap_or("").starts_with("RVS_PANEL|"));
            assert!(doc.layers[0].count() > 10_000, "unlink must create native editable vectors");
            app.run("edit.undo", json!({})).unwrap();
            assert!(app.session.doc().unwrap().doc.node(NodeId(panel.panel_id)).unwrap().name.as_deref().unwrap_or("").starts_with("RVS_PANEL|"));
            app.run("edit.redo", json!({})).unwrap();
            assert!(!app.session.doc().unwrap().doc.node(NodeId(panel.panel_id)).unwrap().name.as_deref().unwrap_or("").starts_with("RVS_PANEL|"));
        }
    }

    #[test]
    fn invalid_svg_does_not_create_a_plot_document() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        apply_rendered_plot(
            &mut app,
            RenderedPlot {
                variable: "broken".into(),
                svg: "<svg><broken".into(),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.starts_with("Could not add broken:"));
        assert!(app.session.active().is_none());
    }

    fn plot_svg(first: &str, last: &str) -> String {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="198.425pt" height="141.732pt" viewBox="0 0 198.425 141.732"><defs><linearGradient id="heat"><stop offset="0%" stop-color="{first}"/><stop offset="100%" stop-color="{last}"/></linearGradient></defs><rect width="198.425" height="141.732" fill="url(#heat)"/><text x="30" y="35" font-size="8" fill="#222222">Axis X</text></svg>"##
        )
    }

    #[test]
    fn refreshed_svg_preserves_one_edited_text_label_and_gradient_art() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        apply_rendered_plot(
            &mut app,
            RenderedPlot {
                variable: "plot_A".into(),
                svg: plot_svg("#0011ff", "#ff2200"),
                plots: Vec::new(),
                action: PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        let panel = app.scientific.linked_panels[0].clone();
        let mut label = None;
        let mut panel_label = None;
        app.session.doc().unwrap().doc.walk(|node| {
            if matches!(&node.kind, NodeKind::Text(text) if text.plain_text() == "Axis X") {
                label = Some(node.id.0);
            }
            if node.name.as_deref() == Some("RVS_LABEL") {
                panel_label = Some(node.id);
            }
        });
        let label = label.expect("SVG text should remain editable");
        let panel_label = panel_label.expect("outer panel label should exist");
        app.run("text.setText", json!({"ids": [label], "text": "Edited axis"})).unwrap();
        app.run("text.setStyle", json!({"ids": [label], "size": 12, "fill": "#336633"})).unwrap();
        app.run("object.move", json!({"ids": [label], "dx": 5, "dy": 3})).unwrap();
        let edited = app.session.doc().unwrap().doc.node(NodeId(label)).unwrap().clone();
        let before_bounds = app.session.doc().unwrap().doc.bounds_of(&[NodeId(panel.generated_id)], false).unwrap();
        let before_panel_bounds = app.session.doc().unwrap().doc.bounds_of(&[NodeId(panel.panel_id)], false).unwrap();

        apply_rendered_plot(
            &mut app,
            RenderedPlot {
                variable: "plot_A".into(),
                svg: plot_svg("#00aa44", "#dd00aa"),
                plots: Vec::new(),
                action: PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert!(app.scientific.r_status.contains("preserved 1 edited text"), "{}", app.scientific.r_status);
        let doc = &app.session.doc().unwrap().doc;
        assert_eq!(doc.node(NodeId(label)), Some(&edited));
        assert_eq!(doc.bounds_of(&[NodeId(panel.generated_id)], false).unwrap(), before_bounds);
        assert_eq!(doc.bounds_of(&[NodeId(panel.panel_id)], false).unwrap(), before_panel_bounds);
        assert!(matches!(&doc.node(panel_label).unwrap().kind, NodeKind::Text(text) if text.plain_text() == "A"));
        assert_eq!(app.scientific.linked_panels[0].width_mm, 70.0);
        assert_eq!(app.scientific.linked_panels[0].height_mm, 50.0);
        let mut gradients = 0;
        doc.walk(|node| {
            gradients += node
                .appearance
                .items
                .iter()
                .filter(|item| matches!(item, AppearanceItem::Fill(fill) if matches!(&fill.paint, Paint::Gradient(_))))
                .count();
        });
        assert!(gradients > 0, "refresh should keep native vector gradients");
        assert!(doc.node(NodeId(panel.panel_id)).is_some(), "linked panel identity must persist");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wait_for_r_plot(app: &mut crate::VectorcraftApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while app.scientific.running && std::time::Instant::now() < deadline {
            crate::scientific::poll_messages(app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        crate::scientific::poll_messages(app);
        assert!(!app.scientific.running, "R plot render timed out: {}", app.scientific.r_status);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn svg_device_mm(svg: &str, attribute: &str) -> Option<f64> {
        let tag = svg.split_once("<svg")?.1.split_once('>')?.0;
        let value =
            ['\'', '"'].into_iter().find_map(|quote| tag.split_once(&format!("{attribute}={quote}"))?.1.split_once(quote).map(|(value, _)| value))?;
        value.strip_suffix("pt")?.parse::<f64>().ok().map(|points| points / crate::scientific::DOC_POINTS_PER_MM)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn vector_layout(node: &Node, frame: vectorcraft_geom::Rect, layout: &mut Vec<[i64; 4]>) {
        if let Some(children) = node.children() {
            for child in children {
                vector_layout(child, frame, layout);
            }
        } else if let Some(bounds) = node.visual_bounds() {
            let width = frame.width();
            let height = frame.height();
            if width > 0.0 && height > 0.0 {
                layout.push([
                    ((bounds.x0 - frame.x0) / width * 10_000.0).round() as i64,
                    ((bounds.y0 - frame.y0) / height * 10_000.0).round() as i64,
                    ((bounds.x1 - frame.x0) / width * 10_000.0).round() as i64,
                    ((bounds.y1 - frame.y0) / height * 10_000.0).round() as i64,
                ]);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn generated_layout(app: &crate::VectorcraftApp, generated_id: u64) -> Vec<[i64; 4]> {
        let doc = &app.session.doc().unwrap().doc;
        let node = doc.node(NodeId(generated_id)).unwrap();
        let frame = node.visual_bounds().unwrap();
        let mut layout = Vec::new();
        vector_layout(node, frame, &mut layout);
        layout.sort_unstable();
        layout
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn assert_generated_size_mm(app: &crate::VectorcraftApp, generated_id: u64, width: f64, height: f64) {
        let frame = app.session.doc().unwrap().doc.node(NodeId(generated_id)).unwrap().visual_bounds().unwrap();
        assert!((frame.width() / crate::scientific::DOC_POINTS_PER_MM - width).abs() < 0.05, "generated width: {frame:?}");
        assert!((frame.height() / crate::scientific::DOC_POINTS_PER_MM - height).abs() < 0.05, "generated height: {frame:?}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn real_ggplot_reflow_rerenders_device_and_keeps_edited_label() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        {
            let mut session = app.scientific.session.lock().unwrap();
            let available = session
                .execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
                .unwrap();
            if available.output.trim() != "YES" {
                return;
            }
            let setup = session.execute("plot_A <- ggplot2::ggplot(mtcars, ggplot2::aes(wt, mpg, color=factor(cyl))) + ggplot2::geom_point() + ggplot2::labs(title='Test plot', x='Weight', y='MPG') + ggplot2::theme_classic()").unwrap();
            assert!(setup.error.is_none(), "{:?}", setup.error);
            for (width, height) in [(70.0, 50.0), (100.0, 80.0), (120.0, 90.0)] {
                let svg = session.render_plot_svg_with_style("plot_A", width, height, true).unwrap().0;
                assert!((svg_device_mm(&svg, "width").unwrap() - width).abs() < 0.5);
                assert!((svg_device_mm(&svg, "height").unwrap() - height).abs() < 0.5);
            }
        }
        crate::scientific::render_plot(
            &mut app.scientific,
            app.session.active().unwrap().uid,
            "plot_A".into(),
            PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        let panel = app.scientific.linked_panels[0].clone();
        assert_eq!((panel.width_mm, panel.height_mm), (70.0, 50.0));
        assert_generated_size_mm(&app, panel.generated_id, 70.0, 50.0);
        let first_layout = generated_layout(&app, panel.generated_id);
        app.run("object.scale", json!({"ids": [panel.panel_id], "sx": 100.0 / 70.0 * 100.0, "sy": 80.0 / 50.0 * 100.0})).unwrap();
        let position = app.session.doc().unwrap().doc.position(NodeId(panel.panel_id));
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_A".into(),
            PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 100.0, height_mm: 80.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
        assert_eq!((app.scientific.linked_panels[0].width_mm, app.scientific.linked_panels[0].height_mm), (100.0, 80.0));
        assert_generated_size_mm(&app, panel.generated_id, 100.0, 80.0);
        let second_layout = generated_layout(&app, panel.generated_id);
        assert_ne!(first_layout, second_layout, "reflow must import a new R layout, not scale the old SVG");
        assert_eq!(app.session.doc().unwrap().doc.position(NodeId(panel.panel_id)), position);
        let mut editable_title = None;
        app.session.doc().unwrap().doc.walk(|node| {
            if matches!(&node.kind, NodeKind::Text(text) if text.plain_text() == "Test plot") {
                editable_title = Some(node.id.0);
            }
        });
        let editable_title = editable_title.expect("ggplot title should be editable text");
        app.run("text.setText", json!({"ids": [editable_title], "text": "My plot"})).unwrap();
        app.run("text.setStyle", json!({"ids": [editable_title], "size": 12, "fill": "#336633"})).unwrap();
        app.run("object.move", json!({"ids": [editable_title], "dx": 2, "dy": 2})).unwrap();
        app.run("object.scale", json!({"ids": [panel.panel_id], "sx": 120, "sy": 112.5})).unwrap();
        let second_position = app.session.doc().unwrap().doc.position(NodeId(panel.panel_id));
        let edited = app.session.doc().unwrap().doc.node(NodeId(editable_title)).unwrap().clone();
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_A".into(),
            PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 120.0, height_mm: 90.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
        assert_eq!((app.scientific.linked_panels[0].width_mm, app.scientific.linked_panels[0].height_mm), (120.0, 90.0));
        assert_generated_size_mm(&app, panel.generated_id, 120.0, 90.0);
        assert_eq!(app.session.doc().unwrap().doc.position(NodeId(panel.panel_id)), second_position);
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(editable_title)), Some(&edited));
        let third_layout = generated_layout(&app, panel.generated_id);
        assert_ne!(second_layout, third_layout);
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_A".into(),
            PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 120.0, height_mm: 90.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Updated linked panel"), "{}", app.scientific.r_status);
        assert_eq!((app.scientific.linked_panels[0].width_mm, app.scientific.linked_panels[0].height_mm), (120.0, 90.0));
        assert_generated_size_mm(&app, panel.generated_id, 120.0, 90.0);
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(editable_title)), Some(&edited));
        let graph_style = crate::scientific::graph_style::GraphStyle { axis_x_line_mm: Some(1.2), ..Default::default() };
        crate::scientific::request_graph_apply(&mut app.scientific, &panel, graph_style.clone(), 120.0, 90.0).unwrap();
        crate::scientific::start_next_graph(&mut app);
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
        assert_eq!(app.scientific.linked_panels[0].graph_style, graph_style);
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(editable_title)), Some(&edited));
        let stale_style = crate::scientific::graph_style::GraphStyle { plot_title_pt: Some(24.0), ..Default::default() };
        let latest_style =
            crate::scientific::graph_style::GraphStyle { plot_title_pt: Some(14.0), tick_x_visible: Some(false), ..Default::default() };
        let current = app.scientific.linked_panels[0].clone();
        crate::scientific::request_graph_apply(&mut app.scientific, &current, stale_style, 120.0, 90.0).unwrap();
        crate::scientific::start_next_graph(&mut app);
        assert!(app.scientific.running);
        crate::scientific::request_graph_apply(&mut app.scientific, &current, latest_style.clone(), 120.0, 90.0).unwrap();
        wait_for_r_plot(&mut app);
        assert_eq!(app.scientific.linked_panels[0].graph_style, latest_style, "only the latest queued Graph style may reach the document");
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(editable_title)), Some(&edited));
        let valid_art = app.session.doc().unwrap().doc.clone();
        assert!(crate::scientific::request_graph_apply(&mut app.scientific, &current, latest_style.clone(), f64::NAN, 90.0).is_err());
        assert_eq!(app.session.doc().unwrap().doc, valid_art);
        assert!(app.scientific.graph_pending.is_empty());
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.scientific.linked_panels[0].graph_style, graph_style);
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(app.scientific.linked_panels[0].graph_style, latest_style);
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(editable_title)), Some(&edited));
        let before_failed_render = app.session.doc().unwrap().doc.node(NodeId(panel.generated_id)).unwrap().clone();
        let removed = app.scientific.session.lock().unwrap().execute("rm(plot_A)").unwrap();
        assert!(removed.error.is_none(), "{:?}", removed.error);
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_A".into(),
            PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 120.0, height_mm: 90.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.contains("Could not") || app.scientific.r_status.contains("failed"));
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(panel.generated_id)), Some(&before_failed_render));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn large_real_ggplot_keeps_one_embedded_source_through_graph_and_reflow() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        {
            let mut session = app.scientific.session.lock().unwrap();
            let packages = session
                .execute("cat(if (requireNamespace('ggplot2', quietly=TRUE) && requireNamespace('svglite', quietly=TRUE)) 'YES' else 'NO')")
                .unwrap();
            if packages.output.trim() != "YES" {
                return;
            }
            let setup = session.execute("set.seed(7); plot_B <- ggplot2::ggplot(data.frame(x=rnorm(15000), y=rnorm(15000), cluster=factor(rep(1:4, length.out=15000))), ggplot2::aes(x,y,colour=cluster)) + ggplot2::geom_point(alpha=.45, size=.4) + ggplot2::labs(title='Large plot'); plot_before <- serialize(plot_B, NULL)").unwrap();
            assert!(setup.error.is_none(), "{:?}", setup.error);
        }
        crate::scientific::render_plot(
            &mut app.scientific,
            app.session.active().unwrap().uid,
            "plot_B".into(),
            PanelAction::Create { label: "B".into(), width_mm: 70.0, height_mm: 50.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Added "), "{}", app.scientific.r_status);
        let panel = app.scientific.linked_panels[0].clone();
        let source_key = |app: &crate::VectorcraftApp| {
            let doc = &app.session.doc().unwrap().doc;
            let generated = doc.node(NodeId(panel.generated_id)).unwrap();
            let placed = generated
                .children()
                .unwrap()
                .iter()
                .find_map(|node| match &node.kind {
                    NodeKind::PlacedDocument(placed) => Some(placed),
                    _ => None,
                })
                .expect("large R plot should keep its SVG as one placed source");
            placed.key.clone()
        };
        let first_key = source_key(&app);
        let style = crate::scientific::graph_style::GraphStyle { plot_title_pt: Some(18.0), ..Default::default() };
        crate::scientific::request_graph_apply(&mut app.scientific, &panel, style.clone(), 70.0, 50.0).unwrap();
        crate::scientific::start_next_graph(&mut app);
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
        assert_eq!(app.scientific.linked_panels[0].graph_style, style);
        let second_key = source_key(&app);
        assert_ne!(first_key, second_key);
        assert!(!app.session.doc().unwrap().doc.images.contains_key(&first_key));
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_B".into(),
            PanelAction::Reflow { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 100.0, height_mm: 80.0 },
        );
        wait_for_r_plot(&mut app);
        assert!(app.scientific.r_status.starts_with("Reflowed "), "{}", app.scientific.r_status);
        assert_generated_size_mm(&app, panel.generated_id, 100.0, 80.0);
        assert_eq!(app.scientific.linked_panels[0].graph_style, style);
        assert_eq!(app.scientific.linked_panels[0].panel_id, panel.panel_id);
        let unchanged = app.scientific.session.lock().unwrap().execute("cat(identical(serialize(plot_B, NULL), plot_before))").unwrap();
        assert_eq!(unchanged.output.trim(), "TRUE", "R source plot must remain unchanged");
        let before_failure = app.session.doc().unwrap().doc.node(NodeId(panel.generated_id)).unwrap().clone();
        app.scientific.session.lock().unwrap().execute("rm(plot_B)").unwrap();
        crate::scientific::render_plot(
            &mut app.scientific,
            panel.document_uid,
            "plot_B".into(),
            PanelAction::Refresh { panel_id: panel.panel_id, generated_id: panel.generated_id, width_mm: 100.0, height_mm: 80.0 },
        );
        wait_for_r_plot(&mut app);
        assert_eq!(app.session.doc().unwrap().doc.node(NodeId(panel.generated_id)), Some(&before_failure));
    }
}
