use super::{
    ActiveCompletion, QueuedRCommand, R_CODE_LINE_GAP, R_CODE_PADDING, R_CODE_SIZE, RMessage, ScriptDocument, ScriptEncoding, Workspace,
    decode_script_as, install_script_document, save_script_for,
};

pub(super) fn code_style(ui: &mut egui::Ui) {
    ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, egui::FontId::monospace(R_CODE_SIZE));
    ui.spacing_mut().item_spacing.y = R_CODE_LINE_GAP;
}

pub(super) fn show_source_editor(workspace: &mut Workspace, ui: &mut egui::Ui, height: f32) {
    let active_id = workspace.active_script_id;
    let mut popup_anchor = None;
    let mut editor_widget_id = None;
    let response = ui
        .scope(|ui| {
            code_style(ui);
            let t = crate::theme::Tokens::get(ui.ctx());
            egui::Frame::new()
                .fill(t.input)
                .stroke(egui::Stroke::new(1.0, t.input_border))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("scientific-r-script-scroll", active_id))
                        .min_scrolled_height(height)
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let pending_focus = workspace.pending_focus_script_id == Some(active_id);
                            let Some(script_idx) = workspace.scripts.iter().position(|s| s.id == active_id) else {
                                return ui.label("No active R script");
                            };
                            if workspace.scripts[script_idx].text.contains('\r') {
                                workspace.scripts[script_idx].text = workspace.scripts[script_idx].text.replace("\r\n", "\n").replace('\r', "\n");
                            }
                            let editor_id = ui.make_persistent_id("scientific-r-script-editor").with(active_id);
                            editor_widget_id = Some(editor_id);
                            if pending_focus {
                                ui.memory_mut(|m| m.request_focus(editor_id));
                            }
                            let cursor_before = egui::TextEdit::load_state(ui.ctx(), editor_id).and_then(|s| s.cursor.char_range());
                            let pending_reveal_index = workspace.scripts[script_idx].pending_reveal_index;
                            let navigating = pending_reveal_index.is_some();
                            if let Some(index) = workspace.scripts[script_idx].pending_cursor_index.take() {
                                let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(
                                    index.min(workspace.scripts[script_idx].text.chars().count()),
                                ))));
                                state.store(ui.ctx(), editor_id);
                            }

                            let has_focus = ui.memory(|m| m.has_focus(editor_id));
                            let is_completion_open =
                                workspace.completion.as_ref().is_some_and(|c| c.script_id == active_id && !c.candidates.is_empty());

                            if is_completion_open {
                                let arrow_down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                                let arrow_up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                                let escape = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                                let accept = ui.input_mut(|i| {
                                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                });

                                if arrow_down {
                                    if let Some(active) = workspace.completion.as_mut() {
                                        active.selected = (active.selected + 1) % active.candidates.len().max(1);
                                        active.scroll_to_selected = true;
                                    }
                                } else if arrow_up {
                                    if let Some(active) = workspace.completion.as_mut() {
                                        if active.selected == 0 {
                                            active.selected = active.candidates.len().saturating_sub(1);
                                        } else {
                                            active.selected -= 1;
                                        }
                                        active.scroll_to_selected = true;
                                    }
                                } else if escape {
                                    workspace.completion = None;
                                    ui.memory_mut(|m| m.request_focus(editor_id));
                                } else if accept && let Some(active) = workspace.completion.take() {
                                    if let Some(candidate) = active.candidates.get(active.selected).cloned() {
                                        let new_cursor_idx = {
                                            let text = &mut workspace.scripts[script_idx].text;
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let before_range = state.cursor.char_range().unwrap_or_else(|| {
                                                egui::text::CCursorRange::one(egui::text::CCursor::new(active.context.replace_end_char))
                                            });
                                            let mut undoer = state.undoer();
                                            undoer.add_undo(&(before_range, text.clone()));
                                            let idx = crate::r_editor::apply_completion(text, &active.context, &candidate);
                                            let after_range = egui::text::CCursorRange::one(egui::text::CCursor::new(idx));
                                            undoer.add_undo(&(after_range, text.clone()));
                                            state.set_undoer(undoer);
                                            state.cursor.set_char_range(Some(after_range));
                                            state.store(ui.ctx(), editor_id);
                                            idx
                                        };

                                        if candidate.kind == crate::r_editor::CompletionCandidateKind::Directory {
                                            let script_text = workspace.scripts[script_idx].text.clone();
                                            if let Some(new_ctx) = crate::r_editor::completion_context_at(&script_text, new_cursor_idx) {
                                                let candidates = build_completion_candidates(workspace, active_id, &script_text, &new_ctx);
                                                if !candidates.is_empty() {
                                                    workspace.completion = Some(ActiveCompletion {
                                                        script_id: active_id,
                                                        source_revision: script_text,
                                                        context: new_ctx,
                                                        candidates,
                                                        selected: 0,
                                                        scroll_to_selected: true,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if has_focus && !is_completion_open {
                                let delim_typed = ui.input_mut(|input| {
                                    let mut found = None;
                                    let mut found_idx = None;
                                    for (idx, event) in input.events.iter().enumerate() {
                                        if let egui::Event::Text(text) = event
                                            && text.len() == 1
                                        {
                                            let c = text.chars().next().unwrap_or(' ');
                                            if crate::r_editor::is_open_delimiter(c) || crate::r_editor::is_close_delimiter(c) {
                                                found = Some(c);
                                                found_idx = Some(idx);
                                                break;
                                            }
                                        }
                                    }
                                    if let Some(idx) = found_idx {
                                        input.events.remove(idx);
                                        input.events.retain(|e| {
                                            !matches!(
                                                e,
                                                egui::Event::Key { key: egui::Key::Quote | egui::Key::OpenBracket | egui::Key::CloseBracket, .. }
                                            )
                                        });
                                    }
                                    found
                                });

                                let is_between_empty_pair = cursor_before.is_some_and(|r| {
                                    let idx = usize::from(r.primary.index);
                                    r.primary.index == r.secondary.index
                                        && crate::r_editor::is_cursor_between_empty_pair(&workspace.scripts[script_idx].text, idx)
                                });
                                let empty_pair_backspace = if is_between_empty_pair {
                                    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
                                } else {
                                    false
                                };

                                let enter_pressed = ui.input_mut(|i| {
                                    let pressed = i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                                        || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter);
                                    if pressed {
                                        i.events.retain(|e| if let egui::Event::Text(t) = e { t != "\n" && t != "\r" && t != "\r\n" } else { true });
                                    }
                                    pressed
                                });

                                if let Some(d) = delim_typed {
                                    let sorted = cursor_before.map(|r| r.as_sorted_char_range());
                                    if let Some(range) = sorted
                                        && range.start.0 != range.end.0
                                    {
                                        if let Some((open, close)) = crate::r_editor::delimiter_pair(d) {
                                            let text = &mut workspace.scripts[script_idx].text;
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let before = state.cursor.char_range().unwrap_or_else(|| {
                                                egui::text::CCursorRange::two(
                                                    egui::text::CCursor::new(range.start.0),
                                                    egui::text::CCursor::new(range.end.0),
                                                )
                                            });
                                            let mut undoer = state.undoer();
                                            undoer.add_undo(&(before, text.clone()));
                                            let (new_start, new_end) =
                                                crate::r_editor::wrap_selection_with_pair(text, range.start.0, range.end.0, open, close);
                                            let after =
                                                egui::text::CCursorRange::two(egui::text::CCursor::new(new_start), egui::text::CCursor::new(new_end));
                                            undoer.add_undo(&(after, text.clone()));
                                            state.set_undoer(undoer);
                                            state.cursor.set_char_range(Some(after));
                                            state.store(ui.ctx(), editor_id);
                                        }
                                    } else {
                                        let cursor_idx = cursor_before
                                            .map(|r| usize::from(r.primary.index))
                                            .unwrap_or_else(|| workspace.scripts[script_idx].text.chars().count());
                                        let text = &mut workspace.scripts[script_idx].text;
                                        if crate::r_editor::is_close_delimiter(d)
                                            && crate::r_editor::should_skip_closing_delimiter(text, cursor_idx, d)
                                        {
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let after = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx + 1));
                                            state.cursor.set_char_range(Some(after));
                                            state.store(ui.ctx(), editor_id);
                                        } else if matches!(d, '}' | ')' | ']') && {
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let before = state
                                                .cursor
                                                .char_range()
                                                .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx)));
                                            if let Some(new_cursor) = crate::r_editor::dedent_closing_bracket(text, cursor_idx, d) {
                                                let mut undoer = state.undoer();
                                                undoer.add_undo(&(before, text.clone()));
                                                let after = egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor));
                                                undoer.add_undo(&(after, text.clone()));
                                                state.set_undoer(undoer);
                                                state.cursor.set_char_range(Some(after));
                                                state.store(ui.ctx(), editor_id);
                                                true
                                            } else {
                                                false
                                            }
                                        } {
                                            // Dedented successfully
                                        } else if crate::r_editor::is_open_delimiter(d)
                                            && crate::r_editor::can_auto_pair_delimiter(text, cursor_idx, d)
                                        {
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let before = state
                                                .cursor
                                                .char_range()
                                                .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx)));
                                            let mut undoer = state.undoer();
                                            undoer.add_undo(&(before, text.clone()));
                                            let new_cursor = crate::r_editor::auto_pair_delimiter(text, cursor_idx, d);
                                            let after = egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor));
                                            undoer.add_undo(&(after, text.clone()));
                                            state.set_undoer(undoer);
                                            state.cursor.set_char_range(Some(after));
                                            state.store(ui.ctx(), editor_id);
                                        } else {
                                            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                            let before = state
                                                .cursor
                                                .char_range()
                                                .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx)));
                                            let mut undoer = state.undoer();
                                            undoer.add_undo(&(before, text.clone()));
                                            let byte_idx = crate::r_editor::char_to_byte(text, cursor_idx);
                                            text.insert(byte_idx, d);
                                            let after = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx + 1));
                                            undoer.add_undo(&(after, text.clone()));
                                            state.set_undoer(undoer);
                                            state.cursor.set_char_range(Some(after));
                                            state.store(ui.ctx(), editor_id);
                                        }
                                    }
                                } else if empty_pair_backspace {
                                    let cursor_idx = cursor_before.map(|r| usize::from(r.primary.index)).unwrap_or(0);
                                    let text = &mut workspace.scripts[script_idx].text;
                                    let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                    let before = state
                                        .cursor
                                        .char_range()
                                        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx)));
                                    let mut undoer = state.undoer();
                                    undoer.add_undo(&(before, text.clone()));
                                    let new_cursor = crate::r_editor::delete_empty_pair(text, cursor_idx);
                                    let after = egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor));
                                    undoer.add_undo(&(after, text.clone()));
                                    state.set_undoer(undoer);
                                    state.cursor.set_char_range(Some(after));
                                    state.store(ui.ctx(), editor_id);
                                } else if enter_pressed {
                                    let sorted = cursor_before.map(|r| r.as_sorted_char_range());
                                    let (cursor_idx, mut state) = {
                                        let st = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                        let idx = if let Some(range) = sorted
                                            && range.start.0 != range.end.0
                                        {
                                            let text = &mut workspace.scripts[script_idx].text;
                                            let before = st.cursor.char_range().unwrap_or_else(|| {
                                                egui::text::CCursorRange::two(
                                                    egui::text::CCursor::new(range.start.0),
                                                    egui::text::CCursor::new(range.end.0),
                                                )
                                            });
                                            let mut undoer = st.undoer();
                                            undoer.add_undo(&(before, text.clone()));
                                            let start_b = crate::r_editor::char_to_byte(text, range.start.0);
                                            let end_b = crate::r_editor::char_to_byte(text, range.end.0);
                                            text.drain(start_b..end_b);
                                            range.start.0
                                        } else {
                                            cursor_before
                                                .map(|r| usize::from(r.primary.index))
                                                .unwrap_or_else(|| workspace.scripts[script_idx].text.chars().count())
                                        };
                                        (idx, st)
                                    };
                                    let text = &mut workspace.scripts[script_idx].text;
                                    let before = state
                                        .cursor
                                        .char_range()
                                        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx)));
                                    let mut undoer = state.undoer();
                                    undoer.add_undo(&(before, text.clone()));
                                    let new_cursor = crate::r_editor::apply_smart_enter(text, cursor_idx);
                                    let after = egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor));
                                    undoer.add_undo(&(after, text.clone()));
                                    state.set_undoer(undoer);
                                    state.cursor.set_char_range(Some(after));
                                    state.store(ui.ctx(), editor_id);
                                }

                                let ctrl_space = ui.input_mut(|i| {
                                    i.consume_key(egui::Modifiers::CTRL, egui::Key::Space)
                                        || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Space)
                                });
                                let tab =
                                    ui.input_mut(|i| if !i.modifiers.any() { i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) } else { false });

                                if tab {
                                    let sorted = cursor_before.map(|r| r.as_sorted_char_range());
                                    if let Some(range) = sorted
                                        && range.start.0 != range.end.0
                                    {
                                        let text = &mut workspace.scripts[script_idx].text;
                                        let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
                                        let before = state.cursor.char_range().unwrap_or_else(|| {
                                            egui::text::CCursorRange::two(
                                                egui::text::CCursor::new(range.start.0),
                                                egui::text::CCursor::new(range.end.0),
                                            )
                                        });
                                        let mut undoer = state.undoer();
                                        undoer.add_undo(&(before, text.clone()));
                                        let new_range = crate::r_editor::indent_selection(text, range.start.0..range.end.0);
                                        let after = egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(new_range.start),
                                            egui::text::CCursor::new(new_range.end),
                                        );
                                        undoer.add_undo(&(after, text.clone()));
                                        state.set_undoer(undoer);
                                        state.cursor.set_char_range(Some(after));
                                        state.store(ui.ctx(), editor_id);
                                    } else {
                                        let (cursor_idx, text_copy) = {
                                            let text = &workspace.scripts[script_idx].text;
                                            let cursor_idx =
                                                cursor_before.map(|r| usize::from(r.primary.index)).unwrap_or_else(|| text.chars().count());
                                            (cursor_idx, text.clone())
                                        };
                                        if let Some(ctx) = crate::r_editor::completion_context_at(&text_copy, cursor_idx) {
                                            let candidates = build_completion_candidates(workspace, active_id, &text_copy, &ctx);
                                            if !candidates.is_empty() {
                                                request_completion_metadata(workspace, active_id, text_copy.clone(), &ctx);
                                                workspace.completion = Some(ActiveCompletion {
                                                    script_id: active_id,
                                                    source_revision: text_copy,
                                                    context: ctx,
                                                    candidates,
                                                    selected: 0,
                                                    scroll_to_selected: true,
                                                });
                                            } else {
                                                insert_indentation_spaces(ui.ctx(), editor_id, &mut workspace.scripts[script_idx].text, cursor_idx);
                                            }
                                        } else {
                                            insert_indentation_spaces(ui.ctx(), editor_id, &mut workspace.scripts[script_idx].text, cursor_idx);
                                        }
                                    }
                                } else if ctrl_space {
                                    let (cursor_idx, text_copy) = {
                                        let text = &workspace.scripts[script_idx].text;
                                        let cursor_idx = cursor_before.map(|r| usize::from(r.primary.index)).unwrap_or_else(|| text.chars().count());
                                        (cursor_idx, text.clone())
                                    };
                                    let ctx = crate::r_editor::completion_context_at(&text_copy, cursor_idx).unwrap_or_else(|| {
                                        crate::r_editor::CompletionContext {
                                            kind: crate::r_editor::CompletionContextKind::General,
                                            prefix: String::new(),
                                            replace_start_char: cursor_idx,
                                            replace_end_char: cursor_idx,
                                        }
                                    });
                                    let candidates = build_completion_candidates(workspace, active_id, &text_copy, &ctx);
                                    if !candidates.is_empty() {
                                        request_completion_metadata(workspace, active_id, text_copy.clone(), &ctx);
                                        workspace.completion = Some(ActiveCompletion {
                                            script_id: active_id,
                                            source_revision: text_copy,
                                            context: ctx,
                                            candidates,
                                            selected: 0,
                                            scroll_to_selected: true,
                                        });
                                    }
                                }
                            }

                            let shift_tab = has_focus && ui.input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
                            let line_count = workspace.scripts[script_idx].text.bytes().filter(|byte| *byte == b'\n').count().saturating_add(1);
                            let font = egui::FontId::monospace(R_CODE_SIZE);
                            let gutter_width =
                                ui.fonts_mut(|fonts| fonts.layout_no_wrap(line_count.to_string(), font.clone(), t.text_dim)).size().x + 16.0;
                            let (response, reveal_rect) = ui
                                .horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    let (gutter, gutter_response) = ui.allocate_exact_size(egui::vec2(gutter_width, height), egui::Sense::click());
                                    if gutter_response.clicked() {
                                        ui.memory_mut(|m| m.request_focus(editor_id));
                                    }
                                    // Keep a fixed gap between the gutter and the horizontally scrolling code viewport.
                                    ui.add_space(6.0);
                                    let (edit_output, caret_anchor) = egui::ScrollArea::horizontal()
                                        .id_salt(("r-source-horizontal", active_id))
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            let script_doc = &mut workspace.scripts[script_idx];
                                            let script = &mut script_doc.text;
                                            let cache = &mut script_doc.highlight_cache;
                                            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _: f32| {
                                                let job = crate::r_editor::layout_job(ui, text.as_str(), f32::INFINITY, cache);
                                                ui.fonts_mut(|fonts| fonts.layout_job(job))
                                            };
                                            let output = egui::TextEdit::multiline(script)
                                                .id(editor_id)
                                                .code_editor()
                                                .font(font.clone())
                                                .frame(egui::Frame::NONE)
                                                .margin(egui::Margin::same(R_CODE_PADDING))
                                                .layouter(&mut layouter)
                                                .desired_rows(1)
                                                .min_size(egui::vec2(0.0, (height - 16.0).max(0.0)))
                                                .desired_width(ui.available_width())
                                                .show(ui);
                                            if script.contains('\r') {
                                                *script = script.replace("\r\n", "\n").replace('\r', "\n");
                                            }

                                            if (navigating || output.cursor_range != cursor_before)
                                                && let Some(range) = output.cursor_range
                                            {
                                                let (cursor, align) = if navigating {
                                                    let index = pending_reveal_index.unwrap_or(usize::from(range.primary.index));
                                                    let start = script
                                                        .chars()
                                                        .take(index)
                                                        .enumerate()
                                                        .filter(|(_, ch)| *ch == '\n')
                                                        .map(|(position, _)| position + 1)
                                                        .last()
                                                        .unwrap_or(0);
                                                    (egui::text::CCursor::new(start), Some(egui::Align::Min))
                                                } else {
                                                    (range.primary, None)
                                                };
                                                let caret = output.galley.pos_from_cursor(cursor).translate(output.galley_pos.to_vec2());
                                                ui.scroll_to_rect(caret.expand2(egui::vec2(12.0, 0.0)), align);
                                            }
                                            if shift_tab && let Some(range) = output.cursor_range {
                                                let sorted = range.as_sorted_char_range();
                                                let adjusted = crate::r_editor::outdent_selection(script, sorted.start.0..sorted.end.0);
                                                let mut state = output.state.clone();
                                                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                                                    egui::text::CCursor::new(adjusted.start),
                                                    egui::text::CCursor::new(adjusted.end),
                                                )));
                                                state.store(ui.ctx(), editor_id);
                                            }

                                            let caret_anchor = output
                                                .cursor_range
                                                .map(|range| output.galley.pos_from_cursor(range.primary).translate(output.galley_pos.to_vec2()));

                                            (output, caret_anchor)
                                        })
                                        .inner;

                                    popup_anchor = caret_anchor;

                                    // If text changed while completion popup was open, update prefix/context
                                    if edit_output.response.response.changed()
                                        && workspace.completion.as_ref().is_some_and(|c| c.script_id == active_id)
                                    {
                                        let (cursor_idx, text_copy) = {
                                            let text = &workspace.scripts[script_idx].text;
                                            let cursor_idx = edit_output.cursor_range.map(|r| usize::from(r.primary.index)).unwrap_or(0);
                                            (cursor_idx, text.clone())
                                        };
                                        if let Some(new_ctx) = crate::r_editor::completion_context_at(&text_copy, cursor_idx) {
                                            let candidates = build_completion_candidates(workspace, active_id, &text_copy, &new_ctx);
                                            if !candidates.is_empty() {
                                                if let Some(active) = workspace.completion.as_mut() {
                                                    active.context = new_ctx;
                                                    active.candidates = candidates;
                                                    active.selected = 0;
                                                    active.source_revision = text_copy;
                                                }
                                            } else {
                                                workspace.completion = None;
                                            }
                                        } else {
                                            workspace.completion = None;
                                        }
                                    }

                                    // The inner horizontal ScrollArea consumes both scroll targets. Forward
                                    // cursor movement to the vertical parent so keyboard navigation stays visible.
                                    if edit_output.response.response.has_focus()
                                        && !navigating
                                        && edit_output.cursor_range != cursor_before
                                        && let Some(range) = edit_output.cursor_range
                                    {
                                        ui.scroll_to_rect(
                                            edit_output.galley.pos_from_cursor(range.primary).translate(edit_output.galley_pos.to_vec2()),
                                            None,
                                        );
                                    }
                                    let reveal_rect = pending_reveal_index.map(|index| {
                                        edit_output
                                            .galley
                                            .pos_from_cursor(egui::text::CCursor::new(index))
                                            .translate(edit_output.galley_pos.to_vec2())
                                    });
                                    // Use actual row positions, including empty lines and CJK fallback heights.
                                    // This gutter stays fixed when code scrolls horizontally.
                                    let gutter =
                                        egui::Rect::from_min_max(gutter.min, egui::pos2(gutter.right(), edit_output.response.response.rect.bottom()));
                                    ui.painter().rect_filled(gutter, 0.0, t.panel_darker);
                                    ui.painter().vline(gutter.right(), gutter.y_range(), egui::Stroke::new(1.0, t.divider));
                                    for (index, row) in edit_output.galley.rows.iter().enumerate() {
                                        let y = edit_output.galley_pos.y + row.pos.y;
                                        if y + row.size.y >= ui.clip_rect().top() && y <= ui.clip_rect().bottom() {
                                            ui.painter().text(
                                                egui::pos2(gutter.right() - 6.0, y),
                                                egui::Align2::RIGHT_TOP,
                                                index.saturating_add(1).to_string(),
                                                font.clone(),
                                                t.text_dim,
                                            );
                                        }
                                    }
                                    (edit_output.response.response, reveal_rect)
                                })
                                .inner;
                            if let Some(caret) = reveal_rect {
                                let clip = ui.clip_rect();
                                let visible = if clip.height() > caret.height() {
                                    caret.top() >= clip.top() && caret.bottom() <= clip.bottom()
                                } else {
                                    clip.y_range().intersects(caret.y_range())
                                };
                                if visible {
                                    workspace.scripts[script_idx].pending_reveal_index = None;
                                    workspace.scripts[script_idx].reveal_requested = false;
                                } else {
                                    let target = egui::Rect::from_min_size(
                                        egui::pos2(caret.left(), caret.top() - height / 3.0),
                                        egui::vec2(1.0, caret.height()),
                                    );
                                    ui.scroll_to_rect(target, Some(egui::Align::Min));
                                    workspace.scripts[script_idx].reveal_requested = true;
                                    ui.ctx().request_repaint();
                                }
                            }
                            response
                        })
                        .inner
                })
                .inner
        })
        .inner;
    if let Some(target_id) = editor_widget_id {
        workspace.editor_widget_ids.insert(active_id, target_id);
        if workspace.pending_focus_script_id == Some(active_id) {
            ui.memory_mut(|m| m.request_focus(target_id));
            workspace.pending_focus_script_id = None;
        }
    } else {
        let id = response.id;
        workspace.editor_widget_ids.insert(active_id, id);
        if workspace.pending_focus_script_id == Some(active_id) {
            response.request_focus();
            workspace.pending_focus_script_id = None;
        }
    }

    if let (Some(editor_id), Some(anchor)) = (editor_widget_id, popup_anchor) {
        show_completion_popup(workspace, ui, editor_id, active_id, anchor.left_bottom());
    }
}

fn insert_indentation_spaces(ctx: &egui::Context, editor_id: egui::Id, text: &mut String, cursor_idx: usize) {
    let mut state = egui::TextEdit::load_state(ctx, editor_id).unwrap_or_default();
    let before = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx));
    let mut undoer = state.undoer();
    undoer.add_undo(&(before, text.clone()));
    let byte = crate::r_editor::char_to_byte(text, cursor_idx);
    text.insert_str(byte, "  ");
    let after = egui::text::CCursorRange::one(egui::text::CCursor::new(cursor_idx.saturating_add(2)));
    undoer.add_undo(&(after, text.clone()));
    state.set_undoer(undoer);
    state.cursor.set_char_range(Some(after));
    state.store(ctx, editor_id);
}

pub(crate) fn build_completion_candidates(
    workspace: &Workspace,
    _script_id: u64,
    source: &str,
    ctx: &crate::r_editor::CompletionContext,
) -> Vec<crate::r_editor::CompletionCandidate> {
    match &ctx.kind {
        crate::r_editor::CompletionContextKind::Path { raw_path, .. } => {
            let working_dir = workspace.working_directory.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            crate::r_editor::build_path_candidates(&working_dir, raw_path)
        }
        crate::r_editor::CompletionContextKind::Namespace { package, internal } => {
            let mut list = crate::r_editor::builtin_namespace(package, *internal);
            if let Some(cached) = workspace.completion_namespace_cache.get(&(package.clone(), *internal)) {
                list.extend(cached.iter().cloned());
            }
            crate::r_editor::rank_and_filter_candidates(&list, &ctx.prefix)
        }
        crate::r_editor::CompletionContextKind::MemberAccess { object, .. } => {
            let mut list = Vec::new();
            if let Some(cols) = workspace.completion_object_columns.get(object) {
                for (col_name, col_type) in cols {
                    list.push(crate::r_editor::CompletionCandidate {
                        name: col_name.clone(),
                        kind: crate::r_editor::CompletionCandidateKind::Column,
                        signature: None,
                        detail: col_type.clone(),
                        source: object.clone(),
                    });
                }
            }
            if let Some((viewer_name, page)) = &workspace.viewer
                && viewer_name == object
            {
                for (col_name, col_type) in &page.columns {
                    list.push(crate::r_editor::CompletionCandidate {
                        name: col_name.clone(),
                        kind: crate::r_editor::CompletionCandidateKind::Column,
                        signature: None,
                        detail: col_type.clone(),
                        source: object.clone(),
                    });
                }
            }
            let (_script_syms, script_cols, _) = crate::r_editor::extract_script_symbols(source);
            if let Some(cols) = script_cols.get(object) {
                for col in cols {
                    list.push(crate::r_editor::CompletionCandidate {
                        name: col.clone(),
                        kind: crate::r_editor::CompletionCandidateKind::Column,
                        signature: None,
                        detail: "column".into(),
                        source: object.clone(),
                    });
                }
            }
            if list.is_empty() && (object == "df" || object == "mtcars") {
                for col in ["mpg", "cyl", "disp", "hp", "drat", "wt", "qsec", "vs", "am", "gear", "carb"] {
                    list.push(crate::r_editor::CompletionCandidate {
                        name: col.into(),
                        kind: crate::r_editor::CompletionCandidateKind::Column,
                        signature: None,
                        detail: "numeric".into(),
                        source: object.clone(),
                    });
                }
            } else if list.is_empty() && object == "iris" {
                for col in ["Sepal.Length", "Sepal.Width", "Petal.Length", "Petal.Width", "Species"] {
                    list.push(crate::r_editor::CompletionCandidate {
                        name: col.into(),
                        kind: crate::r_editor::CompletionCandidateKind::Column,
                        signature: None,
                        detail: "numeric".into(),
                        source: object.clone(),
                    });
                }
            }
            crate::r_editor::rank_and_filter_candidates(&list, &ctx.prefix)
        }
        crate::r_editor::CompletionContextKind::General => {
            let mut list = crate::r_editor::builtin_keywords_and_constants();
            list.extend(crate::r_editor::builtin_base_and_packages());
            let (script_syms, _, _) = crate::r_editor::extract_script_symbols(source);
            list.extend(script_syms);
            for obj in &workspace.objects {
                let is_fn = obj.category == "Functions" || obj.data_type == "closure" || obj.classes.iter().any(|c| c == "function");
                if is_fn {
                    let sig = if obj.description.starts_with("function") {
                        obj.description.strip_prefix("function").map(|s| s.trim().to_owned())
                    } else {
                        Some("()".into())
                    };
                    list.push(crate::r_editor::CompletionCandidate {
                        name: obj.variable.clone(),
                        kind: crate::r_editor::CompletionCandidateKind::Function,
                        signature: sig,
                        detail: "function".into(),
                        source: ".GlobalEnv".into(),
                    });
                } else {
                    let detail = obj.classes.first().cloned().unwrap_or_else(|| obj.data_type.clone());
                    list.push(crate::r_editor::CompletionCandidate {
                        name: obj.variable.clone(),
                        kind: crate::r_editor::CompletionCandidateKind::Variable,
                        signature: None,
                        detail,
                        source: ".GlobalEnv".into(),
                    });
                }
            }
            crate::r_editor::rank_and_filter_candidates(&list, &ctx.prefix)
        }
    }
}

pub(crate) fn request_completion_metadata(
    workspace: &mut Workspace,
    script_id: u64,
    source_revision: String,
    ctx: &crate::r_editor::CompletionContext,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !workspace.session_active || workspace.running || workspace.completion_request_in_flight {
            return;
        }
        match &ctx.kind {
            crate::r_editor::CompletionContextKind::MemberAccess { object, .. } => {
                if workspace.completion_object_columns.contains_key(object) {
                    return;
                }
                workspace.completion_request_in_flight = true;
                let session = workspace.session.clone();
                let tx = workspace._tx.clone();
                let ctx_repaint = workspace.repaint_ctx.clone();
                let obj = object.clone();
                if std::thread::Builder::new()
                    .name("r-completion-meta".into())
                    .spawn(move || {
                        let Ok(mut session) = session.try_lock() else { return };
                        let output = session.completion_metadata(&format!("MEMBER\t{obj}")).unwrap_or_default();
                        let mut cols = Vec::new();
                        for line in output.lines() {
                            let mut parts = line.split('\t');
                            if let (Some(name_hex), Some(type_hex)) = (parts.next(), parts.next())
                                && let (Some(col_name), Some(col_type)) =
                                    (crate::r_session::decode_hex(name_hex), crate::r_session::decode_hex(type_hex))
                            {
                                cols.push((col_name, col_type));
                            }
                        }
                        let _ = tx.send(RMessage::CompletionMetadata {
                            script_id,
                            source_revision,
                            object_columns: Some((obj, cols)),
                            namespace_items: None,
                        });
                        if let Some(ctx) = ctx_repaint {
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    workspace.completion_request_in_flight = false;
                }
            }
            crate::r_editor::CompletionContextKind::Namespace { package, internal } => {
                if workspace.completion_namespace_cache.contains_key(&(package.clone(), *internal)) {
                    return;
                }
                workspace.completion_request_in_flight = true;
                let session = workspace.session.clone();
                let tx = workspace._tx.clone();
                let ctx_repaint = workspace.repaint_ctx.clone();
                let pkg = package.clone();
                let is_internal = *internal;
                if std::thread::Builder::new()
                    .name("r-completion-meta".into())
                    .spawn(move || {
                        let Ok(mut session) = session.try_lock() else { return };
                        let output = session.completion_metadata(&format!("NS\t{pkg}\t{}", if is_internal { 1 } else { 0 })).unwrap_or_default();
                        let mut items = Vec::new();
                        for line in output.lines() {
                            let mut parts = line.split('\t');
                            if let (Some(sym_hex), Some(kind_hex), Some(type_hex), Some(sig_hex)) =
                                (parts.next(), parts.next(), parts.next(), parts.next())
                                && let (Some(sym), Some(kind), Some(detail), Some(sig)) = (
                                    crate::r_session::decode_hex(sym_hex),
                                    crate::r_session::decode_hex(kind_hex),
                                    crate::r_session::decode_hex(type_hex),
                                    crate::r_session::decode_hex(sig_hex),
                                )
                            {
                                items.push(crate::r_editor::CompletionCandidate {
                                    name: sym,
                                    kind: if kind == "function" {
                                        crate::r_editor::CompletionCandidateKind::Function
                                    } else {
                                        crate::r_editor::CompletionCandidateKind::Variable
                                    },
                                    signature: if sig.is_empty() { None } else { Some(sig) },
                                    detail,
                                    source: if is_internal { format!("{pkg}:::") } else { pkg.clone() },
                                });
                            }
                        }
                        let _ = tx.send(RMessage::CompletionMetadata {
                            script_id,
                            source_revision,
                            object_columns: None,
                            namespace_items: Some(((pkg, is_internal), items)),
                        });
                        if let Some(ctx) = ctx_repaint {
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    workspace.completion_request_in_flight = false;
                }
            }
            _ => {}
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (workspace, script_id, source_revision, ctx);
    }
}

fn show_completion_popup(workspace: &mut Workspace, ui: &mut egui::Ui, editor_id: egui::Id, active_id: u64, anchor: egui::Pos2) {
    let Some(active) = workspace.completion.clone() else { return };
    if active.script_id != active_id || active.candidates.is_empty() {
        return;
    }

    let t = crate::theme::Tokens::get(ui.ctx());
    let clip = ui.clip_rect();
    let popup_x = anchor.x.clamp(clip.left() + 4.0, (clip.right() - 320.0).max(clip.left() + 4.0));
    let popup_y = (anchor.y + 4.0).min(clip.bottom() - 170.0).max(clip.top() + 4.0);

    let mut accepted_candidate = None;
    let close_popup = false;

    egui::Area::new(egui::Id::unique("r-completion-popup")).order(egui::Order::Foreground).fixed_pos(egui::pos2(popup_x, popup_y)).show(
        ui.ctx(),
        |ui| {
            egui::Frame::popup(ui.style())
                .fill(t.panel)
                .stroke(egui::Stroke::new(1.0, t.input_border))
                .corner_radius(t.radius)
                .inner_margin(egui::Margin::symmetric(3, 3))
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    egui::ScrollArea::vertical().id_salt("r-completion-scroll").max_height(160.0).auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (idx, candidate) in active.candidates.iter().enumerate() {
                            let is_selected = idx == active.selected;
                            let bg = if is_selected { t.row_selected } else { egui::Color32::TRANSPARENT };
                            let frame_resp =
                                egui::Frame::new().fill(bg).corner_radius(2.0).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_text = egui::RichText::new(&candidate.name)
                                            .font(crate::theme::semibold(t.font_size))
                                            .color(if is_selected { t.text_strong } else { t.text });
                                        ui.add(egui::Label::new(name_text).truncate());

                                        if let Some(sig) = &candidate.signature {
                                            ui.add(egui::Label::new(egui::RichText::new(sig).size(t.font_size - 1.0).color(t.text_dim)).truncate());
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let badge = format!("{}: {}", candidate.source, candidate.detail);
                                            ui.add(egui::Label::new(egui::RichText::new(badge).size(t.font_size - 2.0).color(t.text_dim)).truncate());
                                        });
                                    });
                                });

                            let row_resp = ui.interact(frame_resp.response.rect, ui.make_persistent_id(("r-comp-row", idx)), egui::Sense::click());
                            if row_resp.clicked() {
                                accepted_candidate = Some(candidate.clone());
                            }
                            if is_selected && active.scroll_to_selected {
                                row_resp.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                    });
                });
        },
    );

    if let Some(active_mut) = workspace.completion.as_mut() {
        active_mut.scroll_to_selected = false;
    }

    if let Some(candidate) = accepted_candidate {
        let is_dir = candidate.kind == crate::r_editor::CompletionCandidateKind::Directory;
        let mut new_cursor = None;
        if let Some(document) = workspace.active_script_mut() {
            let mut state = egui::TextEdit::load_state(ui.ctx(), editor_id).unwrap_or_default();
            let before_range =
                state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(active.context.replace_end_char)));
            let mut undoer = state.undoer();
            undoer.add_undo(&(before_range, document.text.clone()));
            let new_cursor_idx = crate::r_editor::apply_completion(&mut document.text, &active.context, &candidate);
            new_cursor = Some(new_cursor_idx);
            let after_range = egui::text::CCursorRange::one(egui::text::CCursor::new(new_cursor_idx));
            undoer.add_undo(&(after_range, document.text.clone()));
            state.set_undoer(undoer);
            state.cursor.set_char_range(Some(after_range));
            state.store(ui.ctx(), editor_id);
        }
        if is_dir
            && let Some(new_cursor_idx) = new_cursor
            && let Some(document) = workspace.active_script()
            && let Some(new_ctx) = crate::r_editor::completion_context_at(&document.text, new_cursor_idx)
        {
            let candidates = build_completion_candidates(workspace, active_id, &document.text, &new_ctx);
            if !candidates.is_empty() {
                workspace.completion = Some(ActiveCompletion {
                    script_id: active_id,
                    source_revision: document.text.clone(),
                    context: new_ctx,
                    candidates,
                    selected: 0,
                    scroll_to_selected: true,
                });
                ui.memory_mut(|m| m.request_focus(editor_id));
                return;
            }
        }
        workspace.completion = None;
        ui.memory_mut(|m| m.request_focus(editor_id));
    } else if close_popup {
        workspace.completion = None;
    }
}

pub(super) fn show_script_tabs(app: &mut crate::VectorcraftApp, ui: &mut egui::Ui) {
    app.scientific.sync_tab_order();
    let tab_order = app.scientific.tab_order.clone();
    let active_viewer = app.scientific.active_viewer_tab.clone();
    let active_id = app.scientific.active_script_id;
    let t = crate::theme::Tokens::get(ui.ctx());
    let width = ui.available_width();
    let tab_width = (width - 48.0).clamp(96.0, 200.0);
    let visibility_id = ui.make_persistent_id("r-source-tab-visibility");
    let active_key = if let Some(ref name) = active_viewer { format!("viewer:{name}") } else { format!("script:{active_id}") };
    let previous = ui.ctx().data(|d| d.get_temp::<(String, f32)>(visibility_id));
    let reveal_active = previous.is_none_or(|(key, old_width)| key != active_key || (old_width - width).abs() > 1.0);
    ui.ctx().data_mut(|d| d.insert_temp(visibility_id, (active_key, width)));
    egui::ScrollArea::horizontal().id_salt("r-script-source-tabs").auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for tab_item in tab_order {
                match tab_item {
                    super::CentralTabId::Script(id) => {
                        let Some(script) = app.scientific.scripts.iter().find(|s| s.id == id) else {
                            continue;
                        };
                        let filename = script.filename().to_owned();
                        let path = script.path.clone();
                        let dirty = script.is_dirty();
                        let active = active_viewer.is_none() && active_id == id;
                        ui.push_id(("script-tab", id), |ui| {
                            let tab = egui::Frame::new()
                                .fill(if active { t.input } else { t.tab_strip })
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .corner_radius(t.radius)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let label = if dirty { format!("* {filename}") } else { filename.clone() };
                                        let tooltip =
                                            format!("{}\n{}", path.as_deref().unwrap_or(&filename), if dirty { "Unsaved changes" } else { "Saved" });
                                        let mut text =
                                            egui::RichText::new(label).size(t.font_size).color(if active { t.text_strong } else { t.text_dim });
                                        if active {
                                            text = text.strong();
                                        }
                                        let response = ui
                                            .add_sized([tab_width - 28.0, 22.0], egui::Button::new(text).frame(false).selected(active).truncate())
                                            .on_hover_text(tooltip);
                                        if response.clicked() {
                                            app.scientific.active_viewer_tab = None;
                                            app.scientific.activate_script(id);
                                        }
                                        if ui.small_button("x").on_hover_text(format!("Close {filename}")).clicked() {
                                            request_close_script(app, id);
                                        }
                                    });
                                });
                            if active {
                                if reveal_active {
                                    tab.response.scroll_to_me(Some(egui::Align::Center));
                                }
                                ui.painter().hline(tab.response.rect.x_range(), tab.response.rect.bottom() - 1.0, egui::Stroke::new(2.0, t.accent));
                            }
                        });
                    }
                    super::CentralTabId::DataViewer(name) => {
                        let active = active_viewer.as_deref() == Some(&name);
                        ui.push_id(("viewer-tab", &name), |ui| {
                            let tab = egui::Frame::new()
                                .fill(if active { t.input } else { t.tab_strip })
                                .inner_margin(egui::Margin::symmetric(4, 2))
                                .corner_radius(t.radius)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 3.0;
                                        let icon_size = egui::vec2(12.0, 12.0);
                                        let (icon_rect, _) = ui.allocate_exact_size(icon_size, egui::Sense::hover());
                                        let icon_color = if active { t.accent } else { t.icon };
                                        crate::icons::paint(ui, "grid-3x3", icon_rect, icon_color);

                                        let tooltip = format!("Data Viewer: {name}\nClick to view object data");
                                        let mut text =
                                            egui::RichText::new(&name).size(t.font_size).color(if active { t.text_strong } else { t.text_dim });
                                        if active {
                                            text = text.strong();
                                        }
                                        let response = ui
                                            .add_sized([tab_width - 44.0, 22.0], egui::Button::new(text).frame(false).selected(active).truncate())
                                            .on_hover_text(tooltip);
                                        if response.clicked() {
                                            app.scientific.active_viewer_tab = Some(name.clone());
                                        }
                                        if ui.small_button("x").on_hover_text(format!("Close {name}")).clicked() {
                                            app.scientific.close_data_viewer(&name);
                                        }
                                    });
                                });
                            if active {
                                if reveal_active {
                                    tab.response.scroll_to_me(Some(egui::Align::Center));
                                }
                                ui.painter().hline(tab.response.rect.x_range(), tab.response.rect.bottom() - 1.0, egui::Stroke::new(2.0, t.accent));
                            }
                        });
                    }
                }
            }
        });
    });
}

pub(super) fn request_close_script(app: &mut crate::VectorcraftApp, id: u64) {
    let dirty = app.scientific.scripts.iter().find(|script| script.id == id).is_some_and(ScriptDocument::is_dirty);
    if dirty {
        app.scientific.pending_close_script_id = Some(id);
    } else {
        app.scientific.close_script_now(id);
    }
}

pub(super) fn show_close_script_prompt(app: &mut crate::VectorcraftApp, ctx: &egui::Context) {
    let Some(id) = app.scientific.pending_close_script_id else { return };
    let Some(filename) = app.scientific.scripts.iter().find(|script| script.id == id).map(|script| script.filename().to_owned()) else {
        app.scientific.pending_close_script_id = None;
        return;
    };
    egui::Modal::new(egui::Id::unique("r-script-close-confirmation")).show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.heading("Unsaved R script");
        ui.label(format!("Save changes to {filename} before closing?"));
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                crate::picks::button(app, move |app| save_script_for(app, id, false, true));
            }
            if ui.button("Discard").clicked() {
                app.scientific.close_script_now(id);
            }
            if ui.button("Cancel").clicked() {
                app.scientific.pending_close_script_id = None;
            }
        });
    });
}

pub(super) fn show_script_encoding_prompt(app: &mut crate::VectorcraftApp, ctx: &egui::Context) {
    let Some(pending) = app.scientific.pending_script_decode.clone() else { return };
    let mut selected = None;
    let mut cancel = false;
    egui::Modal::new(egui::Id::unique("r-script-encoding")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.heading("Choose R script encoding");
        ui.label(format!("Could not safely decode {}: {}", pending.path, pending.reason));
        ui.label("No text has been replaced or inserted into the editor.");
        for (label, encoding) in [
            ("UTF-8", ScriptEncoding::Utf8),
            ("UTF-8 with BOM", ScriptEncoding::Utf8Bom),
            ("UTF-16 Little Endian", ScriptEncoding::Utf16Le),
            ("UTF-16 Big Endian", ScriptEncoding::Utf16Be),
            ("Windows Korean (CP949 / EUC-KR)", ScriptEncoding::WindowsKorean),
        ] {
            if ui.button(label).clicked() {
                selected = Some(encoding);
            }
        }
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if cancel {
        app.scientific.pending_script_decode = None;
    }
    if let Some(encoding) = selected {
        choose_script_encoding(app, encoding);
    }
}

pub(super) fn choose_script_encoding(app: &mut crate::VectorcraftApp, encoding: ScriptEncoding) {
    let Some(pending) = app.scientific.pending_script_decode.clone() else { return };
    match decode_script_as(&pending.bytes, encoding) {
        Ok((script, format)) => {
            app.scientific.pending_script_decode = None;
            install_script_document(app, &pending.path, pending.bytes, script, format);
        }
        Err(error) => {
            app.scientific.r_status = format!("Could not decode {} as {}: {error}", pending.path, script_encoding_name(encoding));
            if let Some(still_pending) = app.scientific.pending_script_decode.as_mut() {
                still_pending.reason = error;
            }
        }
    }
}

fn script_encoding_name(encoding: ScriptEncoding) -> &'static str {
    match encoding {
        ScriptEncoding::Utf8 => "UTF-8",
        ScriptEncoding::Utf8Bom => "UTF-8 with BOM",
        ScriptEncoding::Utf16Le => "UTF-16 LE",
        ScriptEncoding::Utf16Be => "UTF-16 BE",
        ScriptEncoding::WindowsKorean => "CP949 / EUC-KR",
    }
}

pub(super) fn selected_code(script: &str, ctx: &egui::Context, editor_id: egui::Id) -> Option<(String, usize)> {
    let range = egui::TextEdit::load_state(ctx, editor_id).and_then(|state| state.cursor.char_range());
    let range = range?;
    let range = range.as_sorted_char_range();
    (range.start.0 != range.end.0).then(|| (code_for_char_range(script, range.start.0, range.end.0), range.end.0))
}

pub(super) fn line_start_byte(script: &str, zero_based_line: usize) -> Option<usize> {
    if zero_based_line == 0 { Some(0) } else { script.match_indices('\n').nth(zero_based_line - 1).map(|(byte, _)| byte + 1) }
}

pub(super) fn navigate_source(workspace: &mut Workspace, script_id: u64, char_index: usize) -> bool {
    let clamped_index = {
        let Some(script) = workspace.scripts.iter_mut().find(|script| script.id == script_id) else { return false };
        let index = char_index.min(script.text.chars().count());
        script.pending_cursor_index = Some(index);
        script.pending_reveal_index = Some(index);
        script.reveal_requested = false;
        index
    };
    if let Some(ctx) = workspace.repaint_ctx.as_ref() {
        if let Some(editor_id) = workspace.editor_widget_ids.get(&script_id).copied() {
            let mut state = egui::TextEdit::load_state(ctx, editor_id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(clamped_index))));
            state.store(ctx, editor_id);
        }
        ctx.request_repaint();
    }
    workspace.activate_script(script_id)
}

pub(super) fn go_to_plot_source(workspace: &mut Workspace, variable: &str) {
    if let Some(origin) = workspace.source_origins.get(variable) {
        let Some(script) = workspace.scripts.iter().find(|script| script.id == origin.script_id) else {
            workspace.r_status = format!("Source for {variable} is in a closed script");
            return;
        };
        if script.text != origin.source_revision {
            workspace.r_status = format!("Source for {variable} changed since execution; run the script again or use an unambiguous open definition");
            return;
        }
        let index = line_start_byte(&script.text, origin.line).and_then(|byte| script.text.get(..byte)).map(|prefix| prefix.chars().count());
        if let Some(index) = index {
            let id = origin.script_id;
            navigate_source(workspace, id, index);
            workspace.r_status = format!("Opened source for {variable}");
        }
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let scripts: Vec<_> = workspace.scripts.iter().map(|script| (script.id, script.text.clone())).collect();
        let session = workspace.session.clone();
        let tx = workspace._tx.clone();
        let ctx = workspace.repaint_ctx.clone();
        let name = variable.to_owned();
        workspace.r_status = format!("Finding source for {name}...");
        if let Err(error) = std::thread::Builder::new().name("r-source-lookup".into()).spawn(move || {
            let result = session.lock().map_err(|error| format!("R session lock failed: {error}")).map(|mut session| {
                let mut candidates = Vec::new();
                for (id, source) in scripts {
                    if let Ok(definitions) = session.assignment_definitions(&source) {
                        candidates
                            .extend(definitions.into_iter().filter(|(variable, _)| variable == &name).map(|(_, line)| (id, line, source.clone())));
                    }
                }
                candidates
            });
            let _ = tx.send(RMessage::SourceDefinitions(name, result));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        }) {
            workspace.r_status = format!("Could not start source lookup: {error}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        workspace.r_status = "Source lookup requires the native R session".into();
    }
}

pub(super) fn append_eof_newline(document: &mut ScriptDocument, ctx: Option<&egui::Context>, editor_id: Option<egui::Id>) {
    if document.text.ends_with('\n') {
        return;
    }
    if let (Some(ctx), Some(editor_id), Some(mut state)) =
        (ctx, editor_id, editor_id.and_then(|id| ctx.and_then(|ctx| egui::TextEdit::load_state(ctx, id))))
    {
        let before =
            state.cursor.char_range().unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(document.text.chars().count())));
        let mut undoer = state.undoer();
        undoer.add_undo(&(before, document.text.clone()));
        document.text.push('\n');
        let after = egui::text::CCursorRange::one(egui::text::CCursor::new(document.text.chars().count()));
        undoer.add_undo(&(after, document.text.clone()));
        state.set_undoer(undoer);
        state.store(ctx, editor_id);
    } else {
        document.text.push('\n');
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Delim {
    Paren,
    Bracket,
    Brace,
}

pub(crate) fn is_r_continuation_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token == "," || token == "~" || token == "?" || token == "$" || token == "::" || token == ":::" {
        return true;
    }
    if matches!(
        token,
        "+" | "-" | "*" | "/" | "^" | "|>" | "<-" | "<<-" | "->" | "->>" | "=" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "&" | "&&" | "|" | "||"
    ) {
        return true;
    }
    if token.len() >= 2 && token.starts_with('%') && token.ends_with('%') {
        return true;
    }
    matches!(token, "function" | "if" | "else" | "for" | "while" | "repeat")
}

fn next_line_starts_with_continuation(lines: &[&str], from_line: usize) -> bool {
    for line in lines.iter().skip(from_line) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("else") && (trimmed.len() == 4 || !trimmed.chars().nth(4).is_some_and(|c| c.is_alphanumeric() || c == '_')) {
            return true;
        }
        if trimmed.starts_with("|>") {
            return true;
        }
        if trimmed.starts_with('%') && trimmed[1..].contains('%') {
            let second_percent = trimmed[1..].find('%').map(|idx| idx + 1);
            if let Some(pos) = second_percent {
                let op = &trimmed[..=pos];
                if is_r_continuation_token(op) {
                    return true;
                }
            }
        }
        return false;
    }
    false
}

pub(crate) fn find_top_level_expressions(script: &str) -> Vec<(usize, usize)> {
    let mut expressions = Vec::new();
    let lines: Vec<&str> = script.split('\n').collect();
    if lines.is_empty() {
        return expressions;
    }

    let mut current_start: Option<usize> = None;
    let mut delim_stack: Vec<Delim> = Vec::new();
    let mut in_string: Option<char> = None;

    for (line_idx, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut line_has_code = false;
        let mut last_code_token = String::new();
        let mut first_code_token = String::new();

        while i < chars.len() {
            let c = chars[i];

            // If inside a multiline string
            if let Some(quote) = in_string {
                line_has_code = true;
                if c == '\\' {
                    i += 2;
                    continue;
                }
                if c == quote {
                    in_string = None;
                }
                i += 1;
                continue;
            }

            // Whitespace
            if c.is_whitespace() {
                i += 1;
                continue;
            }

            // Comment
            if c == '#' {
                break;
            }

            // String start
            if c == '"' || c == '\'' {
                in_string = Some(c);
                line_has_code = true;
                if first_code_token.is_empty() {
                    first_code_token = c.to_string();
                }
                last_code_token = c.to_string();
                i += 1;
                continue;
            }

            // Raw string r"..." or R"..."
            if (c == 'r' || c == 'R') && chars.get(i + 1) == Some(&'"') {
                in_string = Some('"');
                line_has_code = true;
                if first_code_token.is_empty() {
                    first_code_token = "r\"".into();
                }
                last_code_token = "r\"".into();
                i += 2;
                continue;
            }

            line_has_code = true;

            // Delimiters
            match c {
                '(' => {
                    delim_stack.push(Delim::Paren);
                    if first_code_token.is_empty() {
                        first_code_token = "(".into();
                    }
                    last_code_token = "(".into();
                    i += 1;
                    continue;
                }
                ')' => {
                    if let Some(pos) = delim_stack.iter().rposition(|d| *d == Delim::Paren) {
                        delim_stack.remove(pos);
                    }
                    if first_code_token.is_empty() {
                        first_code_token = ")".into();
                    }
                    last_code_token = ")".into();
                    i += 1;
                    continue;
                }
                '[' => {
                    delim_stack.push(Delim::Bracket);
                    if first_code_token.is_empty() {
                        first_code_token = "[".into();
                    }
                    last_code_token = "[".into();
                    i += 1;
                    continue;
                }
                ']' => {
                    if let Some(pos) = delim_stack.iter().rposition(|d| *d == Delim::Bracket) {
                        delim_stack.remove(pos);
                    }
                    if first_code_token.is_empty() {
                        first_code_token = "]".into();
                    }
                    last_code_token = "]".into();
                    i += 1;
                    continue;
                }
                '{' => {
                    delim_stack.push(Delim::Brace);
                    if first_code_token.is_empty() {
                        first_code_token = "{".into();
                    }
                    last_code_token = "{".into();
                    i += 1;
                    continue;
                }
                '}' => {
                    if let Some(pos) = delim_stack.iter().rposition(|d| *d == Delim::Brace) {
                        delim_stack.remove(pos);
                    }
                    if first_code_token.is_empty() {
                        first_code_token = "}".into();
                    }
                    last_code_token = "}".into();
                    i += 1;
                    continue;
                }
                _ => {}
            }

            if c == ';' {
                if first_code_token.is_empty() {
                    first_code_token = ";".into();
                }
                last_code_token = ";".into();
                i += 1;
                continue;
            }

            // Word or identifier
            let start = i;
            if c.is_alphanumeric() || c == '.' || c == '_' {
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                if first_code_token.is_empty() {
                    first_code_token = token.clone();
                }
                last_code_token = token;
            } else {
                // Operator or symbol
                while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '#' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ';')
                {
                    i += 1;
                }
                let token: String = chars[start..i].iter().collect();
                if first_code_token.is_empty() {
                    first_code_token = token.clone();
                }
                last_code_token = token;
            }
        }

        if !line_has_code {
            continue;
        }

        if current_start.is_none() {
            current_start = Some(line_idx);
        }

        let is_continuation_token = is_r_continuation_token(&last_code_token);
        let next_continues = next_line_starts_with_continuation(&lines, line_idx + 1);

        let control_clause_without_body = (first_code_token == "if" || first_code_token == "for" || first_code_token == "while")
            && last_code_token == ")"
            && delim_stack.is_empty();

        if delim_stack.is_empty()
            && in_string.is_none()
            && !is_continuation_token
            && !next_continues
            && !control_clause_without_body
            && let Some(start) = current_start.take()
        {
            expressions.push((start, line_idx));
        }
    }

    if let Some(start) = current_start {
        expressions.push((start, lines.len().saturating_sub(1)));
    }

    expressions
}

pub(crate) fn complete_expression_span(script: &str, cursor_line: usize) -> Result<(String, usize, usize, Option<usize>), String> {
    let expressions = find_top_level_expressions(script);
    if expressions.is_empty() {
        return Err("No complete R expression at or after the cursor".into());
    }
    let chosen = expressions
        .iter()
        .position(|(start, end)| *start <= cursor_line && cursor_line <= *end)
        .or_else(|| expressions.iter().position(|(start, _)| *start > cursor_line))
        .ok_or_else(|| "No complete R expression at or after the cursor".to_owned())?;

    let (start_line, end_line) = expressions[chosen];
    let next_line = expressions.get(chosen + 1).map(|(next_start, _)| *next_start);
    let lines: Vec<&str> = script.split('\n').collect();
    let expression = lines.get(start_line..=end_line).ok_or_else(|| "Expression span out of bounds".to_owned())?.join("\n");
    Ok((expression, start_line, end_line, next_line))
}

pub(super) fn run_current_expression(workspace: &mut Workspace, editor_id: egui::Id) {
    if let Some(script_mut) = workspace.active_script_mut()
        && script_mut.text.contains('\r')
    {
        script_mut.text = script_mut.text.replace("\r\n", "\n").replace('\r', "\n");
    }
    let Some(active_script) = workspace.active_script() else { return };
    let script_id = active_script.id;
    let script = active_script.text.clone();
    let cursor = workspace
        .repaint_ctx
        .as_ref()
        .and_then(|ctx| egui::TextEdit::load_state(ctx, editor_id))
        .and_then(|state| state.cursor.char_range())
        .map(|range| usize::from(range.primary.index))
        .or(active_script.pending_cursor_index)
        .unwrap_or(0)
        .min(script.chars().count());
    let cursor_line = script.chars().take(cursor).filter(|character| *character == '\n').count();
    let tail_is_blank_or_comments = script.split('\n').skip(cursor_line).all(|line| {
        let trimmed = line.trim_start();
        trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.contains(['"', '\'', '`']))
    });
    if tail_is_blank_or_comments {
        let end_cursor = script.chars().count();
        if cursor < end_cursor {
            navigate_source(workspace, script_id, end_cursor);
        }
        workspace.r_status = "No R expression at or after the cursor".into();
        return;
    }

    let (expression, start_line, end_line, mut next_line) = match complete_expression_span(&script, cursor_line) {
        Ok(span) => span,
        Err(error) => {
            if error.contains("No complete R expression") {
                let end_cursor = script.chars().count();
                if cursor < end_cursor {
                    navigate_source(workspace, script_id, end_cursor);
                }
                workspace.r_status = "No R expression at or after the cursor".into();
            } else {
                workspace.r_status = error;
                workspace.console_error = true;
            }
            return;
        }
    };

    if next_line.is_some_and(|line| line <= end_line) {
        next_line = complete_expression_span(&script, end_line.saturating_add(1)).ok().map(|(_, next_start, _, _)| next_start);
    }
    if expression.trim().is_empty() {
        workspace.r_status = "No complete R expression at the cursor".into();
        return;
    }

    let append_newline = next_line.is_none() && !script.ends_with('\n');
    let mut source_revision = script.clone();
    if append_newline {
        let ctx = workspace.repaint_ctx.clone();
        if let Some(active_mut) = workspace.active_script_mut() {
            append_eof_newline(active_mut, ctx.as_ref(), Some(editor_id));
            let updated_revision = active_mut.text.clone();
            for origin in workspace.source_origins.values_mut() {
                if origin.script_id == script_id && origin.source_revision == script {
                    origin.source_revision = updated_revision.clone();
                }
            }
            for queued in workspace.execution_queue.iter_mut() {
                if queued.script_id == Some(script_id) && queued.source_revision.as_ref() == Some(&script) {
                    queued.source_revision = Some(updated_revision.clone());
                }
            }
            source_revision = updated_revision;
        }
    }

    let next_cursor = next_line
        .and_then(|line| line_start_byte(&source_revision, line))
        .and_then(|byte| source_revision.get(..byte))
        .map_or_else(|| source_revision.chars().count(), |prefix| prefix.chars().count());

    navigate_source(workspace, script_id, next_cursor);

    let command = QueuedRCommand {
        id: workspace.next_queue_id,
        script_id: Some(script_id),
        code: expression,
        source_revision: Some(source_revision),
        start_line: Some(start_line),
        refresh_linked: false,
        is_selection: false,
    };
    workspace.next_queue_id = workspace.next_queue_id.wrapping_add(1).max(1);
    super::enqueue_or_execute(workspace, command);
}

pub(super) fn code_for_char_range(script: &str, start_char: usize, end_char: usize) -> String {
    let char_to_byte = |char_index: usize| script.char_indices().nth(char_index).map_or(script.len(), |(byte_index, _)| byte_index);
    let start_byte = char_to_byte(start_char);
    let end_byte = char_to_byte(end_char.max(start_char));
    if start_byte < end_byte {
        return script.get(start_byte..end_byte).unwrap_or_default().to_owned();
    }
    let before = script.get(..start_byte).unwrap_or_default();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let after = script.get(start_byte..).unwrap_or_default();
    let line_end = after.find('\n').map_or(script.len(), |index| start_byte + index);
    script.get(line_start..line_end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_top_level_expressions_single_and_multiline() {
        let code = "x <- 10\nSys.sleep(2)\ny <- x * 2\nprint(y)\n";
        let exprs = find_top_level_expressions(code);
        assert_eq!(exprs, vec![(0, 0), (1, 1), (2, 2), (3, 3)]);

        let (exp, start, end, next) = complete_expression_span(code, 0).unwrap();
        assert_eq!(exp, "x <- 10");
        assert_eq!((start, end), (0, 0));
        assert_eq!(next, Some(1));

        let (exp, start, end, next) = complete_expression_span(code, 2).unwrap();
        assert_eq!(exp, "y <- x * 2");
        assert_eq!((start, end), (2, 2));
        assert_eq!(next, Some(3));

        let (exp, start, end, next) = complete_expression_span(code, 3).unwrap();
        assert_eq!(exp, "print(y)");
        assert_eq!((start, end), (3, 3));
        assert_eq!(next, None);
    }

    #[test]
    fn test_multiline_continuation_operators_and_comments() {
        let code = "# comment\nplot_A <- ggplot(df, aes(wt, mpg)) +\n  geom_point() +\n  theme_classic()\nnext_value <- 1";
        let (exp, start, end, next) = complete_expression_span(code, 2).unwrap();
        assert!(exp.contains("plot_A <- ggplot"));
        assert!(exp.contains("theme_classic()"));
        assert!(!exp.contains("next_value"));
        assert_eq!((start, end), (1, 3));
        assert_eq!(next, Some(4));

        let (exp_next, start_next, end_next, next_next) = complete_expression_span(code, 4).unwrap();
        assert_eq!(exp_next, "next_value <- 1");
        assert_eq!((start_next, end_next), (4, 4));
        assert_eq!(next_next, None);
    }

    #[test]
    fn test_braces_and_strings_with_comments() {
        let code = "fn_test <- function(a) {\n  # inside comment\n  a + 1\n}\nfinal_val <- 5\n";
        let (exp, start, end, next) = complete_expression_span(code, 0).unwrap();
        assert_eq!(exp, "fn_test <- function(a) {\n  # inside comment\n  a + 1\n}");
        assert_eq!((start, end), (0, 3));
        assert_eq!(next, Some(4));
    }
}
