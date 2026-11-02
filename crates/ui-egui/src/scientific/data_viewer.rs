use super::{Workspace, request_data_page};
use crate::theme::Tokens;
use egui::{RichText, Ui};

pub(crate) fn show_data_viewer_tab(workspace: &mut Workspace, ui: &mut Ui, height: f32, name: &str) {
    let Some(tab_idx) = workspace.viewer_tabs.iter().position(|t| t.name == name) else {
        return;
    };

    let t = Tokens::get(ui.ctx());
    let mut action_close = false;
    let mut action_refresh = false;
    let mut action_next = false;
    let mut action_prev = false;

    egui::Frame::new().fill(t.input).stroke(egui::Stroke::new(1.0, t.input_border)).show(ui, |ui| {
        ui.set_min_height(height);
        ui.set_max_height(height);

        // 1. Toolbar / Status Strip
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Object name
            ui.strong(RichText::new(name).size(t.font_size).color(t.text_strong));

            // Record count & summary
            if let Some(page) = &workspace.viewer_tabs[tab_idx].page {
                let total = page.total_rows;
                let cols = page.columns.len();
                let start = workspace.viewer_tabs[tab_idx].page_start;
                let end = (start + page.rows.len()).min(total);
                let summary = if total == 0 {
                    format!("(0 entries, {cols} columns)")
                } else {
                    format!("(Showing {}-{} of {total} entries, {cols} columns)", start + 1, end)
                };
                ui.label(RichText::new(summary).size(t.font_size - 1.0).color(t.text_dim));
            }

            if workspace.viewer_tabs[tab_idx].loading {
                ui.spinner();
                ui.label(RichText::new("Loading...").size(t.font_size - 1.0).color(t.text_dim));
            }

            // Right-aligned controls
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕ Close").on_hover_text("Close Data Viewer tab").clicked() {
                    action_close = true;
                }
                if ui.small_button("⟳ Refresh").on_hover_text("Reload data from R").clicked() {
                    action_refresh = true;
                }

                let page_start = workspace.viewer_tabs[tab_idx].page_start;
                let (total_rows, rows_len) = workspace.viewer_tabs[tab_idx].page.as_ref().map(|p| (p.total_rows, p.rows.len())).unwrap_or((0, 0));
                let can_next = page_start + rows_len < total_rows && !workspace.viewer_tabs[tab_idx].loading;
                let can_prev = page_start > 0 && !workspace.viewer_tabs[tab_idx].loading;

                if ui.add_enabled(can_next, egui::Button::new("Next ⮞").small()).clicked() {
                    action_next = true;
                }
                if ui.add_enabled(can_prev, egui::Button::new("⮜ Prev").small()).clicked() {
                    action_prev = true;
                }

                // Filter search input
                let filter_rect =
                    ui.add(egui::TextEdit::singleline(&mut workspace.viewer_tabs[tab_idx].filter).desired_width(120.0).hint_text("Filter rows..."));
                if !workspace.viewer_tabs[tab_idx].filter.is_empty() && ui.small_button("✕").on_hover_text("Clear filter").clicked() {
                    workspace.viewer_tabs[tab_idx].filter.clear();
                }
                let _ = filter_rect;
                ui.label(RichText::new("Filter:").size(t.font_size - 1.0).color(t.text_dim));
            });
        });

        ui.separator();

        // 2. Action dispatching
        if action_close {
            workspace.close_data_viewer(name);
            return;
        }
        if action_refresh {
            let start = workspace.viewer_tabs[tab_idx].page_start;
            request_data_page(workspace, name.to_owned(), start);
            return;
        }
        if action_next {
            workspace.viewer_tabs[tab_idx].page_start = workspace.viewer_tabs[tab_idx].page_start.saturating_add(50);
            let start = workspace.viewer_tabs[tab_idx].page_start;
            request_data_page(workspace, name.to_owned(), start);
            return;
        }
        if action_prev {
            workspace.viewer_tabs[tab_idx].page_start = workspace.viewer_tabs[tab_idx].page_start.saturating_sub(50);
            let start = workspace.viewer_tabs[tab_idx].page_start;
            request_data_page(workspace, name.to_owned(), start);
            return;
        }

        // 3. Tab Body
        if let Some(err) = workspace.viewer_tabs[tab_idx].error.clone() {
            let mut action_retry = false;
            let mut action_close_tab = false;
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("⚠ Unable to display data").size(15.0).strong().color(t.accent));
                ui.add_space(8.0);
                ui.label(RichText::new(&err).color(t.text_dim));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("⟳ Retry").clicked() {
                        action_retry = true;
                    }
                    if ui.button("✕ Close Tab").clicked() {
                        action_close_tab = true;
                    }
                });
            });
            if action_retry {
                let start = workspace.viewer_tabs[tab_idx].page_start;
                request_data_page(workspace, name.to_owned(), start);
                return;
            }
            if action_close_tab {
                workspace.close_data_viewer(name);
                return;
            }
        } else if workspace.viewer_tabs[tab_idx].page.is_none() && workspace.viewer_tabs[tab_idx].loading {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.spinner();
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Loading '{name}' from R session...")).color(t.text_dim));
            });
        } else if let Some(page) = workspace.viewer_tabs[tab_idx].page.clone() {
            let mut toggle_sort = None;
            egui::ScrollArea::both().id_salt(("data_viewer_scroll", name)).auto_shrink([false, false]).show(ui, |ui| {
                egui::Grid::new(("data_viewer_grid", name)).striped(true).spacing(egui::vec2(14.0, 4.0)).show(ui, |ui| {
                    // Header row
                    ui.label(RichText::new("#").strong().color(t.text_dim));
                    for (col_idx, (col_name, col_type)) in page.columns.iter().enumerate() {
                        let sort_marker = if workspace.viewer_tabs[tab_idx].sort_column == Some(col_idx) {
                            if workspace.viewer_tabs[tab_idx].sort_ascending { " ▲" } else { " ▼" }
                        } else {
                            ""
                        };
                        let header_text = format!("{col_name}{sort_marker}\n<{col_type}>");
                        let btn = egui::Button::new(RichText::new(header_text).size(t.font_size - 1.0).strong().color(t.text_strong)).frame(false);
                        if ui.add(btn).on_hover_text(format!("Click to sort by {col_name}")).clicked() {
                            toggle_sort = Some(col_idx);
                        }
                    }
                    ui.end_row();

                    // Filter rows
                    let filter = workspace.viewer_tabs[tab_idx].filter.trim().to_lowercase();
                    let mut visible_rows: Vec<(usize, &Vec<String>)> = page
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(_, row)| filter.is_empty() || row.iter().any(|cell| cell.to_lowercase().contains(&filter)))
                        .collect();

                    // Sort rows
                    if let Some(sort_col) = workspace.viewer_tabs[tab_idx].sort_column {
                        let ascending = workspace.viewer_tabs[tab_idx].sort_ascending;
                        visible_rows.sort_by(|(_, a), (_, b)| {
                            let val_a = a.get(sort_col).map(String::as_str).unwrap_or("");
                            let val_b = b.get(sort_col).map(String::as_str).unwrap_or("");
                            let ord = match (val_a.parse::<f64>(), val_b.parse::<f64>()) {
                                (Ok(na), Ok(nb)) => na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal),
                                _ => val_a.cmp(val_b),
                            };
                            if ascending { ord } else { ord.reverse() }
                        });
                    }

                    if visible_rows.is_empty() {
                        ui.label("");
                        ui.label(RichText::new("No matching rows found").italics().color(t.text_dim));
                        ui.end_row();
                    } else {
                        let page_start = workspace.viewer_tabs[tab_idx].page_start;
                        for (orig_offset, row) in visible_rows {
                            let row_num = page_start + orig_offset + 1;
                            ui.label(RichText::new(row_num.to_string()).color(t.text_dim).monospace());
                            for cell in row {
                                if cell == "NA" {
                                    ui.label(RichText::new("NA").italics().color(t.text_dim));
                                } else {
                                    ui.label(RichText::new(cell).color(t.text));
                                }
                            }
                            ui.end_row();
                        }
                    }
                });
            });

            if let Some(col_idx) = toggle_sort {
                if workspace.viewer_tabs[tab_idx].sort_column == Some(col_idx) {
                    workspace.viewer_tabs[tab_idx].sort_ascending = !workspace.viewer_tabs[tab_idx].sort_ascending;
                } else {
                    workspace.viewer_tabs[tab_idx].sort_column = Some(col_idx);
                    workspace.viewer_tabs[tab_idx].sort_ascending = true;
                }
            }
        } else {
            let mut action_retry = false;
            let mut action_close_tab = false;
            ui.vertical_centered(|ui| {
                ui.add_space(32.0);
                ui.label(RichText::new("ℹ R Object Unavailable").size(15.0).strong().color(t.text_dim));
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Object '{name}' is not currently available in the active R session.\nImport workspace data or run R code to view."
                    ))
                    .color(t.text_dim),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("⟳ Load / Retry").clicked() {
                        action_retry = true;
                    }
                    if ui.button("✕ Close Tab").clicked() {
                        action_close_tab = true;
                    }
                });
            });
            if action_retry {
                let start = workspace.viewer_tabs[tab_idx].page_start;
                request_data_page(workspace, name.to_owned(), start);
                return;
            }
            if action_close_tab {
                workspace.close_data_viewer(name);
                return;
            }
        }
    });
}
