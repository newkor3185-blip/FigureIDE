//! The FigureIDE `.fide` project container format.
//!
//! A `.fide` file is a versioned ZIP package logically structured as:
//! ```text
//! Project.fide
//! ├── manifest.json
//! ├── scripts/
//! │   ├── analysis.R
//! │   └── figures.R
//! ├── figures/
//! │   ├── figure_001.json
//! │   └── figure_002.json
//! ├── linked_plots/
//! │   ├── panel_001.svg
//! │   └── panel_002.svg
//! ├── previews/
//! │   ├── panel_001.png
//! │   └── panel_002.png
//! ├── workspace/
//! │   └── layout.json
//! └── assets/
//! ```
//!
//! Reopening restores the exact editable figures, script tabs, linked plots, and workspace
//! layout without requiring an active R session, executing scripts, or converting linked
//! graphs into thousands of independent vector objects.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vectorcraft_doc::Document;

use crate::zip::{MAX_ARCHIVE_ENTRY_BYTES, MAX_TOTAL_EXPANSION_BYTES, ZipArchive, ZipError, ZipFileEntry, create_zip, is_safe_entry_path};

pub const FIDE_EXTENSION: &str = "fide";
pub const FIDE_FORMAT_IDENTIFIER: &str = "fide";
pub const FIDE_FIGURE_FORMAT: &str = "figureide-figure";
pub const FIDE_PROJECT_FORMAT: &str = "figureide-project";
pub const FIDE_CURRENT_VERSION: u32 = 1;
pub const FIDE_PROJECT_VERSION: u32 = 2;

#[derive(Debug, thiserror::Error)]
pub enum FideError {
    #[error("ZIP container error: {0}")]
    Zip(#[from] ZipError),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("missing `manifest.json` in project archive")]
    MissingManifest,
    #[error("corrupt manifest: {0}")]
    CorruptManifest(String),
    #[error("unsupported project format `{0}` (expected `{FIDE_FORMAT_IDENTIFIER}`)")]
    UnsupportedFormat(String),
    #[error("project version {0} is newer than this version of FigureIDE supports ({FIDE_CURRENT_VERSION})")]
    UnsupportedVersion(u32),
    #[error("referenced file `{0}` is missing from the project archive")]
    MissingReferencedFile(String),
    #[error("corrupt figure `{0}`: {1}")]
    CorruptFigure(String, String),
    #[error("corrupt script `{0}`: {1}")]
    CorruptScript(String, String),
    #[error("corrupt workspace layout: {0}")]
    CorruptWorkspace(String),
    #[error("failed to serialize project component: {0}")]
    Serialization(String),
}

/// Logical metadata stored in `manifest.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FideManifest {
    pub format: String,
    pub version: u32,
    pub generator: String,
    pub created_at: String,
    #[serde(default)]
    pub primary_figure: String,
    #[serde(default)]
    pub figures: Vec<FideFigureMeta>,
    #[serde(default)]
    pub scripts: Vec<FideScriptMeta>,
    #[serde(default)]
    pub linked_plots: Vec<FideLinkedPlotMeta>,
    #[serde(default = "default_workspace_path")]
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rdata_reference: Option<String>,
}

fn default_workspace_path() -> String {
    "workspace/layout.json".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FideFigureMeta {
    pub id: String,
    pub name: String,
    pub file: String,
    #[serde(default)]
    pub artboard_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FideScriptMeta {
    pub id: u64,
    pub name: String,
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_path: Option<String>,
    #[serde(default)]
    pub is_dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FideLinkedPlotMeta {
    pub panel_id: u64,
    pub generated_id: u64,
    pub r_variable: String,
    pub label: String,
    pub width_mm: f64,
    pub height_mm: f64,
    pub resize_mode: String,
    pub small_figure_typography: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub svg_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_style_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FideWorkspaceLayout {
    pub tab_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<String>,
    pub active_script_id: u64,
    pub editor_fraction: f32,
    pub editor_maximized: bool,
    #[serde(default)]
    pub console_history: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rdata_reference: Option<String>,
}

impl Default for FideWorkspaceLayout {
    fn default() -> Self {
        Self {
            tab_order: vec!["script:1".into()],
            active_tab: Some("script:1".into()),
            active_script_id: 1,
            editor_fraction: 0.5,
            editor_maximized: false,
            console_history: Vec::new(),
            working_directory: None,
            environment_filter: None,
            rdata_reference: None,
        }
    }
}

/// Project metadata stored in `<project>/project.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub generator: String,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub scripts: Vec<FideScriptMeta>,
    #[serde(default)]
    pub figures: Vec<FideFigureMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_figure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_r_workspace: Option<bool>,
    #[serde(default)]
    pub layout: FideWorkspaceLayout,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            format: FIDE_PROJECT_FORMAT.into(),
            version: FIDE_PROJECT_VERSION,
            name: "Untitled Project".into(),
            generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
            created_at: current_iso_time(),
            updated_at: current_iso_time(),
            scripts: Vec::new(),
            figures: Vec::new(),
            active_figure: None,
            workspace_file: None,
            save_r_workspace: None,
            layout: FideWorkspaceLayout::default(),
        }
    }
}

/// Complete representation of a folder-based FigureIDE project workspace.
#[derive(Clone, Debug)]
pub struct FideProjectFolder {
    pub metadata: ProjectMetadata,
    pub scripts: Vec<FideScript>,
    pub figures: Vec<FideFigure>,
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Complete in-memory representation of a FigureIDE project.
#[derive(Clone, Debug)]
pub struct FideProject {
    pub manifest: FideManifest,
    pub figures: Vec<FideFigure>,
    pub scripts: Vec<FideScript>,
    pub linked_plots: Vec<FideLinkedPlot>,
    pub layout: FideWorkspaceLayout,
    pub assets: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct FideFigure {
    pub id: String,
    pub name: String,
    pub doc: Document,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FideScript {
    pub id: u64,
    pub name: String,
    pub text: String,
    pub external_path: Option<String>,
    pub is_dirty: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FideLinkedPlot {
    pub meta: FideLinkedPlotMeta,
    pub svg: Option<String>,
    pub preview_png: Option<Vec<u8>>,
}

impl Default for FideProject {
    fn default() -> Self {
        Self::empty()
    }
}

pub fn current_iso_time() -> String {
    let now_secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    chrono::DateTime::from_timestamp(now_secs, 0).map(|dt| dt.to_rfc3339()).unwrap_or_else(|| "1980-01-01T00:00:00Z".into())
}

impl FideProject {
    pub fn empty() -> Self {
        let manifest = FideManifest {
            format: FIDE_FORMAT_IDENTIFIER.into(),
            version: FIDE_CURRENT_VERSION,
            generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
            created_at: current_iso_time(),
            primary_figure: String::new(),
            figures: Vec::new(),
            scripts: Vec::new(),
            linked_plots: Vec::new(),
            workspace: default_workspace_path(),
            rdata_reference: None,
        };

        Self {
            manifest,
            figures: Vec::new(),
            scripts: Vec::new(),
            linked_plots: Vec::new(),
            layout: FideWorkspaceLayout::default(),
            assets: BTreeMap::new(),
        }
    }

    pub fn new(title: &str) -> Self {
        let manifest = FideManifest {
            format: FIDE_FORMAT_IDENTIFIER.into(),
            version: FIDE_CURRENT_VERSION,
            generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
            created_at: current_iso_time(),
            primary_figure: "figures/figure_001.json".into(),
            figures: vec![FideFigureMeta { id: "figure_001".into(), name: title.into(), file: "figures/figure_001.json".into(), artboard_count: 1 }],
            scripts: vec![FideScriptMeta {
                id: 1,
                name: "Untitled.R".into(),
                file: "scripts/Untitled.R".into(),
                external_path: None,
                is_dirty: false,
            }],
            linked_plots: Vec::new(),
            workspace: default_workspace_path(),
            rdata_reference: None,
        };

        let default_doc = Document::new(210.0 * (72.0 / 25.4), 297.0 * (72.0 / 25.4));
        Self {
            manifest,
            figures: vec![FideFigure { id: "figure_001".into(), name: title.into(), doc: default_doc }],
            scripts: vec![FideScript {
                id: 1,
                name: "Untitled.R".into(),
                text: "# FigureIDE Analysis Script\n".into(),
                external_path: None,
                is_dirty: false,
            }],
            linked_plots: Vec::new(),
            layout: FideWorkspaceLayout::default(),
            assets: BTreeMap::new(),
        }
    }

    /// Serializes this project into a complete `.fide` ZIP package.
    pub fn save_to_bytes(&self) -> Result<Vec<u8>, FideError> {
        let mut zip_entries = Vec::new();

        // 1. Build figure metadata and serialize figures.
        let mut figure_bytes_map = Vec::new();
        let figure_metas: Vec<FideFigureMeta> = self
            .figures
            .iter()
            .map(|f| FideFigureMeta {
                id: f.id.clone(),
                name: f.name.clone(),
                file: format!("figures/{}.json", sanitize_file_stem(&f.id)),
                artboard_count: f.doc.artboards.len(),
            })
            .collect();

        for (figure, meta) in self.figures.iter().zip(&figure_metas) {
            let bytes = crate::save_with(&figure.doc, &crate::SaveOptions { compress: false, pretty: false, ..crate::SaveOptions::default() })
                .map_err(|e| FideError::Serialization(format!("figure {}: {e}", figure.id)))?;
            figure_bytes_map.push((meta.file.clone(), bytes));
        }

        // 2. Build script metadata and stage scripts.
        let mut script_bytes_map = Vec::new();
        let script_metas: Vec<FideScriptMeta> = self
            .scripts
            .iter()
            .map(|s| FideScriptMeta {
                id: s.id,
                name: s.name.clone(),
                file: format!("scripts/{}", sanitize_file_name(&s.name)),
                external_path: s.external_path.clone(),
                is_dirty: s.is_dirty,
            })
            .collect();

        for (script, meta) in self.scripts.iter().zip(&script_metas) {
            script_bytes_map.push((meta.file.clone(), script.text.as_bytes().to_vec()));
        }

        // 3. Stage linked plot SVGs and previews.
        let mut plot_svg_map = Vec::new();
        let mut plot_preview_map = Vec::new();
        let mut linked_plot_metas = Vec::new();

        for plot in &self.linked_plots {
            let mut meta = plot.meta.clone();
            if let Some(svg) = &plot.svg {
                let svg_path = meta.svg_file.clone().unwrap_or_else(|| format!("linked_plots/panel_{}.svg", plot.meta.panel_id));
                plot_svg_map.push((svg_path.clone(), svg.as_bytes().to_vec()));
                meta.svg_file = Some(svg_path);
            }
            if let Some(png) = &plot.preview_png {
                let png_path = meta.preview_file.clone().unwrap_or_else(|| format!("previews/panel_{}.png", plot.meta.panel_id));
                plot_preview_map.push((png_path.clone(), png.clone()));
                meta.preview_file = Some(png_path);
            }
            linked_plot_metas.push(meta);
        }

        // 4. Update manifest to reflect all current staged components.
        let mut manifest = self.manifest.clone();
        manifest.format = FIDE_FORMAT_IDENTIFIER.into();
        if manifest.version == 0 {
            manifest.version = FIDE_CURRENT_VERSION;
        }
        manifest.figures = figure_metas;
        manifest.scripts = script_metas;
        manifest.linked_plots = linked_plot_metas;
        if manifest.rdata_reference.is_none() {
            manifest.rdata_reference = self.layout.rdata_reference.clone();
        }
        if manifest.primary_figure.is_empty() {
            manifest.primary_figure = manifest.figures.first().map(|f| f.file.clone()).unwrap_or_else(|| "figures/figure_001.json".into());
        }

        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| FideError::Serialization(format!("manifest: {e}")))?;

        // 5. Serialize workspace layout.
        let layout_bytes = serde_json::to_vec_pretty(&self.layout).map_err(|e| FideError::Serialization(format!("layout: {e}")))?;

        // 6. Build entry list for ZIP archive.
        // manifest.json
        zip_entries.push(ZipFileEntry { name: "manifest.json", data: &manifest_bytes, deflate: true });

        // layout.json
        zip_entries.push(ZipFileEntry { name: &manifest.workspace, data: &layout_bytes, deflate: true });

        // Figures
        for (path, bytes) in &figure_bytes_map {
            zip_entries.push(ZipFileEntry { name: path, data: bytes, deflate: true });
        }

        // Scripts
        for (path, bytes) in &script_bytes_map {
            zip_entries.push(ZipFileEntry { name: path, data: bytes, deflate: true });
        }

        // Linked plot SVGs
        for (path, bytes) in &plot_svg_map {
            zip_entries.push(ZipFileEntry { name: path, data: bytes, deflate: true });
        }

        // Previews (PNG already compressed: store uncompressed)
        for (path, bytes) in &plot_preview_map {
            zip_entries.push(ZipFileEntry { name: path, data: bytes, deflate: false });
        }

        // Assets
        let asset_paths: Vec<(String, &[u8])> =
            self.assets.iter().map(|(name, bytes)| (format!("assets/{}", sanitize_file_name(name)), bytes.as_slice())).collect();
        for (path, bytes) in &asset_paths {
            zip_entries.push(ZipFileEntry { name: path, data: bytes, deflate: false });
        }

        create_zip(&zip_entries).map_err(FideError::Zip)
    }

    /// Loads and parses a `.fide` project from raw bytes.
    pub fn load_from_bytes(data: &[u8]) -> Result<Self, FideError> {
        let archive = ZipArchive::open(data).map_err(FideError::Zip)?;

        // 1. Read manifest.json
        let manifest_bytes = archive.read("manifest.json", MAX_ARCHIVE_ENTRY_BYTES)?.ok_or(FideError::MissingManifest)?;

        let manifest: FideManifest = serde_json::from_slice(&manifest_bytes).map_err(|e| FideError::CorruptManifest(e.to_string()))?;

        if !manifest.format.eq_ignore_ascii_case(FIDE_FORMAT_IDENTIFIER) {
            return Err(FideError::UnsupportedFormat(manifest.format));
        }
        if manifest.version > FIDE_CURRENT_VERSION {
            return Err(FideError::UnsupportedVersion(manifest.version));
        }

        let mut total_expanded_bytes: usize = manifest_bytes.len();

        // 2. Read workspace layout
        let workspace_file = if manifest.workspace.is_empty() { "workspace/layout.json" } else { &manifest.workspace };

        let mut layout: FideWorkspaceLayout = match archive.read(workspace_file, MAX_ARCHIVE_ENTRY_BYTES)? {
            Some(bytes) => {
                total_expanded_bytes = total_expanded_bytes.saturating_add(bytes.len());
                serde_json::from_slice(&bytes).map_err(|e| FideError::CorruptWorkspace(e.to_string()))?
            }
            None => FideWorkspaceLayout::default(),
        };
        if layout.rdata_reference.is_none() {
            layout.rdata_reference = manifest.rdata_reference.clone();
        }

        // 3. Read figures
        let mut figures = Vec::new();
        for fig_meta in &manifest.figures {
            let file_bytes =
                archive.read(&fig_meta.file, MAX_ARCHIVE_ENTRY_BYTES)?.ok_or_else(|| FideError::MissingReferencedFile(fig_meta.file.clone()))?;

            total_expanded_bytes = total_expanded_bytes.saturating_add(file_bytes.len());
            if total_expanded_bytes > MAX_TOTAL_EXPANSION_BYTES {
                return Err(FideError::Zip(ZipError::TotalSizeExceeded));
            }

            let native_doc = crate::load(&file_bytes).map_err(|e| FideError::CorruptFigure(fig_meta.id.clone(), e.to_string()))?;

            figures.push(FideFigure { id: fig_meta.id.clone(), name: fig_meta.name.clone(), doc: native_doc });
        }

        // If no figures list in manifest, try primary_figure
        if figures.is_empty() && !manifest.primary_figure.is_empty() {
            if let Some(file_bytes) = archive.read(&manifest.primary_figure, MAX_ARCHIVE_ENTRY_BYTES)? {
                total_expanded_bytes = total_expanded_bytes.saturating_add(file_bytes.len());
                let native_doc = crate::load(&file_bytes).map_err(|e| FideError::CorruptFigure(manifest.primary_figure.clone(), e.to_string()))?;
                figures.push(FideFigure { id: "figure_001".into(), name: "Figure 1".into(), doc: native_doc });
            }
        }

        // 4. Read scripts
        let mut scripts = Vec::new();
        for script_meta in &manifest.scripts {
            let script_bytes = archive
                .read(&script_meta.file, MAX_ARCHIVE_ENTRY_BYTES)?
                .ok_or_else(|| FideError::MissingReferencedFile(script_meta.file.clone()))?;

            total_expanded_bytes = total_expanded_bytes.saturating_add(script_bytes.len());
            if total_expanded_bytes > MAX_TOTAL_EXPANSION_BYTES {
                return Err(FideError::Zip(ZipError::TotalSizeExceeded));
            }

            let text = String::from_utf8(script_bytes).map_err(|e| FideError::CorruptScript(script_meta.name.clone(), e.to_string()))?;

            scripts.push(FideScript {
                id: script_meta.id,
                name: script_meta.name.clone(),
                text,
                external_path: script_meta.external_path.clone(),
                is_dirty: script_meta.is_dirty,
            });
        }

        // 5. Read linked plots
        let mut linked_plots = Vec::new();
        for plot_meta in &manifest.linked_plots {
            let svg = if let Some(svg_path) = &plot_meta.svg_file {
                match archive.read(svg_path, MAX_ARCHIVE_ENTRY_BYTES)? {
                    Some(bytes) => {
                        total_expanded_bytes = total_expanded_bytes.saturating_add(bytes.len());
                        String::from_utf8(bytes).ok()
                    }
                    None => None,
                }
            } else {
                None
            };

            let preview_png = if let Some(preview_path) = &plot_meta.preview_file {
                match archive.read(preview_path, MAX_ARCHIVE_ENTRY_BYTES)? {
                    Some(bytes) => {
                        total_expanded_bytes = total_expanded_bytes.saturating_add(bytes.len());
                        Some(bytes)
                    }
                    None => None,
                }
            } else {
                None
            };

            linked_plots.push(FideLinkedPlot { meta: plot_meta.clone(), svg, preview_png });
        }

        // 6. Read extra assets
        let mut assets = BTreeMap::new();
        for name in archive.entry_names() {
            if let Some(asset_subpath) = name.strip_prefix("assets/") {
                if !asset_subpath.is_empty() && is_safe_entry_path(name) {
                    if let Some(bytes) = archive.read(name, MAX_ARCHIVE_ENTRY_BYTES)? {
                        assets.insert(asset_subpath.to_string(), bytes);
                    }
                }
            }
        }

        Ok(Self { manifest, figures, scripts, linked_plots, layout, assets })
    }
}

/// Helper to sanitize file names for entries inside the ZIP archive.
fn sanitize_file_name(name: &str) -> String {
    let sanitized: String = name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' }).collect();
    if sanitized.is_empty() { "file".into() } else { sanitized }
}

fn sanitize_file_stem(stem: &str) -> String {
    let sanitized: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if sanitized.is_empty() { "figure".into() } else { sanitized }
}

/// Validates a prospective project folder name according to cross-platform and Windows safety rules.
pub fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Project name cannot be empty.".to_string());
    }
    if name.trim().is_empty() {
        return Err("Project name cannot be whitespace only.".to_string());
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("Project name cannot end with a dot or space.".to_string());
    }
    if name == "." || name == ".." {
        return Err("Project name cannot be '.' or '..'.".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("Project name cannot contain path separators ('/' or '\\').".to_string());
    }
    for c in name.chars() {
        if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (c as u32) < 32 {
            return Err("Project name contains invalid characters (< > : \" | ? * or control characters).".to_string());
        }
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5",
        "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("'{name}' is a reserved Windows device name."));
    }
    Ok(())
}

/// Validates that the selected parent directory exists and is a valid directory.
pub fn validate_parent_location(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("Location cannot be empty.".to_string());
    }
    if !path.exists() {
        return Err(format!("Selected location does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("Selected location is not a directory: {}", path.display()));
    }
    Ok(())
}

/// Destination conflict status for prospective project folder creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationConflict {
    None,
    ExistingFigureIDEProject(PathBuf),
    DirectoryNotEmpty(PathBuf),
    FileExists(PathBuf),
}

/// Checks whether a target path inside `parent` with `name` conflicts with existing files/folders.
pub fn check_destination_conflict(parent: &Path, name: &str) -> DestinationConflict {
    let target = parent.join(name);
    if !target.exists() {
        return DestinationConflict::None;
    }
    if target.is_file() {
        return DestinationConflict::FileExists(target);
    }
    if target.join("project.json").exists() {
        return DestinationConflict::ExistingFigureIDEProject(target);
    }
    if target.is_dir() {
        if let Ok(mut entries) = std::fs::read_dir(&target) {
            if entries.next().is_some() {
                return DestinationConflict::DirectoryNotEmpty(target);
            }
        }
    }
    DestinationConflict::None
}

/// Generates a clean default project name from a document title or base string.
pub fn sanitize_project_stem(raw: &str) -> String {
    let sanitized: String = raw.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() { "UntitledProject".into() } else { trimmed.to_string() }
}

/// Checks whether `bytes` begins with a `.fide` project container signature (a valid ZIP with manifest.json).
pub fn is_fide_archive(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&0x0403_4b50u32.to_le_bytes()) {
        return false;
    }
    let Ok(archive) = ZipArchive::open(bytes) else {
        return false;
    };
    archive.contains("manifest.json")
}

/// Alias for `is_fide_archive` for legacy single-file archives.
pub use is_fide_archive as is_legacy_fide_archive;

/// Serialize an individual `.fide` figure document to bytes.
pub fn save_fide_figure_to_bytes(doc: &Document) -> Result<Vec<u8>, crate::FormatError> {
    let options =
        crate::SaveOptions { format_override: Some(FIDE_FIGURE_FORMAT), compress: true, include_linked: false, ..crate::SaveOptions::default() };
    crate::save_with(doc, &options)
}

/// Save an individual `.fide` figure document to disk atomically.
pub fn save_fide_figure(path: &Path, doc: &Document) -> Result<(), FideError> {
    let bytes = save_fide_figure_to_bytes(doc).map_err(|e| FideError::Serialization(e.to_string()))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::write_atomic(path, &bytes).map_err(|e| FideError::Io(e.to_string()))?;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::fs::write(path, &bytes).map_err(|e| FideError::Io(e.to_string()))
    }
}

/// Load an individual `.fide` figure document from bytes.
pub fn load_fide_figure_from_bytes(bytes: &[u8]) -> Result<Document, crate::FormatError> {
    crate::load(bytes)
}

/// Load an individual `.fide` figure document from disk.
pub fn load_fide_figure(path: &Path) -> Result<Document, FideError> {
    let bytes = std::fs::read(path).map_err(|e| FideError::Io(e.to_string()))?;
    load_fide_figure_from_bytes(&bytes).map_err(|e| FideError::CorruptFigure(path.display().to_string(), e.to_string()))
}

impl FideProjectFolder {
    pub fn new(name: &str) -> Self {
        let metadata = ProjectMetadata {
            format: FIDE_PROJECT_FORMAT.into(),
            version: FIDE_PROJECT_VERSION,
            name: name.into(),
            generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
            created_at: current_iso_time(),
            updated_at: current_iso_time(),
            scripts: vec![FideScriptMeta {
                id: 1,
                name: "Untitled.R".into(),
                file: "scripts/Untitled.R".into(),
                external_path: None,
                is_dirty: false,
            }],
            figures: vec![FideFigureMeta { id: "fig_1".into(), name: "Figure 1".into(), file: "figures/Figure_01.fide".into(), artboard_count: 1 }],
            active_figure: Some("figures/Figure_01.fide".into()),
            workspace_file: None,
            save_r_workspace: None,
            layout: FideWorkspaceLayout::default(),
        };

        let default_doc = Document::new(210.0 * (72.0 / 25.4), 297.0 * (72.0 / 25.4));
        Self {
            metadata,
            scripts: vec![FideScript {
                id: 1,
                name: "Untitled.R".into(),
                text: "# FigureIDE Analysis Script\n".into(),
                external_path: None,
                is_dirty: false,
            }],
            figures: vec![FideFigure { id: "fig_1".into(), name: "Figure 1".into(), doc: default_doc }],
            assets: BTreeMap::new(),
        }
    }
}

/// Saves a folder-based FigureIDE project workspace to disk.
pub fn save_project_folder(folder_path: &Path, project: &FideProjectFolder) -> Result<(), FideError> {
    std::fs::create_dir_all(folder_path.join("scripts")).map_err(|e| FideError::Io(e.to_string()))?;
    std::fs::create_dir_all(folder_path.join("figures")).map_err(|e| FideError::Io(e.to_string()))?;
    std::fs::create_dir_all(folder_path.join("workspace")).map_err(|e| FideError::Io(e.to_string()))?;
    std::fs::create_dir_all(folder_path.join("assets")).map_err(|e| FideError::Io(e.to_string()))?;

    // 1. Save scripts to scripts/
    let mut script_metas = Vec::new();
    for script in &project.scripts {
        let sname = sanitize_file_name(&script.name);
        let sname = if sname.ends_with(".R") || sname.ends_with(".r") { sname } else { format!("{sname}.R") };
        let file_rel = format!("scripts/{sname}");
        let file_path = folder_path.join(&file_rel);
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::write_atomic(&file_path, script.text.as_bytes()).map_err(|e| FideError::Io(e.to_string()))?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            std::fs::write(&file_path, script.text.as_bytes()).map_err(|e| FideError::Io(e.to_string()))?;
        }
        script_metas.push(FideScriptMeta {
            id: script.id,
            name: script.name.clone(),
            file: file_rel,
            external_path: script.external_path.clone(),
            is_dirty: script.is_dirty,
        });
    }

    // 2. Save figures to figures/*.fide
    let mut figure_metas = Vec::new();
    for (idx, figure) in project.figures.iter().enumerate() {
        let file_rel = if let Some(meta) = project.metadata.figures.iter().find(|m| m.id == figure.id && m.name == figure.name) {
            meta.file.clone()
        } else {
            let stem = sanitize_file_stem(if figure.name.is_empty() { "Figure" } else { &figure.name });
            if project.figures.len() > 1 { format!("figures/{stem}_{:02}.fide", idx + 1) } else { format!("figures/{stem}.fide") }
        };
        let file_path = folder_path.join(&file_rel);
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        save_fide_figure(&file_path, &figure.doc).map_err(|e| FideError::Serialization(e.to_string()))?;
        figure_metas.push(FideFigureMeta {
            id: if figure.id.is_empty() { format!("fig_{}", idx + 1) } else { figure.id.clone() },
            name: figure.name.clone(),
            file: file_rel,
            artboard_count: figure.doc.artboards.len(),
        });
    }

    // 3. Save assets to assets/
    for (name, bytes) in &project.assets {
        let file_path = folder_path.join("assets").join(sanitize_file_name(name));
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::write_atomic(&file_path, bytes).map_err(|e| FideError::Io(e.to_string()))?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            std::fs::write(&file_path, bytes).map_err(|e| FideError::Io(e.to_string()))?;
        }
    }

    // 4. Update metadata and write project.json
    let mut meta = project.metadata.clone();
    meta.format = FIDE_PROJECT_FORMAT.into();
    meta.version = FIDE_PROJECT_VERSION;
    meta.updated_at = current_iso_time();
    meta.scripts = script_metas;
    meta.figures = figure_metas;
    if meta.active_figure.is_none() {
        meta.active_figure = meta.figures.first().map(|f| f.file.clone());
    }
    if meta.workspace_file.is_none() {
        let ws_path = folder_path.join("workspace").join("workspace.RData");
        if ws_path.exists() {
            meta.workspace_file = Some("workspace/workspace.RData".into());
        }
    }

    let json_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| FideError::Serialization(e.to_string()))?;
    let project_json_path = folder_path.join("project.json");
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::write_atomic(&project_json_path, &json_bytes).map_err(|e| FideError::Io(e.to_string()))?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        std::fs::write(&project_json_path, &json_bytes).map_err(|e| FideError::Io(e.to_string()))?;
    }

    Ok(())
}

/// Loads a folder-based FigureIDE project workspace from disk.
pub fn load_project_folder(folder_path: &Path) -> Result<FideProjectFolder, FideError> {
    let project_json_path = if folder_path.is_file() {
        if folder_path.file_name().and_then(|n| n.to_str()) == Some("project.json") {
            folder_path.to_path_buf()
        } else {
            return Err(FideError::MissingReferencedFile(folder_path.display().to_string()));
        }
    } else {
        folder_path.join("project.json")
    };

    let base_dir = project_json_path.parent().unwrap_or(folder_path);

    if !project_json_path.exists() {
        return Err(FideError::MissingManifest);
    }

    let bytes = std::fs::read(&project_json_path).map_err(|e| FideError::Io(e.to_string()))?;
    let metadata: ProjectMetadata = serde_json::from_slice(&bytes).map_err(|e| FideError::CorruptManifest(e.to_string()))?;

    if metadata.format != FIDE_PROJECT_FORMAT && metadata.format != "figureide" && metadata.format != "fide" {
        return Err(FideError::UnsupportedFormat(metadata.format));
    }
    if metadata.version > FIDE_PROJECT_VERSION {
        return Err(FideError::UnsupportedVersion(metadata.version));
    }

    // Load scripts
    let mut scripts = Vec::new();
    for sm in &metadata.scripts {
        let script_path = base_dir.join(&sm.file);
        let text = if script_path.exists() {
            std::fs::read_to_string(&script_path).map_err(|e| FideError::CorruptScript(sm.file.clone(), e.to_string()))?
        } else {
            String::new()
        };
        scripts.push(FideScript { id: sm.id, name: sm.name.clone(), text, external_path: sm.external_path.clone(), is_dirty: sm.is_dirty });
    }

    // Load figures
    let mut figures = Vec::new();
    for fm in &metadata.figures {
        let fig_path = base_dir.join(&fm.file);
        if fig_path.exists() {
            let doc = load_fide_figure(&fig_path).map_err(|e| FideError::CorruptFigure(fm.file.clone(), e.to_string()))?;
            figures.push(FideFigure { id: fm.id.clone(), name: fm.name.clone(), doc });
        }
    }

    if figures.is_empty() {
        if let Some(ref af) = metadata.active_figure {
            let fig_path = base_dir.join(af);
            if fig_path.exists() {
                let doc = load_fide_figure(&fig_path).map_err(|e| FideError::CorruptFigure(af.clone(), e.to_string()))?;
                figures.push(FideFigure { id: "fig_1".into(), name: "Figure 1".into(), doc });
            }
        }
    }

    // Load assets
    let mut assets = BTreeMap::new();
    let assets_dir = base_dir.join("assets");
    if assets_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(assets_dir) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if let Ok(data) = std::fs::read(entry.path()) {
                            assets.insert(file_name.to_string(), data);
                        }
                    }
                }
            }
        }
    }

    Ok(FideProjectFolder { metadata, scripts, figures, assets })
}

/// Migrates a legacy single-file `.fide` ZIP package into a folder-based project without modifying the original file.
pub fn migrate_legacy_fide_archive_to_folder(bytes: &[u8], target_folder: &Path) -> Result<FideProjectFolder, FideError> {
    let legacy_proj = FideProject::load_from_bytes(bytes)?;
    let name = target_folder.file_name().and_then(|n| n.to_str()).filter(|s| !s.is_empty()).unwrap_or("Migrated_Project").to_string();

    let meta = ProjectMetadata {
        format: FIDE_PROJECT_FORMAT.into(),
        version: FIDE_PROJECT_VERSION,
        name,
        generator: format!("FigureIDE {}", env!("CARGO_PKG_VERSION")),
        created_at: legacy_proj.manifest.created_at.clone(),
        updated_at: current_iso_time(),
        scripts: legacy_proj.manifest.scripts.clone(),
        figures: legacy_proj
            .manifest
            .figures
            .iter()
            .map(|f| FideFigureMeta {
                id: f.id.clone(),
                name: f.name.clone(),
                file: format!("figures/{}.fide", sanitize_file_stem(&f.name)),
                artboard_count: f.artboard_count,
            })
            .collect(),
        active_figure: legacy_proj.manifest.figures.first().map(|f| format!("figures/{}.fide", sanitize_file_stem(&f.name))),
        workspace_file: legacy_proj.manifest.rdata_reference.clone(),
        save_r_workspace: None,
        layout: legacy_proj.layout.clone(),
    };

    let folder_proj = FideProjectFolder { metadata: meta, scripts: legacy_proj.scripts, figures: legacy_proj.figures, assets: legacy_proj.assets };

    save_project_folder(target_folder, &folder_proj)?;
    Ok(folder_proj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_doc::{Appearance, Node};
    use vectorcraft_geom::{Rect, shapes};

    #[test]
    fn test_fide_project_roundtrip() {
        let mut proj = FideProject::new("Airway Study");

        // Two R scripts
        proj.scripts = vec![
            FideScript {
                id: 1,
                name: "01_analysis.R".into(),
                text: "library(DESeq2)\nres <- results(dds)\n".into(),
                external_path: Some("C:/data/01_analysis.R".into()),
                is_dirty: false,
            },
            FideScript {
                id: 2,
                name: "02_figures.R".into(),
                text: "library(ggplot2)\np <- ggplot(res, aes(x=log2FoldChange, y=-log10(pvalue))) + geom_point()\n".into(),
                external_path: None,
                is_dirty: true,
            },
        ];

        // Two figures
        let mut doc1 = Document::new(210.0, 297.0);
        let id1 = doc1.alloc_id();
        doc1.insert(Some(doc1.layers[0].id), 0, Node::path(id1, shapes::rectangle(Rect::new(10.0, 10.0, 50.0, 50.0)), Appearance::default_art()))
            .unwrap();

        let mut doc2 = Document::new(100.0, 100.0);
        let id2 = doc2.alloc_id();
        doc2.insert(Some(doc2.layers[0].id), 0, Node::path(id2, shapes::ellipse(Rect::new(5.0, 5.0, 40.0, 40.0)), Appearance::default_art()))
            .unwrap();

        proj.figures = vec![
            FideFigure { id: "fig_1".into(), name: "Differential Expression".into(), doc: doc1 },
            FideFigure { id: "fig_2".into(), name: "Pathway Enrichment".into(), doc: doc2 },
        ];

        // Linked plot with SVG and preview
        proj.linked_plots = vec![FideLinkedPlot {
            meta: FideLinkedPlotMeta {
                panel_id: 101,
                generated_id: 102,
                r_variable: "volcano_plot".into(),
                label: "Figure 1A".into(),
                width_mm: 75.0,
                height_mm: 55.0,
                resize_mode: "reflow".into(),
                small_figure_typography: true,
                svg_file: None,
                preview_file: None,
                graph_style_json: Some(r#"{"font_family":"Arial","base_size":9.0}"#.into()),
            },
            svg: Some(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><circle cx="50" cy="50" r="40" fill="blue"/></svg>"#.into(),
            ),
            preview_png: Some(vec![137, 80, 78, 71, 13, 10, 26, 10]),
        }];

        // Custom layout
        proj.layout = FideWorkspaceLayout {
            tab_order: vec!["script:1".into(), "script:2".into(), "viewer:res".into()],
            active_tab: Some("script:2".into()),
            active_script_id: 2,
            editor_fraction: 0.65,
            editor_maximized: false,
            console_history: vec!["summary(res)".into(), "plotMA(res)".into()],
            working_directory: Some("C:/projects/airway".into()),
            environment_filter: Some("sig_".into()),
            rdata_reference: Some("workspace/airway.RData".into()),
        };

        // Save to bytes
        let bytes = proj.save_to_bytes().unwrap();
        assert!(is_fide_archive(&bytes));

        // Reopen project
        let loaded = FideProject::load_from_bytes(&bytes).unwrap();

        // Validate manifest
        assert_eq!(loaded.manifest.format, FIDE_FORMAT_IDENTIFIER);
        assert_eq!(loaded.manifest.version, FIDE_CURRENT_VERSION);

        // Validate scripts
        assert_eq!(loaded.scripts.len(), 2);
        assert_eq!(loaded.scripts[0].name, "01_analysis.R");
        assert_eq!(loaded.scripts[0].external_path, Some("C:/data/01_analysis.R".into()));
        assert!(!loaded.scripts[0].is_dirty);
        assert!(loaded.scripts[0].text.contains("DESeq2"));

        assert_eq!(loaded.scripts[1].name, "02_figures.R");
        assert!(loaded.scripts[1].is_dirty);
        assert!(loaded.scripts[1].text.contains("ggplot2"));

        // Validate figures
        assert_eq!(loaded.figures.len(), 2);
        assert_eq!(loaded.figures[0].name, "Differential Expression");
        assert_eq!(loaded.figures[0].doc.node_count(), 2); // layer + path
        assert_eq!(loaded.figures[1].name, "Pathway Enrichment");
        assert_eq!(loaded.figures[1].doc.node_count(), 2);

        // Validate linked plots
        assert_eq!(loaded.linked_plots.len(), 1);
        let plot = &loaded.linked_plots[0];
        assert_eq!(plot.meta.panel_id, 101);
        assert_eq!(plot.meta.r_variable, "volcano_plot");
        assert_eq!(plot.meta.label, "Figure 1A");
        assert_eq!(plot.meta.width_mm, 75.0);
        assert_eq!(plot.meta.height_mm, 55.0);
        assert_eq!(plot.meta.resize_mode, "reflow");
        assert!(plot.meta.small_figure_typography);
        assert_eq!(
            plot.svg.as_deref(),
            Some(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><circle cx="50" cy="50" r="40" fill="blue"/></svg>"#)
        );
        assert_eq!(plot.preview_png.as_deref(), Some(&[137, 80, 78, 71, 13, 10, 26, 10][..]));

        // Validate layout
        assert_eq!(loaded.layout.tab_order, vec!["script:1", "script:2", "viewer:res"]);
        assert_eq!(loaded.layout.active_tab.as_deref(), Some("script:2"));
        assert_eq!(loaded.layout.active_script_id, 2);
        assert_eq!(loaded.layout.rdata_reference.as_deref(), Some("workspace/airway.RData"));
        assert_eq!(loaded.manifest.rdata_reference.as_deref(), Some("workspace/airway.RData"));
        assert_eq!(loaded.layout.editor_fraction, 0.65);
        assert_eq!(loaded.layout.console_history.len(), 2);
        assert_eq!(loaded.layout.working_directory.as_deref(), Some("C:/projects/airway"));
    }

    #[test]
    fn test_rejects_corrupted_archive() {
        assert!(FideProject::load_from_bytes(b"corrupt non-zip bytes").is_err());
    }

    #[test]
    fn test_rejects_unsupported_version() {
        let proj = FideProject::new("Future Test");
        // Modify manifest version to 999
        let future_manifest = serde_json::to_vec(&FideManifest { version: 999, ..proj.manifest }).unwrap();
        let entries = vec![ZipFileEntry { name: "manifest.json", data: &future_manifest, deflate: true }];
        let bytes = create_zip(&entries).unwrap();
        let res = FideProject::load_from_bytes(&bytes);
        assert!(matches!(res, Err(FideError::UnsupportedVersion(999))));
    }

    #[test]
    fn test_rejects_unsafe_traversal_paths() {
        assert!(!is_safe_entry_path("../../etc/passwd"));
        assert!(!is_safe_entry_path("/root/secrets.txt"));
        assert!(!is_safe_entry_path("C:\\Windows\\System32\\cmd.exe"));
        assert!(!is_safe_entry_path("scripts/../../sneaky.R"));
        assert!(is_safe_entry_path("scripts/analysis.R"));
        assert!(is_safe_entry_path("figures/figure_001.json"));
        assert!(is_safe_entry_path("previews/panel_1.png"));
    }

    #[test]
    fn test_fide_figure_document_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("fide_fig_test_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let fig_path = temp_dir.join("Figure_01.fide");

        let mut doc = Document::new(210.0, 297.0);
        let id1 = doc.alloc_id();
        doc.insert(Some(doc.layers[0].id), 0, Node::path(id1, shapes::rectangle(Rect::new(10.0, 10.0, 50.0, 50.0)), Appearance::default_art()))
            .unwrap();

        // 1. Save as .fide
        save_fide_figure(&fig_path, &doc).unwrap();
        assert!(fig_path.exists());

        // 2. Load via load_fide_figure
        let loaded_doc = load_fide_figure(&fig_path).unwrap();
        assert_eq!(loaded_doc.artboards.len(), 1);
        assert_eq!(loaded_doc.node_count(), 2);

        // 3. Load transparently via crate::load (native loader)
        let bytes = std::fs::read(&fig_path).unwrap();
        let native_doc = crate::load(&bytes).unwrap();
        assert_eq!(native_doc.artboards.len(), 1);
        assert_eq!(native_doc.node_count(), 2);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_folder_based_project_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("fide_folder_proj_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut proj = FideProjectFolder::new("Test Study");
        proj.scripts = vec![
            FideScript { id: 1, name: "analysis.R".into(), text: "data <- rnorm(100)\n".into(), external_path: None, is_dirty: false },
            FideScript {
                id: 2,
                name: "figures.R".into(),
                text: "plot(data)\n".into(),
                external_path: Some("C:/scripts/figures.R".into()),
                is_dirty: true,
            },
        ];

        let mut doc1 = Document::new(200.0, 200.0);
        doc1.title = "Main Figure".into();
        proj.figures = vec![FideFigure { id: "fig_1".into(), name: "Main Figure".into(), doc: doc1 }];

        // Save folder project
        save_project_folder(&temp_dir, &proj).unwrap();

        // Check directory tree
        assert!(temp_dir.join("project.json").exists());
        assert!(temp_dir.join("scripts").join("analysis.R").exists());
        assert!(temp_dir.join("scripts").join("figures.R").exists());
        assert!(temp_dir.join("figures").join("Main_Figure.fide").exists());
        assert!(temp_dir.join("workspace").exists());
        assert!(temp_dir.join("assets").exists());

        // Load folder project back
        let loaded = load_project_folder(&temp_dir).unwrap();
        assert_eq!(loaded.metadata.name, "Test Study");
        assert_eq!(loaded.metadata.format, FIDE_PROJECT_FORMAT);
        assert_eq!(loaded.metadata.version, FIDE_PROJECT_VERSION);
        assert_eq!(loaded.scripts.len(), 2);
        assert_eq!(loaded.scripts[0].name, "analysis.R");
        assert_eq!(loaded.scripts[0].text, "data <- rnorm(100)\n");
        assert_eq!(loaded.scripts[1].name, "figures.R");
        assert_eq!(loaded.scripts[1].text, "plot(data)\n");
        assert_eq!(loaded.figures.len(), 1);
        assert_eq!(loaded.figures[0].name, "Main Figure");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_migrate_legacy_fide_archive() {
        let mut legacy_proj = FideProject::new("Legacy Study");
        legacy_proj.scripts =
            vec![FideScript { id: 10, name: "legacy.R".into(), text: "print('hello from legacy')\n".into(), external_path: None, is_dirty: false }];
        let legacy_bytes = legacy_proj.save_to_bytes().unwrap();
        assert!(is_legacy_fide_archive(&legacy_bytes));

        let temp_dir = std::env::temp_dir().join(format!("fide_migrated_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let migrated = migrate_legacy_fide_archive_to_folder(&legacy_bytes, &temp_dir).unwrap();
        assert_eq!(migrated.metadata.format, FIDE_PROJECT_FORMAT);
        assert_eq!(migrated.metadata.version, FIDE_PROJECT_VERSION);
        assert_eq!(migrated.scripts.len(), 1);
        assert_eq!(migrated.scripts[0].name, "legacy.R");
        assert!(temp_dir.join("project.json").exists());
        assert!(temp_dir.join("scripts").join("legacy.R").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_validate_project_name_rules() {
        // Valid names
        assert!(validate_project_name("SingleCell_Analysis").is_ok());
        assert!(validate_project_name("Airway-Analysis_2026").is_ok());
        assert!(validate_project_name("단일세포_분석").is_ok()); // Korean
        assert!(validate_project_name("Project with spaces").is_ok());
        assert!(validate_project_name("My.Project").is_ok());

        // Invalid: empty or whitespace
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name("   ").is_err());

        // Invalid: . or ..
        assert!(validate_project_name(".").is_err());
        assert!(validate_project_name("..").is_err());

        // Invalid: trailing dot or space
        assert!(validate_project_name("MyProject.").is_err());
        assert!(validate_project_name("MyProject ").is_err());

        // Invalid: path separators
        assert!(validate_project_name("a/b").is_err());
        assert!(validate_project_name("a\\b").is_err());

        // Invalid: Windows-forbidden characters
        assert!(validate_project_name("proj:name").is_err());
        assert!(validate_project_name("proj*name").is_err());
        assert!(validate_project_name("proj?name").is_err());
        assert!(validate_project_name("proj\"name").is_err());
        assert!(validate_project_name("proj<name").is_err());
        assert!(validate_project_name("proj>name").is_err());
        assert!(validate_project_name("proj|name").is_err());

        // Invalid: reserved Windows device names
        assert!(validate_project_name("CON").is_err());
        assert!(validate_project_name("con").is_err());
        assert!(validate_project_name("PRN").is_err());
        assert!(validate_project_name("AUX").is_err());
        assert!(validate_project_name("NUL").is_err());
        assert!(validate_project_name("COM1").is_err());
        assert!(validate_project_name("com9").is_err());
        assert!(validate_project_name("LPT3").is_err());
        assert!(validate_project_name("aux.data").is_err());
    }

    #[test]
    fn test_check_destination_conflict() {
        let temp_dir = std::env::temp_dir().join(format!("dest_conflict_{}", std::time::SystemTime::now().elapsed().unwrap().as_nanos()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // 1. None for non-existent target
        assert_eq!(check_destination_conflict(&temp_dir, "NewFolder"), DestinationConflict::None);

        // 2. ExistingFigureIDEProject
        let proj_dir = temp_dir.join("ExistingProject");
        std::fs::create_dir_all(&proj_dir).unwrap();
        std::fs::write(proj_dir.join("project.json"), b"{}").unwrap();
        assert_eq!(check_destination_conflict(&temp_dir, "ExistingProject"), DestinationConflict::ExistingFigureIDEProject(proj_dir.clone()));

        // 3. DirectoryNotEmpty
        let non_proj_dir = temp_dir.join("NonProjFolder");
        std::fs::create_dir_all(&non_proj_dir).unwrap();
        std::fs::write(non_proj_dir.join("notes.txt"), b"some notes").unwrap();
        assert_eq!(check_destination_conflict(&temp_dir, "NonProjFolder"), DestinationConflict::DirectoryNotEmpty(non_proj_dir.clone()));

        // 4. FileExists
        let file_path = temp_dir.join("SomeFile.txt");
        std::fs::write(&file_path, b"content").unwrap();
        assert_eq!(check_destination_conflict(&temp_dir, "SomeFile.txt"), DestinationConflict::FileExists(file_path.clone()));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
