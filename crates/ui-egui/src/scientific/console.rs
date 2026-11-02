use super::{
    R_CODE_LINE_GAP, R_CODE_PADDING, R_CONSOLE_INPUT_SIZE, RIndicator, Workspace, append_console_output, history_next, history_previous, r_indicator,
    source_editor::code_style, submit_console,
};

pub(super) fn show_console(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.scope(|ui| {
        code_style(ui);
        show_console_contents(app, ui);
    });
}

fn show_console_contents(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("Clear console").on_hover_text("Clear transcript (Ctrl+L); keep R objects and history").clicked() {
            clear_console_transcript(&mut app.scientific);
        }
        show_console_status(&app.scientific, ui);
    });
    ui.add_space(2.0);
    let t = crate::theme::Tokens::get(ui.ctx());
    let lines: Vec<_> = app.scientific.console_output.split_terminator('\n').collect();
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace).max(1.0);
    let stick_to_bottom = app.scientific.console_scroll_at_bottom;
    if app.scientific.console_input.contains('\r') {
        app.scientific.console_input = app.scientific.console_input.replace("\r\n", "\n").replace('\r', "\n");
    }
    let console_input_before = app.scientific.console_input.clone();
    let input_was_focused = app.scientific.console_input_id.is_some_and(|id| ui.memory(|m| m.has_focus(id)));
    // Virtualize output only. Always create the real live input, even when it is
    // clipped during scrollback, so it retains its identity, draft and focus.
    // Both belong to one continuous scrolling transcript.
    let output_frame = egui::Frame::new()
        .fill(t.input)
        .stroke(egui::Stroke::new(1.0, t.input_border))
        .corner_radius(t.radius)
        .inner_margin(egui::Margin::symmetric(4, 4));
    let transcript = output_frame
        .show(ui, |ui| {
            let available = ui.available_height().max(0.0);
            let input_height = (available * 0.3).max(row_height + 16.0);
            let pitch = row_height + R_CODE_LINE_GAP;
            egui::ScrollArea::both()
                .id_salt("r-studio-console-output")
                .auto_shrink([false, false])
                .min_scrolled_height(available)
                .max_height(available)
                .stick_to_bottom(stick_to_bottom)
                .show_viewport(ui, |ui, viewport| {
                    let origin = ui.cursor().min;
                    // Long visible output can expand the content's width. Keep the live editor
                    // at the viewport width so revealing it never aligns to a long line's tail.
                    let input_width = ui.available_width();
                    let output_height = pitch * lines.len() as f32;
                    let first = ((viewport.min.y / pitch).floor().max(0.0) as usize).min(lines.len());
                    let last = ((viewport.max.y / pitch).ceil().max(0.0) as usize).saturating_add(1).min(lines.len());
                    let visible = egui::Rect::from_min_max(
                        egui::pos2(origin.x, origin.y + first as f32 * pitch),
                        egui::pos2(ui.max_rect().right(), origin.y + last as f32 * pitch),
                    );
                    ui.scope_builder(egui::UiBuilder::new().id_salt("console-visible-output").max_rect(visible), |ui| {
                        ui.skip_ahead_auto_ids(first);
                        for row in first..last {
                            if let Some(line) = lines.get(row).copied() {
                                let color = console_line_color(line, ui.visuals());
                                ui.add(
                                    egui::Label::new(egui::RichText::new(line).monospace().color(color))
                                        .wrap_mode(egui::TextWrapMode::Extend)
                                        .selectable(true),
                                );
                            }
                        }
                    });
                    let input_rect = egui::Rect::from_min_size(egui::pos2(origin.x, origin.y + output_height), egui::vec2(input_width, input_height));
                    if !lines.is_empty() {
                        ui.painter().hline(input_rect.x_range(), origin.y + output_height, egui::Stroke::new(1.0, t.divider));
                    }
                    let response = ui
                        .scope_builder(egui::UiBuilder::new().id_salt("console-live-input").max_rect(input_rect), |ui| {
                            let input_bg = if t.dark { t.panel.gamma_multiply(0.2) } else { t.panel_darker.gamma_multiply(0.15) };
                            let input_border_stroke = if input_was_focused {
                                egui::Stroke::new(1.0, t.accent.gamma_multiply(0.5))
                            } else {
                                egui::Stroke::new(0.5, t.input_border.gamma_multiply(0.4))
                            };
                            egui::Frame::new()
                                .fill(input_bg)
                                .stroke(input_border_stroke)
                                .corner_radius(t.radius)
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("r-console-input-scroll")
                                        .max_height(input_height)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            ui.horizontal_top(|ui| {
                                                ui.spacing_mut().item_spacing.x = 0.0;
                                                let id = ui.make_persistent_id("r-studio-console-input");
                                                app.scientific.console_input_id = Some(id);
                                                if app.scientific.pending_console_focus {
                                                    ui.memory_mut(|m| m.request_focus(id));
                                                }
                                                let prompt = if app.scientific.console_continuation { "+" } else { ">" };
                                                let input_font = egui::FontId::monospace(R_CONSOLE_INPUT_SIZE);
                                                if ui
                                                    .add(
                                                        egui::Label::new(
                                                            egui::RichText::new(prompt).font(input_font.clone()).strong().color(t.accent),
                                                        )
                                                        .sense(egui::Sense::click()),
                                                    )
                                                    .on_hover_text(if app.scientific.console_continuation {
                                                        "Continue the incomplete R expression"
                                                    } else {
                                                        "R Console input"
                                                    })
                                                    .clicked()
                                                {
                                                    ui.memory_mut(|m| m.request_focus(id));
                                                }
                                                let response = ui.add(
                                                    egui::TextEdit::multiline(&mut app.scientific.console_input)
                                                        .id(id)
                                                        .event_filter(egui::EventFilter {
                                                            escape: true,
                                                            vertical_arrows: true,
                                                            horizontal_arrows: true,
                                                            ..Default::default()
                                                        })
                                                        .font(input_font.clone())
                                                        .code_editor()
                                                        .frame(egui::Frame::NONE)
                                                        .margin(egui::Margin::symmetric(R_CODE_PADDING, 0))
                                                        .desired_width(ui.available_width())
                                                        .desired_rows(1)
                                                        .hint_text(egui::RichText::new("Enter R code").font(input_font.clone()).color(t.text_dim)),
                                                );
                                                if app.scientific.console_input.contains('\r') {
                                                    app.scientific.console_input =
                                                        app.scientific.console_input.replace("\r\n", "\n").replace('\r', "\n");
                                                }
                                                response
                                            })
                                            .inner
                                        })
                                        .inner
                                })
                                .inner
                        })
                        .inner;
                    if response.changed() || app.scientific.pending_console_focus {
                        // Reveal the prompt's left edge, rather than right-aligning the entire
                        // editor response (whose layout may include rounding or frame margins).
                        ui.scroll_to_rect(egui::Rect::from_min_size(input_rect.min, egui::vec2(1.0, input_height)), Some(egui::Align::Max));
                    }
                    response
                })
        })
        .inner;
    let max_offset = (transcript.content_size.y - transcript.inner_rect.height()).max(0.0);
    app.scientific.console_scroll_at_bottom = max_offset - transcript.state.clamped_offset().y <= row_height;
    let input_response = transcript.inner;
    {
        let response = input_response;
        if app.scientific.pending_console_focus {
            response.request_focus();
            app.scientific.pending_console_focus = false;
        }
        // Escape can make TextEdit surrender focus before its response is returned.
        // Still handle that keystroke for the Console that owned it at frame start.
        let input_focused = response.has_focus() || input_was_focused;
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let submit = enter_pressed && input_focused;
        let (history_up, history_down, clear, cancel) = ui.input(|input| {
            if !input_focused {
                return (false, false, false, false);
            }
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
                input.modifiers.command && input.key_pressed(egui::Key::L),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if history_up || history_down {
            app.scientific.console_scroll_at_bottom = true;
            app.scientific.pending_console_focus = true;
            if history_up {
                history_previous(&mut app.scientific);
            } else {
                history_next(&mut app.scientific);
            }
            let cursor_end = app.scientific.console_input.chars().count();
            let mut state = egui::TextEdit::load_state(ui.ctx(), response.id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_end))));
            state.store(ui.ctx(), response.id);
            ui.ctx().request_repaint();
        }
        if clear {
            clear_console_transcript(&mut app.scientific);
        }
        if cancel {
            response.request_focus();
            app.scientific.pending_console_focus = true;
            app.scientific.console_input.clear();
            if !app.scientific.console_pending_lines.is_empty() {
                app.scientific.console_pending_lines.clear();
                app.scientific.console_continuation = false;
                append_console_output(&mut app.scientific, "Info: R input cancelled\n");
                app.scientific.r_status = "R input cancelled".into();
            }
        }
        if submit {
            // A multiline TextEdit inserts a newline for Enter. Remove exactly that keystroke
            // from the submitted buffer while preserving multiline paste and cursor position.
            remove_console_enter_newline(&mut app.scientific.console_input, &console_input_before);
            app.scientific.pending_console_focus = true;
            if !app.scientific.running {
                submit_console(&mut app.scientific);
                app.scientific.console_scroll_at_bottom = true;
            }
        }
    }
}

pub(super) fn remove_console_enter_newline(input: &mut String, before_enter: &str) {
    if input.len() != before_enter.len() + 1 {
        return;
    }
    let prefix_len = input.bytes().zip(before_enter.bytes()).take_while(|(after, before)| after == before).count();
    let (Some(after_tail), Some(before_tail)) = (input.get(prefix_len..), before_enter.get(prefix_len..)) else { return };
    if after_tail.strip_prefix('\n') == Some(before_tail) {
        input.remove(prefix_len);
    }
}

pub(super) fn clear_console_transcript(workspace: &mut Workspace) {
    workspace.console_output.clear();
    workspace.console_error = false;
    workspace.console_scroll_at_bottom = true;
    workspace.pending_console_focus = true;
}

fn console_line_color(line: &str, visuals: &egui::Visuals) -> egui::Color32 {
    let trimmed = line.trim_start();
    if trimmed.starts_with("Error:") {
        visuals.error_fg_color
    } else if trimmed.starts_with("Warning:") {
        visuals.warn_fg_color
    } else if trimmed.starts_with("Info:") || trimmed.starts_with("R input cancelled") {
        visuals.weak_text_color()
    } else if trimmed.starts_with("> ") || trimmed.starts_with("+ ") {
        visuals.strong_text_color()
    } else {
        visuals.text_color()
    }
}

fn show_console_status(workspace: &Workspace, ui: &mut egui::Ui) {
    let indicator = r_indicator(workspace);
    let (label, color) = match indicator {
        RIndicator::Running => ("Running", indicator.color()),
        RIndicator::Checking if workspace.checking_r => ("Checking R", indicator.color()),
        RIndicator::Checking => ("Available", indicator.color()),
        RIndicator::Error => ("Error", indicator.color()),
        RIndicator::Ready if workspace.console_continuation => ("Waiting for input", RIndicator::Checking.color()),
        RIndicator::Ready => ("Ready", indicator.color()),
        RIndicator::Disconnected => ("Disconnected", indicator.color()),
    };
    ui.colored_label(color, format!("● {label}")).on_hover_text(
        "Console output is selectable and copyable. R code cannot be safely interrupted in this session; Stop R ends the session and loses its in-memory objects.",
    );
}
