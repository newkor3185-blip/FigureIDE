//! Project management for FigureIDE folder-based workspaces and .fide figure documents.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vectorcraft_doc::Document;
use vectorcraft_format::fide::{
    FIDE_EXTENSION, FIDE_PROJECT_FORMAT, FIDE_PROJECT_VERSION, FideFigure, FideFigureMeta, FideLinkedPlot, FideLinkedPlotMeta, FideProject,
    FideProjectFolder, FideScript, FideScriptMeta, FideWorkspaceLayout, ProjectMetadata, current_iso_time, is_legacy_fide_archive,
    load_fide_figure_from_bytes, load_project_folder, migrate_legacy_fide_archive_to_folder, save_fide_figure, save_project_folder,
};

use super::{CentralTabId, ScriptDocument};
use crate::VectorcraftApp;

/// Inspect the workspace and build a serializable `FideProject`.
#[allow(dead_code)]
pub(crate) fn build_fide_project(app: &VectorcraftApp) -> Result<FideProject, String> {
    let mut project = FideProject::default();

    // 1. Figures:
    let docs = app.session.documents();
    if docs.is_empty() {
        project.figures.push(FideFigure {
            id: "figure_001".to_string(),
            name: "Figure 1".to_string(),
            doc: Document::new(210.0 * 72.0 / 25.4, 297.0 * 72.0 / 25.4),
        });
    } else {
        for (idx, st) in docs.iter().enumerate() {
            let name = if st.doc.title.is_empty() { format!("Figure {}", idx + 1) } else { st.doc.title.clone() };
            project.figures.push(FideFigure { id: format!("figure_{:03}", idx + 1), name, doc: (*st.doc).clone() });
        }
    }

    // 2. Scripts:
    for s in &app.scientific.scripts {
        let name = if let Some(ref p) = s.path {
            Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(&s.untitled_name).to_string()
        } else {
            s.untitled_name.clone()
        };
        project.scripts.push(FideScript { id: s.id, name, text: s.text.clone(), external_path: s.path.clone(), is_dirty: s.text != s.saved_text });
    }

    // 3. Linked plots:
    let active_doc = app.session.doc().ok();
    for panel in &app.scientific.linked_panels {
        let mut svg_string = None;
        let mut preview_png = None;

        if let Some(st) = active_doc
            && let Some(node) = st.doc.node(vectorcraft_doc::NodeId(panel.generated_id))
            && let Some(children) = node.children()
        {
            // Find placed document SVG key from node hierarchy.
            for child in children {
                if let vectorcraft_doc::NodeKind::PlacedDocument(ref placed) = child.kind
                    && placed.key.starts_with("rvs-svg-")
                    && let Some(blob) = st.doc.images.get(&placed.key)
                {
                    svg_string = String::from_utf8(blob.bytes.to_vec()).ok();
                    if let Some(ref proxy) = blob.proxy {
                        preview_png = Some((**proxy).clone());
                    }
                }
            }
        }

        let has_svg = svg_string.is_some();
        let has_preview = preview_png.is_some();
        let graph_style_json = serde_json::to_string(&panel.graph_style).ok();

        project.linked_plots.push(FideLinkedPlot {
            meta: FideLinkedPlotMeta {
                panel_id: panel.panel_id,
                generated_id: panel.generated_id,
                r_variable: panel.r_variable.clone(),
                label: panel.label.clone(),
                width_mm: panel.width_mm,
                height_mm: panel.height_mm,
                resize_mode: panel.resize_mode.as_str().to_string(),
                small_figure_typography: panel.small_figure_typography,
                svg_file: if has_svg { Some(format!("linked_plots/panel_{:03}.svg", panel.panel_id)) } else { None },
                preview_file: if has_preview { Some(format!("previews/panel_{:03}.png", panel.panel_id)) } else { None },
                graph_style_json,
            },
            svg: svg_string,
            preview_png,
        });
    }

    // 4. Workspace layout:
    let tab_order = app
        .scientific
        .tab_order
        .iter()
        .map(|tab| match tab {
            CentralTabId::Script(id) => format!("script:{id}"),
            CentralTabId::DataViewer(name) => format!("viewer:{name}"),
        })
        .collect();

    let active_tab = if let Some(ref viewer) = app.scientific.active_viewer_tab {
        Some(format!("viewer:{viewer}"))
    } else {
        Some(format!("script:{}", app.scientific.active_script_id))
    };

    project.manifest.rdata_reference = app.scientific.project_rdata_reference.clone();
    project.layout = FideWorkspaceLayout {
        tab_order,
        active_tab,
        active_script_id: app.scientific.active_script_id,
        editor_fraction: app.ui.r_studio_editor_fraction,
        editor_maximized: app.ui.r_studio_editor_maximized,
        console_history: app.scientific.console_history.clone(),
        working_directory: app.scientific.working_directory.as_ref().map(|p| p.to_string_lossy().into_owned()),
        environment_filter: if app.scientific.environment_filter.is_empty() { None } else { Some(app.scientific.environment_filter.clone()) },
        rdata_reference: app.scientific.project_rdata_reference.clone(),
    };

    Ok(project)
}

/// Apply a restored `FideProject` to the application and scientific workspace.
pub(crate) fn apply_fide_project(app: &mut VectorcraftApp, project: FideProject, path: Option<String>) -> Result<(), String> {
    // Close existing session documents before restoring project figures:
    while !app.session.documents().is_empty() {
        app.session.close_document(0);
    }

    // 1. Figures:
    let mut figure_iter = project.figures.into_iter();
    if let Some(primary) = figure_iter.next() {
        app.session.add_document(primary.doc, path.clone());
        app.sync_views();
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }
        for extra in figure_iter {
            app.session.add_document(extra.doc, None);
        }
        if !app.session.documents().is_empty() {
            app.session.set_active(0);
        }
    } else {
        app.run(
            "file.new",
            json!({
                "width": "210 mm",
                "height": "297 mm",
                "units": "Millimeters",
                "backgroundContents": "white"
            }),
        )?;
    }

    // 2. Scripts:
    app.scientific.scripts.clear();
    let mut max_id = 0u64;
    for script in project.scripts {
        if script.id > max_id {
            max_id = script.id;
        }
        let doc = ScriptDocument::new(script.id, script.text, script.external_path, None);
        app.scientific.scripts.push(doc);
    }
    if app.scientific.scripts.is_empty() {
        app.scientific.new_script();
    } else {
        app.scientific.next_script_id = max_id.saturating_add(1);
    }

    // 3. Restore linked panels:
    super::restore_linked_panels(app);
    app.scientific.panel_states.clear();
    app.scientific.project_rdata_reference = project.manifest.rdata_reference.or(project.layout.rdata_reference);

    // 4. Restore workspace layout:
    app.ui.r_studio_editor_fraction = project.layout.editor_fraction.clamp(0.25, 0.85);
    app.ui.r_studio_editor_maximized = project.layout.editor_maximized;
    app.scientific.console_history = project.layout.console_history;
    if let Some(wd) = project.layout.working_directory {
        app.scientific.working_directory = Some(PathBuf::from(wd));
    }
    if let Some(env_filt) = project.layout.environment_filter {
        app.scientific.environment_filter = env_filt;
    }

    // Restore central tabs:
    if !project.layout.tab_order.is_empty() {
        app.scientific.tab_order = project
            .layout
            .tab_order
            .into_iter()
            .filter_map(|t| {
                if let Some(id_str) = t.strip_prefix("script:") {
                    id_str.parse::<u64>().ok().map(CentralTabId::Script)
                } else {
                    t.strip_prefix("viewer:").map(|name| CentralTabId::DataViewer(name.to_string()))
                }
            })
            .collect();
    }
    app.scientific.viewer_tabs.clear();
    for tab in &app.scientific.tab_order {
        if let CentralTabId::DataViewer(name) = tab
            && !app.scientific.viewer_tabs.iter().any(|v| &v.name == name)
        {
            app.scientific.viewer_tabs.push(super::DataViewerTab {
                name: name.clone(),
                page_start: 0,
                page_size: 50,
                page: None,
                error: None,
                filter: String::new(),
                sort_column: None,
                sort_ascending: true,
                loading: false,
            });
        }
    }
    if let Some(active) = project.layout.active_tab {
        if let Some(name) = active.strip_prefix("viewer:") {
            app.scientific.active_viewer_tab = Some(name.to_string());
        } else if let Some(id_str) = active.strip_prefix("script:")
            && let Ok(id) = id_str.parse::<u64>()
        {
            app.scientific.active_script_id = id;
            app.scientific.active_viewer_tab = None;
        }
    } else if let Some(first) = app.scientific.scripts.first() {
        app.scientific.active_script_id = first.id;
        app.scientific.active_viewer_tab = None;
    }

    // 5. Update state
    app.scientific.project_path = path;
    app.scientific.project_dirty = false;
    app.scientific.enabled = true;
    app.status("FigureIDE project loaded successfully");
    Ok(())
}

/// Inspect the workspace and build a serializable `FideProjectFolder`.
pub(crate) fn build_fide_project_folder(app: &VectorcraftApp, name: &str, folder_path: &Path) -> Result<FideProjectFolder, String> {
    let mut scripts = Vec::new();
    let mut script_metas = Vec::new();
    for s in &app.scientific.scripts {
        let name = if let Some(ref p) = s.path {
            Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(&s.untitled_name).to_string()
        } else {
            s.untitled_name.clone()
        };
        let sname = if name.ends_with(".R") || name.ends_with(".r") { name.clone() } else { format!("{name}.R") };
        let file_rel = format!("scripts/{sname}");
        let is_dirty = s.text != s.saved_text;
        script_metas.push(FideScriptMeta { id: s.id, name: name.clone(), file: file_rel, external_path: s.path.clone(), is_dirty });
        scripts.push(FideScript { id: s.id, name, text: s.text.clone(), external_path: s.path.clone(), is_dirty });
    }

    let mut figures = Vec::new();
    let mut figure_metas = Vec::new();
    let docs = app.session.documents();
    if docs.is_empty() {
        let doc = Document::new(210.0 * 72.0 / 25.4, 297.0 * 72.0 / 25.4);
        figures.push(FideFigure { id: "fig_1".to_string(), name: "Figure 1".to_string(), doc });
        figure_metas.push(FideFigureMeta {
            id: "fig_1".to_string(),
            name: "Figure 1".to_string(),
            file: "figures/Figure_1.fide".to_string(),
            artboard_count: 1,
        });
    } else {
        for (idx, st) in docs.iter().enumerate() {
            let name = if st.doc.title.is_empty() { format!("Figure {}", idx + 1) } else { st.doc.title.clone() };
            let stem = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>();
            let stem = if stem.is_empty() { "Figure".to_string() } else { stem };
            let file_rel = project_figure_file(folder_path, st)
                .unwrap_or_else(|| if docs.len() > 1 { format!("figures/{stem}_{:02}.fide", idx + 1) } else { format!("figures/{stem}.fide") });
            figure_metas.push(FideFigureMeta {
                id: format!("fig_{}", idx + 1),
                name: name.clone(),
                file: file_rel,
                artboard_count: st.doc.artboards.len(),
            });
            figures.push(FideFigure { id: format!("fig_{}", idx + 1), name, doc: (*st.doc).clone() });
        }
    }

    let tab_order = app
        .scientific
        .tab_order
        .iter()
        .map(|tab| match tab {
            CentralTabId::Script(id) => format!("script:{id}"),
            CentralTabId::DataViewer(name) => format!("viewer:{name}"),
        })
        .collect();

    let active_tab = if let Some(ref viewer) = app.scientific.active_viewer_tab {
        Some(format!("viewer:{viewer}"))
    } else {
        Some(format!("script:{}", app.scientific.active_script_id))
    };

    let ws_file = if let Some(ref rdata_ref) = app.scientific.project_rdata_reference {
        if let Some(ref p_path) = app.scientific.project_path {
            if let Ok(rel) = Path::new(rdata_ref).strip_prefix(p_path) {
                Some(rel.to_string_lossy().replace('\\', "/"))
            } else {
                Some(rdata_ref.clone())
            }
        } else {
            Some(rdata_ref.clone())
        }
    } else if let Some(ref p_path) = app.scientific.project_path {
        let def = Path::new(p_path).join("workspace").join("workspace.RData");
        if def.exists() { Some("workspace/workspace.RData".into()) } else { None }
    } else {
        None
    };

    let layout = FideWorkspaceLayout {
        tab_order,
        active_tab,
        active_script_id: app.scientific.active_script_id,
        editor_fraction: app.ui.r_studio_editor_fraction,
        editor_maximized: app.ui.r_studio_editor_maximized,
        console_history: app.scientific.console_history.clone(),
        working_directory: app.scientific.working_directory.as_ref().map(|p| p.to_string_lossy().into_owned()),
        environment_filter: if app.scientific.environment_filter.is_empty() { None } else { Some(app.scientific.environment_filter.clone()) },
        rdata_reference: ws_file.clone(),
    };

    let active_figure = figure_metas.first().map(|f| f.file.clone());
    let metadata = ProjectMetadata {
        format: FIDE_PROJECT_FORMAT.into(),
        version: FIDE_PROJECT_VERSION,
        name: name.to_string(),
        generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
        created_at: current_iso_time(),
        updated_at: current_iso_time(),
        scripts: script_metas,
        figures: figure_metas,
        active_figure,
        workspace_file: ws_file,
        save_r_workspace: app.scientific.save_r_workspace_preference,
        layout,
    };

    Ok(FideProjectFolder { metadata, scripts, figures, assets: BTreeMap::new() })
}

fn project_figure_file(folder_path: &Path, st: &vectorcraft_engine::DocState) -> Option<String> {
    let path = Path::new(st.path.as_deref()?);
    let relative = path.strip_prefix(folder_path).ok()?;
    let mut components = relative.components();
    if components.next()?.as_os_str() != "figures" || !components.all(|c| matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    if !relative.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("fide")) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Apply a restored `FideProjectFolder` to the application and scientific workspace.
pub(crate) fn apply_fide_project_folder(app: &mut VectorcraftApp, project: FideProjectFolder, folder_path: Option<String>) -> Result<(), String> {
    while !app.session.documents().is_empty() {
        app.session.close_document(0);
    }

    // 1. Figures:
    let mut figure_iter = project.figures.into_iter();
    if let Some(primary) = figure_iter.next() {
        let fig_path = folder_path.as_ref().map(|fp| {
            let rel = project.metadata.figures.first().map(|f| f.file.clone()).unwrap_or_else(|| "figures/Figure_1.fide".into());
            Path::new(fp).join(rel).to_string_lossy().into_owned()
        });
        app.session.add_document(primary.doc, fig_path);
        app.sync_views();
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }
        for (idx, extra) in figure_iter.enumerate() {
            let extra_path = folder_path.as_ref().map(|fp| {
                let rel = project.metadata.figures.get(idx + 1).map(|f| f.file.clone()).unwrap_or_else(|| format!("figures/Figure_{}.fide", idx + 2));
                Path::new(fp).join(rel).to_string_lossy().into_owned()
            });
            app.session.add_document(extra.doc, extra_path);
        }
        if !app.session.documents().is_empty() {
            app.session.set_active(0);
        }
    } else {
        app.run(
            "file.new",
            json!({
                "width": "210 mm",
                "height": "297 mm",
                "units": "Millimeters",
                "backgroundContents": "white"
            }),
        )?;
    }

    // 2. Scripts:
    app.scientific.scripts.clear();
    let mut max_id = 0u64;
    for script in project.scripts {
        if script.id > max_id {
            max_id = script.id;
        }
        let doc = ScriptDocument::new(script.id, script.text, script.external_path, None);
        app.scientific.scripts.push(doc);
    }
    if app.scientific.scripts.is_empty() {
        app.scientific.new_script();
    } else {
        app.scientific.next_script_id = max_id.saturating_add(1);
    }

    // 3. Restore linked panels:
    super::restore_linked_panels(app);
    app.scientific.panel_states.clear();

    // 4. Restore workspace reference & layout:
    app.scientific.save_r_workspace_preference = project.metadata.save_r_workspace;
    let ws_ref = project.metadata.workspace_file.or(project.metadata.layout.rdata_reference);
    app.scientific.project_rdata_reference = if let Some(ref rel_or_abs) = ws_ref {
        if let Some(ref fp) = folder_path {
            let normalized = rel_or_abs.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR);
            let p = Path::new(fp).join(normalized);
            if p.exists() { Some(p.to_string_lossy().into_owned()) } else { Some(rel_or_abs.clone()) }
        } else {
            Some(rel_or_abs.clone())
        }
    } else if let Some(ref fp) = folder_path {
        let def_ws = Path::new(fp).join("workspace").join("workspace.RData");
        if def_ws.exists() { Some(def_ws.to_string_lossy().into_owned()) } else { None }
    } else {
        None
    };

    app.ui.r_studio_editor_fraction = project.metadata.layout.editor_fraction.clamp(0.25, 0.85);
    app.ui.r_studio_editor_maximized = project.metadata.layout.editor_maximized;
    app.scientific.console_history = project.metadata.layout.console_history;
    if let Some(wd) = project.metadata.layout.working_directory {
        app.scientific.working_directory = Some(PathBuf::from(wd));
    }
    if let Some(env_filt) = project.metadata.layout.environment_filter {
        app.scientific.environment_filter = env_filt;
    }

    if !project.metadata.layout.tab_order.is_empty() {
        app.scientific.tab_order = project
            .metadata
            .layout
            .tab_order
            .into_iter()
            .filter_map(|t| {
                if let Some(id_str) = t.strip_prefix("script:") {
                    id_str.parse::<u64>().ok().map(CentralTabId::Script)
                } else {
                    t.strip_prefix("viewer:").map(|name| CentralTabId::DataViewer(name.to_string()))
                }
            })
            .collect();
    }
    app.scientific.viewer_tabs.clear();
    for tab in &app.scientific.tab_order {
        if let CentralTabId::DataViewer(name) = tab
            && !app.scientific.viewer_tabs.iter().any(|v| &v.name == name)
        {
            app.scientific.viewer_tabs.push(super::DataViewerTab {
                name: name.clone(),
                page_start: 0,
                page_size: 50,
                page: None,
                error: None,
                filter: String::new(),
                sort_column: None,
                sort_ascending: true,
                loading: false,
            });
        }
    }
    if let Some(active) = project.metadata.layout.active_tab {
        if let Some(name) = active.strip_prefix("viewer:") {
            app.scientific.active_viewer_tab = Some(name.to_string());
        } else if let Some(id_str) = active.strip_prefix("script:")
            && let Ok(id) = id_str.parse::<u64>()
        {
            app.scientific.active_script_id = id;
            app.scientific.active_viewer_tab = None;
        }
    } else if let Some(first) = app.scientific.scripts.first() {
        app.scientific.active_script_id = first.id;
        app.scientific.active_viewer_tab = None;
    }

    // 5. Update state
    app.scientific.project_path = folder_path;
    app.scientific.project_dirty = false;
    app.scientific.enabled = true;
    app.status("FigureIDE project loaded successfully");
    Ok(())
}

/// Atomically saves the current R session workspace (.GlobalEnv) to `<project_dir>/workspace/workspace.RData`.
///
/// Writes to a temporary staging file first (`workspace/workspace.RData.tmp.<pid>`) and verifies it,
/// before atomically replacing the destination file. If R fails or serialization aborts, any existing
/// `workspace.RData` remains completely intact and valid.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_r_workspace_to_project_atomic(app: &mut VectorcraftApp, project_dir: &Path) -> Result<(usize, u64), String> {
    if !app.scientific.session_active {
        return Err("R session is not active — cannot save R workspace".into());
    }

    let ws_dir = project_dir.join("workspace");
    std::fs::create_dir_all(&ws_dir).map_err(|e| format!("Could not create workspace directory {}: {e}", ws_dir.display()))?;

    let final_path = ws_dir.join("workspace.RData");
    let tmp_path = ws_dir.join(format!("workspace.RData.tmp.{}", std::process::id()));

    // Serialize using the persistent R session to the temporary file
    let (count, bytes) = {
        let mut session = app.scientific.session.lock().map_err(|e| format!("R session lock error: {e}"))?;
        match session.save_rdata(&tmp_path.to_string_lossy()) {
            Ok(res) => res,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("Failed to serialize R workspace: {e}"));
            }
        }
    };

    if !tmp_path.exists() {
        return Err("R workspace temporary file was not created".into());
    }

    // Atomically replace final_path with tmp_path using vectorcraft_format::write_atomic_with
    let replace_res = vectorcraft_format::write_atomic_with(&final_path, |f| {
        let mut tmp_file = std::fs::File::open(&tmp_path)?;
        std::io::copy(&mut tmp_file, f)?;
        Ok(())
    });
    let _ = std::fs::remove_file(&tmp_path);

    if let Err(e) = replace_res {
        return Err(format!("Failed to atomically replace R workspace file: {e}"));
    }

    if !final_path.exists() {
        return Err(format!("Failed to verify saved R workspace at {}", final_path.display()));
    }

    app.scientific.session_active = true;
    app.scientific.project_rdata_reference = Some(final_path.to_string_lossy().into_owned());
    Ok((count, bytes))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save_r_workspace_to_project_atomic(_app: &mut VectorcraftApp, _project_dir: &Path) -> Result<(usize, u64), String> {
    Err("Saving an R workspace requires the native desktop R session".into())
}

/// Save the current project to the given path (folder-based).
pub(crate) fn save_project_to_path(app: &mut VectorcraftApp, path: &str, save_r_workspace: Option<bool>) -> Result<(), String> {
    let mut folder_path = PathBuf::from(path);
    // Project = directory, NOT a .fide file. Strip .fide extension if user or dialog supplied it.
    if folder_path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("fide")).unwrap_or(false) {
        folder_path.set_extension("");
    }
    if folder_path.file_name().and_then(|n| n.to_str()) == Some("project.json")
        && let Some(parent) = folder_path.parent().map(Path::to_path_buf)
    {
        folder_path = parent;
    }
    let project_name = folder_path.file_name().and_then(|n| n.to_str()).unwrap_or("FigureIDE_Project");

    if let Some(pref) = save_r_workspace {
        app.scientific.save_r_workspace_preference = Some(pref);
    }
    let should_save_ws = save_r_workspace.or(app.scientific.save_r_workspace_preference).unwrap_or(false);

    let mut ws_info: Option<(usize, u64)> = None;
    if should_save_ws && app.scientific.session_active {
        let info = save_r_workspace_to_project_atomic(app, &folder_path)?;
        ws_info = Some(info);
    }

    let proj = build_fide_project_folder(app, project_name, &folder_path)?;
    save_project_folder(&folder_path, &proj).map_err(|e| e.to_string())?;

    let saved_figure_paths: Vec<String> =
        proj.metadata.figures.iter().map(|figure| folder_path.join(&figure.file).to_string_lossy().into_owned()).collect();
    let saved_figure_uids: Vec<u64> = app.session.documents().iter().map(|st| st.uid).collect();
    for (uid, path) in saved_figure_uids.into_iter().zip(saved_figure_paths) {
        if let Some(st) = app.session.document_mut(uid) {
            st.path = Some(path);
        }
    }

    app.scientific.project_path = Some(folder_path.to_string_lossy().into_owned());
    app.scientific.last_project_parent = folder_path.parent().map(Path::to_path_buf);
    app.scientific.project_dirty = false;
    for s in &mut app.scientific.scripts {
        s.saved_text = s.text.clone();
    }
    let doc_uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    for uid in doc_uids {
        if let Some(doc) = app.session.document_mut(uid) {
            doc.mark_saved();
        }
    }
    if let Some((count, bytes)) = ws_info {
        app.status(format!("Project saved to {} (R workspace: {count} objects, {bytes} bytes)", folder_path.display()));
    } else {
        app.status(format!("Project saved to {}", folder_path.display()));
    }
    Ok(())
}

/// Open a project file from disk (folder, project.json, legacy .fide ZIP, or individual .fide figure).
pub(crate) fn open_project_from_path(app: &mut VectorcraftApp, path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_dir() || p.file_name().and_then(|n| n.to_str()) == Some("project.json") {
        let folder = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
        let proj = load_project_folder(folder).map_err(|e| format!("Could not load project {path}: {e}"))?;
        return apply_fide_project_folder(app, proj, Some(folder.to_string_lossy().into_owned()));
    }

    let bytes = std::fs::read(path).map_err(|e| format!("Could not read project file {path}: {e}"))?;
    if is_legacy_fide_archive(&bytes) {
        let p_path = Path::new(path);
        let mut target_folder = p_path.to_path_buf();
        if target_folder.extension().map(|e| e.eq_ignore_ascii_case("fide")).unwrap_or(false) {
            target_folder.set_extension("");
        }
        if target_folder.exists() && !target_folder.join("project.json").exists() {
            let stem = target_folder.file_name().and_then(|n| n.to_str()).unwrap_or("project");
            target_folder = target_folder.with_file_name(format!("{}_migrated", stem));
        }
        let migrated_folder = migrate_legacy_fide_archive_to_folder(&bytes, &target_folder)
            .map_err(|e| format!("Could not migrate legacy project archive {path}: {e}"))?;
        return apply_fide_project_folder(app, migrated_folder, Some(target_folder.to_string_lossy().into_owned()));
    }

    // Individual .fide figure document
    let doc = load_fide_figure_from_bytes(&bytes).map_err(|e| format!("Could not load figure {path}: {e}"))?;
    let doc_idx = app.session.documents().len();
    app.session.add_document(doc, Some(path.to_string()));
    app.session.set_active(doc_idx);
    app.sync_views();
    if let Some(st) = app.session.active_mut() {
        st.mark_saved();
    }
    super::restore_linked_panels(app);
    app.status(format!("Opened figure {path}"));
    Ok(())
}

/// Open a project from in-memory archive bytes.
pub(crate) fn open_fide_project_bytes(app: &mut VectorcraftApp, bytes: &[u8], path: Option<String>) -> Result<(), String> {
    if is_legacy_fide_archive(bytes) {
        let project = FideProject::load_from_bytes(bytes).map_err(|e| format!("Could not load project: {e}"))?;
        return apply_fide_project(app, project, path);
    }
    // Individual .fide figure document
    let doc = load_fide_figure_from_bytes(bytes).map_err(|e| format!("Could not load figure: {e}"))?;
    let doc_idx = app.session.documents().len();
    app.session.add_document(doc, path.clone());
    app.session.set_active(doc_idx);
    app.sync_views();
    if let Some(st) = app.session.active_mut() {
        st.mark_saved();
    }
    super::restore_linked_panels(app);
    app.status("Opened figure");
    Ok(())
}

/// Command handler for `figure.save`.
pub(crate) fn save_figure_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let active_doc = app.session.active().ok_or_else(|| "No active figure to save".to_string())?;
    let path = if let Some(path_str) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        path_str.to_string()
    } else if let Some(ref path_str) = active_doc.path {
        path_str.clone()
    } else {
        return save_figure_as_command(app, p);
    };

    save_fide_figure(Path::new(&path), &active_doc.doc).map_err(|e| e.to_string())?;
    if let Some(doc) = app.session.active_mut() {
        doc.path = Some(path.clone());
        doc.mark_saved();
    }
    app.status(format!("Figure saved to {path}"));
    Ok(json!({ "path": path }))
}

/// Command handler for `figure.saveAs`.
pub(crate) fn save_figure_as_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    if p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()).is_some() {
        return save_figure_command(app, p);
    }
    let default_name = app
        .session
        .active()
        .map(|d| {
            d.path.as_deref().and_then(|p| Path::new(p).file_name()).and_then(|n| n.to_str()).map(str::to_owned).unwrap_or_else(|| {
                let stem = Path::new(&d.doc.title).file_stem().and_then(|n| n.to_str()).unwrap_or(&d.doc.title);
                format!("{stem}.{FIDE_EXTENSION}")
            })
        })
        .unwrap_or_else(|| format!("Figure.{FIDE_EXTENSION}"));

    let pick = crate::FilePick {
        name: default_name,
        filters: vec![crate::FilePick::filter("FigureIDE Figure (.fide)", &[FIDE_EXTENSION])],
        ..Default::default()
    };
    if let Some(path) = crate::picks::save(app, &pick) {
        let p_with_path = json!({ "path": path });
        return save_figure_command(app, &p_with_path);
    }
    Ok(Value::Null)
}

/// Command handler for `r.workspace.save`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn save_r_workspace_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    if !app.scientific.session_active {
        let err = "R session is not active — cannot save R workspace".to_string();
        app.status(format!("Save R Workspace failed: {err}"));
        return Err(err);
    }

    let path = if let Some(path_str) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        path_str.to_string()
    } else if let Some(ref proj_path) = app.scientific.project_path {
        let ws_dir = Path::new(proj_path).join("workspace");
        let _ = std::fs::create_dir_all(&ws_dir);
        ws_dir.join("workspace.RData").to_string_lossy().into_owned()
    } else {
        let pick = crate::FilePick {
            name: "workspace.RData".to_string(),
            filters: vec![crate::FilePick::filter("R Workspace", &["RData", "rda"])],
            ..Default::default()
        };
        let Some(picked) = crate::picks::save(app, &pick) else {
            return Ok(Value::Null);
        };
        picked
    };

    let target_path = PathBuf::from(&path);
    if let Some(parent) = target_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp_path = target_path.with_extension(format!("RData.tmp.{}", std::process::id()));

    let res = {
        let session = std::sync::Arc::clone(&app.scientific.session);
        let mut session = session.lock().map_err(|e| format!("R session lock error: {e}"))?;
        session.save_rdata(&tmp_path.to_string_lossy())
    };
    let (count, bytes) = match res {
        Ok(res) => res,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            let err = format!("Failed to serialize R workspace: {e}");
            app.status(format!("Save R Workspace failed: {err}"));
            return Err(err);
        }
    };
    if !tmp_path.exists() {
        let err = format!("Failed to create temporary R workspace at {}", tmp_path.display());
        app.status(format!("Save R Workspace failed: {err}"));
        return Err(err);
    }
    let replace_res = vectorcraft_format::write_atomic_with(&target_path, |f| {
        let mut tmp_file = std::fs::File::open(&tmp_path)?;
        std::io::copy(&mut tmp_file, f)?;
        Ok(())
    });
    let _ = std::fs::remove_file(&tmp_path);
    if let Err(e) = replace_res {
        let err = format!("Failed to atomically replace R workspace file: {e}");
        app.status(format!("Save R Workspace failed: {err}"));
        return Err(err);
    }
    if !target_path.exists() {
        let err = format!("Failed to verify saved R workspace at {path}");
        app.status(format!("Save R Workspace failed: {err}"));
        return Err(err);
    }
    app.scientific.session_active = true;
    app.scientific.project_rdata_reference = Some(path.clone());
    if let Some(ref proj_path) = app.scientific.project_path {
        let p_json_path = Path::new(proj_path).join("project.json");
        if p_json_path.exists()
            && let Ok(content) = std::fs::read_to_string(&p_json_path)
            && let Ok(mut meta) = serde_json::from_str::<ProjectMetadata>(&content)
        {
            let rel_path = if let Ok(rel) = Path::new(&path).strip_prefix(proj_path) {
                rel.to_string_lossy().replace('\\', "/")
            } else {
                "workspace/workspace.RData".into()
            };
            meta.workspace_file = Some(rel_path);
            meta.updated_at = current_iso_time();
            if let Ok(updated_bytes) = serde_json::to_vec_pretty(&meta) {
                let _ = vectorcraft_format::write_atomic(&p_json_path, &updated_bytes);
            }
        }
    }
    app.status(format!("R workspace saved to {path} ({count} objects, {bytes} bytes)"));
    Ok(json!({ "path": path, "object_count": count, "size_bytes": bytes }))
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn save_r_workspace_command(_app: &mut VectorcraftApp, _p: &Value) -> Result<Value, String> {
    Err("Saving an R workspace requires the native desktop R session".into())
}

/// Command handler for `r.workspace.load`.
pub(crate) fn load_r_workspace_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let path = if let Some(path_str) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        path_str.to_string()
    } else if let Some(ref rdata_ref) = app.scientific.project_rdata_reference {
        if Path::new(rdata_ref).exists() {
            rdata_ref.clone()
        } else {
            let pick = crate::FilePick { filters: vec![crate::FilePick::filter("R Workspace", &["RData", "rda"])], ..Default::default() };
            let Some(picked) = crate::picks::open(app, &pick) else {
                return Ok(Value::Null);
            };
            picked
        }
    } else {
        let pick = crate::FilePick { filters: vec![crate::FilePick::filter("R Workspace", &["RData", "rda"])], ..Default::default() };
        let Some(picked) = crate::picks::open(app, &pick) else {
            return Ok(Value::Null);
        };
        picked
    };

    // Inspect the RData file first for collisions
    let inspection = {
        let mut session = app.scientific.session.lock().map_err(|e| format!("R session lock error: {e}"))?;
        session.inspect_rdata(&path)?
    };
    if !inspection.collisions.is_empty() {
        app.scientific.pending_rdata_import = Some(inspection);
        app.status(format!("Importing R workspace from {path}: name collision detected. Choose Overwrite or Skip."));
        return Ok(json!({ "status": "pending_collision_resolution", "path": path }));
    }

    app.scientific.project_rdata_reference = Some(path.clone());
    super::apply_rdata_import_async(app, path.clone(), crate::r_session::ImportCollisionPolicy::Overwrite);
    app.status(format!("Loading R workspace from {path}..."));
    Ok(json!({ "status": "loading", "path": path }))
}

/// Check if the current session consists only of standalone figure documents opened directly,
/// without a project workspace folder or script edits.
pub(crate) fn is_standalone_figure_session(app: &VectorcraftApp) -> bool {
    if !app.scientific.enabled {
        return false;
    }
    if app.scientific.project_path.is_some() {
        return false;
    }
    if app.scientific.project_dirty {
        return false;
    }
    let scripts_untouched = app.scientific.scripts.is_empty()
        || (app.scientific.scripts.len() == 1
            && app.scientific.scripts[0].path.is_none()
            && app.scientific.scripts[0].text == app.scientific.scripts[0].saved_text
            && app.scientific.scripts[0].text.trim().is_empty());
    if !scripts_untouched {
        return false;
    }
    let docs = app.session.documents();
    !docs.is_empty() && docs.iter().all(|d| d.path.is_some())
}

/// Check if the project has unsaved edits across scripts or documents.
pub(crate) fn is_project_dirty(app: &VectorcraftApp) -> bool {
    if app.scientific.project_dirty {
        return true;
    }
    if app.scientific.scripts.iter().any(|s| s.text != s.saved_text) {
        return true;
    }
    if app.session.documents().iter().any(|d| d.is_dirty()) {
        return true;
    }
    if !app.scientific.graph_drafts.is_empty() || !app.scientific.graph_pending.is_empty() {
        return true;
    }
    false
}

/// Resolve a sensible default parent directory for new/saved projects.
pub(crate) fn default_project_parent(app: &VectorcraftApp) -> PathBuf {
    if let Some(ref parent) = app.scientific.last_project_parent
        && parent.exists()
        && parent.is_dir()
    {
        return parent.clone();
    }
    if let Some(ref proj_path) = app.scientific.project_path
        && let Some(parent) = Path::new(proj_path).parent()
        && parent.exists()
        && parent.is_dir()
    {
        return parent.to_path_buf();
    }
    if let Some(ref wd) = app.scientific.working_directory
        && wd.exists()
        && wd.is_dir()
    {
        return wd.clone();
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(user_profile) = std::env::var_os("USERPROFILE") {
            let docs = PathBuf::from(user_profile).join("Documents");
            if docs.exists() && docs.is_dir() {
                return docs;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            let docs = home.join("Documents");
            if docs.exists() && docs.is_dir() {
                return docs;
            }
            return home;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Opens the Create Project / Save Project dialog.
pub(crate) fn open_create_project_dialog(app: &mut VectorcraftApp, mode: &str) {
    let parent = default_project_parent(app);
    let default_name = match mode {
        "saveAs" | "save" | "saveAndExit" => {
            if let Some(ref p) = app.scientific.project_path {
                let stem = Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or("UntitledProject");
                if mode == "saveAs" {
                    let candidate = format!("{stem}_Copy");
                    if !parent.join(&candidate).exists() {
                        candidate
                    } else {
                        let mut i = 1;
                        loop {
                            let c = format!("{stem}_Copy_{i}");
                            if !parent.join(&c).exists() {
                                break c;
                            }
                            i += 1;
                        }
                    }
                } else {
                    stem.to_string()
                }
            } else if let Some(doc) = app.session.active() {
                let stem = vectorcraft_format::fide::sanitize_project_stem(&doc.title());
                if stem.is_empty() { "UntitledProject".to_string() } else { stem }
            } else {
                "UntitledProject".to_string()
            }
        }
        _ => {
            if !parent.join("UntitledProject").exists() {
                "UntitledProject".to_string()
            } else {
                let mut i = 1;
                loop {
                    let c = format!("UntitledProject_{i}");
                    if !parent.join(&c).exists() {
                        break c;
                    }
                    i += 1;
                }
            }
        }
    };

    let default_save_ws = app.scientific.save_r_workspace_preference.unwrap_or(app.scientific.session_active);
    let fields = json!({
        "name": default_name,
        "location": parent.to_string_lossy().into_owned(),
        "mode": mode,
        "save_r_workspace": default_save_ws,
    });
    app.ui.dialog = Some(crate::state::Dialog::new(crate::dialogs::create_project::KIND, fields));
}

/// Creates a new project directory or saves into an existing one with full validation and recovery.
pub(crate) fn create_or_save_project(
    app: &mut VectorcraftApp,
    target_dir: &Path,
    name: &str,
    mode: &str,
    save_r_workspace: Option<bool>,
) -> Result<(), String> {
    vectorcraft_format::fide::validate_project_name(name)?;
    let parent = target_dir.parent().ok_or_else(|| "Target directory has no parent.".to_string())?;
    vectorcraft_format::fide::validate_parent_location(parent)?;

    let was_new_dir = !target_dir.exists();
    if was_new_dir {
        std::fs::create_dir_all(target_dir).map_err(|e| format!("Could not create directory {}: {e}", target_dir.display()))?;
    }
    std::fs::create_dir_all(target_dir.join("scripts")).map_err(|e| format!("Could not create scripts directory: {e}"))?;
    std::fs::create_dir_all(target_dir.join("figures")).map_err(|e| format!("Could not create figures directory: {e}"))?;
    std::fs::create_dir_all(target_dir.join("workspace")).map_err(|e| format!("Could not create workspace directory: {e}"))?;
    std::fs::create_dir_all(target_dir.join("assets")).map_err(|e| format!("Could not create assets directory: {e}"))?;

    if mode == "new" {
        // Reset workspace for new project
        app.scientific.scripts.clear();
        app.scientific.new_script();
        app.scientific.linked_panels.clear();
        while !app.session.documents().is_empty() {
            app.session.close_document(0);
        }
        app.run(
            "file.new",
            json!({
                "width": "210 mm",
                "height": "297 mm",
                "units": "Millimeters",
                "backgroundContents": "white"
            }),
        )?;
    }

    if let Some(pref) = save_r_workspace {
        app.scientific.save_r_workspace_preference = Some(pref);
    }
    let should_save_ws = save_r_workspace.or(app.scientific.save_r_workspace_preference).unwrap_or(false);

    let mut ws_info: Option<(usize, u64)> = None;
    if should_save_ws && app.scientific.session_active {
        match save_r_workspace_to_project_atomic(app, target_dir) {
            Ok(info) => ws_info = Some(info),
            Err(e) => {
                if was_new_dir {
                    let _ = std::fs::remove_dir_all(target_dir);
                }
                return Err(e);
            }
        }
    }

    let prev_project_path = app.scientific.project_path.clone();
    app.scientific.project_path = Some(target_dir.to_string_lossy().into_owned());
    let proj = build_fide_project_folder(app, name, target_dir)?;
    if let Err(e) = save_project_folder(target_dir, &proj) {
        app.scientific.project_path = prev_project_path;
        if was_new_dir {
            let _ = std::fs::remove_dir_all(target_dir);
        }
        return Err(format!("Failed to save project files: {e}"));
    }

    app.scientific.last_project_parent = Some(parent.to_path_buf());
    app.scientific.project_dirty = false;
    for s in &mut app.scientific.scripts {
        s.saved_text = s.text.clone();
    }
    let doc_uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
    for uid in doc_uids {
        if let Some(doc) = app.session.document_mut(uid) {
            doc.mark_saved();
        }
    }
    let action_verb = if mode == "new" { "created at" } else { "saved to" };
    if let Some((count, bytes)) = ws_info {
        app.status(format!("Project {action_verb} {} (R workspace: {count} objects, {bytes} bytes)", target_dir.display()));
    } else {
        app.status(format!("Project {action_verb} {}", target_dir.display()));
    }
    Ok(())
}

/// Command handler for `project.new`.
pub(crate) fn new_project_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let save_ws = p.get("save_r_workspace").and_then(Value::as_bool);
    if let Some(path) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        let p_path = PathBuf::from(path);
        let name = p_path.file_name().and_then(|n| n.to_str()).unwrap_or("UntitledProject");
        if let Err(e) = create_or_save_project(app, &p_path, name, "new", save_ws) {
            app.status(format!("New Project failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }
    if let (Some(name), Some(loc)) =
        (p.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()), p.get("location").and_then(Value::as_str).filter(|s| !s.is_empty()))
    {
        let target_dir = Path::new(loc).join(name);
        if let Err(e) = create_or_save_project(app, &target_dir, name, "new", save_ws) {
            app.status(format!("New Project failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }

    let force = p.get("force").and_then(Value::as_bool).unwrap_or(false);
    if !force && is_project_dirty(app) {
        app.ui.dialog = Some(crate::state::Dialog::new(crate::dialogs::confirm_new_project::KIND, json!({})));
        return Ok(json!({ "pending": crate::dialogs::confirm_new_project::KIND }));
    }

    open_create_project_dialog(app, "new");
    Ok(json!({ "pending": crate::dialogs::create_project::KIND }))
}

/// Command handler for `project.open`.
pub(crate) fn open_project_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    if let Some(path) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        open_project_from_path(app, path)?;
        return Ok(Value::Null);
    }
    // Try folder picker first for native folder-based projects
    if let Some(folder_path) = crate::picks::folder(app) {
        open_project_from_path(app, &folder_path)?;
        return Ok(Value::Null);
    }
    // Fallback if folder picker was dismissed or for selecting project.json / legacy archive
    let pick = crate::FilePick {
        filters: vec![
            crate::FilePick::filter("FigureIDE Project (project.json)", &["json"]),
            crate::FilePick::filter("Legacy FigureIDE Project Archive (.fide)", &[FIDE_EXTENSION]),
        ],
        ..Default::default()
    };
    if let Some(path) = crate::picks::open(app, &pick) {
        open_project_from_path(app, &path)?;
    }
    Ok(Value::Null)
}

/// Command handler for `project.save`.
pub(crate) fn save_project_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let save_ws = p.get("save_r_workspace").and_then(Value::as_bool);
    if let Some(path) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        if let Err(e) = save_project_to_path(app, path, save_ws) {
            app.status(format!("Save Project failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }
    if let Some(existing_path) = app.scientific.project_path.clone() {
        if let Err(e) = save_project_to_path(app, &existing_path, save_ws) {
            app.status(format!("Save Project failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }
    // No active project: open Create Project dialog in "save" mode
    open_create_project_dialog(app, "save");
    Ok(json!({ "pending": crate::dialogs::create_project::KIND }))
}

/// Command handler for `project.saveAs`.
pub(crate) fn save_project_as_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let save_ws = p.get("save_r_workspace").and_then(Value::as_bool);
    if let Some(path) = p.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()) {
        if let Err(e) = save_project_to_path(app, path, save_ws) {
            app.status(format!("Save Project As failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }
    if let (Some(name), Some(loc)) =
        (p.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()), p.get("location").and_then(Value::as_str).filter(|s| !s.is_empty()))
    {
        let target_dir = Path::new(loc).join(name);
        if let Err(e) = create_or_save_project(app, &target_dir, name, "saveAs", save_ws) {
            app.status(format!("Save Project As failed: {e}"));
            return Err(e);
        }
        return Ok(Value::Null);
    }
    open_create_project_dialog(app, "saveAs");
    Ok(json!({ "pending": crate::dialogs::create_project::KIND }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scientific::graph_style::GraphStyle;
    use crate::scientific::{RLinkedPanel, ResizeMode};
    use std::sync::Arc;
    use vectorcraft_doc::{ImageBlob, Node, NodeKind, PlacedDocument};

    fn make_test_app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app
    }

    #[test]
    fn renaming_a_project_figure_updates_the_project_reference_and_roundtrips() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 80, "name": "Original"})).unwrap();
        let rect = app.run("shape.rectangle", json!({"x": 10, "y": 12, "width": 20, "height": 15})).unwrap()["id"].as_u64().unwrap();
        let uid = app.session.active().unwrap().uid;
        let script_id = app.scientific.next_script_id;
        app.scientific.scripts.push(ScriptDocument::new(script_id, "plot(1:3)".into(), Some("/study/scripts/analysis.R".into()), None));
        app.scientific.next_script_id = script_id.saturating_add(1);
        app.run("select.set", json!({"ids": [rect]})).unwrap();
        let generated = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        app.run("object.markRGenerated", json!({"group": generated})).unwrap();
        let label = app.run("text.create", json!({"x": 10, "y": 8, "text": "A"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": label, "name": "RVS_LABEL"})).unwrap();
        app.run("select.set", json!({"ids": [generated, label]})).unwrap();
        let panel_id = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();
        let panel = RLinkedPanel {
            panel_id,
            generated_id: generated,
            label: "A".into(),
            r_variable: "p_scatter".into(),
            width_mm: 50.0,
            height_mm: 40.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: false,
            document_uid: uid,
        };
        let panel_name = super::super::persistent_panel_name(&panel, 50.0, 40.0, ResizeMode::Reflow);
        app.run("object.setName", json!({"id": panel_id, "name": panel_name})).unwrap();
        app.scientific.linked_panels.push(panel);
        assert_eq!(app.session.active().unwrap().doc.layers[0].children().unwrap().len(), 1);
        let project_path = vectorcraft_testkit::temp_dir("project_tab_rename").join("study");
        let _ = std::fs::remove_dir_all(&project_path);
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).unwrap();
        let old = project_path.join("figures/Original.fide");
        assert!(old.exists());

        let new = crate::io::rename_document(&mut app, uid, "RNAseq_図表").unwrap().unwrap();
        assert_eq!(std::path::Path::new(&new), project_path.join("figures/RNAseq_図表.fide"));
        assert!(!old.exists());
        assert!(app.scientific.project_dirty);
        let renamed_metadata = load_project_folder(&project_path).unwrap().metadata;
        assert_eq!(renamed_metadata.figures.first().map(|f| (f.name.as_str(), f.file.as_str())), Some(("RNAseq_図表", "figures/RNAseq_図表.fide")));
        assert_eq!(renamed_metadata.active_figure.as_deref(), Some("figures/RNAseq_図表.fide"));
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).unwrap();
        assert!(!app.scientific.project_dirty);
        let folder = load_project_folder(&project_path).unwrap();
        assert_eq!(folder.metadata.figures.first().map(|f| f.file.as_str()), Some("figures/RNAseq_図表.fide"));
        assert_eq!(folder.figures[0].doc.title, "RNAseq_図表");
        assert_eq!(folder.figures[0].doc.layers[0].children().unwrap().len(), 1);
        assert!(!old.exists(), "saving the project must not recreate the stale path");
        assert!(project_path.join("figures/RNAseq_図表.fide").exists());

        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &project_path.to_string_lossy()).unwrap();
        assert_eq!(reopened.session.active().unwrap().doc.layers[0].children().unwrap().len(), 1);
        assert_eq!(reopened.session.active().unwrap().title(), "RNAseq_図表.fide");
        assert!(
            reopened
                .scientific
                .scripts
                .iter()
                .any(|script| script.text == "plot(1:3)" && script.path.as_deref() == Some("/study/scripts/analysis.R"))
        );
        assert_eq!(reopened.scientific.linked_panels.len(), 1);
        assert_eq!(reopened.scientific.linked_panels[0].r_variable, "p_scatter");
        assert_eq!(reopened.scientific.linked_panels[0].document_uid, reopened.session.active().unwrap().uid);
        std::fs::remove_dir_all(project_path).unwrap();
    }

    // 1. Full roundtrip save and reopen of Airway Analysis with 2 scripts, 2 figures, 3 linked plots
    #[test]
    fn test_roundtrip_save_and_reopen_airway_analysis() {
        let mut app = make_test_app();
        while !app.session.documents().is_empty() {
            app.session.close_document(0);
        }
        // 1. Figures:
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.run("artboard.new", json!({"x": 0.0, "y": 300.0, "width": 210.0, "height": 297.0})).unwrap();

        // Configure 2 scripts
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "library(airway)\ndata(airway)\n".into(), Some("/path/to/analysis.R".into()), None));
        app.scientific.scripts.push(ScriptDocument::new(
            2,
            "plot_volcano(res)\nplot_heatmap(assay(airway))\n".into(),
            Some("/path/to/figures.R".into()),
            None,
        ));
        app.scientific.next_script_id = 3;

        // Configure 3 linked plots (1 hybrid UMAP, 1 volcano, 1 heatmap)
        let svg_umap = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"10\" cy=\"10\" r=\"5\"/></svg>";
        let key_umap = "rvs-svg-umap123".to_string();
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            doc.title = "Airway Analysis Figure 1".into();
            let mut blob = ImageBlob::new("image/svg+xml", svg_umap.as_bytes().to_vec());
            blob.proxy = Some(Arc::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3]));
            doc.images.insert(key_umap.clone(), blob);

            let placed = PlacedDocument {
                link: vectorcraft_doc::LinkInfo::new("rvs-embedded.svg"),
                key: key_umap,
                bounding: false,
                page_box: Default::default(),
                width: 100.0,
                height: 100.0,
                xf: vectorcraft_geom::Affine::IDENTITY,
                placement: Default::default(),
            };
            let child = Node::new(vectorcraft_doc::NodeId(101), NodeKind::PlacedDocument(Box::new(placed)));
            let group = Node::group(vectorcraft_doc::NodeId(100), vec![Arc::new(child)]);
            doc.insert(None, usize::MAX, group).unwrap();
        }

        app.scientific.linked_panels.push(RLinkedPanel {
            panel_id: 1,
            generated_id: 100,
            label: "A".into(),
            r_variable: "p_umap".into(),
            width_mm: 70.0,
            height_mm: 50.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: true,
            document_uid: app.session.active().unwrap().uid,
        });

        app.scientific.linked_panels.push(RLinkedPanel {
            panel_id: 2,
            generated_id: 200,
            label: "B".into(),
            r_variable: "p_volcano".into(),
            width_mm: 80.0,
            height_mm: 60.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: false,
            document_uid: app.session.active().unwrap().uid,
        });

        app.scientific.linked_panels.push(RLinkedPanel {
            panel_id: 3,
            generated_id: 300,
            label: "C".into(),
            r_variable: "p_heatmap".into(),
            width_mm: 90.0,
            height_mm: 70.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: false,
            document_uid: app.session.active().unwrap().uid,
        });

        // Add second figure
        let mut doc2 = Document::new(400.0, 300.0);
        doc2.title = "Supplementary Figure".into();
        app.session.add_document(doc2, None);
        app.session.set_active(0);

        // Workspace layout
        app.scientific.tab_order = vec![CentralTabId::Script(1), CentralTabId::Script(2), CentralTabId::DataViewer("coldata".into())];
        app.scientific.active_script_id = 2;
        app.ui.r_studio_editor_fraction = 0.72;
        app.ui.r_studio_editor_maximized = false;
        app.scientific.console_history = vec!["dim(airway)".into(), "colData(airway)".into()];
        app.scientific.working_directory = Some(PathBuf::from("/workspace/airway"));

        // Build and serialize project
        let project = build_fide_project(&app).expect("build fide project");
        let archive_bytes = project.save_to_bytes().expect("save fide project");

        // Reopen into a fresh app
        let mut reopened_app = make_test_app();
        open_fide_project_bytes(&mut reopened_app, &archive_bytes, Some("/tmp/Airway_Analysis.fide".into())).expect("open fide project");

        // Assert scripts
        assert_eq!(reopened_app.scientific.scripts.len(), 2);
        assert_eq!(reopened_app.scientific.scripts[0].filename(), "analysis.R");
        assert_eq!(reopened_app.scientific.scripts[0].text, "library(airway)\ndata(airway)\n");
        assert_eq!(reopened_app.scientific.scripts[0].path.as_deref(), Some("/path/to/analysis.R"));
        assert_eq!(reopened_app.scientific.scripts[1].filename(), "figures.R");
        assert_eq!(reopened_app.scientific.scripts[1].text, "plot_volcano(res)\nplot_heatmap(assay(airway))\n");

        // Assert figures
        assert_eq!(reopened_app.session.documents().len(), 2);
        assert_eq!(reopened_app.session.documents()[0].doc.artboards.len(), 2);
        assert_eq!(reopened_app.session.documents()[0].doc.title, "Airway Analysis Figure 1");
        assert_eq!(reopened_app.session.documents()[1].doc.title, "Supplementary Figure");

        // Assert layout
        assert_eq!(reopened_app.ui.r_studio_editor_fraction, 0.72);
        assert!(!reopened_app.ui.r_studio_editor_maximized);
        assert_eq!(reopened_app.scientific.console_history, vec!["dim(airway)", "colData(airway)"]);
        assert_eq!(reopened_app.scientific.working_directory, Some(PathBuf::from("/workspace/airway")));
        assert_eq!(reopened_app.scientific.active_script_id, 2);
    }

    // 2. Authoritative script snapshot policy: embedded snapshot is authoritative, never overwrites external file
    #[test]
    fn test_authoritative_script_snapshot() {
        let mut app = make_test_app();
        let temp_dir = std::env::temp_dir().join(format!("test_authoritative_script_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let external_path = temp_dir.join("test_authoritative_script.R");
        std::fs::write(&external_path, "external_code <- 100\n").unwrap();

        // In app, the user has edited the buffer to something else
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(
            1,
            "embedded_code <- 200\n".into(),
            Some(external_path.to_string_lossy().into_owned()),
            None,
        ));

        let project_path = temp_dir.join("project");
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).expect("save project");

        // Verify external file was NOT modified!
        let external_content = std::fs::read_to_string(&external_path).unwrap();
        assert_eq!(external_content, "external_code <- 100\n", "saving project must never overwrite external .R files");

        // Reopen into a new app
        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &project_path.to_string_lossy()).expect("open project");

        // Verify the reopened app restored the embedded snapshot
        assert_eq!(reopened.scientific.scripts[0].text, "embedded_code <- 200\n");
        assert_eq!(reopened.scientific.scripts[0].path.as_deref(), Some(external_path.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    // 3. Hybrid Linked Plot instant preview on reopen: does not trigger R and does not expand to points
    #[test]
    fn test_hybrid_linked_plot_instant_preview_on_reopen() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // Inject placed SVG representing 50k points
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0,0 L10,10\"/></svg>";
        let key = "rvs-svg-umap50k".to_string();
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            let mut blob = ImageBlob::new("image/svg+xml", svg.as_bytes().to_vec());
            blob.proxy = Some(Arc::new(vec![0x89, b'P', b'N', b'G', 5, 5, 5]));
            doc.images.insert(key.clone(), blob);

            let placed = PlacedDocument {
                link: vectorcraft_doc::LinkInfo::new("rvs-embedded.svg"),
                key: key.clone(),
                bounding: false,
                page_box: Default::default(),
                width: 140.0,
                height: 100.0,
                xf: vectorcraft_geom::Affine::IDENTITY,
                placement: Default::default(),
            };
            let child = Node::new(vectorcraft_doc::NodeId(10), NodeKind::PlacedDocument(Box::new(placed)));
            let group = Node::group(vectorcraft_doc::NodeId(11), vec![Arc::new(child)]);
            let layer_id = doc.layers.first().map(|l| l.id);
            doc.insert(layer_id, usize::MAX, group).unwrap();
        }

        let project_path = std::env::temp_dir().join(format!("test_hybrid_preview_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project_path);
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).expect("save project");

        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &project_path.to_string_lossy()).expect("open project");

        // Verify the document image blob is intact with proxy preview
        let doc = &reopened.session.active().unwrap().doc;
        assert!(doc.images.contains_key(&key));
        assert!(doc.images[&key].proxy.is_some());
        // Verify placed document exists as a single placed node, NOT expanded into individual points
        let mut placed_count = 0;
        doc.walk(|node| {
            if matches!(node.kind, NodeKind::PlacedDocument(_)) {
                placed_count += 1;
            }
        });
        assert_eq!(placed_count, 1);

        let _ = std::fs::remove_dir_all(project_path);
    }

    // 4. GraphStyle overrides persistence
    #[test]
    fn test_linked_graph_style_persistence() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let style = GraphStyle { plot_title_pt: Some(22.0), axis_x_line_mm: Some(0.8), tick_x_visible: Some(false), ..Default::default() };

        app.scientific.linked_panels.push(RLinkedPanel {
            panel_id: 1,
            generated_id: 10,
            label: "A".into(),
            r_variable: "p".into(),
            width_mm: 70.0,
            height_mm: 50.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: style.clone(),
            small_figure_typography: false,
            document_uid: app.session.active().unwrap().uid,
        });

        let project = build_fide_project(&app).expect("build project");
        assert_eq!(project.linked_plots.len(), 1);
        let stored_json = project.linked_plots[0].meta.graph_style_json.as_deref().unwrap();
        let parsed_style: GraphStyle = serde_json::from_str(stored_json).unwrap();
        assert_eq!(parsed_style.plot_title_pt, Some(22.0));
        assert_eq!(parsed_style.axis_x_line_mm, Some(0.8));
        assert_eq!(parsed_style.tick_x_visible, Some(false));
    }

    // 5. Unlinked manually edited figure persistence
    #[test]
    fn test_unlinked_manually_edited_figure_persistence() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.run("shape.rectangle", json!({"x": 10.0, "y": 10.0, "width": 50.0, "height": 30.0})).unwrap();

        let project_path = std::env::temp_dir().join(format!("test_manual_edit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project_path);
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).expect("save project");

        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &project_path.to_string_lossy()).expect("open project");

        let doc = &reopened.session.active().unwrap().doc;
        let mut rect_count = 0;
        doc.walk(|node| {
            if matches!(node.kind, NodeKind::Path { .. }) {
                rect_count += 1;
            }
        });
        assert!(rect_count >= 1, "manually added rectangles must be preserved");

        let _ = std::fs::remove_dir_all(project_path);
    }

    // 6. Central tab order and active tab restoration
    #[test]
    fn test_tab_order_and_active_tab_restoration() {
        let mut app = make_test_app();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "# 1\n".into(), None, None));
        app.scientific.scripts.push(ScriptDocument::new(2, "# 2\n".into(), None, None));
        app.scientific.tab_order = vec![CentralTabId::Script(2), CentralTabId::DataViewer("my_df".into()), CentralTabId::Script(1)];
        app.scientific.active_viewer_tab = Some("my_df".into());

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut reopened = make_test_app();
        open_fide_project_bytes(&mut reopened, &bytes, None).expect("open bytes");

        assert_eq!(reopened.scientific.tab_order.len(), 3);
        assert_eq!(reopened.scientific.tab_order[0], CentralTabId::Script(2));
        assert_eq!(reopened.scientific.tab_order[1], CentralTabId::DataViewer("my_df".into()));
        assert_eq!(reopened.scientific.tab_order[2], CentralTabId::Script(1));
        assert_eq!(reopened.scientific.active_viewer_tab.as_deref(), Some("my_df"));
    }

    // 7. Console history persistence
    #[test]
    fn test_console_history_persistence() {
        let mut app = make_test_app();
        app.scientific.console_history = vec!["x <- 1:10".into(), "mean(x)".into(), "sd(x)".into()];

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut reopened = make_test_app();
        open_fide_project_bytes(&mut reopened, &bytes, None).expect("open bytes");

        assert_eq!(reopened.scientific.console_history, vec!["x <- 1:10", "mean(x)", "sd(x)"]);
    }

    // 8. Working directory restoration
    #[test]
    fn test_working_directory_restoration() {
        let mut app = make_test_app();
        app.scientific.working_directory = Some(PathBuf::from("/analysis/experiment_01"));

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut reopened = make_test_app();
        open_fide_project_bytes(&mut reopened, &bytes, None).expect("open bytes");

        assert_eq!(reopened.scientific.working_directory, Some(PathBuf::from("/analysis/experiment_01")));
    }

    // 9. Atomic save safety: writing fails halfway without destroying existing file
    #[test]
    fn test_atomic_save_safety() {
        let path = std::env::temp_dir().join("test_atomic_safety.fide");
        std::fs::write(&path, b"ORIGINAL_FIDE_PROJECT_CONTENT").unwrap();

        // write_atomic writes to a sibling temp file and renames
        let updated = b"NEW_FIDE_PROJECT_CONTENT";
        vectorcraft_format::write_atomic(&path, updated).unwrap();

        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, updated);

        let _ = std::fs::remove_file(path);
    }

    // 10. Corrupt archive graceful rejection
    #[test]
    fn test_corrupt_archive_graceful_rejection() {
        let mut app = make_test_app();
        let garbage = b"PK\x03\x04garbage not a valid zip container";
        let res = open_fide_project_bytes(&mut app, garbage, None);
        assert!(res.is_err(), "corrupt archive must return Err, not panic");
    }

    // 11. Unsupported schema version rejection
    #[test]
    fn test_unsupported_version_rejection() {
        let mut app = make_test_app();
        let mut project = build_fide_project(&app).unwrap();
        project.manifest.version = 999;
        let bytes = project.save_to_bytes().unwrap();

        let res = open_fide_project_bytes(&mut app, &bytes, None);
        assert!(res.is_err(), "version 999 must be rejected");
        assert!(res.unwrap_err().contains("version 999 is newer"));
    }

    // 12. Path traversal attack prevention
    #[test]
    fn test_path_traversal_prevention() {
        use vectorcraft_format::zip::{ZipFileEntry, create_zip};
        let creation_res = create_zip(&[
            ZipFileEntry { name: "../../etc/shadow", data: b"root:!", deflate: false },
            ZipFileEntry { name: "manifest.json", data: b"{}", deflate: false },
        ]);
        assert!(creation_res.is_err(), "path traversal in zip creation must be rejected");

        // Verify opening a zip with traversal is also rejected by reader
        let safe_zip = create_zip(&[
            ZipFileEntry { name: "safe_entry.txt", data: b"hello", deflate: false },
            ZipFileEntry { name: "manifest.json", data: b"{}", deflate: false },
        ])
        .unwrap();

        let mut malicious_bytes = safe_zip;
        let from = b"safe_entry.txt";
        let to = b"../unsafe/evil";
        assert_eq!(from.len(), to.len());
        for i in 0..malicious_bytes.len().saturating_sub(from.len()) {
            if &malicious_bytes[i..i + from.len()] == from {
                malicious_bytes[i..i + to.len()].copy_from_slice(to);
            }
        }

        let mut app = make_test_app();
        let res = open_fide_project_bytes(&mut app, &malicious_bytes, None);
        assert!(res.is_err(), "path traversal in zip reading must be rejected");
    }

    // 13. Project dirty state tracking across scripts and documents
    #[test]
    fn test_project_dirty_tracking() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "x <- 1\n".into(), None, None));
        app.scientific.project_dirty = false;
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }

        // Clean initially
        assert!(!is_project_dirty(&app));

        // Script edited -> dirty
        app.scientific.scripts[0].text = "x <- 2\n".into();
        assert!(is_project_dirty(&app));

        // Revert script text -> clean
        app.scientific.scripts[0].text = "x <- 1\n".into();
        assert!(!is_project_dirty(&app));

        // Document edited -> dirty
        app.run("shape.rectangle", json!({"x": 10.0, "y": 10.0, "width": 50.0, "height": 30.0})).unwrap();
        assert!(is_project_dirty(&app));

        // Save project -> clean
        let project_path = std::env::temp_dir().join(format!("test_dirty_clean_{}", std::process::id()));
        save_project_to_path(&mut app, &project_path.to_string_lossy(), None).unwrap();
        assert!(!is_project_dirty(&app));

        let _ = std::fs::remove_dir_all(project_path);
    }

    // 14. Ctrl+S saves project in FigureIDE mode
    #[test]
    fn test_ctrl_s_project_save() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let project_path = std::env::temp_dir().join(format!("test_ctrl_s_{}", std::process::id()));
        app.scientific.project_path = Some(project_path.to_string_lossy().into_owned());

        // Make dirty
        app.scientific.scripts[0].text = "saved_by_ctrl_s <- TRUE\n".into();
        assert!(is_project_dirty(&app));

        // Execute file.save (Ctrl+S / Cmd+S)
        app.run("file.save", json!({})).unwrap();

        // Project should be clean and saved on disk
        assert!(!is_project_dirty(&app));
        assert!(project_path.exists());
        assert!(project_path.join("project.json").exists());

        let _ = std::fs::remove_dir_all(project_path);
    }

    fn sample_r_object(variable: &str, plot_kind: &str) -> crate::r_session::RObject {
        crate::r_session::RObject {
            variable: variable.to_string(),
            classes: if plot_kind.is_empty() { vec!["data.frame".into()] } else { vec![plot_kind.into()] },
            data_type: if plot_kind.is_empty() { "list".into() } else { "plot".into() },
            dimensions: "100x10".into(),
            length: 10,
            preview: "".into(),
            category: if plot_kind.is_empty() { "Data".into() } else { "Plots".into() },
            description: "".into(),
            plot_kind: plot_kind.to_string(),
        }
    }

    fn add_test_panel(app: &mut VectorcraftApp, panel_id: u64, generated_id: u64, r_variable: &str, label: &str, width_mm: f64, height_mm: f64) {
        add_test_panel_with_style(app, panel_id, generated_id, r_variable, label, (width_mm, height_mm), (GraphStyle::default(), false));
    }

    fn add_test_panel_with_style(
        app: &mut VectorcraftApp,
        panel_id: u64,
        generated_id: u64,
        r_variable: &str,
        label: &str,
        dimensions_mm: (f64, f64),
        appearance: (GraphStyle, bool),
    ) {
        let (width_mm, height_mm) = dimensions_mm;
        let (graph_style, small_figure_typography) = appearance;
        let doc_uid = app.session.active().map(|d| d.uid).unwrap_or(1);
        let panel = RLinkedPanel {
            panel_id,
            generated_id,
            label: label.to_string(),
            r_variable: r_variable.to_string(),
            width_mm,
            height_mm,
            resize_mode: ResizeMode::Reflow,
            graph_style,
            small_figure_typography,
            document_uid: doc_uid,
        };
        let panel_name = super::super::persistent_panel_name(&panel, width_mm, height_mm, ResizeMode::Reflow);
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            let mut label_node = Node::path(
                vectorcraft_doc::NodeId(panel_id + 10000),
                vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0)),
                vectorcraft_doc::Appearance::default_art(),
            );
            label_node.name = Some("RVS_LABEL".into());
            let inner_rect = Node::path(
                vectorcraft_doc::NodeId(panel_id + 20000),
                vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, width_mm, height_mm)),
                vectorcraft_doc::Appearance::default_art(),
            );
            let mut gen_node = Node::group(vectorcraft_doc::NodeId(generated_id), vec![Arc::new(inner_rect)]);
            gen_node.name = Some("RVS_GENERATED".into());
            let mut group = Node::group(vectorcraft_doc::NodeId(panel_id), vec![Arc::new(label_node), Arc::new(gen_node)]);
            group.name = Some(panel_name);
            let _ = doc.insert(None, usize::MAX, group);
        }
        app.scientific.linked_panels.push(panel);
    }

    // 15. Open .fide project without R installed / active
    #[test]
    fn test_open_fide_without_r_installed() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts[0].text = "x <- 1:10\nplot(x)\n".into();
        add_test_panel(&mut app, 10, 11, "p1", "A", 140.0, 100.0);

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut fresh = make_test_app();
        fresh.scientific.session_active = false;
        open_fide_project_bytes(&mut fresh, &bytes, Some("offline.fide".into())).expect("open fide");

        assert_eq!(fresh.scientific.scripts.len(), 1);
        assert_eq!(fresh.scientific.scripts[0].text, "x <- 1:10\nplot(x)\n");
        assert_eq!(fresh.scientific.linked_panels.len(), 1);
        let panel = &fresh.scientific.linked_panels[0];
        assert_eq!(fresh.scientific.panel_connection_state(panel), super::super::LinkedPanelState::Offline);
        assert!(fresh.scientific.execution_queue.is_empty(), "scripts must not auto-execute on load");
    }

    // 16. Restore multiple linked panels from saved previews
    #[test]
    fn test_restore_multiple_linked_panels_from_saved_previews() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_1", "A", 100.0, 75.0);
        add_test_panel(&mut app, 3, 4, "plot_2", "B", 120.0, 80.0);

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("multi.fide".into())).expect("open");

        assert_eq!(fresh.scientific.linked_panels.len(), 2);
        assert_eq!(fresh.scientific.linked_panels[0].r_variable, "plot_1");
        assert_eq!(fresh.scientific.linked_panels[1].r_variable, "plot_2");
        assert_eq!(fresh.scientific.panel_connection_state(&fresh.scientific.linked_panels[0]), super::super::LinkedPanelState::Offline);
        assert_eq!(fresh.scientific.panel_connection_state(&fresh.scientific.linked_panels[1]), super::super::LinkedPanelState::Offline);
    }

    // 17. Preserve large UMAP fast display without expanding to points
    #[test]
    fn test_preserve_large_umap_fast_display() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"2\"/></svg>";
        let key = "rvs-svg-umap100k".to_string();
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            let mut blob = ImageBlob::new("image/svg+xml", svg.as_bytes().to_vec());
            blob.proxy = Some(Arc::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3]));
            doc.images.insert(key.clone(), blob);

            let placed = PlacedDocument {
                link: vectorcraft_doc::LinkInfo::new("rvs-embedded.svg"),
                key: key.clone(),
                bounding: false,
                page_box: Default::default(),
                width: 140.0,
                height: 100.0,
                xf: vectorcraft_geom::Affine::IDENTITY,
                placement: Default::default(),
            };
            let child = Node::new(vectorcraft_doc::NodeId(20), NodeKind::PlacedDocument(Box::new(placed)));
            let mut inner_gen_node = Node::group(vectorcraft_doc::NodeId(22), vec![Arc::new(child)]);
            inner_gen_node.name = Some("RVS_GENERATED".into());
            let mut label_node = Node::path(
                vectorcraft_doc::NodeId(23),
                vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0)),
                vectorcraft_doc::Appearance::default_art(),
            );
            label_node.name = Some("RVS_LABEL".into());
            let mut group = Node::group(vectorcraft_doc::NodeId(21), vec![Arc::new(label_node), Arc::new(inner_gen_node)]);
            let panel = RLinkedPanel {
                panel_id: 21,
                generated_id: 22,
                label: "A".into(),
                r_variable: "umap_plot".into(),
                width_mm: 140.0,
                height_mm: 100.0,
                resize_mode: ResizeMode::Reflow,
                graph_style: GraphStyle::default(),
                small_figure_typography: false,
                document_uid: st.uid,
            };
            group.name = Some(super::super::persistent_panel_name(&panel, 140.0, 100.0, ResizeMode::Reflow));
            doc.insert(None, usize::MAX, group).unwrap();
            app.scientific.linked_panels.push(panel);
        }

        let project = build_fide_project(&app).expect("build");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("umap.fide".into())).expect("open");

        let doc = fresh.session.doc().unwrap();
        let target_node = doc.doc.node(vectorcraft_doc::NodeId(22)).expect("generated node");
        let children = target_node.children().expect("children");
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0].kind, NodeKind::PlacedDocument(p) if p.key == key));
    }

    // 18. Open project with empty R session
    #[test]
    fn test_open_project_with_empty_r_session() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "missing_plot", "A", 100.0, 75.0);

        let project = build_fide_project(&app).expect("build");
        let bytes = project.save_to_bytes().expect("save");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("empty_env.fide".into())).expect("open");

        fresh.scientific.session_active = true;
        fresh.scientific.objects.clear();

        let panel = &fresh.scientific.linked_panels[0];
        assert_eq!(fresh.scientific.panel_connection_state(panel), super::super::LinkedPanelState::MissingObject);
        assert!(fresh.session.doc().is_ok());
    }

    // 19. Detect missing plot variable
    #[test]
    fn test_detect_missing_plot_variable() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "volcano_plot", "A", 100.0, 75.0);

        app.scientific.session_active = true;
        app.scientific.objects = vec![sample_r_object("pca_plot", "ggplot")];

        let panel = &app.scientific.linked_panels[0];
        assert_eq!(app.scientific.panel_connection_state(panel), super::super::LinkedPanelState::MissingObject);
    }

    // 20. Detect incompatible object type
    #[test]
    fn test_detect_incompatible_object_type() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "counts_table", "A", 100.0, 75.0);

        app.scientific.session_active = true;
        app.scientific.objects = vec![sample_r_object("counts_table", "")];

        let panel = &app.scientific.linked_panels[0];
        assert_eq!(app.scientific.panel_connection_state(panel), super::super::LinkedPanelState::IncompatibleObject);
    }

    // 21. Detect available compatible ggplot
    #[test]
    fn test_detect_available_compatible_ggplot() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "p1", "A", 100.0, 75.0);

        app.scientific.session_active = true;
        app.scientific.objects = vec![sample_r_object("p1", "ggplot")];

        let panel = &app.scientific.linked_panels[0];
        assert_eq!(app.scientific.panel_connection_state(panel), super::super::LinkedPanelState::ReadyToUpdate);
    }

    // 22. Import referenced RData explicitly
    #[test]
    fn test_import_referenced_rdata_explicitly() {
        let mut app = make_test_app();
        app.scientific.project_path = Some("C:/my_project/analysis.fide".into());
        app.scientific.project_rdata_reference = Some("data/airway.RData".into());

        let resolved = app.scientific.resolve_project_rdata_path().expect("resolved path");
        assert!(resolved.ends_with("data/airway.RData") || resolved.ends_with("data\\airway.RData"));

        super::super::restore_project_rdata_action(&mut app);
        assert!(app.scientific.r_status.contains("Referenced RData file not found"));
    }

    // 23. Handle RData name collisions
    #[test]
    fn test_handle_rdata_name_collisions() {
        let mut app = make_test_app();
        let inspection = crate::r_session::RDataInspection {
            file_path: "airway.RData".into(),
            total_objects: 3,
            object_names: vec!["counts".into(), "meta".into(), "genes".into()],
            collisions: vec!["counts".into()],
        };

        app.scientific
            ._tx
            .send(super::super::RMessage::RDataInspected { path: "airway.RData".into(), policy_override: None, inspection: Ok(inspection) })
            .unwrap();
        super::super::poll_messages(&mut app);

        assert!(app.scientific.pending_rdata_import.is_some());
        assert_eq!(app.scientific.pending_rdata_import.as_ref().unwrap().collisions, vec!["counts"]);

        app.scientific.pending_rdata_import = None;
        assert!(app.scientific.pending_rdata_import.is_none());
    }

    // 24. Refresh environment and plot registry after import
    #[test]
    fn test_refresh_environment_and_plot_registry_after_import() {
        let mut app = make_test_app();
        let execution = crate::r_session::Execution {
            output: "Imported\n".into(),
            error: None,
            working_directory: None,
            objects: vec![sample_r_object("counts", ""), sample_r_object("volcano", "ggplot")],
        };

        app.scientific._tx.send(super::super::RMessage::RDataImported { path: "data.RData".into(), result: Ok((execution, 2, 0)) }).unwrap();
        super::super::poll_messages(&mut app);

        assert_eq!(app.scientific.objects.len(), 2);
        assert_eq!(app.scientific.plots.len(), 1);
        assert_eq!(app.scientific.plots[0].variable, "volcano");
    }

    // 25. Update previously saved linked panel
    #[test]
    fn test_update_previously_saved_linked_panel() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "p1", "A", 100.0, 75.0);

        app.scientific.session_active = true;
        app.scientific.objects = vec![sample_r_object("p1", "ggplot")];

        let res = super::super::update_panel_async(&mut app, 1);
        assert!(res.is_ok());
        assert_eq!(app.scientific.panel_connection_state(&app.scientific.linked_panels[0]), super::super::LinkedPanelState::Updating);
    }

    // 26. Relink panel to another compatible plot
    #[test]
    fn test_relink_panel_to_another_compatible_plot() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_A", "A", 100.0, 75.0);

        app.scientific.session_active = true;
        app.scientific.objects = vec![sample_r_object("plot_A", "ggplot"), sample_r_object("plot_B", "ggplot"), sample_r_object("df_table", "")];
        app.scientific.plots = app.scientific.objects.iter().filter(|o| !o.plot_kind.is_empty()).cloned().collect();

        let err = super::super::relink_panel_async(&mut app, 1, "df_table".into());
        assert!(err.is_err());

        let ok = super::super::relink_panel_async(&mut app, 1, "plot_B".into());
        assert!(ok.is_ok());
        assert_eq!(app.scientific.panel_connection_state(&app.scientific.linked_panels[0]), super::super::LinkedPanelState::Updating);
    }

    // 27. Cancel relink without changing artwork
    #[test]
    fn test_cancel_relink_without_changing_artwork() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_A", "A", 100.0, 75.0);

        app.scientific.panel_relink_open = Some(1);
        app.scientific.panel_relink_drafts.insert(1, "plot_B".into());

        app.scientific.panel_relink_drafts.remove(&1);
        app.scientific.panel_relink_open = None;

        assert_eq!(app.scientific.linked_panels[0].r_variable, "plot_A");
    }

    // 28. Preserve last valid SVG on render failure
    #[test]
    fn test_preserve_last_valid_svg_on_render_failure() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_A", "A", 100.0, 75.0);

        let doc_uid = app.session.active().unwrap().uid;
        let epoch = app.scientific.render_epoch.get(&doc_uid).copied().unwrap_or(0);

        app.scientific
            ._tx
            .send(super::super::RMessage::Rendered(
                doc_uid,
                epoch,
                "plot_A".into(),
                Box::new((
                    super::super::PanelAction::Refresh { panel_id: 1, generated_id: 2, width_mm: 100.0, height_mm: 75.0 },
                    Err("Evaluation error in ggplot".into()),
                )),
            ))
            .unwrap();
        super::super::poll_messages(&mut app);

        assert_eq!(app.scientific.panel_states.get(&1), Some(&super::super::LinkedPanelState::Error));
        assert_eq!(app.scientific.linked_panels[0].r_variable, "plot_A");
        assert!(app.scientific.r_status.contains("existing artwork was kept"));
    }

    // 29. Prevent stale updates
    #[test]
    fn test_prevent_stale_updates() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_A", "A", 100.0, 75.0);

        let doc_uid = app.session.active().unwrap().uid;
        app.scientific.render_epoch.insert(doc_uid, 5);

        app.scientific
            ._tx
            .send(super::super::RMessage::Rendered(
                doc_uid,
                0,
                "plot_A".into(),
                Box::new((
                    super::super::PanelAction::Refresh { panel_id: 1, generated_id: 2, width_mm: 100.0, height_mm: 75.0 },
                    Ok(super::super::RenderedPlot {
                        variable: "plot_A".into(),
                        svg: "<svg></svg>".into(),
                        plots: vec![],
                        action: super::super::PanelAction::Refresh { panel_id: 1, generated_id: 2, width_mm: 100.0, height_mm: 75.0 },
                    }),
                )),
            ))
            .unwrap();
        super::super::poll_messages(&mut app);

        assert_ne!(app.scientific.panel_states.get(&1), Some(&super::super::LinkedPanelState::Connected));
    }

    // 30. Preserve GraphStyle overrides and dimensions
    #[test]
    fn test_preserve_graph_style_overrides_and_dimensions() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        let style = GraphStyle { plot_title_pt: Some(22.0), axis_x_line_mm: Some(0.8), ..Default::default() };
        add_test_panel_with_style(&mut app, 10, 11, "plot_A", "A", (125.0, 85.0), (style, true));

        let project = build_fide_project(&app).expect("build");
        let bytes = project.save_to_bytes().expect("save");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("styles.fide".into())).expect("open");

        let restored = &fresh.scientific.linked_panels[0];
        assert_eq!((restored.width_mm, restored.height_mm), (125.0, 85.0));
        assert_eq!(restored.graph_style.plot_title_pt, Some(22.0));
        assert_eq!(restored.graph_style.axis_x_line_mm, Some(0.8));
        assert!(restored.small_figure_typography);
    }

    // 31. Preserve independent linked panels
    #[test]
    fn test_preserve_independent_linked_panels() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_panel(&mut app, 1, 2, "plot_A", "A", 80.0, 60.0);
        add_test_panel(&mut app, 3, 4, "plot_B", "B", 100.0, 75.0);

        app.scientific.panel_states.insert(1, super::super::LinkedPanelState::Error);
        assert_eq!(app.scientific.panel_states.get(&1), Some(&super::super::LinkedPanelState::Error));
        assert_eq!(app.scientific.panel_states.get(&3), None);

        assert_eq!(app.scientific.linked_panels[1].r_variable, "plot_B");
        assert_eq!(app.scientific.linked_panels[1].width_mm, 100.0);
    }

    // 32. Restore Data Viewer references
    #[test]
    fn test_restore_data_viewer_references() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.tab_order = vec![CentralTabId::Script(1), CentralTabId::DataViewer("airway_counts".into())];
        app.scientific.active_viewer_tab = Some("airway_counts".into());

        let project = build_fide_project(&app).expect("build");
        let bytes = project.save_to_bytes().expect("save");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("viewer.fide".into())).expect("open");

        assert!(fresh.scientific.tab_order.iter().any(|t| matches!(t, CentralTabId::DataViewer(n) if n == "airway_counts")));
        assert_eq!(fresh.scientific.viewer_tabs.len(), 1);
        let viewer_tab = &fresh.scientific.viewer_tabs[0];
        assert_eq!(viewer_tab.name, "airway_counts");
        assert!(viewer_tab.page.is_none());
        assert!(!viewer_tab.loading);
        assert!(viewer_tab.error.is_none());
    }

    // 33. Preserve Ctrl+Enter queue semantics
    #[test]
    fn test_preserve_ctrl_enter_queue_semantics() {
        let mut app = make_test_app();
        app.scientific.active_viewer_tab = Some("airway_counts".into());

        let enabled = super::super::command_enabled(&app, "r.run").unwrap_or(false);
        assert!(!enabled, "Ctrl+Enter (r.run) must be disabled while Data Viewer is active");

        app.scientific.active_viewer_tab = None;
        app.scientific.execution_queue.push_back(crate::scientific::QueuedRCommand {
            id: 1,
            script_id: None,
            code: "x <- 1\n".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });
        app.scientific.execution_queue.push_back(crate::scientific::QueuedRCommand {
            id: 2,
            script_id: None,
            code: "y <- 2\n".into(),
            source_revision: None,
            start_line: None,
            refresh_linked: false,
            is_selection: false,
        });

        assert_eq!(app.scientific.execution_queue.pop_front().unwrap().id, 1);
        assert_eq!(app.scientific.execution_queue.pop_front().unwrap().id, 2);
    }

    // 34. Verify Undo/Redo consistency after relink
    #[test]
    fn test_verify_undo_redo_consistency_after_relink() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let rect = app.run("shape.rectangle", json!({"x": 20.0, "y": 30.0, "width": 100.0, "height": 75.0})).unwrap()["id"].as_u64().unwrap();
        app.run("select.set", json!({"ids": [rect]})).unwrap();
        let generated = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": generated, "name": "RVS_GENERATED"})).unwrap();
        app.run("object.markRGenerated", json!({"group": generated})).unwrap();

        let label = app.run("text.create", json!({"x": 20.0, "y": 15.0, "text": "A"})).unwrap()["id"].as_u64().unwrap();
        app.run("object.setName", json!({"id": label, "name": "RVS_LABEL"})).unwrap();

        app.run("select.set", json!({"ids": [generated, label]})).unwrap();
        let panel_id = app.run("object.group", json!({})).unwrap()["id"].as_u64().unwrap();

        let doc_uid = app.session.active().unwrap().uid;
        let panel = RLinkedPanel {
            panel_id,
            generated_id: generated,
            label: "A".into(),
            r_variable: "plot_1".into(),
            width_mm: 100.0,
            height_mm: 75.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: false,
            document_uid: doc_uid,
        };
        let panel_name = super::super::persistent_panel_name(&panel, 100.0, 75.0, ResizeMode::Reflow);
        app.run("object.setName", json!({"id": panel_id, "name": panel_name})).unwrap();
        app.scientific.linked_panels.push(panel);

        let replacement_svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"20\" cy=\"20\" r=\"10\"/></svg>";
        let rendered = super::super::RenderedPlot {
            variable: "plot_2".into(),
            svg: replacement_svg.into(),
            plots: vec![sample_r_object("plot_2", "ggplot")],
            action: super::super::PanelAction::Relink {
                panel_id,
                generated_id: generated,
                target_variable: "plot_2".into(),
                width_mm: 100.0,
                height_mm: 75.0,
            },
        };
        crate::scientific::linked_plots::apply_rendered_plot(&mut app, rendered);
        assert_eq!(app.scientific.linked_panels[0].r_variable, "plot_2");

        // Undo relink
        let undo_res = app.run("edit.undo", json!({}));
        assert!(undo_res.is_ok());
        super::super::restore_linked_panels(&mut app);
        assert_eq!(app.scientific.linked_panels[0].r_variable, "plot_1");

        // Redo relink
        let redo_res = app.run("edit.redo", json!({}));
        assert!(redo_res.is_ok());
        super::super::restore_linked_panels(&mut app);
        assert_eq!(app.scientific.linked_panels[0].r_variable, "plot_2");
    }

    // 35. Folder-based project roundtrip with scripts, figures, and layout
    #[test]
    fn test_folder_based_project_roundtrip_airway() {
        let temp_dir = std::env::temp_dir().join(format!("test_folder_airway_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        while !app.session.documents().is_empty() {
            app.session.close_document(0);
        }
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            doc.title = "Airway Analysis".into();
        }

        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "library(airway)\n".into(), Some("/ext/analysis.R".into()), None));
        app.scientific.scripts.push(ScriptDocument::new(2, "plot_volcano()\n".into(), None, None));
        app.scientific.next_script_id = 3;

        save_project_to_path(&mut app, &temp_dir.to_string_lossy(), None).expect("save folder project");

        // Verify folder structure
        assert!(temp_dir.join("project.json").exists());
        assert!(temp_dir.join("scripts").join("analysis.R").exists());
        assert!(temp_dir.join("scripts").join("Untitled-2.R").exists());
        assert!(temp_dir.join("figures").join("Airway_Analysis.fide").exists());
        assert!(temp_dir.join("workspace").exists());
        assert!(temp_dir.join("assets").exists());

        // Reopen into a fresh app
        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &temp_dir.to_string_lossy()).expect("open folder project");

        assert_eq!(reopened.scientific.scripts.len(), 2);
        assert_eq!(reopened.scientific.scripts[0].text, "library(airway)\n");
        assert_eq!(reopened.scientific.scripts[1].text, "plot_volcano()\n");
        assert_eq!(reopened.session.documents().len(), 1);
        assert_eq!(reopened.session.documents()[0].doc.title, "Airway Analysis");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 36. Save project does NOT mutate workspace.RData
    #[test]
    fn test_save_project_does_not_mutate_workspace_rdata() {
        let temp_dir = std::env::temp_dir().join(format!("test_ws_preserve_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("workspace")).unwrap();

        let rdata_path = temp_dir.join("workspace").join("workspace.RData");
        std::fs::write(&rdata_path, b"ORIGINAL_RDATA_CONTENT").unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        save_project_to_path(&mut app, &temp_dir.to_string_lossy(), Some(false)).expect("save project");

        let current_bytes = std::fs::read(&rdata_path).unwrap();
        assert_eq!(current_bytes, b"ORIGINAL_RDATA_CONTENT", "Ctrl+S / save_project must NEVER overwrite workspace.RData");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 37. Save and load independent .fide figure document
    #[test]
    fn test_save_and_load_figure_independent_fide() {
        let temp_dir = std::env::temp_dir().join(format!("test_fig_indep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let fig_path = temp_dir.join("My_Figure.fide");

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // Inject placed SVG representing linked plot
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"5\" cy=\"5\" r=\"4\"/></svg>";
        let key = "rvs-svg-scatter".to_string();
        let doc_uid = app.session.active().unwrap().uid;
        let panel = RLinkedPanel {
            panel_id: 200,
            generated_id: 201,
            label: "A".into(),
            r_variable: "p_scatter".into(),
            width_mm: 50.0,
            height_mm: 50.0,
            resize_mode: ResizeMode::Reflow,
            graph_style: GraphStyle::default(),
            small_figure_typography: false,
            document_uid: doc_uid,
        };
        let panel_name = super::super::persistent_panel_name(&panel, 50.0, 50.0, ResizeMode::Reflow);

        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            let mut blob = ImageBlob::new("image/svg+xml", svg.as_bytes().to_vec());
            blob.proxy = Some(Arc::new(vec![0x89, b'P', b'N', b'G', 9, 8, 7]));
            doc.images.insert(key.clone(), blob);

            let placed = PlacedDocument {
                link: vectorcraft_doc::LinkInfo::new("rvs-embedded.svg"),
                key,
                bounding: false,
                page_box: Default::default(),
                width: 50.0,
                height: 50.0,
                xf: vectorcraft_geom::Affine::IDENTITY,
                placement: Default::default(),
            };
            let mut label_node = Node::path(
                vectorcraft_doc::NodeId(202),
                vectorcraft_geom::shapes::rectangle(vectorcraft_geom::Rect::new(0.0, 0.0, 10.0, 10.0)),
                vectorcraft_doc::Appearance::default_art(),
            );
            label_node.name = Some("RVS_LABEL".into());
            let placed_node = Node::new(vectorcraft_doc::NodeId(203), NodeKind::PlacedDocument(Box::new(placed)));
            let mut gen_node = Node::group(vectorcraft_doc::NodeId(201), vec![Arc::new(placed_node)]);
            gen_node.name = Some("RVS_GENERATED".into());
            let mut group = Node::group(vectorcraft_doc::NodeId(200), vec![Arc::new(label_node), Arc::new(gen_node)]);
            group.name = Some(panel_name);
            doc.insert(None, usize::MAX, group).unwrap();
        }
        app.scientific.linked_panels.push(panel);

        // Save active figure
        let save_res = save_figure_command(&mut app, &json!({"path": fig_path.to_string_lossy()}));
        assert!(save_res.is_ok());
        assert!(fig_path.exists());

        // Open figure directly in a fresh app without any project
        let mut fresh_app = make_test_app();
        open_project_from_path(&mut fresh_app, &fig_path.to_string_lossy()).expect("open independent figure");

        assert_eq!(fresh_app.session.documents().len(), 1);
        assert_eq!(fresh_app.scientific.linked_panels.len(), 1);
        assert_eq!(fresh_app.scientific.linked_panels[0].r_variable, "p_scatter");
        assert_eq!(fresh_app.scientific.linked_panels[0].width_mm, 50.0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 38. Save R workspace command
    #[test]
    fn test_save_r_workspace_command_to_project_workspace() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_rws_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.project_path = Some(temp_dir.to_string_lossy().into_owned());

        let target_rdata = temp_dir.join("workspace").join("workspace.RData");

        if crate::r_session::find_rscript().is_ok() {
            let res = save_r_workspace_command(&mut app, &json!({}));
            if let Ok(val) = res {
                assert!(target_rdata.exists());
                assert!(val.get("object_count").is_some());
                assert_eq!(app.scientific.project_rdata_reference.as_deref(), Some(target_rdata.to_str().unwrap()));
            }
        }

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 39. Load R workspace collision detection
    #[test]
    fn test_load_r_workspace_collision_detection() {
        let mut app = make_test_app();
        let inspection = crate::r_session::RDataInspection {
            file_path: "/data/sample.RData".into(),
            total_objects: 5,
            collisions: vec!["res".into(), "counts".into()],
            object_names: vec!["res".into(), "counts".into(), "meta".into()],
        };
        app.scientific.pending_rdata_import = Some(inspection.clone());

        assert!(app.scientific.pending_rdata_import.is_some());
        assert_eq!(app.scientific.pending_rdata_import.as_ref().unwrap().collisions.len(), 2);
    }

    // 40. Open project from path handles directories and .fide files
    #[test]
    fn test_open_project_from_path_handles_folders_and_fide_files() {
        let temp_dir = std::env::temp_dir().join(format!("test_open_disambig_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // 1. Save as folder
        let folder_path = temp_dir.join("My_Project");
        save_project_to_path(&mut app, &folder_path.to_string_lossy(), None).unwrap();

        // 2. Open via directory path
        let mut app1 = make_test_app();
        assert!(open_project_from_path(&mut app1, &folder_path.to_string_lossy()).is_ok());

        // 3. Open via project.json path
        let mut app2 = make_test_app();
        let json_path = folder_path.join("project.json");
        assert!(open_project_from_path(&mut app2, &json_path.to_string_lossy()).is_ok());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 41. Figure save and saveAs UI commands
    #[test]
    fn test_figure_save_and_save_as_commands() {
        let temp_dir = std::env::temp_dir().join(format!("test_fig_cmds_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let fig_file = temp_dir.join("Figure_Panel.fide");
        let res = app.run("figure.save", json!({"path": fig_file.to_string_lossy()}));
        assert!(res.is_ok());
        assert!(fig_file.exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 42. Environment toolbar commands registered
    #[test]
    fn test_environment_toolbar_workspace_commands_registered() {
        let app = make_test_app();
        assert_eq!(super::super::command_enabled(&app, "r.workspace.save"), Some(true));
        assert_eq!(super::super::command_enabled(&app, "r.workspace.load"), Some(true));
        assert_eq!(super::super::command_enabled(&app, "figure.save"), Some(false)); // no active doc initially
    }

    // 43. Authoritative script snapshot in folder workspace
    #[test]
    fn test_authoritative_script_snapshot_folder_workspace() {
        let temp_dir = std::env::temp_dir().join(format!("test_auth_script_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let external_file = temp_dir.join("external_code.R");
        std::fs::write(&external_file, "original <- 1\n").unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(
            1,
            "modified_in_app <- 2\n".into(),
            Some(external_file.to_string_lossy().into_owned()),
            None,
        ));

        let project_dir = temp_dir.join("Project");
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).unwrap();

        // External file must remain unchanged
        assert_eq!(std::fs::read_to_string(&external_file).unwrap(), "original <- 1\n");

        // Embedded script in project folder must have the app text
        let saved_script = std::fs::read_to_string(project_dir.join("scripts").join("external_code.R")).unwrap();
        assert_eq!(saved_script, "modified_in_app <- 2\n");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 44. Project dirty tracking with folder workspace
    #[test]
    fn test_project_dirty_tracking_with_folder_workspace() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "x <- 1\n".into(), None, None));
        app.scientific.scripts[0].saved_text = "x <- 1\n".into();

        if let Ok(st) = app.session.doc_mut() {
            st.mark_saved();
        }
        app.scientific.project_dirty = false;
        assert!(!is_project_dirty(&app));

        // Modify script
        app.scientific.scripts[0].text = "x <- 2\n".into();
        assert!(is_project_dirty(&app));
    }

    // 45. Complete 16-step scientific project end-to-end integrity test
    #[test]
    fn test_realistic_scientific_project_end_to_end_integrity_release_blocker() {
        let temp_dir = std::env::temp_dir().join(format!("test_e2e_blocker_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let project_dir = temp_dir.join("Airway_Analysis_Project");

        // 1. Create a new project folder
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.project_path = Some(project_dir.to_string_lossy().into_owned());

        // 2. Create multiple R scripts
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(
            1,
            "library(ggplot2)\np_pca <- ggplot(iris, aes(Sepal.Length, Sepal.Width, color=Species)) + geom_point()\n".into(),
            Some("/work/analysis.R".into()),
            None,
        ));
        app.scientific.scripts.push(ScriptDocument::new(
            2,
            "p_umap <- ggplot(iris, aes(Petal.Length, Petal.Width, color=Species)) + geom_point()\np_volcano <- ggplot(iris, aes(Sepal.Length, Petal.Length)) + geom_point()\n".into(),
            Some("/work/figures.R".into()),
            None,
        ));

        // 3 & 4. Add multiple linked plots (PCA, UMAP, Volcano) with Hybrid Linked SVG
        let style_pca = GraphStyle { plot_title_pt: Some(18.0), axis_x_line_mm: Some(0.6), ..Default::default() };
        add_test_panel_with_style(&mut app, 100, 101, "p_pca", "A", (90.0, 70.0), (style_pca, true));

        let style_umap = GraphStyle { plot_title_pt: Some(16.0), ..Default::default() };
        add_test_panel_with_style(&mut app, 200, 201, "p_umap", "B", (100.0, 80.0), (style_umap, false));

        add_test_panel(&mut app, 300, 301, "p_volcano", "C", 85.0, 65.0);

        // 5. Modify Graph Inspector styling for Panel B
        app.scientific.linked_panels[1].graph_style.axis_title_x_pt = Some(14.0);
        let new_name = super::super::persistent_panel_name(&app.scientific.linked_panels[1], 100.0, 80.0, ResizeMode::Reflow);
        if let Ok(st) = app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            if let Some(node) = doc.node_mut(vectorcraft_doc::NodeId(200)) {
                node.name = Some(new_name);
            }
        }

        // 6. Add native vector annotations (e.g., rectangle)
        let rect_res = app.run("shape.rectangle", json!({"x": 10.0, "y": 10.0, "width": 50.0, "height": 30.0}));
        assert!(rect_res.is_ok(), "Native vector annotation must succeed");

        // 7. Save the project
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).expect("Save project folder");
        assert!(project_dir.join("project.json").exists());
        assert!(project_dir.join("scripts").join("analysis.R").exists());
        assert!(project_dir.join("scripts").join("figures.R").exists());

        // 8. Explicitly save .RData
        let ws_dir = project_dir.join("workspace");
        std::fs::create_dir_all(&ws_dir).unwrap();
        let ws_rdata = ws_dir.join("workspace.RData");
        std::fs::write(&ws_rdata, b"RDX3\nFAKE_RDATA_OBJECTS\np_pca\np_umap\np_volcano\n").unwrap();
        app.scientific.project_rdata_reference = Some(ws_rdata.to_string_lossy().into_owned());

        // 9 & 10. Exit and Restart application (simulate clean restart)
        drop(app);
        let mut fresh_app = make_test_app();

        // 11. Reopen the project folder
        open_project_from_path(&mut fresh_app, &project_dir.to_string_lossy()).expect("Reopen project folder");

        // 12. Verify all scripts, figures and artwork
        assert_eq!(fresh_app.scientific.scripts.len(), 2);
        assert_eq!(fresh_app.scientific.scripts[0].filename(), "analysis.R");
        assert!(fresh_app.scientific.scripts[0].text.contains("p_pca"));
        assert_eq!(fresh_app.scientific.scripts[1].filename(), "figures.R");
        assert!(fresh_app.scientific.scripts[1].text.contains("p_umap"));

        assert_eq!(fresh_app.scientific.linked_panels.len(), 3);
        assert_eq!(fresh_app.scientific.linked_panels[0].r_variable, "p_pca");
        assert_eq!(fresh_app.scientific.linked_panels[0].width_mm, 90.0);
        assert_eq!(fresh_app.scientific.linked_panels[0].graph_style.plot_title_pt, Some(18.0));
        assert!(fresh_app.scientific.linked_panels[0].small_figure_typography);

        assert_eq!(fresh_app.scientific.linked_panels[1].r_variable, "p_umap");
        assert_eq!(fresh_app.scientific.linked_panels[1].width_mm, 100.0);
        assert_eq!(fresh_app.scientific.linked_panels[1].graph_style.axis_title_x_pt, Some(14.0));

        assert_eq!(fresh_app.scientific.linked_panels[2].r_variable, "p_volcano");
        assert_eq!(fresh_app.scientific.linked_panels[2].width_mm, 85.0);

        // Verify native vector annotations preserved
        let doc = fresh_app.session.doc().unwrap();
        assert!(!doc.doc.layers.is_empty());
        let mut node_count = 0;
        doc.doc.walk(|_| node_count += 1);
        assert!(node_count >= 4, "Must retain native vector annotations alongside linked plots");

        // 13. Explicitly load the saved .RData
        assert_eq!(fresh_app.scientific.project_rdata_reference.as_deref(), Some(ws_rdata.to_str().unwrap()));
        assert!(ws_rdata.exists());

        // 14. Refresh Environment and Plot Registry
        fresh_app.scientific.session_active = true;
        fresh_app.scientific.objects = vec![sample_r_object("p_pca", "ggplot"), sample_r_object("p_umap", "ggplot")];

        // 15. Reflow a restored linked plot (Panel A: 90x70 -> 120x80)
        fresh_app.scientific.linked_panels[0].width_mm = 120.0;
        fresh_app.scientific.linked_panels[0].height_mm = 80.0;
        let reflow_name = super::super::persistent_panel_name(&fresh_app.scientific.linked_panels[0], 120.0, 80.0, ResizeMode::Reflow);
        if let Ok(st) = fresh_app.session.doc_mut() {
            let doc = Arc::make_mut(&mut st.doc);
            if let Some(node) = doc.node_mut(vectorcraft_doc::NodeId(100)) {
                node.name = Some(reflow_name);
            }
        }
        fresh_app.scientific.project_dirty = true;

        // 16. Save and reopen the project again
        save_project_to_path(&mut fresh_app, &project_dir.to_string_lossy(), None).expect("Resave project folder");

        let mut second_restart = make_test_app();
        open_project_from_path(&mut second_restart, &project_dir.to_string_lossy()).expect("Second reopen");

        assert_eq!(second_restart.scientific.linked_panels[0].width_mm, 120.0);
        assert_eq!(second_restart.scientific.linked_panels[0].height_mm, 80.0);
        assert_eq!(second_restart.scientific.scripts.len(), 2);

        // 17. Verify failed saves leave the last valid data recoverable
        let bad_path = project_dir.join("project.json").join("invalid_subpath");
        let fail_res = save_project_to_path(&mut second_restart, &bad_path.to_string_lossy(), None);
        assert!(fail_res.is_err(), "Invalid path must fail gracefully");

        // Verify existing project folder remains intact and openable
        let mut recovery_app = make_test_app();
        assert!(open_project_from_path(&mut recovery_app, &project_dir.to_string_lossy()).is_ok());
        assert_eq!(recovery_app.scientific.linked_panels.len(), 3);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 46. Condition 1, 2, 3, 4, 5, 7: Folder structure, no whole-project .fide, no auto .RData
    #[test]
    fn test_folder_save_structure_no_single_fide_archive_and_no_auto_rdata() {
        let temp_dir = std::env::temp_dir().join(format!("test_folder_struct_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "x <- 42\n".into(), Some("script1.R".into()), None));
        app.scientific.scripts.push(ScriptDocument::new(2, "y <- 84\n".into(), Some("script2.R".into()), None));

        let project_dir = temp_dir.join("My_Project");
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).expect("save project folder");

        // 1. Directory created
        assert!(project_dir.is_dir(), "Project must be a directory");
        // 2. No whole-project .fide file
        let fide_file = temp_dir.join("My_Project.fide");
        assert!(!fide_file.exists(), "Must not create My_Project.fide archive file");
        // 3. project.json exists
        let p_json = project_dir.join("project.json");
        assert!(p_json.exists(), "project.json must exist in project directory");
        let meta_str = std::fs::read_to_string(&p_json).unwrap();
        let meta: ProjectMetadata = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta.format, "figureide-project");
        // 4. .R scripts in scripts/
        assert!(project_dir.join("scripts").join("script1.R").exists());
        assert!(project_dir.join("scripts").join("script2.R").exists());
        // 5. Each figure saved as individual .fide in figures/
        let fig_entries: Vec<_> =
            std::fs::read_dir(project_dir.join("figures")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(fig_entries.len(), 1, "Must contain exactly one figure file");
        assert!(fig_entries[0].ends_with(".fide"), "Figure file must have .fide extension");
        // 7. Save project does NOT save workspace/workspace.RData automatically
        assert!(!project_dir.join("workspace").join("workspace.RData").exists(), "Save project must NOT auto-save .RData");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 47. Condition 12 & 15: Saving project twice updates same folder and does not duplicate figures or scripts
    #[test]
    fn test_save_project_twice_does_not_duplicate_figures_or_scripts() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_twice_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "a <- 1\n".into(), Some("script.R".into()), None));

        let project_dir = temp_dir.join("Test_Twice");
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).expect("first save");

        let figure_files_1: Vec<_> = std::fs::read_dir(project_dir.join("figures")).unwrap().map(|e| e.unwrap().file_name()).collect();
        let script_files_1: Vec<_> = std::fs::read_dir(project_dir.join("scripts")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(figure_files_1.len(), 1);
        assert_eq!(script_files_1.len(), 1);

        // Edit script slightly
        app.scientific.scripts[0].text = "a <- 2\n".into();

        // Save again to same folder
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).expect("second save");

        let figure_files_2: Vec<_> = std::fs::read_dir(project_dir.join("figures")).unwrap().map(|e| e.unwrap().file_name()).collect();
        let script_files_2: Vec<_> = std::fs::read_dir(project_dir.join("scripts")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(figure_files_2.len(), 1, "Saving twice must not duplicate figure files");
        assert_eq!(script_files_2.len(), 1, "Saving twice must not duplicate script files");
        assert_eq!(figure_files_1, figure_files_2, "Figure filename must remain stable");

        // Verify no nested project folder was created
        assert!(!project_dir.join("Test_Twice").exists(), "Must not create nested project folders on resave");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 48. Condition 13: Save Project As creates separate folder and leaves original intact
    #[test]
    fn test_save_project_as_creates_separate_directory_leaves_original_intact() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_as_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "# v1\n".into(), Some("script.R".into()), None));

        let orig_dir = temp_dir.join("Original_Project");
        save_project_to_path(&mut app, &orig_dir.to_string_lossy(), None).expect("save original");

        // Now Save As to new directory
        let copy_dir = temp_dir.join("Copy_Project");
        app.scientific.scripts[0].text = "# v2 in copy\n".into();
        save_project_to_path(&mut app, &copy_dir.to_string_lossy(), None).expect("save copy");

        // Both folders exist
        assert!(orig_dir.join("project.json").exists());
        assert!(copy_dir.join("project.json").exists());

        // Original script remains "# v1\n"
        let orig_script = std::fs::read_to_string(orig_dir.join("scripts").join("script.R")).unwrap();
        assert_eq!(orig_script, "# v1\n");

        // Copy script is "# v2 in copy\n"
        let copy_script = std::fs::read_to_string(copy_dir.join("scripts").join("script.R")).unwrap();
        assert_eq!(copy_script, "# v2 in copy\n");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 49. Condition 14: Legacy .fide archive migration to folder preserves original .fide archive
    #[test]
    fn test_legacy_fide_archive_migration_preserves_original_archive() {
        let temp_dir = std::env::temp_dir().join(format!("test_legacy_mig_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Build a legacy .fide zip archive
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "legacy_code <- TRUE\n".into(), Some("legacy_analysis.R".into()), None));
        let legacy_proj = build_fide_project(&app).unwrap();
        let legacy_bytes = legacy_proj.save_to_bytes().unwrap();

        let legacy_archive_path = temp_dir.join("LegacyStudy.fide");
        std::fs::write(&legacy_archive_path, &legacy_bytes).unwrap();

        // Open legacy archive using open_project_from_path
        let mut reopened_app = make_test_app();
        open_project_from_path(&mut reopened_app, &legacy_archive_path.to_string_lossy()).expect("open legacy archive");

        // Original archive must still exist intact!
        assert!(legacy_archive_path.exists(), "Original legacy archive must not be deleted");
        let orig_read_back = std::fs::read(&legacy_archive_path).unwrap();
        assert_eq!(orig_read_back, legacy_bytes, "Original legacy archive must remain byte-identical");

        // Migrated folder must exist with project.json and scripts/
        let migrated_folder = temp_dir.join("LegacyStudy");
        assert!(migrated_folder.is_dir(), "Migrated folder must exist");
        assert!(migrated_folder.join("project.json").exists(), "project.json must exist in migrated folder");
        assert!(migrated_folder.join("scripts").join("legacy_analysis.R").exists());

        // Reopened app state
        assert_eq!(reopened_app.scientific.scripts.len(), 1);
        assert_eq!(reopened_app.scientific.scripts[0].text, "legacy_code <- TRUE\n");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 50. Condition 16: Unicode and spaces in project names save and reopen correctly
    #[test]
    fn test_unicode_and_spaces_project_path_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("test_unicode_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let project_dir = temp_dir.join("Airway 項目 🧬 Study");

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "# 日本語コメント 🧬\nval <- 123\n".into(), Some("解析.R".into()), None));

        save_project_to_path(&mut app, &project_dir.to_string_lossy(), None).expect("save unicode project");
        assert!(project_dir.join("project.json").exists());

        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &project_dir.to_string_lossy()).expect("reopen unicode project");

        assert_eq!(reopened.scientific.scripts.len(), 1);
        assert_eq!(reopened.scientific.scripts[0].text, "# 日本語コメント 🧬\nval <- 123\n");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 51. PyCharm-style: Create Project dialog creates <parent>/<name> automatically
    #[test]
    fn test_create_project_dialog_creates_named_directory_under_parent() {
        let temp_dir = std::env::temp_dir().join(format!("test_create_proj_dlg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.last_project_parent = Some(temp_dir.clone());

        // 1. Invoke project.new (clean state)
        let res = app.run("project.new", json!({})).unwrap();
        assert_eq!(res["pending"], crate::dialogs::create_project::KIND);
        assert!(app.ui.dialog.is_some());

        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.kind, crate::dialogs::create_project::KIND);
        assert_eq!(d.str("location"), temp_dir.to_string_lossy());

        // 2. Set Project Name to "SingleCell_Analysis"
        app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("SingleCell_Analysis"));
        let target_dir = temp_dir.join("SingleCell_Analysis");
        assert!(!target_dir.exists(), "Target directory must not exist before Create Project");

        // 3. Confirm Create Project via dialog spec confirm
        let d = app.ui.dialog.as_ref().unwrap().clone();
        (crate::dialogs::create_project::SPEC.confirm)(&mut app, &d).unwrap();
        assert!(app.ui.dialog.is_none(), "Dialog must close on successful project creation");

        // 4. Verify directory and internal structure
        assert!(target_dir.is_dir(), "SingleCell_Analysis directory must be created automatically");
        assert!(target_dir.join("project.json").exists(), "project.json must be initialized");
        assert!(target_dir.join("scripts").is_dir(), "scripts/ must exist");
        assert!(target_dir.join("figures").is_dir(), "figures/ must exist");
        assert!(target_dir.join("workspace").is_dir(), "workspace/ must exist");
        assert!(target_dir.join("assets").is_dir(), "assets/ must exist");
        assert!(!target_dir.join("workspace").join("workspace.RData").exists(), "workspace.RData must NOT exist automatically");

        // 5. Verify no .fide archive container was created for the whole project
        let whole_fide = temp_dir.join("SingleCell_Analysis.fide");
        assert!(!whole_fide.exists(), "Must not create a single .fide archive");

        // 6. Verify active project root is set to the new directory
        assert_eq!(app.scientific.project_path.as_deref(), Some(target_dir.to_string_lossy().as_ref()));

        // 7. Verify window title reflects the new project name
        let title = crate::chrome::window_title(&app);
        assert!(title.contains("SingleCell_Analysis"), "Window title must reflect project name: {title}");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 52. Dialog validation disables Create button for invalid names and destination conflicts
    #[test]
    fn test_create_project_dialog_validation_and_disabled_button() {
        let temp_dir = std::env::temp_dir().join(format!("test_dlg_validation_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        open_create_project_dialog(&mut app, "new");
        let d = app.ui.dialog.as_ref().unwrap();

        // Location valid, default name valid -> enabled
        assert!(crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, d));

        // Invalid: empty name -> disabled
        let mut invalid_d = d.clone();
        invalid_d.fields.insert("name".into(), json!(""));
        assert!(!crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, &invalid_d));

        // Invalid: path separator -> disabled
        invalid_d.fields.insert("name".into(), json!("folder/name"));
        assert!(!crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, &invalid_d));

        // Invalid: reserved name "CON" -> disabled
        invalid_d.fields.insert("name".into(), json!("CON"));
        assert!(!crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, &invalid_d));

        // Invalid: trailing dot -> disabled
        invalid_d.fields.insert("name".into(), json!("Project."));
        assert!(!crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, &invalid_d));

        // Conflict: destination directory already exists with non-empty contents -> disabled
        let existing_sub = temp_dir.join("ExistingFolder");
        std::fs::create_dir_all(&existing_sub).unwrap();
        std::fs::write(existing_sub.join("data.csv"), b"1,2,3").unwrap();
        invalid_d.fields.insert("location".into(), json!(temp_dir.to_string_lossy()));
        invalid_d.fields.insert("name".into(), json!("ExistingFolder"));
        assert!(!crate::dialogs::create_project::SPEC.ok_enabled.unwrap()(&app, &invalid_d));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 53. Korean project name creation and reopen
    #[test]
    fn test_korean_project_creation_and_reopen() {
        let temp_dir = std::env::temp_dir().join(format!("test_korean_proj_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let target_dir = temp_dir.join("단일세포_유전자_분석");

        create_or_save_project(&mut app, &target_dir, "단일세포_유전자_분석", "new", None).expect("create korean project");
        assert!(target_dir.is_dir());
        assert!(target_dir.join("project.json").exists());

        let mut reopened = make_test_app();
        open_project_from_path(&mut reopened, &target_dir.to_string_lossy()).expect("reopen korean project");
        assert_eq!(reopened.scientific.project_path.as_deref(), Some(target_dir.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 54. Save Project without root opens dialog; with root updates existing folder
    #[test]
    fn test_save_project_root_behavior() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_root_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // 1. No project root -> Save Project opens Create/Save Project dialog
        assert!(app.scientific.project_path.is_none());
        let res = app.run("project.save", json!({})).unwrap();
        assert_eq!(res["pending"], crate::dialogs::create_project::KIND);
        assert_eq!(app.ui.dialog.as_ref().unwrap().str("mode"), "save");

        // Close dialog
        app.ui.dialog = None;

        // 2. Set project root -> Save Project saves directly to existing root, no dialog
        let project_dir = temp_dir.join("ExistingRoot");
        create_or_save_project(&mut app, &project_dir, "ExistingRoot", "new", None).unwrap();
        assert_eq!(app.scientific.project_path.as_deref(), Some(project_dir.to_string_lossy().as_ref()));

        // Mark dirty
        app.scientific.scripts[0].text = "x <- 999\n".into();
        assert!(is_project_dirty(&app));

        let res2 = app.run("project.save", json!({})).unwrap();
        assert_eq!(res2, Value::Null, "Direct save must not return pending dialog");
        assert!(app.ui.dialog.is_none(), "Must not open dialog when root exists");
        assert!(!is_project_dirty(&app), "Project must be marked clean after save");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 55. Save Project As creates separate named directory leaving original intact
    #[test]
    fn test_save_project_as_creates_separate_directory_without_nesting() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_as_nested_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let orig_dir = temp_dir.join("Study_V1");
        create_or_save_project(&mut app, &orig_dir, "Study_V1", "new", None).unwrap();
        app.scientific.scripts[0].untitled_name = "script.R".into();
        app.scientific.scripts[0].text = "# study v1\n".into();
        save_project_to_path(&mut app, &orig_dir.to_string_lossy(), None).unwrap();

        // Save As to "Study_V2"
        let v2_dir = temp_dir.join("Study_V2");
        app.scientific.scripts[0].text = "# study v2\n".into();
        create_or_save_project(&mut app, &v2_dir, "Study_V2", "saveAs", None).unwrap();

        // Verify active root switched to v2
        assert_eq!(app.scientific.project_path.as_deref(), Some(v2_dir.to_string_lossy().as_ref()));

        // Verify no nested folder was created (e.g. Study_V2/Study_V2)
        assert!(!v2_dir.join("Study_V2").exists(), "Must not create nested project folders");

        // Verify original project files remain intact with v1 content
        let orig_script = std::fs::read_to_string(orig_dir.join("scripts").join("script.R")).unwrap();
        assert_eq!(orig_script, "# study v1\n");

        // Verify new project has v2 content
        let v2_script = std::fs::read_to_string(v2_dir.join("scripts").join("script.R")).unwrap();
        assert_eq!(v2_script, "# study v2\n");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 56. Cancel Create Project does not modify filesystem or project state
    #[test]
    fn test_cancel_create_project_preserves_state() {
        let temp_dir = std::env::temp_dir().join(format!("test_cancel_proj_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        open_create_project_dialog(&mut app, "new");
        app.ui.dialog.as_mut().unwrap().fields.insert("location".into(), json!(temp_dir.to_string_lossy()));
        app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("WillBeCancelled"));

        // User hits Cancel (closes dialog)
        app.ui.dialog = None;

        // Verify nothing created on disk
        assert!(!temp_dir.join("WillBeCancelled").exists());
        assert!(app.scientific.project_path.is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 57. Creation failure preserves previous active project
    #[test]
    fn test_creation_failure_preserves_previous_active_project() {
        let temp_dir = std::env::temp_dir().join(format!("test_fail_preserve_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let orig_dir = temp_dir.join("ActiveProject");
        create_or_save_project(&mut app, &orig_dir, "ActiveProject", "new", None).unwrap();
        assert_eq!(app.scientific.project_path.as_deref(), Some(orig_dir.to_string_lossy().as_ref()));

        // Attempt to create a project with an invalid location
        let non_existent_parent = temp_dir.join("NoSuchFolder").join("Sub");
        let fail_res = create_or_save_project(&mut app, &non_existent_parent.join("FailProj"), "FailProj", "new", None);
        assert!(fail_res.is_err(), "Must fail when parent directory does not exist");

        // Previous active project must remain untouched!
        assert_eq!(app.scientific.project_path.as_deref(), Some(orig_dir.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 58. New Project with unsaved changes prompts confirmation
    #[test]
    fn test_new_project_unsaved_changes_confirmation() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.scientific.scripts[0].text = "# unsaved work\n".into();
        assert!(is_project_dirty(&app));

        // project.new without force prompts confirmNewProject
        let res = app.run("project.new", json!({})).unwrap();
        assert_eq!(res["pending"], crate::dialogs::confirm_new_project::KIND);
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_new_project::KIND);

        // Answering Don't Save (discard: true) opens createProject dialog
        let mut d = app.ui.dialog.take().unwrap();
        d.fields.insert("discard".into(), json!(true));
        (crate::dialogs::confirm_new_project::SPEC.confirm)(&mut app, &d).unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::create_project::KIND);
    }

    // 59. Save Project with workspace disabled preserves existing workspace.RData and metadata reference
    #[test]
    fn test_save_project_with_r_workspace_disabled_preserves_existing_rdata() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_ws_disabled_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let project_dir = temp_dir.join("DisabledWS_Project");
        create_or_save_project(&mut app, &project_dir, "DisabledWS_Project", "new", Some(false)).unwrap();

        // Simulate an existing workspace.RData
        let ws_dir = project_dir.join("workspace");
        std::fs::create_dir_all(&ws_dir).unwrap();
        let ws_file = ws_dir.join("workspace.RData");
        let initial_payload = b"VALID_ORIGINAL_RDATA_BYTES_NEVER_OVERWRITTEN";
        std::fs::write(&ws_file, initial_payload).unwrap();
        app.scientific.project_rdata_reference = Some(ws_file.to_string_lossy().into_owned());

        // Resave with workspace saving disabled
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), Some(false)).unwrap();

        // 1. Existing workspace.RData must NOT be removed or modified
        assert!(ws_file.exists());
        let current_payload = std::fs::read(&ws_file).unwrap();
        assert_eq!(current_payload, initial_payload, "Original .RData file must be preserved intact");

        // 2. project.json must keep workspace_file reference and record save_r_workspace = false
        let meta_str = std::fs::read_to_string(project_dir.join("project.json")).unwrap();
        let meta: ProjectMetadata = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta.workspace_file.as_deref(), Some("workspace/workspace.RData"));
        assert_eq!(meta.save_r_workspace, Some(false));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 60. Create Project dialog initializes save_r_workspace from preference or session status
    #[test]
    fn test_create_project_dialog_save_r_workspace_initialization() {
        let mut app = make_test_app();

        // 1. When no preference is set and session is inactive: defaults to false
        app.scientific.session_active = false;
        app.scientific.save_r_workspace_preference = None;
        open_create_project_dialog(&mut app, "new");
        let d1 = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d1.fields.get("save_r_workspace").and_then(Value::as_bool), Some(false));

        // 2. When no preference is set and session is active: defaults to true
        app.scientific.session_active = true;
        app.scientific.save_r_workspace_preference = None;
        open_create_project_dialog(&mut app, "new");
        let d2 = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d2.fields.get("save_r_workspace").and_then(Value::as_bool), Some(true));

        // 3. When preference is explicitly false: preserves false even if session active
        app.scientific.session_active = true;
        app.scientific.save_r_workspace_preference = Some(false);
        open_create_project_dialog(&mut app, "new");
        let d3 = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d3.fields.get("save_r_workspace").and_then(Value::as_bool), Some(false));

        // 4. When preference is explicitly true: preserves true
        app.scientific.save_r_workspace_preference = Some(true);
        open_create_project_dialog(&mut app, "save");
        let d4 = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d4.fields.get("save_r_workspace").and_then(Value::as_bool), Some(true));
    }

    // 61. Dialog confirm passes save_r_workspace and records preference
    #[test]
    fn test_dialog_confirm_records_save_r_workspace_preference() {
        let temp_dir = std::env::temp_dir().join(format!("test_dlg_confirm_ws_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.last_project_parent = Some(temp_dir.clone());
        open_create_project_dialog(&mut app, "new");

        // Set name and uncheck save_r_workspace
        app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("Project_No_WS"));
        app.ui.dialog.as_mut().unwrap().fields.insert("save_r_workspace".into(), json!(false));

        let d = app.ui.dialog.take().unwrap();
        (crate::dialogs::create_project::SPEC.confirm)(&mut app, &d).unwrap();

        // Preference must be recorded on workspace
        assert_eq!(app.scientific.save_r_workspace_preference, Some(false));

        // project.json must reflect the choice
        let target_dir = temp_dir.join("Project_No_WS");
        let meta_str = std::fs::read_to_string(target_dir.join("project.json")).unwrap();
        let meta: ProjectMetadata = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta.save_r_workspace, Some(false));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 62. Ctrl+S uses project's saved preference without prompting repeatedly
    #[test]
    fn test_save_project_ctrl_s_uses_saved_preference() {
        let temp_dir = std::env::temp_dir().join(format!("test_ctrl_s_pref_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let project_dir = temp_dir.join("Preference_Project");
        create_or_save_project(&mut app, &project_dir, "Preference_Project", "new", Some(false)).unwrap();
        assert_eq!(app.scientific.save_r_workspace_preference, Some(false));

        // Make project dirty
        app.scientific.scripts[0].text = "# updated script\n".into();
        assert!(is_project_dirty(&app));

        // Trigger Ctrl+S via file.save command
        let res = app.run("file.save", json!({})).unwrap();
        assert_eq!(res, Value::Null);
        assert!(!is_project_dirty(&app));
        assert!(app.ui.dialog.is_none(), "Ctrl+S must save quietly without re-prompting");

        // Verify project.json preserved preference
        let meta_str = std::fs::read_to_string(project_dir.join("project.json")).unwrap();
        let meta: ProjectMetadata = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta.save_r_workspace, Some(false));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 63. Reopen project restores save_r_workspace preference and rdata reference
    #[test]
    fn test_reopen_project_restores_save_r_workspace_and_reference() {
        let temp_dir = std::env::temp_dir().join(format!("test_reopen_ws_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let project_dir = temp_dir.join("Reopen_Project");
        create_or_save_project(&mut app, &project_dir, "Reopen_Project", "new", Some(true)).unwrap();

        // Place a mock workspace.RData
        let ws_file = project_dir.join("workspace").join("workspace.RData");
        std::fs::write(&ws_file, b"MOCK_RDATA").unwrap();

        // Resave to record reference
        app.scientific.project_rdata_reference = Some(ws_file.to_string_lossy().into_owned());
        save_project_to_path(&mut app, &project_dir.to_string_lossy(), Some(true)).unwrap();

        // Reopen into a fresh app
        let mut fresh = make_test_app();
        open_project_from_path(&mut fresh, &project_dir.to_string_lossy()).expect("reopen project");

        // Preference and workspace reference must be restored
        assert_eq!(fresh.scientific.save_r_workspace_preference, Some(true));
        assert_eq!(fresh.scientific.project_rdata_reference.as_ref().map(std::path::PathBuf::from), Some(ws_file));

        // Core principle verification: Opening project must NOT auto-execute or auto-load R objects
        assert!(fresh.scientific.objects.is_empty(), "Objects must not be auto-loaded into R session");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 64. save_r_workspace_to_project_atomic fails safely when R session is inactive
    #[test]
    fn test_save_r_workspace_atomic_offline_safety() {
        let temp_dir = std::env::temp_dir().join(format!("test_atomic_offline_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.session_active = false;

        let res = save_r_workspace_to_project_atomic(&mut app, &temp_dir);
        assert!(res.is_err(), "Must report error when R session is offline");
        assert!(!temp_dir.join("workspace").join("workspace.RData").exists(), "Must not create empty or corrupt file");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 65. Save Project As preserves user-controlled save_r_workspace setting
    #[test]
    fn test_save_project_as_preserves_save_r_workspace_setting() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_as_ws_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        let orig_dir = temp_dir.join("Source_Project");
        create_or_save_project(&mut app, &orig_dir, "Source_Project", "new", Some(false)).unwrap();

        // Save As to target with explicit save_r_workspace = true
        let copy_dir = temp_dir.join("Copy_Project");
        create_or_save_project(&mut app, &copy_dir, "Copy_Project", "saveAs", Some(true)).unwrap();

        // Check source retained false
        let src_meta: ProjectMetadata = serde_json::from_str(&std::fs::read_to_string(orig_dir.join("project.json")).unwrap()).unwrap();
        assert_eq!(src_meta.save_r_workspace, Some(false));

        // Check copy set to true
        let copy_meta: ProjectMetadata = serde_json::from_str(&std::fs::read_to_string(copy_dir.join("project.json")).unwrap()).unwrap();
        assert_eq!(copy_meta.save_r_workspace, Some(true));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 66. End-to-end R workspace saving and round-trip verification
    #[test]
    fn test_live_r_session_save_workspace_with_project() {
        if crate::r_session::find_rscript().is_err() {
            return;
        }

        let temp_dir = std::env::temp_dir().join(format!("test_live_r_ws_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.session_active = true;

        // 1. Create R objects in the active R session
        {
            let mut session = app.scientific.session.lock().unwrap();
            let exec_res = session.execute("study_val <- 42L; gene_counts <- c(10L, 20L, 30L)");
            assert!(exec_res.is_ok(), "Execution in R session must succeed");
        }

        // 2. Save project with R workspace saving enabled
        let project_dir = temp_dir.join("LiveR_Project");
        let res = create_or_save_project(&mut app, &project_dir, "LiveR_Project", "new", Some(true));
        assert!(res.is_ok(), "create_or_save_project must succeed: {:?}", res.err());

        // 3. Verify workspace/workspace.RData exists and has non-zero size
        let ws_file = project_dir.join("workspace").join("workspace.RData");
        assert!(ws_file.exists(), "workspace.RData must exist when save_r_workspace is true");
        let file_meta = std::fs::metadata(&ws_file).unwrap();
        assert!(file_meta.len() > 0, "workspace.RData must have non-zero size");

        // 4. Verify project.json metadata
        let meta_str = std::fs::read_to_string(project_dir.join("project.json")).unwrap();
        let meta: ProjectMetadata = serde_json::from_str(&meta_str).unwrap();
        assert_eq!(meta.save_r_workspace, Some(true));
        assert_eq!(meta.workspace_file.as_deref(), Some("workspace/workspace.RData"));

        // 5. Inspect the saved .RData file using RSession inspect_rdata to verify contents
        {
            let mut session = app.scientific.session.lock().unwrap();
            let inspection = session.inspect_rdata(ws_file.to_str().unwrap()).unwrap();
            assert!(inspection.object_names.contains(&"study_val".to_string()), "Must contain study_val");
            assert!(inspection.object_names.contains(&"gene_counts".to_string()), "Must contain gene_counts");
            assert_eq!(inspection.total_objects, 2);
        }

        // 6. Test standalone Save R Workspace button (r.workspace.save command)
        {
            let mut session = app.scientific.session.lock().unwrap();
            let _ = session.execute("added_var <- 'StandaloneTest'");
        }
        let save_cmd_res = app.run("r.workspace.save", json!({}));
        assert!(save_cmd_res.is_ok(), "r.workspace.save command must succeed: {:?}", save_cmd_res.err());

        // Verify updated RData contains the new object
        {
            let mut session = app.scientific.session.lock().unwrap();
            let inspection = session.inspect_rdata(ws_file.to_str().unwrap()).unwrap();
            assert!(inspection.object_names.contains(&"added_var".to_string()));
            assert_eq!(inspection.total_objects, 3);
        }

        // 7. Reopen into a fresh app and verify project state
        let mut fresh = make_test_app();
        open_project_from_path(&mut fresh, &project_dir.to_string_lossy()).expect("reopen project");
        assert_eq!(fresh.scientific.save_r_workspace_preference, Some(true));
        assert_eq!(fresh.scientific.project_rdata_reference.as_ref().map(std::path::PathBuf::from), Some(ws_file));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 67. Clean project exits immediately without confirmation
    #[test]
    fn test_exit_clean_project_exits_immediately() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }
        app.scientific.scripts.clear();
        app.scientific.new_script();
        app.scientific.project_dirty = false;
        assert!(!is_project_dirty(&app));

        let res = crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(res, Value::Null);
        assert!(app.ui.dialog.is_none());
        assert_eq!(app.ui.status, "quit");
    }

    // 68. Dirty script triggers confirmExitProject dialog
    #[test]
    fn test_exit_dirty_script_shows_confirm_exit_project_dialog() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }
        app.scientific.scripts.clear();
        app.scientific.new_script();
        app.scientific.scripts[0].text = "data <- c(1, 2, 3)\n".into();
        assert!(is_project_dirty(&app));

        let res = crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(res, json!({"pending": crate::dialogs::confirm_exit_project::KIND}));
        assert!(app.ui.dialog.is_some());
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_exit_project::KIND);
        assert_ne!(app.ui.status, "quit");
    }

    // 69. Dirty figure triggers confirmExitProject dialog even if not active document
    #[test]
    fn test_exit_dirty_figure_shows_confirm_exit_project_dialog() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
        assert!(app.session.documents()[0].is_dirty());

        // Create second document and leave it clean, make it active
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        if let Some(doc) = app.session.active_mut() {
            doc.mark_saved();
        }
        assert_eq!(app.session.documents().len(), 2);
        assert!(!app.session.active().unwrap().is_dirty());
        assert!(app.session.documents()[0].is_dirty());
        assert!(is_project_dirty(&app));

        let res = crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(res, json!({"pending": crate::dialogs::confirm_exit_project::KIND}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_exit_project::KIND);
        assert_ne!(app.ui.status, "quit");
    }

    // 70. Don't Save in confirmExitProject terminates without saving
    #[test]
    fn test_confirm_exit_project_dont_save() {
        let temp_dir = std::env::temp_dir().join(format!("test_dont_save_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.scientific.project_path = Some(temp_dir.to_string_lossy().into_owned());
        app.scientific.scripts.clear();
        app.scientific.new_script();
        app.scientific.scripts[0].text = "unsaved <- TRUE\n".into();

        crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert!(app.ui.dialog.is_some());

        // Choose Don't Save
        app.ui.dialog.as_mut().unwrap().fields.insert("discard".into(), json!(true));
        crate::dialogs::confirm(&mut app).unwrap();

        assert!(app.ui.dialog.is_none());
        assert_eq!(app.ui.status, "quit");
        // No project.json should have been created
        assert!(!temp_dir.join("project.json").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 71. Save & Exit in confirmExitProject saves project and exits
    #[test]
    fn test_confirm_exit_project_save_and_exit_existing_project() {
        let temp_dir = std::env::temp_dir().join(format!("test_save_exit_exist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "x <- 1\n".into(), Some("script.R".into()), None));
        create_or_save_project(&mut app, &temp_dir, "MyProj", "save", Some(false)).unwrap();

        // Make an edit to script
        app.scientific.scripts[0].text = "new_code <- 42\n".into();
        assert!(is_project_dirty(&app));

        crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_exit_project::KIND);

        // Click Save & Exit
        crate::dialogs::confirm(&mut app).unwrap();

        assert!(app.ui.dialog.is_none());
        assert_eq!(app.ui.status, "quit");

        // Script file on disk must contain the updated code
        let saved_script = std::fs::read_to_string(temp_dir.join("scripts").join("script.R")).unwrap();
        assert!(saved_script.contains("new_code <- 42"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // 72. Save & Exit without existing project folder redirects to createProject in saveAndExit mode
    #[test]
    fn test_confirm_exit_project_save_and_exit_unsaved_project_flow() {
        let temp_parent = std::env::temp_dir().join(format!("test_save_exit_new_parent_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_parent);
        std::fs::create_dir_all(&temp_parent).unwrap();

        let mut app = make_test_app();
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        app.scientific.scripts.clear();
        app.scientific.scripts.push(ScriptDocument::new(1, "".into(), Some("script.R".into()), None));
        app.scientific.scripts[0].text = "code <- 'unsaved project'\n".into();
        assert!(app.scientific.project_path.is_none());
        assert!(is_project_dirty(&app));

        // 1. Trigger exit -> confirmExitProject opens
        crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_exit_project::KIND);

        // 2. Confirm Save & Exit -> opens createProject in saveAndExit mode
        crate::dialogs::confirm(&mut app).unwrap();
        assert!(app.ui.dialog.is_some());
        let d = app.ui.dialog.as_ref().unwrap();
        assert_eq!(d.kind, crate::dialogs::create_project::KIND);
        assert_eq!(d.str("mode"), "saveAndExit");

        // 3. Complete createProject dialog
        app.ui.dialog.as_mut().unwrap().fields.insert("name".into(), json!("ExitCreatedProject"));
        app.ui.dialog.as_mut().unwrap().fields.insert("location".into(), json!(temp_parent.to_string_lossy()));
        app.ui.dialog.as_mut().unwrap().fields.insert("save_r_workspace".into(), json!(false));
        crate::dialogs::confirm(&mut app).unwrap();

        // 4. Verify project created and app status is quit
        let target_dir = temp_parent.join("ExitCreatedProject");
        assert!(target_dir.join("project.json").exists());
        assert!(target_dir.join("scripts").join("script.R").exists());
        assert_eq!(app.ui.status, "quit");
        assert!(app.ui.dialog.is_none());

        let _ = std::fs::remove_dir_all(&temp_parent);
    }

    // 73. Standalone figure opened outside project uses document-level saveChanges dialog
    #[test]
    fn test_exit_standalone_figure_preserves_document_save_changes_dialog() {
        let mut app = make_test_app();
        while !app.session.documents().is_empty() {
            app.session.close_document(0);
        }
        app.scientific.scripts.clear();
        app.scientific.project_path = None;
        app.scientific.project_dirty = false;

        // Open a document with a mock standalone figure path
        app.run("file.new", json!({"width": 100, "height": 100})).unwrap();
        if let Some(doc) = app.session.active_mut() {
            doc.path = Some("C:\\data\\single_figure.fide".into());
        }
        // Make it dirty
        app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10})).unwrap();

        assert!(is_standalone_figure_session(&app));

        let res = crate::unsaved::close_all(&mut app, "quit").unwrap();
        assert_eq!(res, json!({"pending": crate::unsaved::KIND}));
        assert_eq!(app.ui.dialog.as_ref().unwrap().kind, crate::unsaved::KIND);
        assert_ne!(app.ui.dialog.as_ref().unwrap().kind, crate::dialogs::confirm_exit_project::KIND);
    }
}
