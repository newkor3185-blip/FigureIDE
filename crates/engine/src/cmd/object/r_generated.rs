//! Saved baseline markers for editable R-linked SVG artwork.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use vectorcraft_doc::{Node, NodeId, NodeKind};
use vectorcraft_geom::{Affine, Point, Rect, Vec2, union_opt};

use crate::{EngineError, Result};

const BASELINE: &str = "rvs-baseline-v1";
const KEY: &str = "rvs-text-key-v1";
const COUNT: &str = "rvs-node-count-v1";
const SIZE: &str = "rvs-size-v1";
const GEOMETRY: &str = "rvs-geometry-v1";
const STRUCTURE: &str = "rvs-children-v1";
// A 100k-point SVG also contains axes, labels and nested groups. Keep an explicit
// cap on traversal and metadata work while allowing representative linked UMAPs.
const MAX_NODES: usize = 200_000;
const MAX_DEPTH: usize = 128;

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::Other(message.into())
}

fn data<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.extras().attrs.as_ref()?.data.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str())
}

fn set_data(node: &mut Node, key: &str, value: String) {
    node.edit_attrs(|attrs| {
        if let Some((_, stored)) = attrs.data.iter_mut().find(|(name, _)| name == key) {
            *stored = value;
        } else {
            attrs.data.push((key.to_owned(), value));
        }
    });
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |value, byte| (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

fn text_key(node: &Node) -> Option<String> {
    let NodeKind::Text(text) = &node.kind else { return None };
    let plain = text.plain_text();
    Some(format!("text:{}:{:016x}", plain.len(), hash_bytes(plain.as_bytes())))
}

fn origin(node: &Node) -> Result<Point> {
    let bounds = node.visual_bounds().ok_or_else(|| invalid("R plot group has no visible bounds"))?;
    if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1].into_iter().all(f64::is_finite) {
        return Err(invalid("R plot bounds are not finite"));
    }
    Ok(Point::new(bounds.x0, bounds.y0))
}

fn content_bounds(nodes: &[Node]) -> Result<Rect> {
    let bounds = nodes.iter().fold(None, |bounds: Option<Rect>, node| union_opt(bounds, node.visual_bounds()));
    bounds.ok_or_else(|| invalid("Rendered R plot has no visible contents"))
}

fn canonicalize_floats(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(raw) = number.as_f64()
                && raw.is_finite()
                && raw.abs() < 1e10
                && let Some(rounded) = serde_json::Number::from_f64((raw * 100_000.0).round() / 100_000.0)
            {
                *number = rounded;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(canonicalize_floats),
        Value::Object(fields) => fields.values_mut().for_each(canonicalize_floats),
        _ => {}
    }
}

/// A stable, versioned fingerprint of one node relative to the generated group's origin.
/// The child tree and our own metadata are excluded; descendants have their own markers.
fn fingerprint(node: &Node, group_origin: Point, normalize: Affine) -> Result<String> {
    let mut normalized = node.clone();
    normalized.id = NodeId(0);
    if let Some(children) = normalized.children_mut() {
        children.clear();
    }
    if let Some(extra) = normalized.extras_if_any()
        && let Some(attrs) = extra.attrs.as_deref_mut()
    {
        attrs.data.retain(|(name, _)| !matches!(name.as_str(), BASELINE | KEY | COUNT | SIZE | GEOMETRY | STRUCTURE));
        if *attrs == vectorcraft_doc::ObjectAttributes::default() {
            extra.attrs = None;
        }
    }
    normalized.transform(normalize, false);
    normalized.transform(Affine::translate(Vec2::new(-group_origin.x, -group_origin.y)), false);
    let mut value = serde_json::to_value(&normalized).map_err(|error| invalid(format!("Cannot fingerprint R plot object: {error}")))?;
    canonicalize_floats(&mut value);
    let bytes = serde_json::to_vec(&value).map_err(|error| invalid(format!("Cannot encode R plot fingerprint: {error}")))?;
    // FNV-1a is deterministic across app launches; the v1 tag guards future format changes.
    let hash = hash_bytes(&bytes);
    Ok(format!("{hash:016x}"))
}

fn mark_child(node: &mut Node, group_origin: Point, depth: usize, count: &mut usize) -> Result<()> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return Err(invalid("R plot contains too many nested objects to track safely"));
    }
    *count += 1;
    if let Some(children) = node.children_mut() {
        for child in children {
            mark_child(Arc::make_mut(child), group_origin, depth + 1, count)?;
        }
    }
    let hash = fingerprint(node, group_origin, Affine::IDENTITY)?;
    if let Some(key) = text_key(node) {
        set_data(node, KEY, key);
    }
    if let Some(structure) = child_structure(node) {
        set_data(node, STRUCTURE, structure);
    }
    set_data(node, BASELINE, hash);
    Ok(())
}

fn child_structure(node: &Node) -> Option<String> {
    let children = node.children()?;
    let mut bytes = Vec::with_capacity(children.len().saturating_mul(8));
    for child in children {
        bytes.extend_from_slice(&child.id.0.to_le_bytes());
    }
    Some(format!("{:016x}", hash_bytes(&bytes)))
}

pub(super) fn mark(group: &mut Node) -> Result<usize> {
    if !matches!(group.kind, NodeKind::Group { clip: false, .. }) {
        return Err(invalid("R-generated artwork must be an unclipped group"));
    }
    let bounds = group.visual_bounds().ok_or_else(|| invalid("R plot group has no visible bounds"))?;
    if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1].into_iter().all(f64::is_finite) {
        return Err(invalid("R plot bounds are not finite"));
    }
    let group_origin = Point::new(bounds.x0, bounds.y0);
    let mut count = 0;
    if let Some(children) = group.children_mut() {
        for child in children {
            mark_child(Arc::make_mut(child), group_origin, 1, &mut count)?;
        }
    }
    set_data(group, BASELINE, fingerprint(group, group_origin, Affine::IDENTITY)?);
    if let Some(structure) = child_structure(group) {
        set_data(group, STRUCTURE, structure);
    }
    set_data(group, COUNT, count.to_string());
    set_data(group, SIZE, format!("{:.9},{:.9}", bounds.width(), bounds.height()));
    if let Some(geometry) = group.geometric_bounds() {
        set_data(group, GEOMETRY, format!("{:.9},{:.9}", geometry.width(), geometry.height()));
    }
    Ok(count)
}

/// A whole linked-panel resize is a supported operation. Capture only text edits that were
/// already present before it, so rebasing the generated geometry cannot bless a shape edit.
pub(super) struct RebaseSnapshot {
    edited_text: BTreeMap<u64, (String, String)>,
}

fn generated_group(node: &Node) -> Option<&Node> {
    if node.name.as_deref() == Some("RVS_GENERATED") {
        return Some(node);
    }
    node.name.as_deref()?.starts_with("RVS_PANEL|").then_some(())?;
    node.children()?.iter().find(|child| child.name.as_deref() == Some("RVS_GENERATED")).map(Arc::as_ref)
}

fn generated_group_mut(node: &mut Node) -> Option<&mut Node> {
    if node.name.as_deref() == Some("RVS_GENERATED") {
        return Some(node);
    }
    node.name.as_deref()?.starts_with("RVS_PANEL|").then_some(())?;
    node.children_mut()?.iter_mut().find(|child| child.name.as_deref() == Some("RVS_GENERATED")).map(Arc::make_mut)
}

fn scan_before_transform(
    node: &Node,
    group_origin: Point,
    depth: usize,
    count: &mut usize,
    edited_text: &mut BTreeMap<u64, (String, String)>,
) -> Result<bool> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return Ok(false);
    }
    *count += 1;
    let Some(saved) = data(node, BASELINE) else { return Ok(false) };
    if child_structure(node).as_deref() != data(node, STRUCTURE) {
        return Ok(false);
    }
    if saved != fingerprint(node, group_origin, Affine::IDENTITY)? {
        let Some(key) = data(node, KEY) else { return Ok(false) };
        if !matches!(node.kind, NodeKind::Text(_)) {
            return Ok(false);
        }
        edited_text.insert(node.id.0, (saved.to_owned(), key.to_owned()));
    }
    if let Some(children) = node.children() {
        for child in children {
            if !scan_before_transform(child, group_origin, depth + 1, count, edited_text)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn prepare_rebase(node: &Node) -> Result<Option<RebaseSnapshot>> {
    let Some(group) = generated_group(node) else { return Ok(None) };
    let Some(saved_count) = data(group, COUNT).and_then(|value| value.parse::<usize>().ok()) else { return Ok(None) };
    let Ok(group_origin) = origin(group) else { return Ok(None) };
    if data(group, BASELINE) != Some(fingerprint(group, group_origin, Affine::IDENTITY)?.as_str())
        || child_structure(group).as_deref() != data(group, STRUCTURE)
    {
        return Ok(None);
    }
    let mut count = 0;
    let mut edited_text = BTreeMap::new();
    if let Some(children) = group.children() {
        for child in children {
            if !scan_before_transform(child, group_origin, 1, &mut count, &mut edited_text)? {
                return Ok(None);
            }
        }
    }
    Ok((count == saved_count).then_some(RebaseSnapshot { edited_text }))
}

fn restore_edited_text(node: &mut Node, edited_text: &BTreeMap<u64, (String, String)>) {
    if let Some((baseline, key)) = edited_text.get(&node.id.0) {
        set_data(node, BASELINE, baseline.clone());
        set_data(node, KEY, key.clone());
    }
    if let Some(children) = node.children_mut() {
        for child in children {
            restore_edited_text(Arc::make_mut(child), edited_text);
        }
    }
}

pub(super) fn finish_rebase(node: &mut Node, snapshot: RebaseSnapshot) -> Result<()> {
    let group = generated_group_mut(node).ok_or_else(|| invalid("Tracked R plot group disappeared during transform"))?;
    mark(group)?;
    restore_edited_text(group, &snapshot.edited_text);
    Ok(())
}

#[derive(Clone)]
struct Override {
    key: String,
    node: Node,
}

fn collect_old(
    node: &Node,
    group_origin: Point,
    normalize: Affine,
    depth: usize,
    count: &mut usize,
    keys: &mut BTreeMap<String, usize>,
    edits: &mut Vec<Override>,
) -> Result<()> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return Err(invalid("R plot contains too many nested objects to reconcile safely"));
    }
    *count += 1;
    let saved = data(node, BASELINE).ok_or_else(|| invalid("R plot artwork has no saved edit baseline; original artwork was kept"))?;
    let current = fingerprint(node, group_origin, normalize)?;
    if child_structure(node).as_deref() != data(node, STRUCTURE) {
        return Err(invalid("Generated R plot grouping or stacking changed; the original artwork was kept"));
    }
    if let Some(key) = data(node, KEY) {
        *keys.entry(key.to_owned()).or_default() += 1;
    }
    if saved != current {
        let Some(key) = data(node, KEY) else {
            return Err(invalid(
                "A generated R plot shape or group was edited; it cannot be safely reapplied, so the original artwork was kept. Use Unlink in Linked Panels to keep the vector edit without the R link",
            ));
        };
        if !matches!(node.kind, NodeKind::Text(_)) {
            return Err(invalid("A generated R plot object changed kind; the original artwork was kept"));
        }
        edits.push(Override { key: key.to_owned(), node: node.clone() });
    }
    if let Some(children) = node.children() {
        for child in children {
            collect_old(child, group_origin, normalize, depth + 1, count, keys, edits)?;
        }
    }
    Ok(())
}

fn count_new_keys(node: &Node, keys: &mut BTreeMap<String, usize>, depth: usize, count: &mut usize) -> Result<()> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return Err(invalid("Rendered R plot contains too many nested objects"));
    }
    *count += 1;
    if let Some(key) = text_key(node) {
        *keys.entry(key).or_default() += 1;
    }
    if let Some(children) = node.children() {
        for child in children {
            count_new_keys(child, keys, depth + 1, count)?;
        }
    }
    Ok(())
}

fn apply_override(node: &mut Node, edit: &Override) -> bool {
    if text_key(node).as_deref() == Some(edit.key.as_str()) {
        *node = edit.node.clone();
        return true;
    }
    if let Some(children) = node.children_mut() {
        for child in children {
            if apply_override(Arc::make_mut(child), edit) {
                return true;
            }
        }
    }
    false
}

fn refresh_child_structures(node: &mut Node) {
    if let Some(children) = node.children_mut() {
        for child in children {
            refresh_child_structures(Arc::make_mut(child));
        }
    }
    if let Some(structure) = child_structure(node) {
        set_data(node, STRUCTURE, structure);
    }
}

/// Stamp the newly rendered children, then reapply only uniquely identified edited text.
/// An uncertain change aborts the caller's document transaction before source art is removed.
pub(super) fn reconcile(old_group: &Node, replacement: &mut [Node], allow_canvas_resize: bool) -> Result<(usize, usize)> {
    let group_origin = origin(old_group)?;
    let saved_count = data(old_group, COUNT)
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| invalid("This linked plot predates edit tracking; re-add it to enable protected refresh. The original artwork was kept"))?;
    let saved_size = data(old_group, SIZE)
        .and_then(|size| size.split_once(','))
        .and_then(|(width, height)| Some((width.parse::<f64>().ok()?, height.parse::<f64>().ok()?)))
        .ok_or_else(|| invalid("R plot geometry baseline is missing; the original artwork was kept"))?;
    let current_bounds = old_group.visual_bounds().ok_or_else(|| invalid("R plot group has no visible bounds"))?;
    let current_geometry = old_group.geometric_bounds().ok_or_else(|| invalid("R plot group has no geometric bounds"))?;
    if ![saved_size.0, saved_size.1, current_bounds.width(), current_bounds.height()].into_iter().all(|size| size.is_finite() && size > 1e-9) {
        return Err(invalid("R plot geometry baseline is invalid; the original artwork was kept"));
    }
    let resized = (current_bounds.width() - saved_size.0).abs() > 1e-4 || (current_bounds.height() - saved_size.1).abs() > 1e-4;
    if resized && !allow_canvas_resize {
        return Err(invalid("Manual edits changed the R plot bounds; the original artwork was kept to avoid resizing it"));
    }
    let normalize = if resized {
        let saved_geometry = data(old_group, GEOMETRY)
            .and_then(|size| size.split_once(','))
            .and_then(|(width, height)| Some((width.parse::<f64>().ok()?, height.parse::<f64>().ok()?)))
            .unwrap_or((
                saved_size.0 - (current_bounds.width() - current_geometry.width()),
                saved_size.1 - (current_bounds.height() - current_geometry.height()),
            ));
        if ![saved_geometry.0, saved_geometry.1, current_geometry.width(), current_geometry.height()]
            .into_iter()
            .all(|size| size.is_finite() && size > 1e-9)
        {
            return Err(invalid("R plot geometric baseline is invalid; the original artwork was kept"));
        }
        let scale_x = saved_geometry.0 / current_geometry.width();
        let scale_y = saved_geometry.1 / current_geometry.height();
        if ![current_geometry.x0, current_geometry.y0, scale_x, scale_y].into_iter().all(f64::is_finite)
            || !(1e-6..=1e6).contains(&scale_x)
            || !(1e-6..=1e6).contains(&scale_y)
        {
            return Err(invalid("R plot resize transform is outside safe limits; the original artwork was kept"));
        }
        let geometry_origin = Point::new(current_geometry.x0, current_geometry.y0);
        Affine::translate(geometry_origin.to_vec2()) * Affine::scale_non_uniform(scale_x, scale_y) * Affine::translate(-geometry_origin.to_vec2())
    } else {
        Affine::IDENTITY
    };
    if data(old_group, BASELINE) != Some(fingerprint(old_group, group_origin, normalize)?.as_str()) {
        return Err(invalid("The generated R plot group was edited; the original artwork was kept"));
    }
    if child_structure(old_group).as_deref() != data(old_group, STRUCTURE) {
        return Err(invalid("Generated R plot grouping or stacking changed; the original artwork was kept"));
    }
    let mut count = 0;
    let mut old_keys = BTreeMap::new();
    let mut edits = Vec::new();
    if let Some(children) = old_group.children() {
        for child in children {
            collect_old(child, group_origin, normalize, 1, &mut count, &mut old_keys, &mut edits)?;
        }
    }
    if count != saved_count {
        return Err(invalid("Generated R plot objects were added or removed; the original artwork was kept"));
    }
    let mut new_keys = BTreeMap::new();
    let mut new_count = 0;
    for node in replacement.iter() {
        count_new_keys(node, &mut new_keys, 1, &mut new_count)?;
    }
    for edit in &edits {
        if old_keys.get(&edit.key) != Some(&1) || new_keys.get(&edit.key) != Some(&1) {
            return Err(invalid("An edited R plot text label is missing or ambiguous in the new render; the original artwork was kept"));
        }
    }
    let new_bounds = content_bounds(replacement)?;
    let new_origin = Point::new(new_bounds.x0, new_bounds.y0);
    let mut stamped = 0;
    for node in replacement.iter_mut() {
        mark_child(node, new_origin, 1, &mut stamped)?;
    }
    for edit in &edits {
        if !replacement.iter_mut().any(|node| apply_override(node, edit)) {
            return Err(invalid("An edited R plot text label could not be matched; the original artwork was kept"));
        }
    }
    let edited_bounds = content_bounds(replacement)?;
    if [edited_bounds.x0 - new_bounds.x0, edited_bounds.y0 - new_bounds.y0, edited_bounds.x1 - new_bounds.x1, edited_bounds.y1 - new_bounds.y1]
        .into_iter()
        .any(|delta| delta.abs() > 1e-4)
    {
        return Err(invalid("Edited R plot text would change the new panel bounds; the original artwork was kept"));
    }
    for node in replacement.iter_mut() {
        refresh_child_structures(node);
    }
    Ok((new_count, edits.len()))
}

pub(super) fn set_root_count(group: &mut Node, count: usize) -> Result<()> {
    let group_origin = origin(group)?;
    let bounds = group.visual_bounds().ok_or_else(|| invalid("R plot group has no visible bounds"))?;
    set_data(group, BASELINE, fingerprint(group, group_origin, Affine::IDENTITY)?);
    if let Some(structure) = child_structure(group) {
        set_data(group, STRUCTURE, structure);
    }
    set_data(group, COUNT, count.to_string());
    set_data(group, SIZE, format!("{:.9},{:.9}", bounds.width(), bounds.height()));
    if let Some(geometry) = group.geometric_bounds() {
        set_data(group, GEOMETRY, format!("{:.9},{:.9}", geometry.width(), geometry.height()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vectorcraft_doc::{NodeId, NodeKind};

    use crate::Session;

    fn group_with_label(session: &mut Session, color: &str) -> (u64, u64, u64) {
        let shape = session.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 180, "height": 100})).unwrap()["id"].as_u64().unwrap();
        session.execute("paint.setFill", &json!({"ids": [shape], "color": color})).unwrap();
        let text =
            session.execute("text.create", &json!({"x": 30, "y": 35, "text": "wt", "size": 10, "color": "#222222"})).unwrap()["id"].as_u64().unwrap();
        let group = session.execute("object.group", &json!({"ids": [shape, text]})).unwrap()["id"].as_u64().unwrap();
        (group, shape, text)
    }

    #[test]
    fn edited_unique_text_survives_refresh_with_one_undo_step() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 500, "height": 400})).unwrap();
        let (target, _shape, label) = group_with_label(&mut session, "#eeeeee");
        session.execute("object.markRGenerated", &json!({"group": target})).unwrap();
        session.execute("text.setText", &json!({"ids": [label], "text": "Weight (kg)"})).unwrap();
        session.execute("text.setStyle", &json!({"ids": [label], "size": 15, "fill": "#cc2233"})).unwrap();
        session.execute("object.move", &json!({"ids": [label], "dx": 4, "dy": 3})).unwrap();
        let before = session.doc().unwrap().doc.node(NodeId(label)).unwrap().clone();
        let old_group_bounds = session.doc().unwrap().doc.bounds_of(&[NodeId(target)], false).unwrap();
        let (source, source_shape, _) = group_with_label(&mut session, "#aabbcc");
        let new_appearance = session.doc().unwrap().doc.node(NodeId(source_shape)).unwrap().appearance.clone();
        let replaced =
            session.execute("object.replaceGroupContents", &json!({"target": target, "source": source, "preserveRUserEdits": true})).unwrap();
        assert_eq!(replaced["preservedText"], 1);
        let doc = &session.doc().unwrap().doc;
        assert_eq!(doc.node(NodeId(label)), Some(&before));
        assert!(doc.node(NodeId(source)).is_none());
        assert_eq!(doc.bounds_of(&[NodeId(target)], false).unwrap(), old_group_bounds);
        let shape = doc.node(NodeId(source_shape)).unwrap();
        assert!(matches!(&shape.kind, NodeKind::Path { .. }));
        assert_eq!(shape.appearance, new_appearance, "new generated paint should replace unedited shape content");
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.doc().unwrap().doc.node(NodeId(source)).is_some());
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.node(NodeId(label)), Some(&before));
    }

    #[test]
    fn unsupported_shape_edit_keeps_original_artwork() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 500, "height": 400})).unwrap();
        let (target, shape, _) = group_with_label(&mut session, "#eeeeee");
        session.execute("object.markRGenerated", &json!({"group": target})).unwrap();
        session.execute("paint.setFill", &json!({"ids": [shape], "color": "#ff6600"})).unwrap();
        let before = session.doc().unwrap().doc.node(NodeId(target)).unwrap().clone();
        let (source, _, _) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute("object.replaceGroupContents", &json!({"target": target, "source": source, "preserveRUserEdits": true}));
        assert!(result.is_err());
        assert_eq!(session.doc().unwrap().doc.node(NodeId(target)), Some(&before));
    }

    #[test]
    fn linked_panel_resize_rebases_generated_art_but_preserves_text_override() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
        let (generated, _shape, label) = group_with_label(&mut session, "#eeeeee");
        let panel = session.execute("object.group", &json!({"ids": [generated]})).unwrap()["id"].as_u64().unwrap();
        session.execute("object.setName", &json!({"id": panel, "name": "RVS_PANEL|A|plot_A"})).unwrap();
        session.execute("object.setName", &json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        session.execute("object.markRGenerated", &json!({"group": generated})).unwrap();
        session.execute("text.setText", &json!({"ids": [label], "text": "Weight (kg)"})).unwrap();
        session.execute("text.setStyle", &json!({"ids": [label], "size": 15, "fill": "#cc2233"})).unwrap();
        session.execute("object.scale", &json!({"ids": [panel], "sx": 150, "sy": 160})).unwrap();
        let edited = session.doc().unwrap().doc.node(NodeId(label)).unwrap().clone();
        let frame = session.doc().unwrap().doc.node(NodeId(generated)).unwrap().visual_bounds().unwrap();
        let (source, new_shape, _) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute(
            "object.resizeRPanel",
            &json!({"panel": panel, "target": generated, "source": source, "width": frame.width(), "height": frame.height(), "panelName": "RVS_PANEL|A|plot_A", "preserveRUserEdits": true}),
        );
        assert!(result.is_ok(), "whole-panel resize must allow reflow: {result:?}");
        assert_eq!(result.unwrap()["preservedText"], 1);
        let doc = &session.doc().unwrap().doc;
        assert_eq!(doc.node(NodeId(label)), Some(&edited));
        assert!(doc.node(NodeId(new_shape)).is_some());
        assert_eq!(doc.node(NodeId(panel)).and_then(|node| node.name.as_deref()), Some("RVS_PANEL|A|plot_A"));
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.doc().unwrap().doc.node(NodeId(source)).is_some());
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.node(NodeId(label)), Some(&edited));
    }

    #[test]
    fn linked_panel_reflow_keeps_top_left_anchor() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
        let (generated, _, _) = group_with_label(&mut session, "#eeeeee");
        let panel = session.execute("object.group", &json!({"ids": [generated]})).unwrap()["id"].as_u64().unwrap();
        session.execute("object.markRGenerated", &json!({"group": generated})).unwrap();
        let before = session.doc().unwrap().doc.bounds_of(&[NodeId(panel)], false).unwrap();
        let (source, _, _) = group_with_label(&mut session, "#aabbcc");
        session
            .execute(
                "object.resizeRPanel",
                &json!({"panel": panel, "target": generated, "source": source, "width": 283.464566929,
                    "height": 226.771653543, "panelName": "RVS_PANEL|A|plot_A", "preserveRUserEdits": true}),
            )
            .unwrap();
        let after = session.doc().unwrap().doc.bounds_of(&[NodeId(panel)], false).unwrap();
        assert!((after.x0 - before.x0).abs() < 1e-6);
        assert!((after.y0 - before.y0).abs() < 1e-6);
        assert!((after.width() - 283.464566929).abs() < 1e-6);
        assert!((after.height() - 226.771653543).abs() < 1e-6);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.bounds_of(&[NodeId(panel)], false), Some(before));
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.bounds_of(&[NodeId(panel)], false), Some(after));
    }

    #[test]
    fn linked_panel_resize_does_not_rebase_real_shape_edit() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
        let (generated, shape, _) = group_with_label(&mut session, "#eeeeee");
        let panel = session.execute("object.group", &json!({"ids": [generated]})).unwrap()["id"].as_u64().unwrap();
        session.execute("object.setName", &json!({"id": panel, "name": "RVS_PANEL|A|plot_A"})).unwrap();
        session.execute("object.setName", &json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        session.execute("object.markRGenerated", &json!({"group": generated})).unwrap();
        session.execute("paint.setFill", &json!({"ids": [shape], "color": "#ff6600"})).unwrap();
        session.execute("object.scale", &json!({"ids": [panel], "sx": 150, "sy": 160})).unwrap();
        let before = session.doc().unwrap().doc.node(NodeId(generated)).unwrap().clone();
        let frame = before.visual_bounds().unwrap();
        let (source, _, _) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute(
            "object.resizeRPanel",
            &json!({"panel": panel, "target": generated, "source": source, "width": frame.width(), "height": frame.height(), "panelName": "RVS_PANEL|A|plot_A", "preserveRUserEdits": true}),
        );
        assert!(result.is_err());
        assert_eq!(session.doc().unwrap().doc.node(NodeId(generated)), Some(&before));
    }

    #[test]
    fn edit_baseline_survives_native_project_save_and_open() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 500, "height": 400})).unwrap();
        let (target, _, label) = group_with_label(&mut session, "#eeeeee");
        session.execute("object.markRGenerated", &json!({"group": target})).unwrap();
        let saved = vectorcraft_format::save(&session.doc().unwrap().doc, false);
        let reopened = vectorcraft_format::load(&saved).unwrap();
        let group = reopened.node(NodeId(target)).unwrap();
        assert!(super::data(group, super::BASELINE).is_some());
        assert!(super::data(reopened.node(NodeId(label)).unwrap(), super::KEY).is_some());
    }

    #[test]
    fn untracked_legacy_group_is_not_silently_replaced() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 500, "height": 400})).unwrap();
        let (target, _, _) = group_with_label(&mut session, "#eeeeee");
        let before = session.doc().unwrap().doc.node(NodeId(target)).unwrap().clone();
        let (source, _, _) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute("object.replaceGroupContents", &json!({"target": target, "source": source, "preserveRUserEdits": true}));
        assert!(result.is_err());
        assert_eq!(session.doc().unwrap().doc.node(NodeId(target)), Some(&before));
    }

    #[test]
    fn canvas_resized_group_can_reflow_from_new_svg() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
        let (generated, _, _) = group_with_label(&mut session, "#eeeeee");
        let outer_label = session.execute("text.create", &json!({"x": 10, "y": 0, "text": "A"})).unwrap()["id"].as_u64().unwrap();
        let panel = session.execute("object.group", &json!({"ids": [generated, outer_label]})).unwrap()["id"].as_u64().unwrap();
        session.execute("object.markRGenerated", &json!({"group": generated})).unwrap();
        session
            .edit("Simulate existing tracked plot", |doc, _| {
                doc.node_mut(NodeId(generated)).unwrap().edit_attrs(|attrs| attrs.data.retain(|(key, _)| key != super::GEOMETRY));
                Ok(())
            })
            .unwrap();
        session.execute("object.scale", &json!({"ids": [generated], "sx": 200, "sy": 150})).unwrap();
        let resized = session.doc().unwrap().doc.bounds_of(&[NodeId(generated)], false).unwrap();
        let (source, new_shape, first_label) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute(
            "object.resizeRPanel",
            &json!({"panel": panel, "target": generated, "source": source, "width": resized.width(), "height": resized.height(), "panelName": "R plot", "preserveRUserEdits": true}),
        );
        assert!(result.is_ok(), "canvas resize should not block R reflow: {result:?}");
        let doc = &session.doc().unwrap().doc;
        assert!(doc.node(NodeId(new_shape)).is_some(), "new SVG geometry must replace scaled old art");
        assert!(doc.node(NodeId(outer_label)).is_some());
        session.execute("text.setText", &json!({"ids": [first_label], "text": "Weight"})).unwrap();
        session.execute("text.setStyle", &json!({"ids": [first_label], "size": 15, "fill": "#cc2233"})).unwrap();
        session.execute("object.move", &json!({"ids": [first_label], "dx": 5, "dy": 4})).unwrap();
        session.execute("object.scale", &json!({"ids": [generated], "sx": 120, "sy": 120})).unwrap();
        let twice_resized = session.doc().unwrap().doc.bounds_of(&[NodeId(generated)], false).unwrap();
        let edited_label = session.doc().unwrap().doc.node(NodeId(first_label)).unwrap().clone();
        let (second_source, second_shape, _) = group_with_label(&mut session, "#44aa66");
        let second = session.execute(
            "object.resizeRPanel",
            &json!({"panel": panel, "target": generated, "source": second_source, "width": twice_resized.width(), "height": twice_resized.height(), "panelName": "R plot 2", "preserveRUserEdits": true}),
        );
        assert!(second.is_ok(), "edited text should survive a second reflow: {second:?}");
        assert_eq!(second.unwrap()["preservedText"], 1);
        let doc = &session.doc().unwrap().doc;
        assert_eq!(doc.node(NodeId(first_label)), Some(&edited_label));
        assert!(doc.node(NodeId(second_shape)).is_some());
        session.execute("edit.undo", &json!({})).unwrap();
        assert!(session.doc().unwrap().doc.node(NodeId(second_source)).is_some());
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.doc().unwrap().doc.node(NodeId(first_label)), Some(&edited_label));
    }

    #[test]
    fn canvas_resize_does_not_discard_unmatched_shape_style() {
        let mut session = Session::new();
        session.execute("file.new", &json!({"width": 800, "height": 600})).unwrap();
        let (generated, shape, _) = group_with_label(&mut session, "#eeeeee");
        let outer_label = session.execute("text.create", &json!({"x": 10, "y": 0, "text": "A"})).unwrap()["id"].as_u64().unwrap();
        let panel = session.execute("object.group", &json!({"ids": [generated, outer_label]})).unwrap()["id"].as_u64().unwrap();
        session.execute("object.markRGenerated", &json!({"group": generated})).unwrap();
        session.execute("paint.setFill", &json!({"ids": [shape], "color": "#ff6600"})).unwrap();
        session.execute("object.scale", &json!({"ids": [generated], "sx": 150, "sy": 160})).unwrap();
        let old_art = session.doc().unwrap().doc.node(NodeId(generated)).unwrap().clone();
        let frame = old_art.visual_bounds().unwrap();
        let (source, _, _) = group_with_label(&mut session, "#aabbcc");
        let result = session.execute(
            "object.resizeRPanel",
            &json!({"panel": panel, "target": generated, "source": source, "width": frame.width(), "height": frame.height(), "panelName": "R plot", "preserveRUserEdits": true}),
        );
        assert!(result.is_err());
        assert_eq!(session.doc().unwrap().doc.node(NodeId(generated)), Some(&old_art));
    }
}
