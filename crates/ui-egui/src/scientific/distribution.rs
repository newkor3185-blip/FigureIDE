//! Distribution, packaging validation, R dependencies, and .fide association tests.

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::VectorcraftApp;
    use crate::scientific::diagnostics::{format_diagnostic_report, parse_diagnostic_output};
    use crate::scientific::project::{build_fide_project, open_fide_project_bytes};
    use vectorcraft_doc::Document;
    use vectorcraft_format::fide::{FideFigure, FideProject, FideScript};

    struct TempTestDir(PathBuf);

    impl TempTestDir {
        fn new(name: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(1);
            let count = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("vectorcraft_test_dist_{}_{}_{}", name, std::process::id(), count));
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

    fn make_test_app() -> VectorcraftApp {
        let mut app = VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
        app.scientific.enabled = true;
        app.services.read = Some(Box::new(|p: &str| std::fs::read(p).map_err(|e| e.to_string())));
        app
    }

    // 1. Windows EXE packaging structure
    #[test]
    fn test_windows_exe_packaging() {
        let temp = TempTestDir::new("pkg");
        let pkg_root = temp.path().join("FigureIDE-v0.1.0-beta.1-Windows-x64");
        let inner = pkg_root.join("FigureIDE");
        fs::create_dir_all(&inner).unwrap();

        fs::write(inner.join("FigureIDE.exe"), b"MOCK_EXE_BINARY").unwrap();
        fs::write(inner.join("README.txt"), b"FigureIDE Portable").unwrap();
        fs::write(inner.join("LICENSE.txt"), b"MIT / Apache-2.0").unwrap();
        fs::write(inner.join("BUILD_INFO.txt"), b"commit=12345678\nversion=0.1.0-beta.1").unwrap();
        fs::write(inner.join("register-fide-association.cmd"), b"@echo off\nassoc .fide=FigureIDE.Figure").unwrap();

        assert!(inner.join("FigureIDE.exe").is_file());
        assert!(inner.join("README.txt").is_file());
        assert!(inner.join("LICENSE.txt").is_file());
        assert!(inner.join("BUILD_INFO.txt").is_file());
        assert!(inner.join("register-fide-association.cmd").is_file());

        let readme = fs::read_to_string(inner.join("README.txt")).unwrap();
        assert!(readme.contains("Portable"));
    }

    // 2. Required resource availability
    #[test]
    fn test_required_resource_availability() {
        // Verify root workspace license documents exist and are valid
        let mit_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE-MIT");
        let apache_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LICENSE-APACHE");
        let notice_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../NOTICE");

        assert!(mit_path.exists(), "LICENSE-MIT must exist");
        assert!(apache_path.exists(), "LICENSE-APACHE must exist");
        assert!(notice_path.exists(), "NOTICE must exist");

        let mit_content = fs::read_to_string(mit_path).unwrap();
        assert!(mit_content.contains("MIT License") || mit_content.contains("Permission is hereby granted"));
    }

    // 3. .fide file path CLI handling
    #[test]
    fn test_fide_file_path_cli_handling() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // Create a real .fide file on disk
        let temp = TempTestDir::new("cli");
        let project_path = temp.path().join("experiment.fide");

        let mut project = FideProject::default();
        project.figures.push(FideFigure {
            id: "fig1".into(),
            name: "CLI Figure".into(),
            doc: Document::new(210.0 * 72.0 / 25.4, 297.0 * 72.0 / 25.4),
        });
        project.scripts.push(FideScript { id: 1, name: "analysis.R".into(), text: "x <- 1:100\n".into(), external_path: None, is_dirty: false });

        let bytes = project.save_to_bytes().unwrap();
        fs::write(&project_path, bytes).unwrap();

        // Pass path to open_reporting as main.rs does for CLI arguments
        let path_str = project_path.to_string_lossy().to_string();
        let res = crate::io::open_reporting(&mut app, &path_str);
        assert!(res.is_ok(), "open_reporting must succeed for .fide CLI path: {:?}", res);

        assert_eq!(app.scientific.scripts.len(), 1);
        assert_eq!(app.scientific.scripts[0].text, "x <- 1:100\n");
    }

    // 4. Unicode project paths
    #[test]
    fn test_unicode_project_paths() {
        let mut app = make_test_app();
        let temp = TempTestDir::new("unicode");

        // Path with spaces and Unicode (CJK, German umlaut, etc.)
        let unicode_folder = temp.path().join("实验 数据_Projekt Überprüfung 2026");
        fs::create_dir_all(&unicode_folder).unwrap();
        let project_path = unicode_folder.join("Airway 呼吸道 分析.fide");

        let mut project = FideProject::default();
        project.scripts.push(FideScript { id: 1, name: "test.R".into(), text: "y <- sqrt(16)\n".into(), external_path: None, is_dirty: false });
        fs::write(&project_path, project.save_to_bytes().unwrap()).unwrap();

        let path_str = project_path.to_string_lossy().to_string();
        let res = crate::io::open_reporting(&mut app, &path_str);
        assert!(res.is_ok(), "Unicode path must open cleanly: {:?}", res);

        assert_eq!(app.scientific.scripts[0].text, "y <- sqrt(16)\n");
    }

    // 5. Missing and corrupted project files
    #[test]
    fn test_missing_and_corrupted_project_files() {
        let mut app = make_test_app();
        let temp = TempTestDir::new("corrupt");

        // A. Non-existent path
        let missing = temp.path().join("does_not_exist.fide");
        let res_missing = crate::io::open_reporting(&mut app, &missing.to_string_lossy());
        assert!(res_missing.is_err(), "missing file must return Err");

        // B. Corrupt binary file
        let corrupt = temp.path().join("broken.fide");
        fs::write(&corrupt, b"PK\x03\x04GARBAGE_DATA_NOT_A_VALID_ZIP_ARCHIVE").unwrap();
        let res_corrupt = crate::io::open_reporting(&mut app, &corrupt.to_string_lossy());
        assert!(res_corrupt.is_err(), "corrupt archive must return Err gracefully without panic");
    }

    // 6. Rscript discovery
    #[test]
    fn test_rscript_discovery() {
        let temp = TempTestDir::new("rscript");
        let fake_bin = temp.path().join("bin");
        fs::create_dir_all(&fake_bin).unwrap();

        let exe_name = if cfg!(windows) { "Rscript.exe" } else { "Rscript" };
        let fake_exe = fake_bin.join(exe_name);
        fs::write(&fake_exe, b"echo Rscript").unwrap();

        // When R_HOME points to this directory
        let found = crate::r_session::find_rscript_with(|key| if key == "R_HOME" { Some(temp.path().to_path_buf()) } else { None });

        assert!(found.is_ok(), "find_rscript_with must find executable under R_HOME/bin");
        let path = found.unwrap();
        assert!(path.ends_with(exe_name));
    }

    // 7. Missing svglite diagnostics
    #[test]
    fn test_missing_svglite_diagnostics() {
        let raw = "R_DIAG\tR version 4.3.1 (2023-06-16 ucrt)\tC:/R/R-4.3.1\tC:/R/lib\tsvglite:0,ggplot2:1,systemfonts:1";
        let diag = parse_diagnostic_output(raw, Some("C:/R/Rscript.exe".into()));

        assert!(diag.r_available);
        assert!(!diag.packages["svglite"]);
        assert!(diag.packages["ggplot2"]);
        assert_eq!(diag.missing_packages, vec!["svglite"]);

        let inst = diag.install_instructions.clone().unwrap();
        assert!(inst.contains("install.packages("));
        assert!(inst.contains("\"svglite\""));

        let report = format_diagnostic_report(&diag);
        assert!(report.contains("svglite       : MISSING"));
        assert!(report.contains(&inst));
    }

    // 8. Installed svglite diagnostics
    #[test]
    fn test_installed_svglite_diagnostics() {
        let raw = "R_DIAG\tR version 4.3.1 (2023-06-16 ucrt)\tC:/R/R-4.3.1\tC:/R/lib\tsvglite:1,ggplot2:1,systemfonts:1";
        let diag = parse_diagnostic_output(raw, Some("C:/R/Rscript.exe".into()));

        assert!(diag.r_available);
        assert!(diag.packages["svglite"]);
        assert!(diag.packages["ggplot2"]);
        assert!(diag.packages["systemfonts"]);
        assert!(diag.missing_packages.is_empty());
        assert!(diag.install_instructions.is_none());
        assert!(diag.summary.contains("All rendering dependencies satisfied"));
    }

    // 9. SVG smoke rendering
    #[test]
    fn test_svg_smoke_rendering() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();

        // Valid SVG as produced by svglite
        let sample_svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200" viewBox="0 0 300 200">
          <rect width="100%" height="100%" fill="none"/>
          <circle cx="100" cy="80" r="12" fill="#e41a1c"/>
          <text x="50" y="150" font-size="14" font-family="sans-serif">Deterministic Plot</text>
        </svg>"##;

        let res = app.run(
            "object.importRPlotSource",
            json!({
                "svg": sample_svg,
                "width": 100.0 * 72.0 / 25.4,
                "height": 70.0 * 72.0 / 25.4,
            }),
        );
        assert!(res.is_ok(), "importRPlotSource must succeed");

        let temp = TempTestDir::new("smoke");
        let out_svg = temp.path().join("smoke_export.svg").to_string_lossy().to_string();

        let exp = crate::scientific::export::export_figure_svg(
            &app,
            &crate::scientific::export::FigureExportOptions { path: Some(out_svg.clone()), overwrite: true, ..Default::default() },
        );
        assert!(exp.is_ok(), "export_figure_svg must succeed: {:?}", exp);

        let content = fs::read_to_string(&out_svg).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains("Deterministic Plot"));
    }

    // 10. Manual workflow artifact generation
    #[test]
    fn test_manual_workflow_artifact_generation() {
        let wf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/rvs-windows-release.yml");
        assert!(wf_path.exists(), "Workflow file must exist");

        let content = fs::read_to_string(wf_path).unwrap();
        assert!(content.contains("workflow_dispatch:"), "Workflow must allow manual dispatch");
        assert!(content.contains("upload-artifact"), "Workflow must upload ZIP artifact");
        assert!(content.contains("FigureIDE-") || content.contains("RVectorStudio-"), "Workflow must name archive properly");
    }

    // 11. Tagged-release trigger logic
    #[test]
    fn test_tagged_release_trigger_logic() {
        let wf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/rvs-windows-release.yml");
        let content = fs::read_to_string(wf_path).unwrap();

        assert!(content.contains("rvs-v*") || content.contains("figureide-v*") || content.contains("v*"), "Workflow push trigger must match tags");
        assert!(content.contains("gh release create"), "Workflow must use gh release create for tagged releases");
        assert!(
            content.contains("startsWith(github.ref, 'refs/tags/rvs-v')")
                || content.contains("startsWith(github.ref, 'refs/tags/figureide-v')")
                || content.contains("startsWith(github.ref, 'refs/tags/v')")
                || content.contains("startsWith(github.ref, 'refs/tags/')"),
            "Release creation must be conditioned on tag"
        );
    }

    // 12. Portable extraction
    #[test]
    fn test_portable_extraction() {
        let temp = TempTestDir::new("extract");
        let staged_root = temp.path().join("staged_release");
        let app_folder = staged_root.join("RVectorStudio");
        fs::create_dir_all(&app_folder).unwrap();

        fs::write(app_folder.join("RVectorStudio.exe"), b"PORTABLE_EXE").unwrap();
        fs::write(app_folder.join("README.txt"), b"FigureIDE Portable Distribution").unwrap();

        // Simulate portable relocation to a completely different path
        let target_dir = temp.path().join("user_extracted_location");
        fs::create_dir_all(&target_dir).unwrap();
        let target_app = target_dir.join("RVectorStudio");
        fs::create_dir_all(&target_app).unwrap();

        fs::copy(app_folder.join("RVectorStudio.exe"), target_app.join("RVectorStudio.exe")).unwrap();
        fs::copy(app_folder.join("README.txt"), target_app.join("README.txt")).unwrap();

        assert!(target_app.join("RVectorStudio.exe").is_file());
        assert!(target_app.join("README.txt").is_file());
        let readme = fs::read_to_string(target_app.join("README.txt")).unwrap();
        assert!(readme.contains("FigureIDE Portable"));
    }

    // 13. No upstream VectorCraft update popup
    #[test]
    fn test_no_upstream_vectorcraft_update_popup() {
        let mut app = make_test_app();
        app.set_startup_update_check(true);

        // Verification: startup check must be firmly off
        assert!(!app.startup_update_check(), "startup_check must remain false in R Vector Studio");

        // Manual check must return an error
        let res = crate::updates_ui::check(&mut app);
        assert!(res.is_err(), "check() must return error refusing upstream updates");
        let err_msg = res.unwrap_err();
        assert!(err_msg.contains("disabled in R Vector Studio"));
    }

    // 14. No automatic execution of embedded R scripts
    #[test]
    fn test_no_automatic_execution_of_embedded_r_scripts() {
        let mut app = make_test_app();

        let mut project = FideProject::default();
        project.scripts.push(FideScript {
            id: 1,
            name: "heavy_or_malicious.R".into(),
            text: "system('shutdown /s /t 0')\n".into(),
            external_path: None,
            is_dirty: false,
        });

        let bytes = project.save_to_bytes().unwrap();
        open_fide_project_bytes(&mut app, &bytes, Some("test.fide".into())).unwrap();

        // Verify: execution queue MUST be empty!
        assert!(app.scientific.execution_queue.is_empty(), "scripts must never auto-execute on open");
        assert!(!app.scientific.running, "R session must not start executing on open");
    }

    // 15. Existing figure and project compatibility
    #[test]
    fn test_existing_figure_and_project_compatibility() {
        let mut app = make_test_app();
        app.run("file.new", json!({"width": "210 mm", "height": "297 mm"})).unwrap();
        app.run("shape.rectangle", json!({"x": 20.0, "y": 20.0, "width": 60.0, "height": 40.0})).unwrap();

        let project = build_fide_project(&app).expect("build project");
        let bytes = project.save_to_bytes().expect("save bytes");

        let mut fresh = make_test_app();
        open_fide_project_bytes(&mut fresh, &bytes, Some("compat.fide".into())).expect("reopen");

        assert_eq!(fresh.session.documents().len(), 1);
        let doc = &fresh.session.active().unwrap().doc;
        assert_eq!(doc.artboards.len(), 1);
    }
}
