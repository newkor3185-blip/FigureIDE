//! Graph controls in the native right-hand dock.

use super::graph_style::{FontFace, GraphStyle, LegendDirection, LegendPosition, TextDetails, TitlePosition};
use super::{panel_caption, request_graph_apply, selected_linked_panel};

fn number_row(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, suggested: f64, suffix: &str, range: std::ops::RangeInclusive<f64>) -> bool {
    ui.horizontal(|ui| {
        let mut committed = false;
        let label_response = ui.add_sized([ui.available_width().min(130.0), 19.0], egui::Label::new(label));
        let mut enabled = value.is_some();
        if ui.checkbox(&mut enabled, "").on_hover_text("Use a panel override; uncheck for the R plot default").changed() {
            *value = enabled.then_some(suggested);
            committed = true;
        }
        if let Some(number) = value {
            let response = ui
                .add(egui::DragValue::new(number).range(range).speed(0.2).suffix(suffix).update_while_editing(false))
                .labelled_by(label_response.id);
            committed |=
                response.lost_focus() || response.drag_stopped() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        } else {
            ui.weak("R default");
        }
        committed
    })
    .inner
}

fn visibility_row(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>) -> bool {
    let before = *value;
    ui.horizontal(|ui| {
        let label_response = ui.add_sized([ui.available_width().min(142.0), 19.0], egui::Label::new(label));
        egui::ComboBox::from_id_salt(("graph-visibility", label))
            .selected_text(match value {
                None => "R default",
                Some(true) => "Show",
                Some(false) => "Hide",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(value, None, "R default");
                ui.selectable_value(value, Some(true), "Show");
                ui.selectable_value(value, Some(false), "Hide");
            })
            .response
            .labelled_by(label_response.id);
    });
    *value != before
}

fn typography_row(ui: &mut egui::Ui, label: &str, size: &mut Option<f64>, visible: &mut Option<bool>, suggested: f64) -> bool {
    number_row(ui, &format!("{label} size"), size, suggested, " pt", 0.1..=100.0) | visibility_row(ui, &format!("{label} visibility"), visible)
}

fn string_row(ui: &mut egui::Ui, label: &str, value: &mut Option<String>, suggested: &str) -> bool {
    ui.horizontal(|ui| {
        let mut committed = false;
        ui.label(label);
        let mut enabled = value.is_some();
        if ui.checkbox(&mut enabled, "").on_hover_text("Use a custom value; uncheck to inherit from R").changed() {
            *value = enabled.then(|| suggested.to_owned());
            committed = true;
        }
        if let Some(value) = value {
            let response = ui.add_sized([ui.available_width().max(60.0), 20.0], egui::TextEdit::singleline(value));
            committed |= response.lost_focus() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        } else {
            ui.weak("R default");
        }
        committed
    })
    .inner
}

fn face_row(ui: &mut egui::Ui, label: &str, value: &mut Option<FontFace>) -> bool {
    let before = *value;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(("graph-face", label)).selected_text(value.map_or("R default", FontFace::label)).show_ui(ui, |ui| {
            ui.selectable_value(value, None, "R default");
            for face in FontFace::ALL {
                ui.selectable_value(value, Some(face), face.label());
            }
        });
    });
    *value != before
}

fn text_details(ui: &mut egui::Ui, label: &str, details: &mut TextDetails, tick: bool, title: bool) -> bool {
    let mut committed = false;
    committed |= string_row(ui, &format!("{label} family"), &mut details.family, "sans");
    committed |= face_row(ui, &format!("{label} face"), &mut details.face);
    committed |= string_row(ui, &format!("{label} colour"), &mut details.colour, "#222222");
    if tick {
        committed |= number_row(ui, &format!("{label} angle"), &mut details.angle, 45.0, "°", -360.0..=360.0);
        committed |= number_row(ui, &format!("{label} hjust"), &mut details.hjust, 1.0, "", 0.0..=1.0);
        committed |= number_row(ui, &format!("{label} vjust"), &mut details.vjust, 1.0, "", 0.0..=1.0);
    } else if title {
        committed |= number_row(ui, "Title alignment", &mut details.hjust, 0.5, "", 0.0..=1.0);
    }
    committed
}

fn text_section(
    ui: &mut egui::Ui,
    label: &str,
    size: &mut Option<f64>,
    visible: &mut Option<bool>,
    details: &mut TextDetails,
    options: (f64, bool, bool),
) -> bool {
    let (suggested, tick, title) = options;
    let mut committed = false;
    egui::CollapsingHeader::new(label).id_salt(("graph-text", label)).default_open(false).show(ui, |ui| {
        committed |= typography_row(ui, label, size, visible, suggested);
        committed |= text_details(ui, label, details, tick, title);
    });
    committed
}

fn margins_row(ui: &mut egui::Ui, label: &str, value: &mut Option<[f64; 4]>) -> bool {
    let mut committed = false;
    let mut enabled = value.is_some();
    if ui.checkbox(&mut enabled, label).on_hover_text("Top, right, bottom, left in millimeters; uncheck to inherit from R").changed() {
        *value = enabled.then_some([5.0; 4]);
        committed = true;
    }
    if let Some(margins) = value {
        for (index, side) in ["Top", "Right", "Bottom", "Left"].into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(side);
                let response =
                    ui.add(egui::DragValue::new(&mut margins[index]).range(-100.0..=100.0).speed(0.2).suffix(" mm").update_while_editing(false));
                committed |= response.lost_focus()
                    || response.drag_stopped()
                    || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            });
        }
    } else {
        ui.weak("R default");
    }
    committed
}

fn justification_row(ui: &mut egui::Ui, value: &mut Option<[f64; 2]>) -> bool {
    let mut committed = false;
    let mut enabled = value.is_some();
    if ui.checkbox(&mut enabled, "Legend justification").on_hover_text("Override both X and Y; uncheck to inherit both from R").changed() {
        *value = enabled.then_some([0.5, 0.5]);
        committed = true;
    }
    if let Some(values) = value {
        for (index, axis) in ["X", "Y"].into_iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(axis);
                let response = ui.add(egui::DragValue::new(&mut values[index]).range(0.0..=1.0).speed(0.01).update_while_editing(false));
                committed |= response.lost_focus()
                    || response.drag_stopped()
                    || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
            });
        }
    } else {
        ui.weak("R default");
    }
    committed
}

pub(crate) fn show(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    if !app.scientific.enabled {
        ui.label("The Graph Inspector is available in the R workspace.");
        return;
    }
    let document_uid = app.session.active().map(|doc| doc.uid).unwrap_or_default();
    let linked_count = app.scientific.linked_panels.iter().filter(|panel| panel.document_uid == document_uid).count();
    egui::CollapsingHeader::new(format!("Plot Registry ({})", app.scientific.plots.len()))
        .id_salt("graph-plot-registry")
        .default_open(true)
        .show(ui, |ui| super::plot_registry::show_plot_registry(app, ui));
    egui::CollapsingHeader::new(format!("Linked Panels ({linked_count})"))
        .id_salt("graph-linked-panels")
        .default_open(true)
        .show(ui, |ui| super::linked_plots::show_linked_panel_entries(app, ui));
    egui::CollapsingHeader::new("Selected Graph Settings")
        .id_salt("graph-selected-settings")
        .default_open(true)
        .show(ui, |ui| show_selected_graph_settings(app, ui));
}

fn show_selected_graph_settings(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    let Some(panel) = selected_linked_panel(app) else {
        ui.label("Select an R-linked plot on the artboard to edit its graph settings.");
        return;
    };
    let connected = app.scientific.plots.iter().any(|plot| plot.variable == panel.r_variable);
    let is_ggplot = app.scientific.plots.iter().any(|plot| plot.variable == panel.r_variable && plot.plot_kind == "ggplot");
    let draft_key = (panel.document_uid, panel.panel_id);
    let mut draft = app.scientific.graph_drafts.entry(draft_key).or_insert_with(|| panel.graph_style.clone()).clone();
    ui.strong(format!("Panel {} · {}", panel.label, panel.r_variable));
    ui.label(if connected { "Connected to R" } else { "R variable missing · existing artwork retained" });
    if app.scientific.running {
        panel_caption(ui, "Rendering in progress; committed changes will apply afterward.");
    }
    ui.separator();
    ui.add_enabled_ui(connected, |ui| {
        ui.strong("Figure Size");
        super::inspector::show_graph_plot_properties(app, ui);
    });
    ui.separator();
    if !is_ggplot {
        panel_caption(ui, "Axes, typography and legend controls require a ggplot object. Figure Size remains available for supported plots.");
    }
    let mut committed = false;
    ui.add_enabled_ui(is_ggplot, |ui| {
        egui::CollapsingHeader::new("Axes").default_open(true).show(ui, |ui| {
            committed |= number_row(ui, "X line width", &mut draft.axis_x_line_mm, 0.5, " mm", 0.1..=20.0);
            committed |= number_row(ui, "Y line width", &mut draft.axis_y_line_mm, 0.5, " mm", 0.1..=20.0);
            committed |= visibility_row(ui, "X-axis line", &mut draft.axis_x_visible);
            committed |= visibility_row(ui, "Y-axis line", &mut draft.axis_y_visible);
            egui::CollapsingHeader::new("Tick marks").default_open(false).show(ui, |ui| {
                committed |= visibility_row(ui, "X tick marks", &mut draft.axis_x_ticks_visible);
                committed |= visibility_row(ui, "Y tick marks", &mut draft.axis_y_ticks_visible);
                committed |= string_row(ui, "X line colour", &mut draft.axis_x_line_colour, "#222222");
                committed |= string_row(ui, "Y line colour", &mut draft.axis_y_line_colour, "#222222");
                committed |= string_row(ui, "X tick colour", &mut draft.axis_x_tick_colour, "#222222");
                committed |= string_row(ui, "Y tick colour", &mut draft.axis_y_tick_colour, "#222222");
                committed |= number_row(ui, "X tick width", &mut draft.axis_x_tick_width_mm, 0.4, " mm", 0.01..=20.0);
                committed |= number_row(ui, "Y tick width", &mut draft.axis_y_tick_width_mm, 0.4, " mm", 0.01..=20.0);
                committed |= number_row(ui, "X tick length", &mut draft.axis_x_tick_length_mm, 1.5, " mm", 0.0..=100.0);
                committed |= number_row(ui, "Y tick length", &mut draft.axis_y_tick_length_mm, 1.5, " mm", 0.0..=100.0);
            });
        });
        egui::CollapsingHeader::new("Typography").default_open(true).show(ui, |ui| {
            committed |=
                text_section(ui, "Plot title", &mut draft.plot_title_pt, &mut draft.plot_title_visible, &mut draft.title_details, (12.0, false, true));
            committed |= text_section(ui, "Plot subtitle", &mut draft.subtitle_pt, &mut draft.subtitle_visible, &mut draft.subtitle_details, (10.0, false, false));
            committed |=
                text_section(ui, "Plot caption", &mut draft.caption_pt, &mut draft.caption_visible, &mut draft.caption_details, (8.0, false, false));
            committed |= text_section(ui, "X-axis title", &mut draft.axis_title_x_pt, &mut draft.axis_title_x_visible, &mut draft.axis_title_x_details, (10.0, false, false));
            committed |= text_section(ui, "Y-axis title", &mut draft.axis_title_y_pt, &mut draft.axis_title_y_visible, &mut draft.axis_title_y_details, (10.0, false, false));
            committed |=
                text_section(ui, "X-axis tick labels", &mut draft.tick_x_pt, &mut draft.tick_x_visible, &mut draft.tick_x_details, (8.0, true, false));
            committed |=
                text_section(ui, "Y-axis tick labels", &mut draft.tick_y_pt, &mut draft.tick_y_visible, &mut draft.tick_y_details, (8.0, true, false));
            committed |= text_section(ui, "Legend title", &mut draft.legend_title_pt, &mut draft.legend_title_visible, &mut draft.legend_title_details, (10.0, false, false));
            committed |= text_section(ui, "Legend text", &mut draft.legend_text_pt, &mut draft.legend_text_visible, &mut draft.legend_text_details, (8.0, false, false));
            let before = draft.plot_title_position;
            ui.horizontal(|ui| {
                ui.label("Title position");
                egui::ComboBox::from_id_salt("graph-title-position")
                    .selected_text(draft.plot_title_position.map_or("R default", TitlePosition::label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.plot_title_position, None, "R default");
                        for position in TitlePosition::ALL {
                            ui.selectable_value(&mut draft.plot_title_position, Some(position), position.label());
                        }
                    });
            });
            committed |= before != draft.plot_title_position;
        });
        egui::CollapsingHeader::new("Grid and Panel").default_open(false).show(ui, |ui| {
            committed |= visibility_row(ui, "Major grid", &mut draft.major_grid_visible);
            committed |= visibility_row(ui, "Major grid X", &mut draft.major_grid_x_visible);
            committed |= visibility_row(ui, "Major grid Y", &mut draft.major_grid_y_visible);
            committed |= visibility_row(ui, "Minor grid X", &mut draft.minor_grid_x_visible);
            committed |= visibility_row(ui, "Minor grid Y", &mut draft.minor_grid_y_visible);
            committed |= string_row(ui, "Major grid colour", &mut draft.major_grid_colour, "grey85");
            committed |= string_row(ui, "Minor grid colour", &mut draft.minor_grid_colour, "grey90");
            committed |= number_row(ui, "Major grid width", &mut draft.major_grid_width_mm, 0.2, " mm", 0.01..=20.0);
            committed |= number_row(ui, "Minor grid width", &mut draft.minor_grid_width_mm, 0.1, " mm", 0.01..=20.0);
            committed |= string_row(ui, "Panel fill", &mut draft.panel_background, "#ffffff");
            committed |= string_row(ui, "Plot fill", &mut draft.plot_background, "#ffffff");
            committed |= visibility_row(ui, "Panel border", &mut draft.panel_border_visible);
            committed |= string_row(ui, "Border colour", &mut draft.panel_border_colour, "#222222");
            committed |= number_row(ui, "Border width", &mut draft.panel_border_width_mm, 0.4, " mm", 0.01..=20.0);
        });
        egui::CollapsingHeader::new("Legend").default_open(true).show(ui, |ui| {
            let old_position = draft.legend_position;
            ui.horizontal(|ui| {
                ui.label("Position");
                egui::ComboBox::from_id_salt(("graph-legend-position", panel.panel_id))
                    .selected_text(draft.legend_position.map_or("R default", LegendPosition::label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.legend_position, None, "R default");
                        for position in LegendPosition::ALL {
                            ui.selectable_value(&mut draft.legend_position, Some(position), position.label());
                        }
                    });
            });
            committed |= old_position != draft.legend_position;
            if draft.legend_position == Some(LegendPosition::Inside) {
                committed |= number_row(ui, "Inside X", &mut draft.inside_x, 0.95, "", 0.0..=1.0);
                committed |= number_row(ui, "Inside Y", &mut draft.inside_y, 0.95, "", 0.0..=1.0);
            }
            committed |= number_row(ui, "Legend key", &mut draft.legend_key_mm, 5.0, " mm", 0.1..=100.0);
            egui::CollapsingHeader::new("Advanced legend").default_open(false).show(ui, |ui| {
                let before = draft.legend_direction;
                ui.horizontal(|ui| {
                    ui.label("Direction");
                    egui::ComboBox::from_id_salt("graph-legend-direction")
                        .selected_text(draft.legend_direction.map_or("R default", LegendDirection::label))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.legend_direction, None, "R default");
                            for direction in LegendDirection::ALL {
                                ui.selectable_value(&mut draft.legend_direction, Some(direction), direction.label());
                            }
                        });
                });
                committed |= before != draft.legend_direction;
                committed |= justification_row(ui, &mut draft.legend_justification);
                committed |= number_row(ui, "Key width", &mut draft.legend_key_width_mm, 5.0, " mm", 0.0..=100.0);
                committed |= number_row(ui, "Key height", &mut draft.legend_key_height_mm, 5.0, " mm", 0.0..=100.0);
                committed |= string_row(ui, "Key fill", &mut draft.legend_key_background, "#ffffff");
                committed |= string_row(ui, "Legend fill", &mut draft.legend_background, "#ffffff");
                committed |= number_row(ui, "Spacing X", &mut draft.legend_spacing_x_mm, 2.0, " mm", 0.0..=100.0);
                committed |= number_row(ui, "Spacing Y", &mut draft.legend_spacing_y_mm, 2.0, " mm", 0.0..=100.0);
                committed |= margins_row(ui, "Legend margins", &mut draft.legend_margin_mm);
            });
        });
        egui::CollapsingHeader::new("Margins and Facets").default_open(false).show(ui, |ui| {
            committed |= margins_row(ui, "Plot margins", &mut draft.plot_margin_mm);
            committed |= text_section(ui, "Facet strip text", &mut draft.strip_text_pt, &mut draft.strip_text_visible, &mut draft.strip_text_details, (9.0, false, false));
            committed |= string_row(ui, "Facet strip fill", &mut draft.strip_background, "#eeeeee");
            committed |= number_row(ui, "Panel spacing", &mut draft.panel_spacing_mm, 2.0, " mm", 0.0..=100.0);
        });
    });
    app.scientific.graph_drafts.insert(draft_key, draft.clone());
    ui.separator();
    if ui.add_enabled(is_ggplot && !draft.is_default(), egui::Button::new("Reset Graph Styles to R")).clicked() {
        draft = GraphStyle::default();
        app.scientific.graph_drafts.insert(draft_key, draft.clone());
        committed = true;
    }
    if committed {
        let dimensions = app.scientific.graph_pending.get(&draft_key).map(|request| (request.width_mm, request.height_mm));
        let (width_mm, height_mm) = dimensions.unwrap_or((panel.width_mm, panel.height_mm));
        if let Err(error) = request_graph_apply(&mut app.scientific, &panel, draft, width_mm, height_mm) {
            app.scientific.r_status = format!("Could not apply Graph settings: {error}");
        }
    }
    if app.scientific.r_status.starts_with("Could not") || app.scientific.r_status.contains("failed") {
        ui.colored_label(crate::theme::Tokens::get(ui.ctx()).warning, &app.scientific.r_status);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn advanced_graph_sections_render_for_a_selected_ggplot_panel() {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.run("file.new", serde_json::json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        crate::scientific::apply_rendered_plot(
            &mut app,
            crate::scientific::RenderedPlot {
                variable: "plot_A".into(),
                svg: "<svg xmlns='http://www.w3.org/2000/svg' width='198.425pt' height='141.732pt'><rect width='198.425' height='141.732' fill='white'/><text x='20' y='20'>Title</text></svg>".into(),
                plots: Vec::new(),
                action: crate::scientific::PanelAction::Create { label: "A".into(), width_mm: 70.0, height_mm: 50.0 },
            },
        );
        assert_eq!(app.scientific.linked_panels.len(), 1, "{}", app.scientific.r_status);
        app.scientific.plots.push(crate::r_session::RObject {
            variable: "plot_A".into(),
            classes: vec!["ggplot".into()],
            data_type: "list".into(),
            dimensions: String::new(),
            length: 1,
            preview: String::new(),
            category: "Values".into(),
            description: "ggplot2::ggplot".into(),
            plot_kind: "ggplot".into(),
        });
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 900.0));
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            super::show(&mut app, ui);
        });
        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
        assert!(!app.scientific.r_status.starts_with("Could not"), "{}", app.scientific.r_status);
    }
}
