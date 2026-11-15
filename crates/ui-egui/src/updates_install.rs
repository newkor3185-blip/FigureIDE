//! Archive validation and staged replacement for the Linux portable FigureIDE package.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::updates_client::Package;

const MAX_ENTRIES: usize = 20_000;
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;
const LINUX_ROOT: &str = "FigureIDE-Linux-x86_64";
const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinuxInstallPlan {
    plan_file: PathBuf,
    root: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    archive: PathBuf,
    sha256: String,
    marker: PathBuf,
    parent_pid: u32,
    version: String,
}

#[cfg(windows)]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct WindowsInstallPlan {
    plan_file: PathBuf,
    root: PathBuf,
    stage: PathBuf,
    backup: PathBuf,
    archive: PathBuf,
    sha256: String,
    marker: PathBuf,
    parent_pid: u32,
    version: String,
}

impl LinuxInstallPlan {
    pub fn plan_path(&self) -> &Path {
        &self.plan_file
    }

    pub fn stage_path(&self) -> &Path {
        &self.stage
    }
}

/// Check every archive entry before retaining it as an installable package. No downloaded program
/// or script is run here.
pub fn validate_archive(path: &Path, package: Package, version: &str) -> Result<(), String> {
    match package {
        Package::LinuxX86_64 => validate_linux_tar(path, version),
        Package::WindowsX64 => validate_windows_zip(path, version),
        Package::MacOsArm64 => validate_macos_zip(path, version, "aarch64-apple-darwin"),
        Package::MacOsX86_64 => validate_macos_zip(path, version, "x86_64-apple-darwin"),
    }
}

/// Restrict the app's first-frame marker to a new file in FigureIDE's private update cache.
pub fn confirmation_marker_allowed(path: &Path) -> bool {
    let Ok(paths) = craft_update::Paths::for_app(&crate::updates_client::FIGUREIDE_APP) else { return false };
    let Ok(cache) = paths.cache.canonicalize() else { return false };
    let Some(parent) = path.parent() else { return false };
    let Ok(parent) = parent.canonicalize() else { return false };
    parent == cache
        && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("figureide-update-confirm-"))
        && std::fs::symlink_metadata(path).is_err()
}

fn safe_relative(path: &Path, raw: &str) -> bool {
    !raw.is_empty()
        && !raw.starts_with('/')
        && !raw.contains(['\\', ':'])
        && !raw.chars().any(char::is_control)
        && path.components().all(|part| matches!(part, Component::Normal(_)))
}

fn check_build_info(bytes: &[u8], version: &str, target: &str) -> Result<(), String> {
    if bytes.len() > 16 * 1024 {
        return Err("The update package build metadata is too large.".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "The update package build metadata is not UTF-8.")?.trim_start_matches('\u{feff}');
    let mut found_version = false;
    let mut found_target = false;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("version=") {
            if found_version {
                return Err("The update package has duplicate version metadata.".into());
            }
            found_version = value == version;
        } else if let Some(value) = line.strip_prefix("target=") {
            if found_target {
                return Err("The update package has duplicate platform metadata.".into());
            }
            found_target = value == target;
        }
    }
    if !found_version || !found_target {
        return Err("The update package does not match its release version and platform.".into());
    }
    Ok(())
}

fn verify_archive_sha256(path: &Path, expected: &str) -> Result<(), String> {
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("The update package has no valid expected SHA-256 value.".into());
    }
    let actual = craft_update::http::sha256_file(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("The update archive changed after verification; download it again before installing.".into())
    }
}

fn validate_linux_tar(path: &Path, version: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| format!("Could not open update archive: {error}"))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut build_info = None;
    let mut executable = false;
    let mut license_files = BTreeSet::new();
    let mut has_asset_license = false;
    for (index, entry) in archive.entries().map_err(|error| format!("Invalid TAR.GZ archive: {error}"))?.enumerate() {
        if index >= MAX_ENTRIES {
            return Err("The update archive contains too many entries.".into());
        }
        let mut entry = entry.map_err(|error| format!("Invalid TAR.GZ entry: {error}"))?;
        let path = entry.path().map_err(|error| format!("Invalid archive path: {error}"))?.into_owned();
        let raw = path.to_string_lossy();
        let mut components = path.components();
        if components.next().and_then(|component| component.as_os_str().to_str()) != Some(LINUX_ROOT)
            || !safe_relative(&path, &raw)
            || !seen.insert(raw.to_string())
        {
            return Err("The update archive contains an unsafe or duplicate path.".into());
        }
        let kind = entry.header().entry_type();
        let relative = path.strip_prefix(LINUX_ROOT).map_err(|_| "The Linux package root is invalid.")?;
        if relative.as_os_str().is_empty() {
            if kind.is_dir() {
                continue;
            }
            return Err("The Linux package root entry is not a directory.".into());
        }
        if !linux_package_path_is_expected(relative) {
            return Err("The Linux update archive contains an unexpected package path.".into());
        }
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err("The Linux update archive contains a link or special file.".into());
        }
        total = total.checked_add(entry.size()).filter(|size| *size <= MAX_EXPANDED).ok_or("The update archive expands beyond the allowed size.")?;
        if relative == Path::new("BUILD_INFO.txt") {
            if entry.size() > 16 * 1024 {
                return Err("The update package build metadata is too large.".into());
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            build_info = Some(bytes);
        }
        if relative == Path::new("FigureIDE") {
            executable = entry.header().mode().is_ok_and(|mode| mode & 0o111 != 0);
        }
        if ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"].iter().any(|name| relative == Path::new(name))
            && let Some(name) = relative.file_name().and_then(|name| name.to_str())
        {
            license_files.insert(name.to_string());
        }
        if raw.starts_with(&format!("{LINUX_ROOT}/asset-licenses/")) && entry.size() > 0 {
            has_asset_license = true;
        }
    }
    let bytes = build_info.ok_or("The Linux package has no BUILD_INFO.txt.")?;
    check_build_info(&bytes, version, LINUX_TARGET)?;
    for required in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
        if !license_files.contains(required) {
            return Err(format!("The Linux update package is missing {required}."));
        }
    }
    if !executable || !has_asset_license {
        return Err("The Linux update package is missing its executable or asset license texts.".into());
    }
    Ok(())
}

fn linux_package_path_is_expected(path: &Path) -> bool {
    let Some(first) = path.components().next().and_then(|part| part.as_os_str().to_str()) else { return false };
    matches!(
        first,
        "FigureIDE" | "BUILD_INFO.txt" | "LICENSE-MIT" | "LICENSE-APACHE" | "NOTICE" | "ASSETS.md" | "THIRD-PARTY-LICENSES.md" | "README-Linux.md"
    ) || first == "licenses"
        || first == "asset-licenses"
}

fn validate_windows_zip(path: &Path, version: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("Invalid Windows ZIP: {error}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err("The update ZIP contains too many entries.".into());
    }
    let mut root = None;
    let mut build_info = None;
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    let mut asset_license = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let path = entry.enclosed_name().ok_or("The update ZIP contains an unsafe path.")?.to_path_buf();
        let raw = entry.name().map_err(|error| error.to_string())?.into_owned();
        if !safe_relative(&path, &raw) || !seen.insert(raw.to_ascii_lowercase()) || entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("The update ZIP contains a link, duplicate, or unsafe path.".into());
        }
        let Some(Component::Normal(package_dir)) = path.components().next() else {
            return Err("The Windows update archive has an invalid package root.".into());
        };
        if package_dir != "FigureIDE" || (path.components().count() < 2 && !entry.is_dir()) {
            return Err("The Windows update archive contains files outside the expected package folder.".into());
        }
        let relative = path.iter().skip(1).collect::<PathBuf>();
        if !(relative.as_os_str().is_empty() && entry.is_dir()) && !windows_package_path_is_expected(&relative) {
            return Err("The Windows update archive contains an unexpected package path.".into());
        }
        if entry.is_file() && relative == Path::new("FigureIDE.exe") {
            let candidate = PathBuf::from("FigureIDE");
            if root.replace(candidate).is_some() {
                return Err("The update ZIP contains more than one FigureIDE executable.".into());
            }
        }
        total = total.checked_add(entry.size()).filter(|size| *size <= MAX_EXPANDED).ok_or("The update ZIP expands beyond the allowed size.")?;
        if relative == Path::new("BUILD_INFO.txt") {
            if entry.size() > 16 * 1024 {
                return Err("The update package build metadata is too large.".into());
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            build_info = Some(bytes);
        }
        if raw.to_ascii_lowercase().contains("/asset-licenses/") && entry.size() > 0 {
            asset_license = true;
        }
    }
    root.ok_or("The Windows update package is missing FigureIDE.exe.")?;
    let build = build_info.ok_or("The Windows update package is missing BUILD_INFO.txt.")?;
    check_build_info(&build, version, "x86_64-pc-windows-msvc")?;
    for required in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
        if !archive_member_exists(&mut archive, &Path::new("FigureIDE").join(required))? {
            return Err(format!("The Windows update package is missing {required}."));
        }
    }
    if !asset_license {
        return Err("The Windows update package is missing asset license texts.".into());
    }
    Ok(())
}

fn windows_package_path_is_expected(path: &Path) -> bool {
    let Some(first) = path.components().next().and_then(|part| part.as_os_str().to_str()) else { return false };
    matches!(
        first,
        "FigureIDE.exe"
            | "BUILD_INFO.txt"
            | "LICENSE-MIT"
            | "LICENSE-APACHE"
            | "NOTICE"
            | "ASSETS.md"
            | "THIRD-PARTY-LICENSES.md"
            | "README.txt"
            | "register-fide-association.cmd"
            | "unregister-fide-association.cmd"
    ) || first.eq_ignore_ascii_case("licenses")
        || first.eq_ignore_ascii_case("asset-licenses")
}

#[cfg(windows)]
pub fn windows_portable_install() -> bool {
    let Ok(executable) = std::env::current_exe() else { return false };
    let Some(root) = executable.parent() else { return false };
    std::fs::read_to_string(root.join("BUILD_INFO.txt")).is_ok_and(|text| {
        text.lines().any(|line| line == "target=x86_64-pc-windows-msvc") && executable.file_name().is_some_and(|name| name == "FigureIDE.exe")
    })
}

#[cfg(windows)]
pub fn prepare_windows_install(archive_path: &Path, sha256: &str, version: &str, cache: &Path) -> Result<PathBuf, String> {
    verify_archive_sha256(archive_path, sha256)?;
    validate_windows_zip(archive_path, version)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?.canonicalize().map_err(|error| error.to_string())?;
    if executable.file_name().and_then(|name| name.to_str()) != Some("FigureIDE.exe") {
        return Err("Automatic Windows updates require the portable FigureIDE folder. Use the release page for this installation.".into());
    }
    let root = executable.parent().ok_or("The running executable has no install directory.")?.to_path_buf();
    if std::fs::symlink_metadata(&root).map_err(|error| error.to_string())?.file_type().is_symlink() {
        return Err("The Windows install directory is a symbolic link; use manual installation.".into());
    }
    if !windows_portable_install() {
        return Err("This is not a supported FigureIDE portable ZIP installation.".into());
    }
    let parent = root.parent().ok_or("The FigureIDE install directory has no parent.")?;
    let probe = parent.join(format!(".figureide-write-check-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| "FigureIDE is installed in a protected or unwritable directory. Install the downloaded package manually.".to_string())?;
    std::fs::remove_file(&probe).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(cache).map_err(|error| error.to_string())?;
    let cache = cache.canonicalize().map_err(|error| error.to_string())?;
    let stamp = craft_update::now();
    let stage = parent.join(format!(".figureide-update-stage-{}-{stamp}", std::process::id()));
    let backup = parent.join(format!(".figureide-update-backup-{}-{stamp}", std::process::id()));
    let plan_path = cache.join(format!("figureide-update-{}-{stamp}.json", std::process::id()));
    let marker = cache.join(format!("figureide-update-confirm-{}-{stamp}", std::process::id()));
    if stage.exists() || backup.exists() || plan_path.exists() || marker.exists() {
        return Err("A FigureIDE update staging path already exists. Check for another update before retrying.".into());
    }
    std::fs::create_dir(&stage).map_err(|error| error.to_string())?;
    if let Err(error) = copy_tree(&root, &stage, 0, &mut 0, &mut 0) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    if let Err(error) = overlay_windows_archive(archive_path, &stage, version) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    validate_windows_tree(&stage, version)?;
    let plan = WindowsInstallPlan {
        plan_file: plan_path.clone(),
        root,
        stage,
        backup,
        archive: archive_path.to_path_buf(),
        sha256: sha256.to_ascii_lowercase(),
        marker,
        parent_pid: std::process::id(),
        version: version.into(),
    };
    write_plan(&plan_path, &plan)?;
    Ok(plan_path)
}

#[cfg(windows)]
pub fn discard_windows_plan(path: &Path) {
    if let Ok(bytes) = std::fs::read(path)
        && let Ok(plan) = serde_json::from_slice::<WindowsInstallPlan>(&bytes)
        && plan.plan_file == path
        && plan.stage.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with(".figureide-update-stage-"))
    {
        let _ = std::fs::remove_dir_all(plan.stage);
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(windows)]
fn validate_windows_tree(root: &Path, version: &str) -> Result<(), String> {
    let bytes = std::fs::read(root.join("BUILD_INFO.txt")).map_err(|error| error.to_string())?;
    check_build_info(&bytes, version, "x86_64-pc-windows-msvc")?;
    if !root.join("FigureIDE.exe").is_file() {
        return Err("The staged Windows update has no FigureIDE.exe.".into());
    }
    for required in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
        if !root.join(required).is_file() {
            return Err(format!("The staged Windows update is missing {required}."));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn overlay_windows_archive(archive_path: &Path, destination: &Path, version: &str) -> Result<(), String> {
    let file = std::fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let raw = entry.name().map_err(|error| error.to_string())?.into_owned();
        let path = entry.enclosed_name().ok_or("The Windows update archive contains an unsafe path.")?;
        if !safe_relative(&path, &raw) || !seen.insert(raw.to_ascii_lowercase()) || entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("The Windows update archive contains a link or unsafe path.".into());
        }
        let mut parts = path.components();
        if parts.next().and_then(|part| part.as_os_str().to_str()) != Some("FigureIDE") {
            return Err("The Windows update archive contains files outside the FigureIDE package folder.".into());
        }
        let relative = parts.as_path();
        if relative.as_os_str().is_empty() || !safe_relative(relative, &relative.to_string_lossy()) {
            if entry.is_dir() && relative.as_os_str().is_empty() {
                continue;
            }
            return Err("The Windows update archive has an invalid application path.".into());
        }
        if !windows_package_path_is_expected(relative) {
            return Err("The Windows update archive contains an unexpected package path.".into());
        }
        if entry.is_dir() {
            continue;
        }
        total = total
            .checked_add(entry.size())
            .filter(|size| *size <= MAX_EXPANDED)
            .ok_or("The Windows update archive expands beyond the allowed size.")?;
        let target = destination.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&target).map_err(|error| error.to_string())?;
        let expected = entry.size();
        let copied = std::io::copy(&mut (&mut entry).take(expected.saturating_add(1)), &mut output).map_err(|error| error.to_string())?;
        if copied != expected {
            return Err("A Windows update file has an unexpected expanded size.".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
    }
    validate_windows_tree(destination, version)
}

#[cfg(windows)]
fn write_plan(path: &Path, plan: &WindowsInstallPlan) -> Result<(), String> {
    let bytes = serde_json::to_vec(plan).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(())
}

fn archive_member_exists(archive: &mut zip::ZipArchive<std::fs::File>, path: &Path) -> Result<bool, String> {
    let path = path.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    for index in 0..archive.len() {
        let name = archive.by_index(index).map_err(|error| error.to_string())?.name().map_err(|error| error.to_string())?.into_owned();
        if name == path {
            return Ok(true);
        }
    }
    Ok(false)
}

fn validate_macos_zip(path: &Path, version: &str, target: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("Invalid macOS ZIP: {error}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err("The update ZIP contains too many entries.".into());
    }
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    let mut plist = None;
    let mut build_info = None;
    let mut executable = false;
    let mut assets_licensed = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let path = entry.enclosed_name().ok_or("The update ZIP contains an unsafe path.")?;
        let raw = entry.name().map_err(|error| error.to_string())?.into_owned();
        if !safe_relative(&path, &raw) || !seen.insert(raw.clone()) || entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
            return Err("The update ZIP contains a link, duplicate, or unsafe path.".into());
        }
        if !raw.starts_with("FigureIDE.app/") {
            return Err("The macOS update archive contains files outside FigureIDE.app.".into());
        }
        total = total.checked_add(entry.size()).filter(|size| *size <= MAX_EXPANDED).ok_or("The update ZIP expands beyond the allowed size.")?;
        match raw.as_str() {
            "FigureIDE.app/Contents/Info.plist" => {
                if entry.size() > 1 << 20 {
                    return Err("The macOS bundle metadata is too large.".into());
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
                plist = Some(plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|error| format!("Invalid macOS Info.plist: {error}"))?);
            }
            "FigureIDE.app/Contents/Resources/BUILD_INFO.txt" => {
                if entry.size() > 16 * 1024 {
                    return Err("The update package build metadata is too large.".into());
                }
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
                build_info = Some(bytes);
            }
            "FigureIDE.app/Contents/MacOS/FigureIDE" => executable = true,
            _ => {}
        }
        if raw.starts_with("FigureIDE.app/Contents/Resources/asset-licenses/") && entry.size() > 0 {
            assets_licensed = true;
        }
    }
    let info = plist.ok_or("The macOS package is missing its application Info.plist.")?;
    let dict = info.as_dictionary().ok_or("The macOS Info.plist is not a dictionary.")?;
    let bundle_id = dict.get("CFBundleIdentifier").and_then(plist::Value::as_string);
    let figureide_version = dict.get("FigureIDEVersion").and_then(plist::Value::as_string);
    if bundle_id != Some("ai.storyteller.figureide") || figureide_version != Some(version) {
        return Err("The macOS bundle identity or FigureIDE version is incorrect.".into());
    }
    check_build_info(&build_info.ok_or("The macOS package is missing BUILD_INFO.txt.")?, version, target)?;
    for required in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
        if !archive_member_exists(&mut archive, Path::new("FigureIDE.app/Contents/Resources").join(required).as_path())? {
            return Err(format!("The macOS update package is missing {required}."));
        }
    }
    if !executable || !assets_licensed {
        return Err("The macOS package is missing its executable or asset license texts.".into());
    }
    Ok(())
}

/// Prepare an install by copying the existing portable tree, then overlaying the exact validated
/// release files. Extra user files survive; links and oversized trees fail closed.
#[cfg(target_os = "linux")]
pub fn prepare_linux_install(archive_path: &Path, sha256: &str, version: &str, cache: &Path) -> Result<LinuxInstallPlan, String> {
    verify_archive_sha256(archive_path, sha256)?;
    validate_linux_tar(archive_path, version)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?.canonicalize().map_err(|error| error.to_string())?;
    if executable.file_name().and_then(|name| name.to_str()) != Some("FigureIDE") {
        return Err("Automatic Linux updates require the portable FigureIDE directory. Use the release page for this installation.".into());
    }
    let root = executable.parent().ok_or("The running executable has no install directory.")?.to_path_buf();
    if std::fs::symlink_metadata(&root).map_err(|error| error.to_string())?.file_type().is_symlink() {
        return Err("The Linux install directory is a symbolic link; use manual installation.".into());
    }
    let current = std::fs::read_to_string(root.join("BUILD_INFO.txt")).map_err(|_| "This is not a supported FigureIDE portable install.")?;
    if !current.lines().any(|line| line == format!("target={LINUX_TARGET}")) {
        return Err("This Linux installation is not the supported x86_64 portable package.".into());
    }
    let parent = root.parent().ok_or("The FigureIDE install directory has no parent.")?;
    let probe = parent.join(format!(".figureide-write-check-{}", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| "FigureIDE is installed in a protected or unwritable directory. Install the downloaded package manually.".to_string())?;
    std::fs::remove_file(&probe).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(cache).map_err(|error| error.to_string())?;
    let cache = cache.canonicalize().map_err(|error| error.to_string())?;
    let stamp = craft_update::now();
    let stage = parent.join(format!(".figureide-update-stage-{}-{stamp}", std::process::id()));
    let backup = parent.join(format!(".figureide-update-backup-{}-{stamp}", std::process::id()));
    let plan_path = cache.join(format!("figureide-update-{}-{stamp}.json", std::process::id()));
    let marker = cache.join(format!("figureide-update-confirm-{}-{stamp}", std::process::id()));
    if stage.exists() || backup.exists() || plan_path.exists() || marker.exists() {
        return Err("An update staging path already exists. Remove it after checking that no FigureIDE update is running.".into());
    }
    std::fs::create_dir(&stage).map_err(|error| error.to_string())?;
    if let Err(error) = copy_tree(&root, &stage, 0, &mut 0, &mut 0) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    if let Err(error) = overlay_linux_archive(archive_path, &stage) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(error);
    }
    let staged_info = std::fs::read(stage.join("BUILD_INFO.txt")).map_err(|error| error.to_string())?;
    check_build_info(&staged_info, version, LINUX_TARGET)?;
    let plan = LinuxInstallPlan {
        plan_file: plan_path.clone(),
        root,
        stage,
        backup,
        archive: archive_path.to_path_buf(),
        sha256: sha256.to_ascii_lowercase(),
        marker,
        parent_pid: std::process::id(),
        version: version.into(),
    };
    let bytes = serde_json::to_vec(&plan).map_err(|error| error.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&plan_path).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    Ok(plan)
}

#[cfg(any(target_os = "linux", windows))]
fn copy_tree(source: &Path, destination: &Path, depth: usize, count: &mut usize, total: &mut u64) -> Result<(), String> {
    if depth > 64 {
        return Err("The existing install tree is nested too deeply to update safely.".into());
    }
    for item in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err("The existing install tree contains too many files to update safely.".into());
        }
        let item = item.map_err(|error| error.to_string())?;
        let source_path = item.path();
        let target = destination.join(item.file_name());
        let metadata = std::fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("The existing install tree contains a symbolic link; use manual installation.".into());
        }
        if metadata.is_dir() {
            std::fs::create_dir(&target).map_err(|error| error.to_string())?;
            copy_tree(&source_path, &target, depth + 1, count, total)?;
        } else if metadata.is_file() {
            *total = total
                .checked_add(metadata.len())
                .filter(|size| *size <= MAX_EXPANDED)
                .ok_or("The existing install tree exceeds the safe update size.")?;
            if metadata.len() > MAX_EXPANDED {
                return Err("The existing install tree contains an oversized file.".into());
            }
            std::fs::copy(&source_path, &target).map_err(|error| error.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(metadata.permissions().mode() & 0o777))
                    .map_err(|error| error.to_string())?;
            }
        } else {
            return Err("The existing install tree contains a special file; use manual installation.".into());
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", windows))]
fn validate_existing_tree(root: &Path) -> Result<(), String> {
    fn walk(path: &Path, depth: usize, count: &mut usize, total: &mut u64) -> Result<(), String> {
        if depth > 64 {
            return Err("The staged install is nested too deeply.".into());
        }
        for item in std::fs::read_dir(path).map_err(|error| error.to_string())? {
            *count += 1;
            if *count > MAX_ENTRIES {
                return Err("The staged install has too many files.".into());
            }
            let child = item.map_err(|error| error.to_string())?.path();
            let metadata = std::fs::symlink_metadata(&child).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("The staged install contains a symbolic link.".into());
            }
            if metadata.is_dir() {
                walk(&child, depth + 1, count, total)?;
            } else if metadata.is_file() {
                *total = total.checked_add(metadata.len()).filter(|size| *size <= MAX_EXPANDED).ok_or("The staged install is too large.")?;
            } else {
                return Err("The staged install contains a special file.".into());
            }
        }
        Ok(())
    }
    let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("The staged install directory is not a real directory.".into());
    }
    walk(root, 0, &mut 0, &mut 0)
}

#[cfg(target_os = "linux")]
fn overlay_linux_archive(archive_path: &Path, destination: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let file = std::fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    for item in archive.entries().map_err(|error| error.to_string())? {
        let mut entry = item.map_err(|error| error.to_string())?;
        let path = entry.path().map_err(|error| error.to_string())?.into_owned();
        let relative = path.strip_prefix(LINUX_ROOT).map_err(|_| "The Linux package has an unexpected root path.")?;
        let raw = path.to_string_lossy();
        if !safe_relative(&path, &raw) || !seen.insert(raw.to_string()) || !linux_package_path_is_expected(relative) {
            if relative.as_os_str().is_empty() && entry.header().entry_type().is_dir() {
                continue;
            }
            return Err("The Linux update archive contains a duplicate, unexpected, or unsafe path.".into());
        }
        if relative.as_os_str().is_empty() || !safe_relative(relative, &relative.to_string_lossy()) {
            continue;
        }
        let target = destination.join(relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if target.exists() {
                if !std::fs::symlink_metadata(&target).map_err(|error| error.to_string())?.is_dir() {
                    return Err("The package conflicts with a non-directory in the existing install.".into());
                }
            } else {
                std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            }
            continue;
        }
        if !kind.is_file() {
            return Err("The Linux update archive contains a link or special file.".into());
        }
        total = total.checked_add(entry.size()).filter(|size| *size <= MAX_EXPANDED).ok_or("The update archive expands beyond the allowed size.")?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if target.exists() && !std::fs::symlink_metadata(&target).map_err(|error| error.to_string())?.is_file() {
            return Err("The package conflicts with a non-file in the existing install.".into());
        }
        let temp = target.with_extension("figureide-update-tmp");
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|error| error.to_string())?;
        let expected = entry.size();
        let copied = std::io::copy(&mut (&mut entry).take(expected.saturating_add(1)), &mut output).map_err(|error| error.to_string())?;
        if copied != expected {
            let _ = std::fs::remove_file(&temp);
            return Err("A Linux update file has an unexpected expanded size.".into());
        }
        output.sync_all().map_err(|error| error.to_string())?;
        let mode = entry.header().mode().unwrap_or(0o644) & 0o777;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode)).map_err(|error| error.to_string())?;
        std::fs::rename(&temp, &target).map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            error.to_string()
        })?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn helper_arguments(args: &[std::ffi::OsString]) -> Option<Result<(), String>> {
    if args.first().and_then(|arg| arg.to_str()) != Some("--figureide-finish-update") {
        return None;
    }
    let path = args.get(1).map(PathBuf::from).ok_or_else(|| "The update helper is missing its plan file.".to_string());
    Some(path.and_then(|path| finish_linux_update(&path)))
}

#[cfg(windows)]
pub fn helper_arguments(args: &[std::ffi::OsString]) -> Option<Result<(), String>> {
    if args.first().and_then(|arg| arg.to_str()) != Some("--figureide-finish-windows-update") {
        return None;
    }
    let path = args.get(1).map(PathBuf::from).ok_or_else(|| "The update helper is missing its plan file.".to_string());
    Some(path.and_then(|path| finish_windows_update(&path)))
}

#[cfg(windows)]
fn finish_windows_update(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let plan: WindowsInstallPlan = serde_json::from_slice(&bytes).map_err(|error| format!("Invalid update plan: {error}"))?;
    let paths = craft_update::Paths::for_app(&crate::updates_client::FIGUREIDE_APP)?;
    let cache = paths.cache.canonicalize().map_err(|error| error.to_string())?;
    let plan_path = path.canonicalize().map_err(|error| error.to_string())?;
    let root = plan.root.canonicalize().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?.canonicalize().map_err(|error| error.to_string())?;
    let parent = root.parent().ok_or("The install directory has no parent.")?;
    let archive = plan.archive.canonicalize().map_err(|error| error.to_string())?;
    if plan.plan_file.canonicalize().ok().as_deref() != Some(plan_path.as_path())
        || !plan_path.starts_with(&cache)
        || executable.parent() != Some(root.as_path())
        || plan.stage.parent() != Some(parent)
        || plan.backup.parent() != Some(parent)
        || plan.stage.file_name().and_then(|name| name.to_str()).is_none_or(|name| !name.starts_with(".figureide-update-stage-"))
        || plan.backup.file_name().and_then(|name| name.to_str()).is_none_or(|name| !name.starts_with(".figureide-update-backup-"))
        || plan.backup.exists()
        || !plan.stage.join("BUILD_INFO.txt").is_file()
        || !archive.starts_with(cache.join("figureide-downloads"))
        || plan.marker.parent() != Some(cache.as_path())
    {
        return Err("The update plan does not describe a safe FigureIDE folder replacement.".into());
    }
    verify_archive_sha256(&archive, &plan.sha256)?;
    validate_existing_tree(&plan.stage)?;
    validate_windows_zip(&archive, &plan.version)?;
    validate_windows_tree(&plan.stage, &plan.version)?;
    wait_for_windows_parent(plan.parent_pid)?;
    std::fs::rename(&root, &plan.backup).map_err(|error| format!("Could not preserve the current installation: {error}"))?;
    if let Err(error) = std::fs::rename(&plan.stage, &root) {
        let rollback = std::fs::rename(&plan.backup, &root);
        if rollback.is_ok() {
            let _ = launch_windows(&root);
        }
        return Err(format!("Could not install the staged update ({error}); rollback: {rollback:?}"));
    }
    let mut child = match std::process::Command::new(root.join("FigureIDE.exe")).arg("--figureide-confirm-update").arg(&plan.marker).spawn() {
        Ok(child) => child,
        Err(error) => {
            rollback_windows(&root, &plan.backup)?;
            let _ = launch_windows(&root);
            return Err(format!("The updated FigureIDE could not start ({error}); the previous version was restored."));
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if plan.marker.is_file() {
            let _ = std::fs::remove_file(&plan.marker);
            std::fs::remove_dir_all(&plan.backup)
                .map_err(|error| format!("The update is running, but its backup remains at {}: {error}", plan.backup.display()))?;
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(&archive);
            return Ok(());
        }
        if child.try_wait().map_err(|error| error.to_string())?.is_some() || std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            rollback_windows(&root, &plan.backup)?;
            let _ = launch_windows(&root);
            return Err("The updated FigureIDE did not reach its first UI frame; the previous installation was restored.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(windows)]
fn launch_windows(root: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(root.join("FigureIDE.exe")).creation_flags(0x08000000).spawn().map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn rollback_windows(root: &Path, backup: &Path) -> Result<(), String> {
    let failed = root.with_file_name(format!(".figureide-update-failed-{}", craft_update::now()));
    std::fs::rename(root, &failed).map_err(|error| format!("Could not move the failed update aside: {error}"))?;
    if let Err(error) = std::fs::rename(backup, root) {
        return Err(format!("The update failed and the backup remains at {}: {error}", backup.display()));
    }
    let _ = std::fs::remove_dir_all(failed);
    Ok(())
}

#[cfg(windows)]
fn wait_for_windows_parent(pid: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        let filter = format!("PID eq {pid}");
        let result = std::process::Command::new("tasklist.exe")
            .args(["/FI", filter.as_str(), "/NH"])
            .creation_flags(0x08000000)
            .output()
            .map_err(|error| format!("Could not check whether FigureIDE exited: {error}"))?;
        if !result.status.success() {
            return Err("Could not check whether FigureIDE exited; the installation was left unchanged.".into());
        }
        let output = String::from_utf8_lossy(&result.stdout);
        let still_running = output.lines().any(|line| line.split_whitespace().any(|field| field == pid.to_string()));
        if !still_running {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Err("FigureIDE did not exit within two minutes; the installation was left unchanged.".into())
}

#[cfg(target_os = "linux")]
fn finish_linux_update(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let plan: LinuxInstallPlan = serde_json::from_slice(&bytes).map_err(|error| format!("Invalid update plan: {error}"))?;
    let root = plan.root.canonicalize().map_err(|error| error.to_string())?;
    let parent = root.parent().ok_or("The install directory has no parent.")?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?.canonicalize().map_err(|error| error.to_string())?;
    let paths = craft_update::Paths::for_app(&crate::updates_client::FIGUREIDE_APP)?;
    let cache = paths.cache.canonicalize().map_err(|error| error.to_string())?;
    let archive = plan.archive.canonicalize().map_err(|error| error.to_string())?;
    let plan_path = path.canonicalize().map_err(|error| error.to_string())?;
    if executable.parent() != Some(root.as_path())
        || plan.plan_file.canonicalize().ok().as_deref() != Some(plan_path.as_path())
        || !plan_path.starts_with(&cache)
        || plan.stage.parent() != Some(parent)
        || plan.backup.parent() != Some(parent)
        || plan.stage.file_name().and_then(|name| name.to_str()).is_none_or(|name| !name.starts_with(".figureide-update-stage-"))
        || plan.backup.file_name().and_then(|name| name.to_str()).is_none_or(|name| !name.starts_with(".figureide-update-backup-"))
        || plan.backup.exists()
        || !plan.stage.join("BUILD_INFO.txt").is_file()
        || !archive.starts_with(cache.join("figureide-downloads"))
        || plan.marker.parent() != Some(cache.as_path())
    {
        return Err("The update plan paths do not describe a safe sibling-directory replacement.".into());
    }
    verify_archive_sha256(&archive, &plan.sha256)?;
    validate_existing_tree(&plan.stage)?;
    let info = std::fs::read(plan.stage.join("BUILD_INFO.txt")).map_err(|error| error.to_string())?;
    check_build_info(&info, &plan.version, LINUX_TARGET)?;
    validate_linux_tar(&archive, &plan.version)?;
    wait_for_parent(plan.parent_pid)?;
    std::fs::rename(&root, &plan.backup).map_err(|error| format!("Could not preserve the current installation: {error}"))?;
    if let Err(error) = std::fs::rename(&plan.stage, &root) {
        let rollback = std::fs::rename(&plan.backup, &root);
        if rollback.is_ok() {
            let _ = launch_linux(&root);
        }
        return Err(format!("Could not install the staged update ({error}); rollback: {rollback:?}"));
    }
    let executable = root.join("FigureIDE");
    let child = std::process::Command::new(&executable).arg("--figureide-confirm-update").arg(&plan.marker).spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            rollback_linux(&root, &plan.backup, None)?;
            let _ = launch_linux(&root);
            return Err(format!("The new FigureIDE could not start ({error}); the previous version was restored."));
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if plan.marker.is_file() {
            let _ = std::fs::remove_file(&plan.marker);
            std::fs::remove_dir_all(&plan.backup).map_err(|error| format!("The update is running, but its backup could not be removed: {error}"))?;
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(&plan.archive);
            return Ok(());
        }
        if child.try_wait().map_err(|error| error.to_string())?.is_some() || std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            rollback_linux(&root, &plan.backup, Some(&plan.stage))?;
            let _ = launch_linux(&root);
            return Err("The updated FigureIDE did not reach its first UI frame; the previous installation was restored.".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(target_os = "linux")]
fn launch_linux(root: &Path) -> Result<(), String> {
    std::process::Command::new(root.join("FigureIDE")).spawn().map(|_| ()).map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn rollback_linux(root: &Path, backup: &Path, failed: Option<&Path>) -> Result<(), String> {
    if let Some(failed) = failed {
        if failed.exists() {
            let _ = std::fs::remove_dir_all(failed);
        }
        std::fs::rename(root, failed).map_err(|error| format!("Could not move the failed update aside: {error}"))?;
    } else {
        let _ = std::fs::remove_dir_all(root);
    }
    std::fs::rename(backup, root).map_err(|error| format!("The update failed and rollback could not restore the backup: {error}"))
}

#[cfg(target_os = "linux")]
fn wait_for_parent(pid: u32) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err("FigureIDE did not exit; the installation was left unchanged.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_linux_tar_path_traversal_and_links() {
        let temp = std::env::temp_dir().join(format!("figureide-update-test-{}-{}", std::process::id(), craft_update::now()));
        std::fs::create_dir(&temp).unwrap();
        let bad = temp.join("bad.tar.gz");
        let file = std::fs::File::create(&bad).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        let malicious = format!("{LINUX_ROOT}/../evil");
        header.as_mut_bytes()[..malicious.len()].copy_from_slice(malicious.as_bytes());
        header.set_cksum();
        tar.append(&header, &b"evil"[..]).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(validate_linux_tar(&bad, "0.2.0-beta.2").is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn rejects_oversized_archive_metadata_and_bad_build_info() {
        assert!(check_build_info(b"version=0.1.0\ntarget=x86_64-unknown-linux-gnu\n", "0.2.0", LINUX_TARGET).is_err());
        assert!(!safe_relative(Path::new("../bad"), "../bad"));
        assert!(!safe_relative(Path::new("C:/bad"), "C:/bad"));
    }

    #[test]
    fn staging_copy_preserves_user_files_and_existing_application_files() {
        let temp = std::env::temp_dir().join(format!("figureide-copy-test-{}-{}", std::process::id(), craft_update::now()));
        let source = temp.join("FigureIDE");
        let destination = temp.join("stage");
        std::fs::create_dir_all(source.join("user-projects")).unwrap();
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(source.join("FigureIDE.exe"), b"old executable").unwrap();
        std::fs::write(source.join("user-projects/plot.fide"), b"user data").unwrap();
        let mut count = 0;
        let mut total = 0;
        copy_tree(&source, &destination, 0, &mut count, &mut total).unwrap();
        assert_eq!(std::fs::read(destination.join("FigureIDE.exe")).unwrap(), b"old executable");
        assert_eq!(std::fs::read(destination.join("user-projects/plot.fide")).unwrap(), b"user data");
        assert_eq!(count, 3);
        assert_eq!(total, 23);
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn confirmation_marker_is_limited_to_a_new_file_in_the_update_cache() {
        let paths = craft_update::Paths::for_app(&crate::updates_client::FIGUREIDE_APP).unwrap();
        std::fs::create_dir_all(&paths.cache).unwrap();
        let marker = paths.cache.join(format!("figureide-update-confirm-test-{}", craft_update::now()));
        let outside = std::env::temp_dir().join(format!("figureide-update-confirm-test-{}", craft_update::now()));
        assert!(confirmation_marker_allowed(&marker));
        assert!(!confirmation_marker_allowed(&outside));
        std::fs::write(&marker, b"exists").unwrap();
        assert!(!confirmation_marker_allowed(&marker));
        std::fs::remove_file(marker).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_rollback_restores_the_previous_installation() {
        let temp = std::env::temp_dir().join(format!("figureide-linux-rollback-{}-{}", std::process::id(), craft_update::now()));
        let root = temp.join("FigureIDE");
        let backup = temp.join("backup");
        let failed = temp.join("failed");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(root.join("version"), b"broken").unwrap();
        std::fs::write(backup.join("version"), b"previous").unwrap();
        rollback_linux(&root, &backup, Some(&failed)).unwrap();
        assert_eq!(std::fs::read(root.join("version")).unwrap(), b"previous");
        assert!(!failed.exists());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn windows_rollback_restores_the_previous_installation() {
        let temp = std::env::temp_dir().join(format!("figureide-windows-rollback-{}-{}", std::process::id(), craft_update::now()));
        let root = temp.join("FigureIDE");
        let backup = temp.join("backup");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(root.join("version"), b"broken").unwrap();
        std::fs::write(backup.join("version"), b"previous").unwrap();
        rollback_windows(&root, &backup).unwrap();
        assert_eq!(std::fs::read(root.join("version")).unwrap(), b"previous");
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn rejects_corrupted_and_traversal_zip_archives_before_extraction() {
        use zip::write::SimpleFileOptions;
        let temp = std::env::temp_dir().join(format!("figureide-zip-test-{}-{}", std::process::id(), craft_update::now()));
        std::fs::create_dir(&temp).unwrap();
        let corrupted = temp.join("corrupted.zip");
        std::fs::write(&corrupted, b"not a zip").unwrap();
        assert!(validate_windows_zip(&corrupted, "0.2.0-beta.2").is_err());

        let malicious = temp.join("malicious.zip");
        let file = std::fs::File::create(&malicious).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("FigureIDE/../escape", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"unsafe").unwrap();
        zip.finish().unwrap();
        assert!(validate_windows_zip(&malicious, "0.2.0-beta.2").is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn accepts_a_well_formed_windows_portable_package() {
        use zip::write::SimpleFileOptions;
        let temp = std::env::temp_dir().join(format!("figureide-valid-win-{}-{}", std::process::id(), craft_update::now()));
        std::fs::create_dir(&temp).unwrap();
        let path = temp.join("package.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let mut add = |name: &str, contents: &[u8]| {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        };
        add("FigureIDE/FigureIDE.exe", b"synthetic executable");
        add("FigureIDE/BUILD_INFO.txt", b"version=0.2.0-beta.2\ntarget=x86_64-pc-windows-msvc\n");
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
            add(&format!("FigureIDE/{name}"), b"test notice");
        }
        add("FigureIDE/asset-licenses/test/LICENSE.txt", b"test asset license");
        zip.finish().unwrap();
        let result = validate_windows_zip(&path, "0.2.0-beta.2");
        assert!(result.is_ok(), "{result:?}");
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn accepts_a_well_formed_macos_bundle_for_the_exact_architecture() {
        use zip::write::SimpleFileOptions;
        let temp = std::env::temp_dir().join(format!("figureide-valid-mac-{}-{}", std::process::id(), craft_update::now()));
        std::fs::create_dir(&temp).unwrap();
        let path = temp.join("package.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let mut add = |name: &str, contents: &[u8]| {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        };
        add(
            "FigureIDE.app/Contents/Info.plist",
            br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>ai.storyteller.figureide</string><key>FigureIDEVersion</key><string>0.2.0-beta.2</string></dict></plist>"#,
        );
        add("FigureIDE.app/Contents/Resources/BUILD_INFO.txt", b"version=0.2.0-beta.2\ntarget=aarch64-apple-darwin\n");
        add("FigureIDE.app/Contents/MacOS/FigureIDE", b"synthetic executable");
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
            add(&format!("FigureIDE.app/Contents/Resources/{name}"), b"test notice");
        }
        add("FigureIDE.app/Contents/Resources/asset-licenses/test/LICENSE.txt", b"test asset license");
        zip.finish().unwrap();
        let result = validate_macos_zip(&path, "0.2.0-beta.2", "aarch64-apple-darwin");
        assert!(result.is_ok(), "{result:?}");
        assert!(validate_macos_zip(&path, "0.2.0-beta.2", "x86_64-apple-darwin").is_err());
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn accepts_a_well_formed_linux_portable_package() {
        let temp = std::env::temp_dir().join(format!("figureide-valid-linux-{}-{}", std::process::id(), craft_update::now()));
        std::fs::create_dir(&temp).unwrap();
        let path = temp.join("package.tar.gz");
        let encoder = flate2::write::GzEncoder::new(std::fs::File::create(&path).unwrap(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut add = |name: &str, contents: &[u8], mode: u32| {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, format!("{LINUX_ROOT}/{name}"), contents).unwrap();
        };
        add("FigureIDE", b"synthetic executable", 0o755);
        add("BUILD_INFO.txt", b"version=0.2.0-beta.2\ntarget=x86_64-unknown-linux-gnu\n", 0o644);
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "ASSETS.md", "THIRD-PARTY-LICENSES.md"] {
            add(name, b"test notice", 0o644);
        }
        add("asset-licenses/test/LICENSE.txt", b"test asset license", 0o644);
        drop(add);
        tar.into_inner().unwrap().finish().unwrap();
        let result = validate_linux_tar(&path, "0.2.0-beta.2");
        assert!(result.is_ok(), "{result:?}");
        std::fs::remove_dir_all(temp).unwrap();
    }
}
