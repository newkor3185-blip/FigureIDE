//! Confirm operations that would make an R-linked panel impossible to refresh safely.

use serde_json::{Value, json};
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_engine::{LastTransform, ViewInfo};
use vectorcraft_tools::{Cursor, Mods, PointerEvent, PointerKind, SurroundingUnit, ToolKey};

use crate::VectorcraftApp;

#[derive(Clone)]
pub(super) enum ProtectedEdit {
    Command { id: String, params: Value },
    Gesture { events: Vec<PointerEvent>, view: ViewInfo, tool_id: String },
    ToolInput { events: Vec<PendingTypeInput>, view: ViewInfo, target: NodeId, epoch: u64, overflowed: bool },
    ToolKey { key: ToolKey, mods: Mods, view: ViewInfo, tool_id: String },
    UnlinkOnly,
}

#[derive(Clone)]
pub(crate) enum PendingTypeInput {
    Text(String),
    Key(ToolKey, Mods),
    DeleteSurrounding(usize, usize, SurroundingUnit),
}

#[derive(Clone)]
pub(super) struct PendingUnlinkEdit {
    document_uid: u64,
    selection: Vec<NodeId>,
    panel_ids: Vec<NodeId>,
    edit: ProtectedEdit,
}

pub(super) struct CapturedRotation {
    document_uid: u64,
    selection: Vec<NodeId>,
    panel_ids: Vec<NodeId>,
    events: Vec<PointerEvent>,
    view: ViewInfo,
    tool_id: String,
    confirm_on_click: bool,
}

pub(crate) fn linked_ancestor(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    for _ in 0..128 {
        if doc.node(id)?.name.as_deref().is_some_and(|name| name.starts_with("RVS_PANEL|")) {
            return Some(id);
        }
        id = doc.parent_of(id)?;
    }
    None
}

pub(crate) fn selected_panel(app: &VectorcraftApp) -> Option<NodeId> {
    let state = app.session.active()?;
    let first = *state.selection.objects.first()?;
    let panel = linked_ancestor(&state.doc, first)?;
    state.selection.objects.iter().all(|id| linked_ancestor(&state.doc, *id) == Some(panel)).then_some(panel)
}

pub(crate) fn request_unlink(app: &mut VectorcraftApp, panel: NodeId) -> Result<Value, String> {
    if app.scientific.pending_unlink_edit.is_some() || app.scientific.captured_rotation.is_some() {
        return Err("Resolve the pending linked-plot edit first".into());
    }
    let state = app.session.active().ok_or("No document is open")?;
    if linked_ancestor(&state.doc, panel) != Some(panel) {
        return Err("The selected plot is no longer linked to R".into());
    }
    app.scientific.pending_unlink_edit = Some(PendingUnlinkEdit {
        document_uid: state.uid,
        selection: state.selection.objects.clone(),
        panel_ids: vec![panel],
        edit: ProtectedEdit::UnlinkOnly,
    });
    Ok(json!({"pendingUnlinkConfirmation": true}))
}

/// Type tool commits bypass `app.run`. Hold them until the linked text is unlinked, including
/// multiple events from one input frame (such as an IME replacement followed by its commit).
pub(crate) fn intercept_type_input(app: &mut VectorcraftApp, view: ViewInfo, input: PendingTypeInput) -> bool {
    let owner = app.session.tool_ime_target(view);
    if let Some(pending) = app.scientific.pending_unlink_edit.as_mut() {
        if let ProtectedEdit::ToolInput { events, target: saved_target, epoch: saved_epoch, overflowed, .. } = &mut pending.edit
            && owner.is_some_and(|(document_uid, target, epoch)| {
                pending.document_uid == document_uid && *saved_target == NodeId(target) && *saved_epoch == epoch
            })
        {
            if events.len() < 1024 {
                events.push(input);
            } else {
                *overflowed = true;
            }
        }
        return true;
    }
    let Some((document_uid, target, epoch)) = owner else { return false };
    let target = NodeId(target);
    let Some(state) = app.session.active() else { return false };
    if state.uid != document_uid || linked_ancestor(&state.doc, target).is_none() || supported_text_override(app, &[target]) {
        return false;
    }
    let selection = state.selection.objects.clone();
    let panels = targets(app, &[target]);
    app.scientific.pending_unlink_edit = Some(PendingUnlinkEdit {
        document_uid,
        selection,
        panel_ids: panels,
        edit: ProtectedEdit::ToolInput { events: vec![input], view, target, epoch, overflowed: false },
    });
    true
}

/// A tool can claim Delete or a nudge key before the normal command shortcut runs. Its
/// document edit otherwise bypasses `app.run`, so keep the key for replay after unlinking.
pub(crate) fn intercept_tool_key(app: &mut VectorcraftApp, key: ToolKey, mods: Mods, view: ViewInfo) -> bool {
    if !matches!(key, ToolKey::Delete | ToolKey::Backspace | ToolKey::Left | ToolKey::Right | ToolKey::Up | ToolKey::Down) {
        return false;
    }
    if app.scientific.pending_unlink_edit.is_some() {
        return true;
    }
    let Some(state) = app.session.active() else { return false };
    let selection = state.selection.objects.clone();
    let panels = targets(app, &selection);
    if panels.is_empty() || !selection.iter().any(|id| !panels.contains(id)) {
        return false;
    }
    app.scientific.pending_unlink_edit = Some(PendingUnlinkEdit {
        document_uid: state.uid,
        selection,
        panel_ids: panels,
        edit: ProtectedEdit::ToolKey { key, mods, view, tool_id: app.session.tool_id().into() },
    });
    true
}

fn targets(app: &VectorcraftApp, ids: &[NodeId]) -> Vec<NodeId> {
    let Ok(state) = app.session.doc() else { return Vec::new() };
    let mut panels = Vec::new();
    for id in ids {
        if let Some(panel) = linked_ancestor(&state.doc, *id)
            && !panels.contains(&panel)
        {
            panels.push(panel);
        }
    }
    panels
}

fn command_ids(app: &VectorcraftApp, _id: &str, params: &Value) -> Vec<NodeId> {
    if let Some(ids) = params.get("ids").and_then(Value::as_array) {
        return ids.iter().filter_map(Value::as_u64).map(NodeId).collect();
    }
    if let Some(id) = params.get("id").and_then(Value::as_u64) {
        return vec![NodeId(id)];
    }
    app.session.active().map(|state| state.selection.objects.clone()).unwrap_or_default()
}

fn tracked_text_key(node: &vectorcraft_doc::Node) -> Option<&str> {
    node.extras().attrs.as_ref()?.data.iter().find(|(name, _)| name == "rvs-text-key-v1").map(|(_, value)| value.as_str())
}

fn supported_text_override(app: &VectorcraftApp, ids: &[NodeId]) -> bool {
    let Ok(state) = app.session.doc() else { return false };
    !ids.is_empty()
        && ids.iter().all(|id| {
            let Some(node) = state.doc.node(*id) else { return false };
            let Some(panel) = linked_ancestor(&state.doc, *id).and_then(|panel| state.doc.node(panel)) else { return false };
            let Some(generated) = panel.children().and_then(|children| children.iter().find(|child| child.name.as_deref() == Some("RVS_GENERATED")))
            else {
                return false;
            };
            let Some(key) = tracked_text_key(node).filter(|_| matches!(node.kind, vectorcraft_doc::NodeKind::Text(_))) else { return false };
            let mut count = 0;
            generated.walk(&mut |candidate| {
                if tracked_text_key(candidate) == Some(key) {
                    count += 1;
                }
            });
            count == 1
        })
}

fn changes_orientation(matrix: &[Value]) -> bool {
    let Some(a) = matrix.first().and_then(Value::as_f64) else { return false };
    let Some(b) = matrix.get(1).and_then(Value::as_f64) else { return false };
    let Some(c) = matrix.get(2).and_then(Value::as_f64) else { return false };
    let Some(d) = matrix.get(3).and_then(Value::as_f64) else { return false };
    b.abs() > 1e-9 || c.abs() > 1e-9 || a < 0.0 || d < 0.0
}

fn nonzero_turn(angle: Option<f64>) -> bool {
    angle.is_some_and(|angle| angle.is_finite() && angle.rem_euclid(360.0).min((-angle).rem_euclid(360.0)) > 1e-9)
}

fn conflicting_command(app: &VectorcraftApp, id: &str, params: &Value, ids: &[NodeId], panels: &[NodeId]) -> bool {
    let child = ids.iter().any(|id| !panels.contains(id));
    if child {
        if matches!(id, "text.setText" | "text.setStyle" | "text.setRangeStyle" | "object.move") && supported_text_override(app, ids) {
            return false;
        }
        // Generated children have a refresh baseline. Commands that mutate one must unlink
        // before they can change that baseline; creation, isolation and internal SVG replacement
        // are not direct edits of the selected child.
        if matches!(id, "edit.clear" | "edit.cut" | "edit.duplicate")
            || (matches!(id, "links.embed" | "links.relink" | "links.update" | "links.unembed")
                && app.session.doc().ok().is_some_and(|state| {
                    ids.iter().any(|id| state.doc.node(*id).is_some_and(|node| matches!(node.kind, vectorcraft_doc::NodeKind::PlacedDocument(_))))
                }))
            || (id.starts_with("object.")
                && !matches!(
                    id,
                    "object.isolate"
                        | "object.exitIsolation"
                        | "object.markRGenerated"
                        | "object.importRPlotSource"
                        | "object.replaceGroupContents"
                        | "object.resizeRPanel"
                ))
            || id.starts_with("paint.")
            || id.starts_with("appearance.")
            || matches!(id, "graphicStyle.apply" | "effect.apply" | "effect.setParams" | "layer.setProps")
            || (id.starts_with("path.") && id != "path.create")
            || (id.starts_with("text.") && !matches!(id, "text.create" | "text.createInPath"))
        {
            return true;
        }
    }
    match id {
        // Renaming away the persistent panel identity is an unlink, not a harmless label edit.
        // R-driven relink and refresh write another RVS_PANEL name and remain supported.
        "object.setName" => {
            ids.iter().any(|id| panels.contains(id)) && !params.get("name").and_then(Value::as_str).is_some_and(|name| name.starts_with("RVS_PANEL|"))
        }
        // These alter the panel hierarchy or copy its persistent identity. A linked duplicate
        // would otherwise carry the same panel label and R association as its source.
        "object.group" | "edit.duplicate" => true,
        "object.rotate" => {
            let angle = params.get("angle").and_then(Value::as_f64);
            if params.get("absolute").and_then(Value::as_bool) == Some(true) {
                let current = app.session.doc().ok().map(|state| state.doc.bbox_angle(ids)).unwrap_or(0.0);
                nonzero_turn(angle.map(|angle| angle - current))
            } else {
                nonzero_turn(angle)
            }
        }
        "object.transform" => params.get("matrix").and_then(Value::as_array).is_some_and(|matrix| changes_orientation(matrix)),
        "object.transformEach" => {
            nonzero_turn(params.get("rotate").and_then(Value::as_f64))
                || params.get("reflectX").and_then(Value::as_bool) == Some(true)
                || params.get("reflectY").and_then(Value::as_bool) == Some(true)
        }
        "object.transformAgain" => match app.session.active().and_then(|state| state.last_transform.as_ref()) {
            Some(LastTransform::Matrix(matrix, _)) => {
                let [a, b, c, d, _, _] = matrix.as_coeffs();
                b.abs() > 1e-9 || c.abs() > 1e-9 || a < 0.0 || d < 0.0
            }
            Some(LastTransform::Each(params)) => conflicting_command(app, "object.transformEach", params, ids, panels),
            None => false,
        },
        "object.reflect" => true,
        "object.shear" => nonzero_turn(params.get("angle").and_then(Value::as_f64)),
        "object.ungroup" => true,
        // Deleting a generated child invalidates refresh. Deleting the complete panel is an
        // ordinary delete of the artwork and its association, so it needs no unlink step.
        "edit.clear" => false,
        _ => false,
    }
}

pub(crate) fn would_block_command(app: &VectorcraftApp, id: &str, params: &Value) -> bool {
    let ids = command_ids(app, id, params);
    let panels = targets(app, &ids);
    !panels.is_empty() && conflicting_command(app, id, params, &ids, &panels)
}

/// Return a pending reply before the command mutates the document.
pub(crate) fn intercept_command(app: &mut VectorcraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if app.scientific.confirming_unlink {
        return None;
    }
    if !would_block_command(app, id, params) {
        return None;
    }
    if app.scientific.pending_unlink_edit.is_some() || app.scientific.captured_rotation.is_some() {
        return Some(Err("Resolve the pending linked-plot edit first".into()));
    }
    let Some(state) = app.session.active() else { return None };
    let ids = command_ids(app, id, params);
    app.scientific.pending_unlink_edit = Some(PendingUnlinkEdit {
        document_uid: state.uid,
        selection: state.selection.objects.clone(),
        panel_ids: targets(app, &ids),
        edit: ProtectedEdit::Command { id: id.into(), params: params.clone() },
    });
    Some(Ok(json!({"pendingUnlinkConfirmation": true})))
}

fn replay(app: &mut VectorcraftApp, events: &[PointerEvent], view: ViewInfo) -> Result<(), String> {
    for event in events {
        let requests = app.session.pointer(event, view).map_err(|error| error.to_string())?;
        crate::canvas::apply_requests(app, Ok(requests));
    }
    Ok(())
}

/// Capture an unsupported child edit or panel rotation before the tool creates a preview.
pub(crate) fn intercept_pointer(app: &mut VectorcraftApp, event: &PointerEvent, view: ViewInfo) -> bool {
    if let Some(capture) = app.scientific.captured_rotation.as_mut() {
        if matches!(event.kind, PointerKind::Drag | PointerKind::Up) {
            if capture.events.len() >= 1024 {
                app.scientific.captured_rotation = None;
                app.status("Linked-plot rotation was cancelled because the gesture was too long");
                return true;
            }
            capture.events.push(*event);
            if event.kind == PointerKind::Up
                && let Some(capture) = app.scientific.captured_rotation.take()
            {
                let first = capture.events.first().map(|event| event.pos);
                let moved =
                    first.is_some_and(|first| capture.events.iter().any(|event| event.kind == PointerKind::Drag && event.pos.distance(first) > 1e-6));
                if moved || capture.confirm_on_click {
                    app.scientific.pending_unlink_edit = Some(PendingUnlinkEdit {
                        document_uid: capture.document_uid,
                        selection: capture.selection,
                        panel_ids: capture.panel_ids,
                        edit: ProtectedEdit::Gesture { events: capture.events, view: capture.view, tool_id: capture.tool_id },
                    });
                } else if let Err(error) = replay(app, &capture.events, capture.view) {
                    app.status(error);
                }
            }
            return true;
        }
        if matches!(event.kind, PointerKind::Down | PointerKind::Move | PointerKind::DoubleClick) {
            // A release outside the canvas may not reach us. A new gesture must not inherit it.
            app.scientific.captured_rotation = None;
        }
    }
    if event.kind != PointerKind::Down || app.scientific.pending_unlink_edit.is_some() {
        return false;
    }
    let tool = app.session.tool_id().to_owned();
    let cursor = app.session.cursor(event.pos, event.mods, view);
    let Some(state) = app.session.active() else { return false };
    let selection = state.selection.objects.clone();
    let document_uid = state.uid;
    // These tools change existing paths directly inside `session.pointer`, including a click
    // with no drag. Test the actual hit so drawing elsewhere does not trigger unlinking.
    let editing_tool = matches!(
        tool.as_str(),
        "eraser"
            | "knife"
            | "smooth"
            | "pathEraser"
            | "join"
            | "addAnchor"
            | "deleteAnchor"
            | "anchorPoint"
            | "scissors"
            | "reshape"
            | "width"
            | "warp"
            | "twirl"
            | "pucker"
            | "bloat"
            | "scallop"
            | "crystallize"
            | "wrinkle"
            | "mirrorCut"
            | "lineCut"
            | "rectCut"
            | "curvature"
            | "mesh"
            | "gradient"
            | "touchType"
            | "puppetWarp"
            | "perspectiveSelection"
            | "shapeBuilder"
            | "livePaintBucket"
            | "blend"
    ) || tool == "pen"
        && matches!(
            cursor,
            Cursor::PenAdd | Cursor::PenDelete | Cursor::PenClose | Cursor::PenContinue | Cursor::PenJoin | Cursor::PenConvert | Cursor::AnchorPoint
        );
    let hit_panel = editing_tool
        .then(|| vectorcraft_doc::hit::hit_test(&state.doc, event.pos, Default::default()))
        .flatten()
        .and_then(|hit| linked_ancestor(&state.doc, hit.leaf));
    let mut panel_ids = targets(app, &selection);
    if let Some(panel) = hit_panel
        && !panel_ids.contains(&panel)
    {
        panel_ids.push(panel);
    }
    if panel_ids.is_empty() {
        return false;
    }
    let rotating =
        tool == "rotate" || (matches!(tool.as_str(), "selection" | "freeTransform") && matches!(cursor, Cursor::Rotate | Cursor::RotateToward(_)));
    let child_edit = matches!(tool.as_str(), "selection" | "directSelection" | "groupSelection" | "freeTransform")
        && selection.iter().any(|id| !panel_ids.contains(id))
        && !matches!(cursor, Cursor::Arrow | Cursor::ArrowHollow)
        && !(matches!(cursor, Cursor::Move) && supported_text_override(app, &selection));
    if !rotating && !child_edit && hit_panel.is_none() {
        return false;
    }
    app.scientific.captured_rotation = Some(CapturedRotation {
        document_uid,
        selection,
        panel_ids,
        events: vec![*event],
        view,
        tool_id: tool.into(),
        confirm_on_click: hit_panel.is_some(),
    });
    true
}

fn unlink_name(name: &str, id: NodeId) -> String {
    let fields: Vec<_> = name.strip_prefix("RVS_PANEL|").unwrap_or_default().split('|').collect();
    let label = match fields.first().copied() {
        Some("v2" | "v3" | "v4") => fields.get(1).copied().unwrap_or_default(),
        _ => fields.first().copied().unwrap_or_default(),
    };
    if label.is_empty() { format!("Unlinked plot {}", id.0) } else { format!("Panel {label}") }
}

pub(super) fn cancel(app: &mut VectorcraftApp) {
    app.scientific.pending_unlink_edit = None;
}

pub(super) fn confirm(app: &mut VectorcraftApp) -> Result<(), String> {
    let Some(pending) = app.scientific.pending_unlink_edit.take() else { return Err("No linked-plot edit is pending".into()) };
    if matches!(&pending.edit, ProtectedEdit::ToolInput { overflowed: true, .. }) {
        return Err("The pending text edit was too long to replay; no artwork or link was changed".into());
    }
    if let ProtectedEdit::ToolInput { view, target, epoch, .. } = &pending.edit
        && (app.session.tool_id() != "type" || app.session.tool_ime_target(*view) != Some((pending.document_uid, target.0, *epoch)))
    {
        return Err("The active text editor changed; retry the edit".into());
    }
    let state = app.session.active().ok_or("The document was closed before confirmation")?;
    if state.uid != pending.document_uid || state.selection.objects != pending.selection {
        return Err("The document or selection changed; retry the edit".into());
    }
    if let ProtectedEdit::Gesture { tool_id, .. } | ProtectedEdit::ToolKey { tool_id, .. } = &pending.edit
        && app.session.tool_id() != tool_id
    {
        return Err("The active tool changed; retry the rotation".into());
    }
    let names: Vec<_> = pending
        .panel_ids
        .iter()
        .map(|id| {
            let name = state.doc.node(*id).and_then(|node| node.name.as_deref()).ok_or("A linked panel was removed before confirmation")?;
            if !name.starts_with("RVS_PANEL|") {
                return Err("A linked panel changed before confirmation".to_owned());
            }
            Ok((*id, unlink_name(name, *id)))
        })
        .collect::<Result<_, String>>()?;
    let embedded_svg_children: Vec<u64> = pending
        .panel_ids
        .iter()
        .filter_map(|id| state.doc.node(*id))
        .flat_map(|panel| panel.children().into_iter().flatten())
        .flat_map(|generated| generated.children().into_iter().flatten())
        .filter_map(|child| match &child.kind {
            vectorcraft_doc::NodeKind::PlacedDocument(placed) if placed.key.starts_with("rvs-svg-") => Some(child.id.0),
            _ => None,
        })
        .collect();
    if app.scientific.running {
        app.scientific.pending_unlink_edit = Some(pending);
        return Err("Wait for the current R render to finish before unlinking".into());
    }
    app.run("edit.undoGroup.begin", json!({"label": "Unlink R plot and edit"}))?;
    app.scientific.confirming_unlink = true;
    let result = (|| {
        if !embedded_svg_children.is_empty() {
            let converted = app.run("links.embed", json!({"ids": embedded_svg_children}))?;
            if converted["missing"].as_array().is_some_and(|missing| !missing.is_empty()) {
                return Err("The embedded SVG source is unavailable; the R link and artwork were kept".into());
            }
        }
        for (id, name) in names {
            app.run("object.setName", json!({"id": id.0, "name": name}))?;
        }
        match pending.edit {
            ProtectedEdit::Command { id, .. } if id == "links.embed" => {
                // The conversion above already performed the requested Embed.
            }
            ProtectedEdit::Command { id, params } => {
                app.run(&id, params)?;
            }
            ProtectedEdit::Gesture { events, view, .. } => replay(app, &events, view)?,
            ProtectedEdit::ToolInput { events, view, .. } => {
                for event in events {
                    let result = match event {
                        PendingTypeInput::Text(text) => app.session.tool_text(&text, view),
                        PendingTypeInput::Key(key, mods) => app.session.tool_key(key, mods, view),
                        PendingTypeInput::DeleteSurrounding(before, after, unit) => {
                            app.session.tool_ime_delete_surrounding(before, after, unit, view)
                        }
                    };
                    let requests = result.map_err(|error| error.to_string())?;
                    crate::canvas::apply_requests(app, Ok(requests));
                }
                // The Type tool keeps a typing interaction open for subsequent keystrokes.
                // Finish this replay inside the unlink undo group so one Undo restores the link.
                if app.session.in_interaction() {
                    app.session.commit_interaction().map_err(|error| error.to_string())?;
                }
            }
            ProtectedEdit::ToolKey { key, mods, view, .. } => {
                let requests = app.session.tool_key(key, mods, view).map_err(|error| error.to_string())?;
                crate::canvas::apply_requests(app, Ok(requests));
            }
            ProtectedEdit::UnlinkOnly => {}
        }
        Ok::<(), String>(())
    })();
    app.scientific.confirming_unlink = false;
    if result.is_err() && app.session.in_interaction() {
        let _ = app.session.cancel_interaction();
    }
    let end = app.run("edit.undoGroup.end", json!({"cancel": result.is_err()}));
    super::restore_linked_panels(app);
    if result.is_ok() {
        app.scientific.pending_refresh.retain(|panel| !pending.panel_ids.contains(&NodeId(panel.panel_id)));
    }
    result?;
    end?;
    Ok(())
}

pub(crate) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if app.scientific.pending_unlink_edit.is_none() {
        return;
    }
    egui::Modal::new(egui::Id::unique("r-linked-plot-unlink-edit")).show(ctx, |ui| {
        ui.set_min_width(360.0);
        ui.heading("Unlink R Plot?");
        ui.label("This plot is linked to an R variable. To freely edit its generated vector elements, you must disconnect the R link. The artwork will remain editable, but future R updates will no longer change this plot.");
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                cancel(app);
            }
            if ui.add_enabled(!app.scientific.running, egui::Button::new("Unlink & Edit")).clicked()
                && let Err(error) = confirm(app)
            {
                app.status(error);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use egui::vec2;
    use egui_kittest::{Harness, kittest::Queryable as _};
    use serde_json::json;
    use vectorcraft_doc::NodeId;
    use vectorcraft_geom::Point;
    use vectorcraft_tools::{Cursor, PointerEvent, PointerKind};

    use super::{cancel, confirm, intercept_pointer, show};

    fn fixture() -> (crate::VectorcraftApp, NodeId, NodeId) {
        let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app.run("file.new", json!({"width": 400, "height": 300})).unwrap();
        let art = app.run("shape.rectangle", json!({"x": 100, "y": 100, "width": 100, "height": 60})).unwrap()["id"].as_u64().unwrap();
        let generated_text = app.run("text.create", json!({"x": 110, "y": 120, "text": "MA", "size": 10})).unwrap()["id"].as_u64().unwrap();
        let generated = app.run("object.group", json!({"ids": [art, generated_text]})).unwrap()["id"].as_u64().unwrap();
        let label = app.run("text.create", json!({"x": 100, "y": 80, "text": "A", "size": 10})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        app.run("object.setName", json!({"id": label, "name": "RVS_LABEL"})).unwrap();
        let panel = app.run("object.group", json!({"ids": [generated, label]})).unwrap()["id"].as_u64().unwrap();
        let name = format!("RVS_PANEL|v3|A|{}|70.000000|50.000000|reflow|original", super::super::encode_name("plot_A"));
        app.run("object.setName", json!({"id": panel, "name": name})).unwrap();
        app.run("select.set", json!({"ids": [panel]})).unwrap();
        super::super::restore_linked_panels(&mut app);
        (app, NodeId(panel), NodeId(generated))
    }

    fn panel_name(app: &crate::VectorcraftApp, panel: NodeId) -> String {
        app.session.doc().unwrap().doc.node(panel).unwrap().name.clone().unwrap()
    }

    #[test]
    fn numeric_rotation_waits_for_confirmation_cancel_does_not_mutate_and_undo_restores_link() {
        let (mut app, panel, _) = fixture();
        let original = app.session.doc().unwrap().doc.clone();
        let journal_len = app.session.journal.len();
        assert_eq!(app.run("object.rotate", json!({"angle": 45})).unwrap()["pendingUnlinkConfirmation"], true);
        assert!(app.scientific.pending_unlink_edit.is_some());
        assert_eq!(app.session.doc().unwrap().doc, original);
        assert_eq!(app.session.journal.len(), journal_len);

        let mut harness =
            Harness::builder().with_size(vec2(600.0, 400.0)).build_ui_state(|ui, app: &mut crate::VectorcraftApp| show(app, ui.ctx()), app);
        harness.run_steps(2);
        assert!(harness.query_by_label("Cancel").is_some());
        assert!(harness.query_by_label("Unlink & Edit").is_some());
        let app = harness.state_mut();
        cancel(app);
        assert_eq!(app.session.doc().unwrap().doc, original);
        assert_eq!(app.session.journal.len(), journal_len);
        assert!(panel_name(app, panel).starts_with("RVS_PANEL|"));

        app.run("object.rotate", json!({"angle": 45})).unwrap();
        confirm(app).unwrap();
        assert!(!panel_name(app, panel).starts_with("RVS_PANEL|"));
        assert!(app.session.doc().unwrap().doc.bbox_angle(&[panel]).abs() > 1.0);
        assert!(app.scientific.linked_panels.is_empty());
        let unlinked_art = app.session.doc().unwrap().doc.clone();
        super::super::update_all_panels(app);
        assert_eq!(app.scientific.r_status, "There are no linked panels to update");
        assert_eq!(app.session.doc().unwrap().doc, unlinked_art);
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(app, panel).starts_with("RVS_PANEL|"));
        assert_eq!(app.scientific.linked_panels.len(), 1);
        assert_eq!(app.session.doc().unwrap().doc.bbox_angle(&[panel]), 0.0);
        app.run("edit.redo", json!({})).unwrap();
        assert!(!panel_name(app, panel).starts_with("RVS_PANEL|"));
        assert!(app.scientific.linked_panels.is_empty());
        assert!(app.session.doc().unwrap().doc.bbox_angle(&[panel]).abs() > 1.0);
        app.run("object.rotate", json!({"angle": 10})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none(), "ordinary unlinked vectors rotate without another confirmation");
    }

    #[test]
    fn selection_handle_rotation_is_captured_before_preview_and_replayed_after_unlink() {
        let (mut app, panel, _) = fixture();
        app.select_tool("selection");
        let view = vectorcraft_engine::ViewInfo::default();
        let b = app.session.doc().unwrap().doc.bounds_of(&[panel], false).unwrap();
        let corners = [(b.x0, b.y0), (b.x1, b.y0), (b.x0, b.y1), (b.x1, b.y1)];
        let start = corners
            .into_iter()
            .flat_map(|(x, y)| [(-10.0, -10.0), (10.0, -10.0), (-10.0, 10.0), (10.0, 10.0)].map(move |(dx, dy)| Point::new(x + dx, y + dy)))
            .find(|point| matches!(app.session.cursor(*point, Default::default(), view), Cursor::Rotate | Cursor::RotateToward(_)))
            .expect("a selection-box rotation zone");
        let before = app.session.doc().unwrap().doc.clone();
        let event = |kind, pos| PointerEvent { kind, pos, mods: Default::default(), pressure: 1.0 };
        assert!(intercept_pointer(&mut app, &event(PointerKind::Down, start), view));
        assert!(intercept_pointer(&mut app, &event(PointerKind::Drag, Point::new(start.x + 25.0, start.y + 35.0)), view));
        assert!(intercept_pointer(&mut app, &event(PointerKind::Up, Point::new(start.x + 25.0, start.y + 35.0)), view));
        assert_eq!(app.session.doc().unwrap().doc, before, "capturing a drag must not preview document edits");
        assert!(app.scientific.pending_unlink_edit.is_some());
        confirm(&mut app).unwrap();
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert_ne!(app.session.doc().unwrap().doc, before);
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn unrelated_moves_scales_and_ordinary_rotation_do_not_prompt() {
        let (mut app, panel, generated) = fixture();
        app.run("object.move", json!({"dx": 10, "dy": 5})).unwrap();
        app.run("object.scale", json!({"sx": 110, "sy": 100})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none());
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        app.run("select.set", json!({"ids": [generated.0]})).unwrap();
        assert_eq!(app.run("object.ungroup", json!({})).unwrap()["pendingUnlinkConfirmation"], true);
        cancel(&mut app);
        assert!(app.session.doc().unwrap().doc.node(generated).is_some());
        let ordinary = app.run("shape.rectangle", json!({"x": 300, "y": 200, "width": 30, "height": 20})).unwrap()["id"].as_u64().unwrap();
        app.run("select.set", json!({"ids": [ordinary]})).unwrap();
        app.run("object.rotate", json!({"angle": 30})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none());
        assert!(app.session.doc().unwrap().doc.bbox_angle(&[NodeId(ordinary)]).abs() > 1.0);
    }

    #[test]
    fn matrix_rotation_transform_each_and_generated_child_deletion_wait_without_mutation() {
        let (mut app, panel, generated) = fixture();
        let before = app.session.doc().unwrap().doc.clone();
        for (id, params) in [("object.transform", json!({"matrix": [0, 1, -1, 0, 0, 0]})), ("object.transformEach", json!({"rotate": 30}))] {
            assert_eq!(app.run(id, params).unwrap()["pendingUnlinkConfirmation"], true, "{id}");
            assert_eq!(app.session.doc().unwrap().doc, before);
            cancel(&mut app);
        }
        let child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        app.run("select.set", json!({"ids": [child.0]})).unwrap();
        let before_delete = app.session.doc().unwrap().doc.clone();
        assert_eq!(app.run("edit.clear", json!({})).unwrap()["pendingUnlinkConfirmation"], true);
        assert_eq!(app.session.doc().unwrap().doc, before_delete);
        cancel(&mut app);
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn context_unlink_from_generated_child_confirms_and_undo_restores_art() {
        let (mut app, panel, generated) = fixture();
        let child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        app.run("select.set", json!({"ids": [child.0]})).unwrap();
        assert_eq!(super::selected_panel(&app), Some(panel));
        assert!(crate::menus::context_items(&app).iter().any(|item| matches!(item, crate::menus::Item::Cmd("Unlink from R", "ui.unlinkRPanel", _))));
        let before = app.session.doc().unwrap().doc.clone();
        let history = app.session.journal.len();
        assert_eq!(app.run("ui.unlinkRPanel", json!({})).unwrap()["pendingUnlinkConfirmation"], true);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert_eq!(app.session.journal.len(), history);
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, before);
        app.run("ui.unlinkRPanel", json!({"panel": panel.0})).unwrap();
        confirm(&mut app).unwrap();
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert!(app.session.doc().unwrap().doc.node(child).is_some());
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        app.run("edit.redo", json!({})).unwrap();
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn hybrid_unlink_converts_embedded_svg_to_editable_vectors_in_one_undo_group() {
        let (mut app, panel, generated) = fixture();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><circle cx="50" cy="30" r="12" fill="#377eb8"/><text x="5" y="12">MA</text></svg>"##;
        let generated_child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        app.run("select.set", json!({"ids": [generated_child.0]})).unwrap();
        let source = app.run("object.importRPlotSource", json!({"svg": svg, "width": 100.0, "height": 60.0})).unwrap()["id"].as_u64().unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none(), "internal SVG replacement must not request unlinking");
        app.run("object.replaceGroupContents", json!({"target": generated.0, "source": source})).unwrap();
        let child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        assert!(matches!(app.session.doc().unwrap().doc.node(child).unwrap().kind, vectorcraft_doc::NodeKind::PlacedDocument(_)));
        app.run("select.set", json!({"ids": [child.0]})).unwrap();
        assert_eq!(app.run("links.embed", json!({"ids": [child.0]})).unwrap()["pendingUnlinkConfirmation"], true);
        cancel(&mut app);
        assert!(matches!(app.session.doc().unwrap().doc.node(child).unwrap().kind, vectorcraft_doc::NodeKind::PlacedDocument(_)));
        app.run("select.set", json!({"ids": [panel.0]})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        app.run("ui.unlinkRPanel", json!({"panel": panel.0})).unwrap();
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, before);
        app.run("ui.unlinkRPanel", json!({"panel": panel.0})).unwrap();
        confirm(&mut app).unwrap();
        assert!(matches!(app.session.doc().unwrap().doc.node(child).unwrap().kind, vectorcraft_doc::NodeKind::Group { .. }));
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc, before);
        app.run("edit.redo", json!({})).unwrap();
        assert!(matches!(app.session.doc().unwrap().doc.node(child).unwrap().kind, vectorcraft_doc::NodeKind::Group { .. }));
    }

    #[test]
    fn explicit_unlink_uses_the_same_two_action_confirmation() {
        let (mut app, panel, _) = fixture();
        app.run("ui.unlinkRPanel", json!({"panel": panel.0})).unwrap();
        let mut harness =
            Harness::builder().with_size(vec2(600.0, 400.0)).build_ui_state(|ui, app: &mut crate::VectorcraftApp| show(app, ui.ctx()), app);
        harness.run_steps(2);
        assert!(harness.query_by_label("Cancel").is_some());
        assert!(harness.query_by_label("Unlink & Edit").is_some());
        assert!(harness.query_by_label("Unlink from R").is_none());
    }

    #[test]
    fn generated_text_and_shape_edits_wait_but_whole_panel_move_remains_linked() {
        let (mut app, panel, generated) = fixture();
        let children = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap();
        let shape = children[0].id;
        let text = children[1].id;
        let before = app.session.doc().unwrap().doc.clone();
        app.run("select.set", json!({"ids": [text.0]})).unwrap();
        for (command, params) in [
            ("text.setText", json!({"id": text.0, "text": "changed"})),
            ("edit.clear", json!({"ids": [text.0]})),
            ("object.move", json!({"ids": [text.0], "dx": 10, "dy": 0})),
        ] {
            assert_eq!(app.run(command, params).unwrap()["pendingUnlinkConfirmation"], true, "{command}");
            assert_eq!(app.session.doc().unwrap().doc, before);
            cancel(&mut app);
        }
        app.run("select.set", json!({"ids": [shape.0]})).unwrap();
        for (command, params) in
            [("paint.setFill", json!({"ids": [shape.0], "color": "#ff0000"})), ("object.scale", json!({"ids": [shape.0], "sx": 120, "sy": 120}))]
        {
            assert_eq!(app.run(command, params).unwrap()["pendingUnlinkConfirmation"], true, "{command}");
            cancel(&mut app);
        }
        app.run("paint.setFill", json!({"ids": [shape.0], "color": "#ff0000"})).unwrap();
        confirm(&mut app).unwrap();
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert_ne!(app.session.doc().unwrap().doc.node(shape), before.node(shape), "the original paint edit must be replayed");
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert_eq!(app.session.doc().unwrap().doc.node(shape), before.node(shape));
        app.run("select.set", json!({"ids": [panel.0]})).unwrap();
        app.run("object.move", json!({"dx": 5, "dy": 0})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none());
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn panel_identity_edits_and_generated_child_styles_require_unlink() {
        let (mut app, panel, generated) = fixture();
        let before = app.session.doc().unwrap().doc.clone();
        let journal_len = app.session.journal.len();
        for (command, params) in [
            ("object.setName", json!({"id": panel.0, "name": "Renamed"})),
            ("edit.duplicate", json!({})),
            ("object.group", json!({"ids": [panel.0]})),
        ] {
            assert!(super::would_block_command(&app, command, &params), "{command}");
            assert_eq!(app.run(command, params).unwrap()["pendingUnlinkConfirmation"], true);
            assert_eq!(app.session.doc().unwrap().doc, before);
            cancel(&mut app);
            assert_eq!(app.session.journal.len(), journal_len);
        }
        // Internal R metadata updates keep the panel linked.
        assert!(!super::would_block_command(&app, "object.setName", &json!({"id": panel.0, "name": panel_name(&app, panel)})));
        let child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        app.run("select.set", json!({"ids": [child.0]})).unwrap();
        for command in ["graphicStyle.apply", "effect.apply", "effect.setParams", "layer.setProps"] {
            assert!(super::would_block_command(&app, command, &json!({})), "{command}");
        }
        app.run("select.set", json!({"ids": [panel.0]})).unwrap();
        app.run("object.setName", json!({"id": panel.0, "name": "Renamed"})).unwrap();
        confirm(&mut app).unwrap();
        assert_eq!(panel_name(&app, panel), "Renamed");
        assert!(app.scientific.linked_panels.is_empty());
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(panel_name(&app, panel), "Renamed");
    }

    #[test]
    fn claimed_edit_keys_on_generated_children_wait_without_mutating_artwork() {
        let (mut app, panel, generated) = fixture();
        let child = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[0].id;
        let view = app.view_info();
        assert!(!super::intercept_tool_key(&mut app, vectorcraft_tools::ToolKey::Left, Default::default(), view));
        app.run("select.set", json!({"ids": [child.0]})).unwrap();
        let before = app.session.doc().unwrap().doc.clone();
        let journal_len = app.session.journal.len();
        assert!(super::intercept_tool_key(&mut app, vectorcraft_tools::ToolKey::Delete, Default::default(), view));
        assert_eq!(app.session.doc().unwrap().doc, before);
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert_eq!(app.session.journal.len(), journal_len);
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn destructive_path_tool_click_is_held_before_it_changes_linked_art() {
        let (mut app, panel, _) = fixture();
        app.select_tool("eraser");
        let view = app.view_info();
        let before = app.session.doc().unwrap().doc.clone();
        let event = |kind| PointerEvent::new(kind, 150.0, 130.0);
        assert!(intercept_pointer(&mut app, &event(PointerKind::Down), view));
        assert!(intercept_pointer(&mut app, &event(PointerKind::Up), view));
        assert!(app.scientific.pending_unlink_edit.is_some(), "a click-only edit still needs consent");
        assert_eq!(app.session.doc().unwrap().doc, before);
        cancel(&mut app);
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }

    #[test]
    fn uniquely_tracked_text_override_remains_editable_while_linked() {
        let (mut app, panel, generated) = fixture();
        app.run("object.markRGenerated", json!({"group": generated.0})).unwrap();
        let text = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[1].id;
        app.run("select.set", json!({"ids": [text.0]})).unwrap();
        app.run("text.setText", json!({"id": text.0, "text": "MA edited"})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none());
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        app.run("object.move", json!({"ids": [text.0], "dx": 2, "dy": 1})).unwrap();
        assert!(app.scientific.pending_unlink_edit.is_none());
    }

    #[test]
    fn unlinking_one_panel_leaves_another_linked_and_ordinary_menu_unchanged() {
        let (mut app, first, _) = fixture();
        let art = app.run("shape.rectangle", json!({"x": 250, "y": 100, "width": 80, "height": 60})).unwrap()["id"].as_u64().unwrap();
        let generated = app.run("object.group", json!({"ids": [art]})).unwrap()["id"].as_u64().unwrap();
        let label = app.run("text.create", json!({"x": 250, "y": 80, "text": "B"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        app.run("object.setName", json!({"id": label, "name": "RVS_LABEL"})).unwrap();
        let second = app.run("object.group", json!({"ids": [generated, label]})).unwrap()["id"].as_u64().unwrap();
        let name = format!("RVS_PANEL|v3|B|{}|70.000000|50.000000|reflow|original", super::super::encode_name("plot_B"));
        app.run("object.setName", json!({"id": second, "name": name})).unwrap();
        super::super::restore_linked_panels(&mut app);
        app.run("select.set", json!({"ids": [first.0]})).unwrap();
        app.run("ui.unlinkRPanel", json!({})).unwrap();
        confirm(&mut app).unwrap();
        assert!(!panel_name(&app, first).starts_with("RVS_PANEL|"));
        assert!(panel_name(&app, NodeId(second)).starts_with("RVS_PANEL|"));
        assert_eq!(app.scientific.linked_panels.len(), 1);
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, first).starts_with("RVS_PANEL|"));
        assert_eq!(app.scientific.linked_panels.len(), 2);
        let ordinary = app.run("shape.rectangle", json!({"x": 350, "y": 200, "width": 20, "height": 20})).unwrap()["id"].as_u64().unwrap();
        app.run("select.set", json!({"ids": [ordinary]})).unwrap();
        assert!(!crate::menus::context_items(&app).iter().any(|item| matches!(item, crate::menus::Item::Cmd(_, "ui.unlinkRPanel", _))));
    }

    #[test]
    fn type_tool_input_waits_for_unlink_and_replays_after_confirmation() {
        let (mut app, panel, generated) = fixture();
        let text = app.session.doc().unwrap().doc.node(generated).unwrap().children().unwrap()[1].id;
        app.select_tool("type");
        let view = app.view_info();
        for kind in [PointerKind::Down, PointerKind::Up] {
            app.session.pointer(&PointerEvent::new(kind, 112.0, 117.0), view).unwrap();
        }
        assert_eq!(app.session.tool_ime_target(view).map(|target| target.1), Some(text.0));
        let before = app.session.doc().unwrap().doc.clone();
        assert!(super::intercept_type_input(&mut app, view, super::PendingTypeInput::Text("x".into())));
        assert_eq!(app.session.doc().unwrap().doc, before);
        cancel(&mut app);
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert!(super::intercept_type_input(&mut app, view, super::PendingTypeInput::Text("x".into())));
        if let Some(super::PendingUnlinkEdit { edit: super::ProtectedEdit::ToolInput { overflowed, .. }, .. }) =
            app.scientific.pending_unlink_edit.as_mut()
        {
            *overflowed = true;
        }
        assert!(confirm(&mut app).is_err(), "a truncated replay must never unlink");
        assert_eq!(app.session.doc().unwrap().doc, before);
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert!(super::intercept_type_input(&mut app, view, super::PendingTypeInput::Text("x".into())));
        confirm(&mut app).unwrap();
        assert!(!panel_name(&app, panel).starts_with("RVS_PANEL|"));
        assert_ne!(app.session.doc().unwrap().doc, before);
        app.run("edit.undo", json!({})).unwrap();
        assert!(panel_name(&app, panel).starts_with("RVS_PANEL|"));
    }
}
