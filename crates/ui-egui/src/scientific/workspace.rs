use super::{
    BottomTab, RMessage, WorkspaceGeometry, append_console_output, detect_r, observe_selected_panel_resize, panel_caption, poll_messages,
    request_packages, restore_linked_panels, selected_linked_panel, show_close_script_prompt, show_console, show_environment, show_help,
    show_history, show_packages, show_script_encoding_prompt, show_script_tabs, show_source_editor, status::show_status_indicator,
};

pub fn show(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    if app.scientific.scripts.is_empty() {
        app.scientific.new_script();
    }
    app.scientific.repaint_ctx = Some(ui.ctx().clone());
    restore_linked_panels(app);
    poll_messages(app);
    let selected_panel = selected_linked_panel(app);
    let selected_panel_id = selected_panel.as_ref().map(|panel| panel.panel_id);
    if app.scientific.last_selected_linked_panel != selected_panel_id {
        if let Some(panel) = selected_panel {
            app.scientific.selected_plot = Some(panel.r_variable);
            super::focus_graph_tab(app);
        }
        app.scientific.last_selected_linked_panel = selected_panel_id;
    }
    observe_selected_panel_resize(app, ui);
    if selected_linked_panel(app).is_none() {
        app.scientific.selected_properties_panel = None;
        app.scientific.properties_source = None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !app.scientific.checking_r && !app.scientific.availability_checked {
        app.scientific.checking_r = true;
        let tx = app.scientific._tx.clone();
        let ctx = ui.ctx().clone();
        if let Err(error) = std::thread::Builder::new().name("r-availability".into()).spawn(move || {
            let result = detect_r();
            let _ = tx.send(RMessage::Availability(result));
            ctx.request_repaint();
        }) {
            app.scientific.checking_r = false;
            app.scientific.availability_checked = true;
            append_console_output(&mut app.scientific, &format!("Error: Could not start R check: {error}\n"));
        }
    }
    #[cfg(target_arch = "wasm32")]
    if !app.scientific.availability_checked {
        app.scientific.availability_checked = true;
        append_console_output(&mut app.scientific, "Error: R execution is available in the native desktop app only\n");
    }

    let available = ui.available_size();
    let tools_width = crate::toolbar::embedded_width(app);
    let (left_width, right_width) = workspace_widths(available.x, tools_width, app.ui.r_studio_workspace_width);
    let pane_height = available.y.max(0.0);
    let spacing = ui.spacing().item_spacing;
    let mut geometry = WorkspaceGeometry { toolbar: ui.ctx().data(|d| d.get_temp::<egui::Rect>(app_bar_rect_id())), ..Default::default() };
    ui.horizontal(|ui| {
        // Every occupied width is explicit. Implicit inter-widget gaps previously pushed
        // the canvas beyond its allocation and made the requested pane sizes inaccurate.
        ui.spacing_mut().item_spacing.x = 0.0;
        geometry.workspace = Some(
            ui.allocate_ui_with_layout(egui::vec2(left_width, pane_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing = spacing;
                show_script_tabs(app, ui);
                let header_rect = ui
                    .horizontal(|ui| {
                        panel_caption(ui, if app.scientific.active_viewer_tab.is_some() { "Data Viewer" } else { "Source" });
                        if ui.small_button(if app.ui.r_studio_editor_maximized { "Restore editor" } else { "Maximize editor" }).clicked() {
                            app.ui.r_studio_editor_maximized = !app.ui.r_studio_editor_maximized;
                        }
                        show_status_indicator(app, ui);
                    })
                    .response
                    .rect;
                if geometry.toolbar.is_none() {
                    geometry.toolbar = Some(header_rect);
                }
                let budget = (ui.available_height() - R_SPLITTER_SIZE - 2.0 * spacing.y).max(0.0);
                let (min_editor, max_editor) = editor_limits(budget);
                let height = if app.ui.r_studio_editor_maximized {
                    ui.available_height().max(0.0)
                } else {
                    (budget * app.ui.r_studio_editor_fraction).clamp(min_editor, max_editor)
                };
                if let Some(active_viewer) = app.scientific.active_viewer_tab.clone() {
                    super::show_data_viewer_tab(&mut app.scientific, ui, height, &active_viewer);
                } else {
                    show_source_editor(&mut app.scientific, ui, height);
                }
                if !app.ui.r_studio_editor_maximized {
                    let divider = workspace_splitter(ui, "r-source-console-divider", true, left_width);
                    geometry.editor_divider = Some(divider.rect);
                    geometry.editor_divider_id = Some(divider.id);
                    if let Some(pixels) = resized_pane(ui, &divider, true, height, min_editor, max_editor) {
                        app.ui.r_studio_editor_fraction = (pixels / budget.max(1.0)).clamp(0.25, 0.85);
                    }
                    show_bottom_tabs(app, ui);
                }
            })
            .response
            .rect,
        );
        let divider = workspace_splitter(ui, "r-workspace-artboard-divider", false, pane_height);
        geometry.divider = Some(divider.rect);
        geometry.divider_id = Some(divider.id);
        let (minimum, _) = workspace_widths(available.x, tools_width, 0.0);
        let (maximum, _) = workspace_widths(available.x, tools_width, f32::MAX);
        if let Some(width) = resized_pane(ui, &divider, false, left_width, minimum, maximum) {
            app.ui.r_studio_workspace_width = width;
        }
        let tools_origin = ui.cursor().min;
        crate::toolbar::show_embedded(app, ui);
        if tools_width > 0.0 {
            geometry.tools = Some(egui::Rect::from_min_size(tools_origin, egui::vec2(tools_width, pane_height)));
        }
        geometry.vector = Some(
            ui.allocate_ui_with_layout(egui::vec2(right_width, pane_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing = spacing;
                crate::panel_docking::show(app, ui)
            })
            .response
            .rect,
        );
    });
    ui.ctx().data_mut(|d| d.insert_temp(workspace_geometry_id(), geometry));
    show_close_script_prompt(app, ui.ctx());
    show_script_encoding_prompt(app, ui.ctx());
}

const R_SPLITTER_SIZE: f32 = 10.0;

pub(super) fn workspace_geometry_id() -> egui::Id {
    egui::Id::unique("r-workspace-geometry")
}

pub(crate) fn app_bar_rect_id() -> egui::Id {
    egui::Id::unique("r-app-bar-rect")
}

fn workspace_widths(width: f32, tools: f32, requested: f32) -> (f32, f32) {
    let body = (width - tools - R_SPLITTER_SIZE).max(0.0);
    let minimum = 280.0_f32.min(body * 0.45);
    let maximum = (body - 360.0_f32.min(body * 0.55)).max(minimum).min(1600.0);
    let left = requested.clamp(minimum, maximum);
    (left, (body - left).max(0.0))
}

fn editor_limits(budget: f32) -> (f32, f32) {
    let minimum = 80.0_f32.min(budget * 0.4);
    (minimum, (budget - 160.0_f32.min(budget * 0.6)).max(minimum))
}

fn workspace_splitter(ui: &mut egui::Ui, salt: &str, horizontal: bool, length: f32) -> egui::Response {
    let size = if horizontal { egui::vec2(length, R_SPLITTER_SIZE) } else { egui::vec2(R_SPLITTER_SIZE, length) };
    let (_, rect) = ui.allocate_space(size);
    let response = ui.interact(rect, ui.make_persistent_id(salt), egui::Sense::click_and_drag());
    if response.clicked() || response.drag_started() {
        response.request_focus();
    }
    let label = if horizontal { "Resize Source and Console" } else { "Resize R workspace" };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let t = crate::theme::Tokens::get(ui.ctx());
    let active = response.hovered() || response.dragged() || response.has_focus();
    if active {
        ui.painter().rect_filled(rect, 2.0, t.hover);
    }
    let color = if active { t.accent } else { t.divider };
    if horizontal {
        ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, color));
    } else {
        ui.painter().vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, color));
    }
    for offset in [-5.0, 0.0, 5.0] {
        let center = rect.center() + if horizontal { egui::vec2(offset, 0.0) } else { egui::vec2(0.0, offset) };
        ui.painter().circle_filled(center, 1.3, if active { t.accent } else { t.text_dim });
    }
    response
        .on_hover_cursor(if horizontal { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal })
        .on_hover_text(format!("{label}: drag or use arrow keys. Reset sizes in Window › R Studio."))
}

fn resized_pane(ui: &egui::Ui, response: &egui::Response, horizontal: bool, current: f32, min: f32, max: f32) -> Option<f32> {
    let start_id = response.id.with("drag-start");
    if response.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, current));
    }
    let mut value = None;
    if response.dragged() {
        let initial = ui.ctx().data(|d| d.get_temp::<f32>(start_id)).unwrap_or(current);
        let delta = response.total_drag_delta().unwrap_or_default();
        value = Some(initial + if horizontal { delta.y } else { delta.x });
    }
    if response.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<f32>(start_id));
    }
    if response.has_focus() {
        let (less, more) = if horizontal { (egui::Key::ArrowUp, egui::Key::ArrowDown) } else { (egui::Key::ArrowLeft, egui::Key::ArrowRight) };
        let change = ui.input_mut(|i| {
            let step = if i.modifiers.shift { 64.0 } else { 16.0 };
            if i.consume_key(egui::Modifiers::NONE, less) || i.consume_key(egui::Modifiers::SHIFT, less) {
                -step
            } else if i.consume_key(egui::Modifiers::NONE, more) || i.consume_key(egui::Modifiers::SHIFT, more) {
                step
            } else {
                0.0
            }
        });
        if change != 0.0 {
            value = Some(current + change);
        }
    }
    value.map(|v| v.clamp(min, max))
}

pub(super) fn show_bottom_tabs(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let active = BottomTab::from_key(&app.ui.r_studio_tab);
    let strip_frame = egui::Frame::new().fill(t.tab_strip).inner_margin(egui::Margin::symmetric(4, 2)).corner_radius(egui::CornerRadius {
        nw: t.radius,
        ne: t.radius,
        sw: 0,
        se: 0,
    });
    let strip_rect = strip_frame
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                for tab in BottomTab::ALL {
                    let mut label = tab.label().to_owned();
                    if tab == BottomTab::Console && app.scientific.console_error {
                        label.push_str("  !");
                    }
                    let is_active = active == tab;
                    let text = if is_active {
                        egui::RichText::new(&label).size(t.font_size).strong().color(t.text_strong)
                    } else {
                        egui::RichText::new(&label).size(t.font_size).color(t.text_dim)
                    };
                    let response = ui.selectable_label(is_active, text).on_hover_text(tab.label());
                    if is_active {
                        let r = response.rect;
                        ui.painter().hline(r.x_range(), r.bottom(), egui::Stroke::new(2.0, t.accent));
                    }
                    if response.clicked() {
                        app.ui.r_studio_tab = tab.key().to_owned();
                        if tab == BottomTab::Console {
                            app.scientific.console_error = false;
                            app.scientific.console_scroll_at_bottom = true;
                            app.scientific.pending_console_focus = true;
                        }
                    }
                }
            });
        })
        .response
        .rect;
    ui.painter().hline(strip_rect.x_range(), strip_rect.bottom(), egui::Stroke::new(1.0, t.border));
    ui.add_space(2.0);
    let active = BottomTab::from_key(&app.ui.r_studio_tab);
    if active == BottomTab::Console {
        show_console(app, ui);
        return;
    }
    egui::ScrollArea::vertical().id_salt("scientific-r-lower-panes").max_height(ui.available_height()).auto_shrink([false, false]).show(ui, |ui| {
        match active {
            BottomTab::Console => show_console(app, ui),
            #[cfg(test)]
            BottomTab::PlotRegistry | BottomTab::LinkedPanels => show_console(app, ui),
            BottomTab::Environment => show_environment(app, ui),
            BottomTab::History => show_history(app, ui),
            BottomTab::Help => show_help(app, ui),
            BottomTab::Packages => {
                if !app.scientific.packages_loaded && !app.scientific.running {
                    app.scientific.packages_loaded = true;
                    request_packages(&mut app.scientific);
                }
                show_packages(app, ui)
            }
        }
    });
}
