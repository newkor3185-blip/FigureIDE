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

fn property_popover<R>(ui: &mut egui::Ui, id: &str, label: &str, content: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    property_popover_with_response(ui, id, label, content).0
}

fn property_popover_with_response<R>(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> (Option<R>, egui::Response) {
    let response = ui
        .push_id(("graph-advanced", id), |ui| {
            ui.add(egui::Label::new(egui::RichText::new(label).underline()).sense(egui::Sense::click()))
                .on_hover_text(format!("More {label} settings"))
        })
        .inner;
    let popup = crate::widgets::popover(&response, response.clicked(), |ui| {
        ui.set_width(ui.ctx().content_rect().width().clamp(180.0, 300.0));
        egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height().min(480.0)).auto_shrink([false, false]).show(ui, content).inner
    });
    (popup, response)
}

fn basic_visibility(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>) -> bool {
    let shown = *value != Some(false);
    let mut changed = false;
    if crate::widgets::toggle_icon(ui, if shown { "eye" } else { "eye-off" }, shown, 18.0, label) {
        *value = Some(!shown);
        changed = true;
    }
    if value.is_some() && ui.small_button("R").on_hover_text("Use the original R setting").clicked() {
        *value = None;
        changed = true;
    }
    changed
}

struct TextRowSpec<'a> {
    id: &'a str,
    label: &'a str,
    tick: bool,
    title: bool,
}

fn basic_text_row(
    ui: &mut egui::Ui,
    spec: TextRowSpec<'_>,
    size: &mut Option<f64>,
    visible: &mut Option<bool>,
    details: &mut TextDetails,
    suggested: f64,
) -> bool {
    let mut committed = ui
        .horizontal(|ui| {
            let (popover, label_response) =
                property_popover_with_response(ui, spec.id, spec.label, |ui| advanced_text(ui, spec.label, details, spec.tick, spec.title));
            let mut committed = popover.unwrap_or(false);
            committed |= compact_number_control(ui, size, suggested, " pt", 0.1..=100.0, Some(label_response.id));
            committed |= basic_visibility(ui, &format!("{} visibility", spec.label), visible);
            committed
        })
        .inner;
    committed |= face_row(ui, &format!("{} face", spec.label), &mut details.face);
    committed
}

fn compact_number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<f64>,
    suggested: f64,
    suffix: &str,
    range: std::ops::RangeInclusive<f64>,
) -> bool {
    let label_response = ui.label(label);
    compact_number_control(ui, value, suggested, suffix, range, Some(label_response.id))
}

fn compact_number_control(
    ui: &mut egui::Ui,
    value: &mut Option<f64>,
    suggested: f64,
    suffix: &str,
    range: std::ops::RangeInclusive<f64>,
    labelled_by: Option<egui::Id>,
) -> bool {
    let mut committed = false;
    if value.is_none() {
        ui.weak("R default");
        if ui.small_button("+").on_hover_text("Set a FigureIDE override").clicked() {
            *value = Some(suggested);
            committed = true;
        }
    } else {
        if let Some(number) = value.as_mut() {
            let mut response = ui.add_sized(
                [ui.available_width().clamp(48.0, 84.0), 20.0],
                egui::DragValue::new(number).range(range).speed(0.2).suffix(suffix).update_while_editing(false),
            );
            if let Some(labelled_by) = labelled_by {
                response = response.labelled_by(labelled_by);
            }
            committed |=
                response.lost_focus() || response.drag_stopped() || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
        }
        if ui.small_button("R").on_hover_text("Use the original R value").clicked() {
            *value = None;
            committed = true;
        }
    }
    committed
}

fn advanced_text(ui: &mut egui::Ui, label: &str, details: &mut TextDetails, tick: bool, title: bool) -> bool {
    let mut committed = false;
    committed |= string_row(ui, &format!("{label} family"), &mut details.family, "sans");
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

fn basic_grid_visibility(ui: &mut egui::Ui, label: &str, x: &mut Option<bool>, y: &mut Option<bool>) -> bool {
    let shown = !(*x == Some(false) && *y == Some(false));
    let mut changed = false;
    if crate::widgets::toggle_icon(ui, if shown { "eye" } else { "eye-off" }, shown, 18.0, label) {
        *x = Some(!shown);
        *y = Some(!shown);
        changed = true;
    }
    if (x.is_some() || y.is_some()) && ui.small_button("R").on_hover_text("Use the original R setting for both directions").clicked() {
        *x = None;
        *y = None;
        changed = true;
    }
    changed
}

fn position_controls(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui, panel_id: u64) {
    let Some(bounds) = app.session.doc().ok().and_then(|state| state.doc.bounds_of(&[vectorcraft_doc::NodeId(panel_id)], false)) else {
        return;
    };
    let origin = app.ruler_origin();
    let units = app.session.general_unit();
    let width = (ui.available_width() - 30.0).max(60.0) / 2.0;
    ui.horizontal(|ui| {
        ui.label("X:");
        if let Some(value) = crate::widgets::num_field_named(ui, "graph-transform-x", None, Some(bounds.x0 - origin.x), units, width)
            && (value - (bounds.x0 - origin.x)).abs() > f64::EPSILON
        {
            crate::panels::transform::run_transform(app, "object.setBounds", serde_json::json!({"x": value + origin.x}));
        }
        ui.label("Y:");
        if let Some(value) = crate::widgets::num_field_named(ui, "graph-transform-y", None, Some(bounds.y0 - origin.y), units, width)
            && (value - (bounds.y0 - origin.y)).abs() > f64::EPSILON
        {
            crate::panels::transform::run_transform(app, "object.setBounds", serde_json::json!({"y": value + origin.y}));
        }
    });
}

fn axis_component(ui: &mut egui::Ui, title: &str, id: &str, style: &mut GraphStyle, x_axis: bool) -> bool {
    let mut committed = false;
    egui::CollapsingHeader::new(title).id_salt(("graph-component", id)).default_open(true).show(ui, |ui| {
        if x_axis {
            ui.horizontal(|ui| {
                property_popover(ui, "x-axis-line", "Line", |ui| {
                    let mut changed = string_row(ui, "Axis line colour", &mut style.axis_x_line_colour, "#222222");
                    changed |= visibility_row(ui, "Tick marks", &mut style.axis_x_ticks_visible);
                    changed |= string_row(ui, "Tick colour", &mut style.axis_x_tick_colour, "#222222");
                    changed |= number_row(ui, "Tick width", &mut style.axis_x_tick_width_mm, 0.4, " mm", 0.01..=20.0);
                    changed |= number_row(ui, "Tick length", &mut style.axis_x_tick_length_mm, 1.5, " mm", 0.0..=100.0);
                    changed
                });
                committed |= compact_number(ui, "X line width", &mut style.axis_x_line_mm, 0.5, " mm", 0.1..=20.0);
                committed |= basic_visibility(ui, "X-axis line visibility", &mut style.axis_x_visible);
            });
            ui.horizontal(|ui| {
                property_popover(ui, "x-axis-text", "Text", |ui| advanced_text(ui, "X-axis tick labels", &mut style.tick_x_details, true, false));
                committed |= compact_number(ui, "Size", &mut style.tick_x_pt, 8.0, " pt", 0.1..=100.0);
                committed |= basic_visibility(ui, "X-axis tick labels visibility", &mut style.tick_x_visible);
            });
            committed |= face_row(ui, "X-axis tick labels face", &mut style.tick_x_details.face);
            ui.horizontal(|ui| {
                property_popover(ui, "x-axis-title", "Title", |ui| advanced_text(ui, "X-axis title", &mut style.axis_title_x_details, false, true));
                committed |= compact_number(ui, "Title size", &mut style.axis_title_x_pt, 10.0, " pt", 0.1..=100.0);
                committed |= basic_visibility(ui, "X-axis title visibility", &mut style.axis_title_x_visible);
            });
            committed |= face_row(ui, "X-axis title face", &mut style.axis_title_x_details.face);
        } else {
            ui.horizontal(|ui| {
                property_popover(ui, "y-axis-line", "Line", |ui| {
                    let mut changed = string_row(ui, "Axis line colour", &mut style.axis_y_line_colour, "#222222");
                    changed |= visibility_row(ui, "Tick marks", &mut style.axis_y_ticks_visible);
                    changed |= string_row(ui, "Tick colour", &mut style.axis_y_tick_colour, "#222222");
                    changed |= number_row(ui, "Tick width", &mut style.axis_y_tick_width_mm, 0.4, " mm", 0.01..=20.0);
                    changed |= number_row(ui, "Tick length", &mut style.axis_y_tick_length_mm, 1.5, " mm", 0.0..=100.0);
                    changed
                });
                committed |= compact_number(ui, "Y line width", &mut style.axis_y_line_mm, 0.5, " mm", 0.1..=20.0);
                committed |= basic_visibility(ui, "Y-axis line visibility", &mut style.axis_y_visible);
            });
            ui.horizontal(|ui| {
                property_popover(ui, "y-axis-text", "Text", |ui| advanced_text(ui, "Y-axis tick labels", &mut style.tick_y_details, true, false));
                committed |= compact_number(ui, "Size", &mut style.tick_y_pt, 8.0, " pt", 0.1..=100.0);
                committed |= basic_visibility(ui, "Y-axis tick labels visibility", &mut style.tick_y_visible);
            });
            committed |= face_row(ui, "Y-axis tick labels face", &mut style.tick_y_details.face);
            ui.horizontal(|ui| {
                property_popover(ui, "y-axis-title", "Title", |ui| advanced_text(ui, "Y-axis title", &mut style.axis_title_y_details, false, true));
                committed |= compact_number(ui, "Title size", &mut style.axis_title_y_pt, 10.0, " pt", 0.1..=100.0);
                committed |= basic_visibility(ui, "Y-axis title visibility", &mut style.axis_title_y_visible);
            });
            committed |= face_row(ui, "Y-axis title face", &mut style.axis_title_y_details.face);
        }
    });
    committed
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
    if !is_ggplot {
        panel_caption(ui, "This plot type supports size and refresh. Style overrides and R code export require a connected ggplot object.");
    }

    egui::CollapsingHeader::new("Selected Graph Settings").id_salt("graph-selected-settings").default_open(true).show(ui, |ui| {
        egui::CollapsingHeader::new("Transform").id_salt(("graph-component", "transform")).default_open(true).show(ui, |ui| {
            position_controls(app, ui, panel.panel_id);
            ui.strong("Figure Size");
            super::inspector::show_graph_plot_properties(app, ui);
        });
        let mut committed = false;
        if is_ggplot {
            egui::CollapsingHeader::new("Title").id_salt(("graph-component", "title")).default_open(true).show(ui, |ui| {
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "title", label: "Title", tick: false, title: true },
                    &mut draft.plot_title_pt,
                    &mut draft.plot_title_visible,
                    &mut draft.title_details,
                    12.0,
                );
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "subtitle", label: "Subtitle", tick: false, title: false },
                    &mut draft.subtitle_pt,
                    &mut draft.subtitle_visible,
                    &mut draft.subtitle_details,
                    10.0,
                );
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "caption", label: "Caption", tick: false, title: false },
                    &mut draft.caption_pt,
                    &mut draft.caption_visible,
                    &mut draft.caption_details,
                    8.0,
                );
                let before = draft.plot_title_position;
                ui.horizontal(|ui| {
                    let label = ui.label("Position");
                    egui::ComboBox::from_id_salt(("graph-title-position", panel.panel_id))
                        .selected_text(draft.plot_title_position.map_or("R default", TitlePosition::label))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.plot_title_position, None, "R default");
                            for position in TitlePosition::ALL {
                                ui.selectable_value(&mut draft.plot_title_position, Some(position), position.label());
                            }
                        })
                        .response
                        .labelled_by(label.id);
                });
                committed |= before != draft.plot_title_position;
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "facet-text", label: "Facet text", tick: false, title: false },
                    &mut draft.strip_text_pt,
                    &mut draft.strip_text_visible,
                    &mut draft.strip_text_details,
                    9.0,
                );
            });
            committed |= axis_component(ui, "X axis", "x-axis", &mut draft, true);
            committed |= axis_component(ui, "Y axis", "y-axis", &mut draft, false);
            egui::CollapsingHeader::new("Legend").id_salt(("graph-component", "legend")).default_open(true).show(ui, |ui| {
                let before = draft.legend_position;
                ui.horizontal(|ui| {
                    let label = ui.label("Position");
                    egui::ComboBox::from_id_salt(("graph-legend-position", panel.panel_id))
                        .selected_text(draft.legend_position.map_or("R default", LegendPosition::label))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.legend_position, None, "R default");
                            for position in LegendPosition::ALL {
                                ui.selectable_value(&mut draft.legend_position, Some(position), position.label());
                            }
                        })
                        .response
                        .labelled_by(label.id);
                });
                committed |= before != draft.legend_position;
                if draft.legend_position == Some(LegendPosition::Inside) {
                    committed |= number_row(ui, "Inside X", &mut draft.inside_x, 0.95, "", 0.0..=1.0);
                    committed |= number_row(ui, "Inside Y", &mut draft.inside_y, 0.95, "", 0.0..=1.0);
                }
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "legend-title", label: "Title", tick: false, title: true },
                    &mut draft.legend_title_pt,
                    &mut draft.legend_title_visible,
                    &mut draft.legend_title_details,
                    10.0,
                );
                committed |= basic_text_row(
                    ui,
                    TextRowSpec { id: "legend-text", label: "Text", tick: false, title: false },
                    &mut draft.legend_text_pt,
                    &mut draft.legend_text_visible,
                    &mut draft.legend_text_details,
                    8.0,
                );
                ui.horizontal(|ui| {
                    property_popover(ui, "legend-key", "Key", |ui| {
                        let mut changed = number_row(ui, "Key width", &mut draft.legend_key_width_mm, 5.0, " mm", 0.0..=100.0);
                        changed |= number_row(ui, "Key height", &mut draft.legend_key_height_mm, 5.0, " mm", 0.0..=100.0);
                        changed |= string_row(ui, "Key fill", &mut draft.legend_key_background, "#ffffff");
                        changed |= string_row(ui, "Legend fill", &mut draft.legend_background, "#ffffff");
                        changed |= number_row(ui, "Spacing X", &mut draft.legend_spacing_x_mm, 2.0, " mm", 0.0..=100.0);
                        changed |= number_row(ui, "Spacing Y", &mut draft.legend_spacing_y_mm, 2.0, " mm", 0.0..=100.0);
                        changed |= margins_row(ui, "Legend margins", &mut draft.legend_margin_mm);
                        changed
                    });
                    committed |= compact_number(ui, "Key size", &mut draft.legend_key_mm, 5.0, " mm", 0.1..=100.0);
                });
                let before = draft.legend_direction;
                ui.horizontal(|ui| {
                    ui.label("Direction");
                    egui::ComboBox::from_id_salt(("graph-legend-direction", panel.panel_id))
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
            });
            egui::CollapsingHeader::new("Plot").id_salt(("graph-component", "plot")).default_open(true).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (popover, label_response) = property_popover_with_response(ui, "major-grid", "Major grid", |ui| {
                        let mut changed = string_row(ui, "Grid colour", &mut draft.major_grid_colour, "grey85");
                        changed |= visibility_row(ui, "X major grid", &mut draft.major_grid_x_visible);
                        changed |= visibility_row(ui, "Y major grid", &mut draft.major_grid_y_visible);
                        changed
                    });
                    committed |= popover.unwrap_or(false);
                    committed |= compact_number_control(ui, &mut draft.major_grid_width_mm, 0.2, " mm", 0.01..=20.0, Some(label_response.id));
                    committed |= basic_visibility(ui, "Major grid visibility", &mut draft.major_grid_visible);
                });
                ui.horizontal(|ui| {
                    let (popover, label_response) = property_popover_with_response(ui, "minor-grid", "Minor grid", |ui| {
                        let mut changed = string_row(ui, "Grid colour", &mut draft.minor_grid_colour, "grey90");
                        changed |= visibility_row(ui, "X minor grid", &mut draft.minor_grid_x_visible);
                        changed |= visibility_row(ui, "Y minor grid", &mut draft.minor_grid_y_visible);
                        changed
                    });
                    committed |= popover.unwrap_or(false);
                    committed |= compact_number_control(ui, &mut draft.minor_grid_width_mm, 0.1, " mm", 0.01..=20.0, Some(label_response.id));
                    committed |= basic_grid_visibility(ui, "Minor grid visibility", &mut draft.minor_grid_x_visible, &mut draft.minor_grid_y_visible);
                });
                property_popover(ui, "plot-options", "Plot options", |ui| {
                    let mut changed = string_row(ui, "Panel fill", &mut draft.panel_background, "#ffffff");
                    changed |= string_row(ui, "Plot fill", &mut draft.plot_background, "#ffffff");
                    changed |= visibility_row(ui, "Panel border", &mut draft.panel_border_visible);
                    changed |= string_row(ui, "Border colour", &mut draft.panel_border_colour, "#222222");
                    changed |= number_row(ui, "Border width", &mut draft.panel_border_width_mm, 0.4, " mm", 0.01..=20.0);
                    changed |= string_row(ui, "Facet strip fill", &mut draft.strip_background, "#eeeeee");
                    changed |= margins_row(ui, "Plot margins", &mut draft.plot_margin_mm);
                    changed |= number_row(ui, "Panel spacing", &mut draft.panel_spacing_mm, 2.0, " mm", 0.0..=100.0);
                    changed
                });
            });
        }
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
            if let Err(error) = request_graph_apply(&mut app.scientific, &panel, draft.clone(), width_mm, height_mm) {
                app.scientific.r_status = format!("Could not apply Graph settings: {error}");
            }
        }
        let copy_code = is_ggplot.then(|| {
            ui.button("Copy as R Code").on_hover_text(
                "Copies supported ggplot2 style overrides; excludes artboard position and size, and vector artwork. The R script is unchanged.",
            )
        });
        if copy_code.is_some_and(|response| response.clicked()) {
            match super::graph_r_code::generate(&draft, &panel.r_variable) {
                Ok(result) => match result.code {
                    Some(code) => {
                        app.clipboard_out = Some(code);
                        app.status("Copied R code to clipboard");
                        if !result.unsupported.is_empty() {
                            app.status(format!("Copied supported R styles; not represented: {}", result.unsupported.join(", ")));
                        }
                    }
                    None if result.unsupported.is_empty() => app.status("No R-compatible graph changes to copy."),
                    None => app.status(format!("No R-compatible graph changes to copy; not represented: {}", result.unsupported.join(", "))),
                },
                Err(error) => app.status(format!("Could not generate R code: {error}")),
            }
        }
        if app.scientific.r_status.starts_with("Could not") || app.scientific.r_status.contains("failed") {
            ui.colored_label(crate::theme::Tokens::get(ui.ctx()).warning, &app.scientific.r_status);
        }
    });
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
