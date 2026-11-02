//! R installation and package dependency diagnostics for FigureIDE / R Vector Studio.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command;

use super::append_console_output;
use crate::VectorcraftApp;

pub const REQUIRED_RENDERING_PACKAGES: &[&str] = &["svglite", "ggplot2", "systemfonts"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct REnvironmentDiagnostics {
    pub r_available: bool,
    pub rscript_path: Option<String>,
    pub r_version: Option<String>,
    pub r_home: Option<String>,
    pub lib_paths: Vec<String>,
    pub packages: HashMap<String, bool>,
    pub missing_packages: Vec<String>,
    pub install_instructions: Option<String>,
    pub summary: String,
}

impl Default for REnvironmentDiagnostics {
    fn default() -> Self {
        Self {
            r_available: false,
            rscript_path: None,
            r_version: None,
            r_home: None,
            lib_paths: Vec::new(),
            packages: HashMap::new(),
            missing_packages: Vec::new(),
            install_instructions: None,
            summary: String::new(),
        }
    }
}

/// Diagnoses the active or installed R environment and package availability.
pub fn diagnose_r_environment() -> REnvironmentDiagnostics {
    let rscript = match crate::r_session::find_rscript() {
        Ok(path) => path,
        Err(_) => {
            return REnvironmentDiagnostics {
                r_available: false,
                rscript_path: None,
                r_version: None,
                r_home: None,
                lib_paths: Vec::new(),
                packages: HashMap::new(),
                missing_packages: REQUIRED_RENDERING_PACKAGES.iter().map(|&s| s.to_string()).collect(),
                install_instructions: Some(
                    "Install R (>= 4.0 recommended) from https://cloud.r-project.org and ensure Rscript is in PATH or R_HOME is set.".to_string(),
                ),
                summary: "Rscript was not found on PATH, under R_HOME, or in standard install locations.".to_string(),
            };
        }
    };

    let rscript_path_str = rscript.to_string_lossy().into_owned();

    // Query R details using a non-interactive one-liner
    let mut cmd = Command::new(&rscript);
    cmd.arg("-e").arg(
        r#"cat(sprintf("R_DIAG\t%s\t%s\t%s\t%s", R.version.string, R.home(), paste(.libPaths(), collapse = "::"), paste(c("svglite", "ggplot2", "systemfonts"), as.integer(c(requireNamespace("svglite", quietly = TRUE), requireNamespace("ggplot2", quietly = TRUE), requireNamespace("systemfonts", quietly = TRUE))), sep = ":", collapse = ",")))"#,
    );

    #[cfg(windows)]
    crate::r_session::configure_background_r_command(&mut cmd);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return REnvironmentDiagnostics {
                r_available: false,
                rscript_path: Some(rscript_path_str),
                r_version: None,
                r_home: None,
                lib_paths: Vec::new(),
                packages: HashMap::new(),
                missing_packages: REQUIRED_RENDERING_PACKAGES.iter().map(|&s| s.to_string()).collect(),
                install_instructions: Some(format!("Could not execute Rscript: {e}")),
                summary: format!("Found Rscript but execution failed: {e}"),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_diagnostic_output(&stdout, Some(rscript_path_str))
}

/// Parses the output produced by the diagnostic R script.
pub fn parse_diagnostic_output(stdout: &str, rscript_path: Option<String>) -> REnvironmentDiagnostics {
    let diag_line = stdout.lines().find(|l| l.starts_with("R_DIAG\t"));
    let Some(line) = diag_line else {
        return REnvironmentDiagnostics {
            r_available: true,
            rscript_path,
            r_version: None,
            r_home: None,
            lib_paths: Vec::new(),
            packages: HashMap::new(),
            missing_packages: REQUIRED_RENDERING_PACKAGES.iter().map(|&s| s.to_string()).collect(),
            install_instructions: Some(
                "install.packages(c(\"svglite\", \"ggplot2\", \"systemfonts\"), repos = \"https://cloud.r-project.org\")".into(),
            ),
            summary: "Rscript responded but did not return complete diagnostic fields.".into(),
        };
    };

    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 5 {
        return REnvironmentDiagnostics {
            r_available: true,
            rscript_path,
            r_version: parts.get(1).map(|&s| s.to_string()),
            r_home: parts.get(2).map(|&s| s.to_string()),
            lib_paths: Vec::new(),
            packages: HashMap::new(),
            missing_packages: REQUIRED_RENDERING_PACKAGES.iter().map(|&s| s.to_string()).collect(),
            install_instructions: Some(
                "install.packages(c(\"svglite\", \"ggplot2\", \"systemfonts\"), repos = \"https://cloud.r-project.org\")".into(),
            ),
            summary: "R environment diagnostics output was truncated.".into(),
        };
    }

    let r_version = Some(parts[1].to_string());
    let r_home = Some(parts[2].to_string());
    let lib_paths: Vec<String> = parts[3].split("::").filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();

    let mut packages = HashMap::new();
    let mut missing_packages = Vec::new();

    for pkg_entry in parts[4].split(',') {
        if let Some((name, flag_str)) = pkg_entry.split_once(':') {
            let installed = flag_str.trim() == "1";
            packages.insert(name.to_string(), installed);
            if !installed {
                missing_packages.push(name.to_string());
            }
        }
    }

    let install_instructions = if !missing_packages.is_empty() {
        let pkgs_quoted: Vec<String> = missing_packages.iter().map(|p| format!("\"{p}\"")).collect();
        Some(format!("install.packages(c({}), repos = \"https://cloud.r-project.org\")", pkgs_quoted.join(", ")))
    } else {
        None
    };

    let summary = if missing_packages.is_empty() {
        format!("All rendering dependencies satisfied ({})", r_version.as_deref().unwrap_or("R"))
    } else {
        format!("Missing required package(s): {}. Run installation command in R.", missing_packages.join(", "))
    };

    REnvironmentDiagnostics {
        r_available: true,
        rscript_path,
        r_version,
        r_home,
        lib_paths,
        packages,
        missing_packages,
        install_instructions,
        summary,
    }
}

/// Formats the diagnostic results as a clean human-readable text report.
pub fn format_diagnostic_report(diag: &REnvironmentDiagnostics) -> String {
    let mut out = String::new();
    out.push_str("========================================\n");
    out.push_str("     R Environment & Diagnostics        \n");
    out.push_str("========================================\n");

    if !diag.r_available {
        out.push_str("Rscript: NOT FOUND\n");
        out.push_str("Status:  R execution is unavailable.\n");
        if let Some(ref inst) = diag.install_instructions {
            out.push_str(&format!("\nRecommendation:\n  {inst}\n"));
        }
        out.push_str("========================================\n");
        return out;
    }

    if let Some(ref p) = diag.rscript_path {
        out.push_str(&format!("Rscript: {}\n", p));
    }
    if let Some(ref v) = diag.r_version {
        out.push_str(&format!("Version: {}\n", v));
    }
    if let Some(ref h) = diag.r_home {
        out.push_str(&format!("R Home:  {}\n", h));
    }

    if !diag.lib_paths.is_empty() {
        out.push_str("\nLibrary Directories:\n");
        for p in &diag.lib_paths {
            out.push_str(&format!("  - {}\n", p));
        }
    }

    out.push_str("\nRequired Packages:\n");
    for &pkg in REQUIRED_RENDERING_PACKAGES {
        let status = if diag.packages.get(pkg).copied().unwrap_or(false) { "Available" } else { "MISSING" };
        out.push_str(&format!("  - {:<14}: {}\n", pkg, status));
    }

    if diag.missing_packages.is_empty() {
        out.push_str("\nResult: All rendering dependencies are satisfied.\n");
    } else {
        out.push_str(&format!("\nResult: Missing {} required package(s): {}\n", diag.missing_packages.len(), diag.missing_packages.join(", ")));
        if let Some(ref inst) = diag.install_instructions {
            out.push_str("\nTo install missing packages in R, run:\n");
            out.push_str(&format!("  {}\n", inst));
        }
    }

    out.push_str("========================================\n");
    out
}

/// Command handler for `r.environment.diagnostics` / `r.diagnostics`.
pub fn run_diagnostics_command(app: &mut VectorcraftApp, _p: &Value) -> Result<Value, String> {
    let diag = diagnose_r_environment();
    let report = format_diagnostic_report(&diag);

    append_console_output(&mut app.scientific, &report);
    app.scientific.r_status = diag.summary.clone();

    serde_json::to_value(&diag).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostics_missing_r() {
        let diag = REnvironmentDiagnostics {
            r_available: false,
            rscript_path: None,
            r_version: None,
            r_home: None,
            lib_paths: Vec::new(),
            packages: HashMap::new(),
            missing_packages: vec!["svglite".into(), "ggplot2".into(), "systemfonts".into()],
            install_instructions: Some("Install R from https://cloud.r-project.org".into()),
            summary: "Rscript not found".into(),
        };

        let report = format_diagnostic_report(&diag);
        assert!(report.contains("Rscript: NOT FOUND"));
        assert!(report.contains("https://cloud.r-project.org"));
    }

    #[test]
    fn test_parse_diagnostic_output_complete() {
        let raw = "R_DIAG\tR version 4.3.2 (2023-10-31 ucrt)\tC:/R/R-4.3.2\tC:/R/lib1::C:/R/lib2\tsvglite:1,ggplot2:1,systemfonts:1";
        let diag = parse_diagnostic_output(raw, Some("C:/R/bin/Rscript.exe".into()));

        assert!(diag.r_available);
        assert_eq!(diag.rscript_path.as_deref(), Some("C:/R/bin/Rscript.exe"));
        assert_eq!(diag.r_version.as_deref(), Some("R version 4.3.2 (2023-10-31 ucrt)"));
        assert_eq!(diag.lib_paths.len(), 2);
        assert!(diag.packages["svglite"]);
        assert!(diag.packages["ggplot2"]);
        assert!(diag.packages["systemfonts"]);
        assert!(diag.missing_packages.is_empty());
        assert!(diag.install_instructions.is_none());

        let report = format_diagnostic_report(&diag);
        assert!(report.contains("All rendering dependencies are satisfied"));
        assert!(report.contains("svglite       : Available"));
    }

    #[test]
    fn test_parse_diagnostic_output_missing_svglite() {
        let raw = "R_DIAG\tR version 4.3.2 (2023-10-31 ucrt)\tC:/R/R-4.3.2\tC:/R/lib\tsvglite:0,ggplot2:1,systemfonts:0";
        let diag = parse_diagnostic_output(raw, Some("C:/R/bin/Rscript.exe".into()));

        assert!(diag.r_available);
        assert!(!diag.packages["svglite"]);
        assert!(diag.packages["ggplot2"]);
        assert!(!diag.packages["systemfonts"]);
        assert_eq!(diag.missing_packages, vec!["svglite", "systemfonts"]);

        let inst = diag.install_instructions.clone().expect("install instructions");
        assert!(inst.contains("install.packages("));
        assert!(inst.contains("\"svglite\""));
        assert!(inst.contains("\"systemfonts\""));

        let report = format_diagnostic_report(&diag);
        assert!(report.contains("svglite       : MISSING"));
        assert!(report.contains("systemfonts   : MISSING"));
        assert!(report.contains("ggplot2       : Available"));
        assert!(report.contains(&inst));
    }
}
