//! FigureIDE publication-quality scientific figure export to SVG and PDF.

use crate::VectorcraftApp;
use serde_json::{Value, json};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use vectorcraft_doc::{Document, NodeKind};

#[derive(Clone, Debug, Default)]
pub struct FigureExportOptions {
    pub path: Option<String>,
    pub figure_index: Option<usize>,
    pub figure_id: Option<String>,
    pub artboard_index: Option<usize>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub overwrite: bool,
}

#[derive(Clone, Debug)]
pub struct FigureExportResult {
    pub path: String,
    pub bytes_written: usize,
    pub width_mm: f64,
    pub height_mm: f64,
    pub figure_name: String,
    pub warnings: Vec<String>,
}

/// Prepares a document snapshot for publication-quality figure export.
///
/// Ensures all placed documents (including Hybrid Linked Plots) have their authoritative
/// vector art expanded into the export tree, while leaving the active session document untouched.
pub fn prepare_figure_document(doc: &Document) -> Document {
    let has_placed = doc.layers.iter().any(|layer| {
        let mut found = false;
        layer.walk(&mut |node| {
            if matches!(node.kind, NodeKind::PlacedDocument(_)) {
                found = true;
            }
        });
        found
    });

    if has_placed {
        let mut prepared = doc.with_placed_art().into_owned();
        prepared.placed_as_groups();
        prepared
    } else {
        doc.clone()
    }
}

/// Resolves the intended figure document and metadata from the application session.
pub fn resolve_figure<'a>(app: &'a VectorcraftApp, options: &FigureExportOptions) -> Result<(usize, String, &'a Document), String> {
    let docs = app.session.documents();
    if docs.is_empty() {
        return Err("No open figure document to export".into());
    }

    let (index, st) = if let Some(idx) = options.figure_index {
        let state = docs.get(idx).ok_or_else(|| format!("Figure index {idx} out of bounds (0..{})", docs.len()))?;
        (idx, state)
    } else if let Some(ref id) = options.figure_id {
        let pos = docs
            .iter()
            .position(|d| d.doc.title == *id || format!("figure_{:03}", d.uid) == *id)
            .ok_or_else(|| format!("Figure '{id}' not found"))?;
        (pos, &docs[pos])
    } else {
        let active_idx = app.session.active_index().unwrap_or(0);
        (active_idx, app.session.active().ok_or("No active figure document")?)
    };

    let figure_name = if !st.doc.title.trim().is_empty() { st.doc.title.trim().to_string() } else { format!("Figure_{}", index + 1) };

    Ok((index, figure_name, &st.doc))
}

/// Sanitizes a string for use as a filesystem file name.
pub fn sanitize_filename(name: &str) -> String {
    let filtered: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' || c == ' ' { c } else { '_' }).collect();
    let trimmed = filtered.trim();
    if trimmed.is_empty() { "Figure".into() } else { trimmed.into() }
}

fn render_figure_svg(app: &VectorcraftApp, options: &FigureExportOptions) -> Result<(FigureExportResult, Vec<u8>), String> {
    let (_idx, figure_name, doc) = resolve_figure(app, options)?;

    let artboard_idx = options.artboard_index.unwrap_or(0);
    let artboard = doc.artboards.get(artboard_idx).ok_or("Figure document has no artboard")?;
    let width_mm = (artboard.rect.width() * 25.4 / 72.0 * 1000.0).round() / 1000.0;
    let height_mm = (artboard.rect.height() * 25.4 / 72.0 * 1000.0).round() / 1000.0;

    let target_path = options.path.clone().unwrap_or_else(|| format!("{}.svg", sanitize_filename(&figure_name)));

    let prepared = prepare_figure_document(doc);

    let svg_opts = vectorcraft_svg::ExportOptions {
        artboard: Some(artboard_idx),
        artboard_title: true,
        physical_units: true,
        decimals: 3,
        images: vectorcraft_svg::ImageMode::Embed,
        outline_text: false,
        ..Default::default()
    };

    let mut out = vectorcraft_svg::export_full(&prepared, &svg_opts, None);
    let svg_bytes = out.take_bytes();

    let result = FigureExportResult { path: target_path, bytes_written: svg_bytes.len(), width_mm, height_mm, figure_name, warnings: out.warnings };
    Ok((result, svg_bytes))
}

fn render_figure_pdf(app: &VectorcraftApp, options: &FigureExportOptions) -> Result<(FigureExportResult, Vec<u8>), String> {
    let (_idx, figure_name, doc) = resolve_figure(app, options)?;

    let artboard_idx = options.artboard_index.unwrap_or(0);
    let artboard = doc.artboards.get(artboard_idx).ok_or("Figure document has no artboard")?;
    let width_mm = (artboard.rect.width() * 25.4 / 72.0 * 1000.0).round() / 1000.0;
    let height_mm = (artboard.rect.height() * 25.4 / 72.0 * 1000.0).round() / 1000.0;

    let target_path = options.path.clone().unwrap_or_else(|| format!("{}.pdf", sanitize_filename(&figure_name)));

    let prepared = prepare_figure_document(doc);

    let pdf_opts = vectorcraft_pdf::PdfOptions {
        settings: vectorcraft_pdf::PdfSettings::default(),
        artboards: Some(vec![artboard_idx]),
        display_doc_title: true,
        title: Some(figure_name.clone()),
        ..Default::default()
    };

    let rep = vectorcraft_pdf::export_with_report(&prepared, &pdf_opts).map_err(|e| format!("PDF export generation failed: {e}"))?;

    let result = FigureExportResult { path: target_path, bytes_written: rep.bytes.len(), width_mm, height_mm, figure_name, warnings: rep.warnings };
    Ok((result, rep.bytes))
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_native_target(path: &str, overwrite: bool) -> Result<(), String> {
    let path_obj = Path::new(path);
    if path_obj.exists() && !overwrite {
        return Err(format!("Destination file already exists: {path}"));
    }
    if let Some(parent) = path_obj.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        return Err(format!("Destination directory does not exist: {}", parent.display()));
    }
    Ok(())
}

/// Exports the specified or active figure to publication-quality SVG on desktop.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_figure_svg(app: &VectorcraftApp, options: &FigureExportOptions) -> Result<FigureExportResult, String> {
    let (result, bytes) = render_figure_svg(app, options)?;
    validate_native_target(&result.path, options.overwrite)?;
    let path = Path::new(&result.path);
    vectorcraft_format::write_atomic(path, &bytes).map_err(|e| format!("Failed to write SVG atomically to {}: {e}", result.path))?;
    let meta = std::fs::metadata(path).map_err(|e| format!("Verification failed for written SVG {}: {e}", result.path))?;
    if meta.len() != bytes.len() as u64 {
        return Err(format!("SVG file verification size mismatch for {}", result.path));
    }
    Ok(result)
}

/// The path-based exporter is unavailable in the browser; commands use browser downloads instead.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub fn export_figure_svg(_app: &VectorcraftApp, _options: &FigureExportOptions) -> Result<FigureExportResult, String> {
    Err("Use the SVG export command to download a figure in the browser".into())
}

/// Exports the specified or active figure to publication-quality PDF on desktop.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_figure_pdf(app: &VectorcraftApp, options: &FigureExportOptions) -> Result<FigureExportResult, String> {
    let (result, bytes) = render_figure_pdf(app, options)?;
    validate_native_target(&result.path, options.overwrite)?;
    let path = Path::new(&result.path);
    vectorcraft_format::write_atomic(path, &bytes).map_err(|e| format!("Failed to write PDF atomically to {}: {e}", result.path))?;
    let meta = std::fs::metadata(path).map_err(|e| format!("Verification failed for written PDF {}: {e}", result.path))?;
    if meta.len() != bytes.len() as u64 {
        return Err(format!("PDF file verification size mismatch for {}", result.path));
    }
    Ok(result)
}

/// The path-based exporter is unavailable in the browser; commands use browser downloads instead.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub fn export_figure_pdf(_app: &VectorcraftApp, _options: &FigureExportOptions) -> Result<FigureExportResult, String> {
    Err("Use the PDF export command to download a figure in the browser".into())
}

/// Command handler for `figure.export.svg` / `figure.exportSvg`.
pub fn export_figure_svg_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let path = p.get("path").and_then(Value::as_str).map(str::to_string);
    let figure_index = p.get("figureIndex").and_then(Value::as_u64).map(|i| i as usize);
    let figure_id = p.get("figureId").and_then(Value::as_str).map(str::to_string);
    let artboard_index = p.get("artboard").and_then(Value::as_u64).map(|i| i as usize);
    let overwrite = p.get("overwrite").and_then(Value::as_bool).unwrap_or(true);

    let chosen_path = match path {
        Some(explicit) => explicit,
        None => {
            let (_, figure_name, _) =
                resolve_figure(app, &FigureExportOptions { figure_index, figure_id: figure_id.clone(), artboard_index, ..Default::default() })?;
            let default_name = format!("{}.svg", sanitize_filename(&figure_name));
            let pick =
                crate::FilePick { name: default_name.clone(), filters: vec![crate::FilePick::filter("SVG Image", &["svg"])], ..Default::default() };
            match crate::picks::save(app, &pick) {
                Some(picked) => picked,
                None => default_name,
            }
        }
    };

    let options = FigureExportOptions { path: Some(chosen_path.clone()), figure_index, figure_id, artboard_index, overwrite };
    #[cfg(not(target_arch = "wasm32"))]
    let result = export_figure_svg(app, &options)?;
    #[cfg(target_arch = "wasm32")]
    let result = {
        let (mut result, bytes) = render_figure_svg(app, &options)?;
        result.path = crate::io::write_named(app, Some(chosen_path), &result.path.clone(), &bytes)?;
        result
    };

    app.status(format!("Exported {} to {}", result.figure_name, result.path));
    Ok(json!({
        "path": result.path,
        "bytes": result.bytes_written,
        "width_mm": result.width_mm,
        "height_mm": result.height_mm,
        "figure": result.figure_name,
        "warnings": result.warnings,
    }))
}

/// Command handler for `figure.export.pdf` / `figure.exportPdf`.
pub fn export_figure_pdf_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let path = p.get("path").and_then(Value::as_str).map(str::to_string);
    let figure_index = p.get("figureIndex").and_then(Value::as_u64).map(|i| i as usize);
    let figure_id = p.get("figureId").and_then(Value::as_str).map(str::to_string);
    let artboard_index = p.get("artboard").and_then(Value::as_u64).map(|i| i as usize);
    let overwrite = p.get("overwrite").and_then(Value::as_bool).unwrap_or(true);

    let chosen_path = match path {
        Some(explicit) => explicit,
        None => {
            let (_, figure_name, _) =
                resolve_figure(app, &FigureExportOptions { figure_index, figure_id: figure_id.clone(), artboard_index, ..Default::default() })?;
            let default_name = format!("{}.pdf", sanitize_filename(&figure_name));
            let pick = crate::FilePick {
                name: default_name.clone(),
                filters: vec![crate::FilePick::filter("PDF Document", &["pdf"])],
                ..Default::default()
            };
            match crate::picks::save(app, &pick) {
                Some(picked) => picked,
                None => default_name,
            }
        }
    };

    let options = FigureExportOptions { path: Some(chosen_path.clone()), figure_index, figure_id, artboard_index, overwrite };
    #[cfg(not(target_arch = "wasm32"))]
    let result = export_figure_pdf(app, &options)?;
    #[cfg(target_arch = "wasm32")]
    let result = {
        let (mut result, bytes) = render_figure_pdf(app, &options)?;
        result.path = crate::io::write_named(app, Some(chosen_path), &result.path.clone(), &bytes)?;
        result
    };

    app.status(format!("Exported {} to {}", result.figure_name, result.path));
    Ok(json!({
        "path": result.path,
        "bytes": result.bytes_written,
        "width_mm": result.width_mm,
        "height_mm": result.height_mm,
        "figure": result.figure_name,
        "warnings": result.warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scientific::project::{build_fide_project, open_fide_project_bytes};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;
    use vectorcraft_doc::{NodeId, NodeKind};

    const DOC_POINTS_PER_MM: f64 = 72.0 / 25.4;

    fn make_test_app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app
    }

    struct TempTestDir(PathBuf);

    impl TempTestDir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let count = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("vectorcraft_test_export_{}_{}", std::process::id(), count));
            let _ = fs::create_dir_all(&p);
            Self(p)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample_plot_svg() -> &'static str {
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" viewBox="0 0 200 150">
  <defs>
    <clipPath id="plot-clip"><rect x="10" y="10" width="180" height="130"/></clipPath>
  </defs>
  <g clip-path="url(#plot-clip)">
    <circle cx="50" cy="50" r="8" fill="#377eb8" fill-opacity="0.4"/>
    <circle cx="55" cy="52" r="8" fill="#e41a1c" fill-opacity="0.4"/>
    <path d="M 20 120 L 180 30" stroke="#4daf4a" stroke-width="2.5"/>
  </g>
  <text x="30" y="140" font-size="12" font-family="sans-serif">Gene Alpha</text>
</svg>"##
    }

    fn add_test_hybrid_plot(app: &mut VectorcraftApp, svg: &str, width_mm: f64, height_mm: f64) -> u64 {
        let width_pt = width_mm * DOC_POINTS_PER_MM;
        let height_pt = height_mm * DOC_POINTS_PER_MM;
        let res = app
            .run(
                "object.importRPlotSource",
                json!({
                    "svg": svg,
                    "width": width_pt,
                    "height": height_pt,
                }),
            )
            .expect("importRPlotSource");
        res["id"].as_u64().expect("id")
    }

    // 1. Export standalone native vector figure to SVG
    #[test]
    fn test_export_standalone_native_vector_figure_to_svg() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.run("shape.rectangle", json!({"x": 20.0, "y": 30.0, "width": 80.0, "height": 50.0})).unwrap();
        app.run("shape.ellipse", json!({"x": 60.0, "y": 120.0, "width": 60.0, "height": 40.0})).unwrap();
        app.run("text.create", json!({"x": 20.0, "y": 180.0, "text": "Standalone Native Vector"})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("standalone.svg").to_string_lossy().to_string();

        let res = export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() })
            .expect("export_figure_svg");

        assert_eq!(res.path, out_svg);
        assert!(res.bytes_written > 0);
        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("<rect") || content.contains("<path"), "must contain native shapes");
        assert!(content.contains("Standalone Native Vector"), "must contain text");
    }

    // 2. Export Hybrid Linked Plot to SVG
    #[test]
    fn test_export_hybrid_linked_plot_to_svg() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 120.0, 90.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("hybrid.svg").to_string_lossy().to_string();

        let res = export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() })
            .expect("export hybrid svg");

        assert!(res.bytes_written > 0);
        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("<path"), "must contain plot vector paths");
        assert!(content.contains("#377eb8"), "must retain point colors");
        assert!(content.contains("Gene Alpha"), "must contain plot label");
        assert!(!content.contains("<image"), "authoritative vector export must not contain raster fallback");
    }

    // 3. Export Hybrid Linked Plot to PDF
    #[test]
    fn test_export_hybrid_linked_plot_to_pdf() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 120.0, 90.0);

        let temp_dir = TempTestDir::new();
        let out_pdf = temp_dir.path().join("hybrid.pdf").to_string_lossy().to_string();

        let res = export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() })
            .expect("export hybrid pdf");

        assert!(res.bytes_written > 0);
        let bytes = fs::read(&out_pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "must be valid PDF");
        let pdf_str = String::from_utf8_lossy(&bytes);
        assert!(pdf_str.contains("/MediaBox"), "PDF must define page media box");
    }

    // 4. Export combined linked and unlinked artwork
    #[test]
    fn test_export_combined_linked_and_unlinked_artwork() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        // 1. Linked plot
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 100.0, 80.0);
        // 2. Unlinked annotation text & box
        app.run("text.create", json!({"x": 50.0, "y": 20.0, "text": "Annotation: Significant"})).unwrap();
        app.run("shape.rectangle", json!({"x": 45.0, "y": 15.0, "width": 80.0, "height": 20.0})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("combined.svg").to_string_lossy().to_string();
        let out_pdf = temp_dir.path().join("combined.pdf").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();

        let svg_str = fs::read_to_string(&out_svg).unwrap();
        assert!(svg_str.contains("Annotation: Significant"));
        assert!(svg_str.contains("Gene Alpha"));
        assert!(svg_str.contains("<path") || svg_str.contains("<rect"));
        assert!(svg_str.contains("#377eb8"));

        let pdf_bytes = fs::read(&out_pdf).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    // 5. Correct mm dimensions
    #[test]
    fn test_correct_mm_dimensions() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("dimensions.svg").to_string_lossy().to_string();

        let res = export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert_eq!(res.width_mm, 210.0);
        assert_eq!(res.height_mm, 297.0);

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("width=\"210mm\""), "SVG width must be formatted in mm: {content}");
        assert!(content.contains("height=\"297mm\""), "SVG height must be formatted in mm: {content}");
    }

    // 6. Correct SVG viewBox
    #[test]
    fn test_correct_svg_viewbox() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("viewbox.svg").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("viewBox=\"0 0 "), "must define 0 0 origin viewBox");
        assert!(content.contains("595.276") || content.contains("595.28") || content.contains("595"), "viewBox width must reflect 210 mm in points");
        assert!(content.contains("841.89") || content.contains("842"), "viewBox height must reflect 297 mm in points");
    }

    // 7. Correct PDF page size
    #[test]
    fn test_correct_pdf_page_size() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_pdf = temp_dir.path().join("pagesize.pdf").to_string_lossy().to_string();

        let res = export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert_eq!(res.width_mm, 210.0);
        assert_eq!(res.height_mm, 297.0);

        let bytes = fs::read(&out_pdf).unwrap();
        let pdf_str = String::from_utf8_lossy(&bytes);
        assert!(pdf_str.contains("/MediaBox"), "PDF must specify /MediaBox");
        // Extract MediaBox numbers: [0 0 595... 841...]
        assert!(pdf_str.contains("595") && pdf_str.contains("841") || pdf_str.contains("842"));
    }

    // 8. GraphStyle typography and visibility
    #[test]
    fn test_graphstyle_typography_and_visibility() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let styled_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" viewBox="0 0 200 150">
  <text x="20" y="30" font-size="22" font-weight="bold" fill="#111111">Figure 1: Expression</text>
  <line x1="20" y1="120" x2="180" y2="120" stroke="#333333" stroke-width="1.8"/>
  <text x="20" y="140" font-size="14" fill="#555555">Log2 Fold Change</text>
</svg>"##;
        add_test_hybrid_plot(&mut app, styled_svg, 100.0, 75.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("typography.svg").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("Figure 1: Expression"));
        assert!(content.contains("Log2 Fold Change"));
        assert!(content.contains("22") || content.contains("font-size"));
    }

    // 9. Alpha transparency and point overlap
    #[test]
    fn test_alpha_transparency_and_point_overlap() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let alpha_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" viewBox="0 0 200 150">
  <circle cx="60" cy="60" r="15" fill="#ff0000" fill-opacity="0.35"/>
  <circle cx="70" cy="60" r="15" fill="#0000ff" fill-opacity="0.35"/>
</svg>"##;
        add_test_hybrid_plot(&mut app, alpha_svg, 80.0, 60.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("alpha.svg").to_string_lossy().to_string();
        let out_pdf = temp_dir.path().join("alpha.pdf").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("fill-opacity=\"0.35\"") || content.contains("opacity=\"0.35\""));

        let pdf_bytes = fs::read(&out_pdf).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    // 10. Clip paths and nested groups
    #[test]
    fn test_clip_paths_and_nested_groups() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        let clip_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" viewBox="0 0 200 150">
  <defs>
    <clipPath id="panel-boundary"><rect x="15" y="15" width="170" height="120"/></clipPath>
  </defs>
  <g id="outer-group">
    <g id="inner-clipped" clip-path="url(#panel-boundary)">
      <circle cx="30" cy="30" r="10" fill="#222222"/>
    </g>
  </g>
</svg>"##;
        add_test_hybrid_plot(&mut app, clip_svg, 90.0, 70.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("clip.svg").to_string_lossy().to_string();
        let out_pdf = temp_dir.path().join("clip.pdf").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("<clipPath") || content.contains("clip-path"));

        let pdf_bytes = fs::read(&out_pdf).unwrap();
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    // 11. Large UMAP vector retention and performance (10k, 50k, 100k points)
    #[test]
    fn test_large_umap_vector_retention_and_performance() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        for point_count in [10_000, 50_000, 100_000] {
            let mut svg = String::with_capacity(point_count * 50 + 200);
            svg.push_str(r#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 300 300">"#);
            for i in 0..point_count {
                let cx = (i * 7) % 280 + 10;
                let cy = (i * 13) % 280 + 10;
                use std::fmt::Write;
                let _ = write!(svg, "<circle cx=\"{}\" cy=\"{}\" r=\"1.5\" fill=\"#377eb8\" fill-opacity=\"0.3\"/>", cx, cy);
            }
            svg.push_str("</svg>");

            let child_id = add_test_hybrid_plot(&mut app, &svg, 100.0, 100.0);

            // Document must retain single placed document node, NOT 100k nodes in interactive session!
            let doc_st = app.session.doc().unwrap();
            let parent_node = doc_st.doc.node(NodeId(child_id)).unwrap();
            assert_eq!(parent_node.children().unwrap().len(), 1, "interactive document must have single placed child");
            assert!(matches!(parent_node.children().unwrap()[0].kind, NodeKind::PlacedDocument(_)));

            let temp_dir = TempTestDir::new();
            let out_svg = temp_dir.path().join(format!("umap_{point_count}.svg")).to_string_lossy().to_string();

            let started = Instant::now();
            let res = export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
            let elapsed_ms = started.elapsed().as_millis();

            println!("UMAP {point_count} points SVG export: {elapsed_ms} ms, {} bytes", res.bytes_written);
            assert!(res.bytes_written > point_count * 10, "exported SVG must retain all points");

            // Clean up plot from session for next size
            app.run("select.set", json!({"ids": [child_id]})).unwrap();
            app.run("edit.clear", json!({"ids": [child_id]})).unwrap();
        }
    }

    // 12. Export with R unavailable
    #[test]
    fn test_export_with_r_unavailable() {
        let mut app = make_test_app();
        app.scientific.session_active = false;
        app.scientific.running = false;
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 100.0, 75.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("offline.svg").to_string_lossy().to_string();
        let out_pdf = temp_dir.path().join("offline.pdf").to_string_lossy().to_string();

        let svg_res = export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert!(svg_res.bytes_written > 0);

        let pdf_res = export_figure_pdf(&app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert!(pdf_res.bytes_written > 0);
    }

    // 13. Export after .fide save/reopen
    #[test]
    fn test_export_after_fide_save_reopen() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 100.0, 75.0);
        app.run("shape.rectangle", json!({"x": 10.0, "y": 10.0, "width": 50.0, "height": 30.0})).unwrap();

        let project = build_fide_project(&app).expect("build fide");
        let archive_bytes = project.save_to_bytes().expect("save bytes");

        let mut reopened_app = make_test_app();
        open_fide_project_bytes(&mut reopened_app, &archive_bytes, None).expect("reopen fide");

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("reopened.svg").to_string_lossy().to_string();
        let out_pdf = temp_dir.path().join("reopened.pdf").to_string_lossy().to_string();

        let res_svg =
            export_figure_svg(&reopened_app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert!(res_svg.bytes_written > 0);
        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("<path") || content.contains("<g"));
        assert!(content.contains("#377eb8"));
        assert!(content.contains("Gene Alpha"));

        let res_pdf =
            export_figure_pdf(&reopened_app, &FigureExportOptions { path: Some(out_pdf.clone()), overwrite: true, ..Default::default() }).unwrap();
        assert!(res_pdf.bytes_written > 0);
    }

    // 14. Failed export preserves destination and project
    #[test]
    fn test_failed_export_preserves_destination_and_project() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.run("shape.rectangle", json!({"x": 10.0, "y": 10.0, "width": 50.0, "height": 30.0})).unwrap();

        let temp_dir = TempTestDir::new();
        let protected_file = temp_dir.path().join("protected.svg");
        fs::write(&protected_file, b"INITIAL IMPORTANT CONTENT").unwrap();

        // Attempt export with overwrite = false
        let err = export_figure_svg(
            &app,
            &FigureExportOptions { path: Some(protected_file.to_string_lossy().to_string()), overwrite: false, ..Default::default() },
        )
        .unwrap_err();

        assert!(err.contains("already exists"));
        let surviving = fs::read(&protected_file).unwrap();
        assert_eq!(surviving, b"INITIAL IMPORTANT CONTENT", "existing file must not be modified or overwritten");
    }

    // 15. Correct figure selection in multi-figure projects
    #[test]
    fn test_correct_figure_selection_in_multi_figure_projects() {
        let mut app = make_test_app();
        // Figure 1
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Figure_1_Volcano".into();
        app.run("text.create", json!({"x": 20.0, "y": 30.0, "text": "Volcano Content"})).unwrap();

        // Figure 2
        app.session.add_document(Document::new(210.0 * DOC_POINTS_PER_MM, 297.0 * DOC_POINTS_PER_MM), None);
        app.session.set_active(1);
        Arc::make_mut(&mut app.session.active_mut().unwrap().doc).title = "Figure_2_UMAP".into();
        app.run("text.create", json!({"x": 20.0, "y": 30.0, "text": "UMAP Content"})).unwrap();

        let temp_dir = TempTestDir::new();
        let out_fig2 = temp_dir.path().join("fig2.svg").to_string_lossy().to_string();

        let res = export_figure_svg(
            &app,
            &FigureExportOptions { path: Some(out_fig2.clone()), figure_index: Some(1), overwrite: true, ..Default::default() },
        )
        .unwrap();

        assert_eq!(res.figure_name, "Figure_2_UMAP");
        let content = fs::read_to_string(&out_fig2).unwrap();
        assert!(content.contains("UMAP Content"));
        assert!(!content.contains("Volcano Content"), "must not export the wrong figure");
    }

    // 16. No duplicate raster preview in exported output
    #[test]
    fn test_no_duplicate_raster_preview_in_exported_output() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        add_test_hybrid_plot(&mut app, sample_plot_svg(), 100.0, 75.0);

        let temp_dir = TempTestDir::new();
        let out_svg = temp_dir.path().join("no_duplicate.svg").to_string_lossy().to_string();

        export_figure_svg(&app, &FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() }).unwrap();

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(!content.contains("<image"), "SVG must not contain duplicate raster preview image elements");
    }
}
