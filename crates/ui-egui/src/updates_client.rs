//! FigureIDE's release feed. This deliberately does not use the VectorCraft release channel.

use serde::{Deserialize, Serialize};

pub const REPOSITORY: &str = "newkor3185-blip/FigureIDE";
pub const RELEASES_API: &str = "https://api.github.com/repos/newkor3185-blip/FigureIDE/releases?per_page=100";
pub const RELEASES_PAGE: &str = "https://github.com/newkor3185-blip/FigureIDE/releases";
pub fn format_timestamp(timestamp: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp, 0).map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    #[default]
    Beta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub automatic_checks: bool,
    pub channel: Channel,
    pub last_successful_check: Option<i64>,
    pub skipped_version: Option<String>,
    pub notified_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { automatic_checks: true, channel: Channel::Beta, last_successful_check: None, skipped_version: None, notified_version: None }
    }
}

impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use craft_update::Release;
    use serde_json::{Value, json};

    pub const FIGUREIDE_APP: craft_update::App = craft_update::App::new("figureide", "FigureIDE", "Vector illustration", REPOSITORY);
    const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

    #[derive(Clone, Debug)]
    pub struct DownloadedPackage {
        pub path: std::path::PathBuf,
        pub sha256: String,
    }

    /// Reads published FigureIDE releases and orders them by semantic version. GitHub's prerelease
    /// flag is intentionally ignored: tags are the version source of truth for this repository.
    pub fn parse_releases(body: &str, channel: Channel) -> Result<Vec<Release>, String> {
        let entries: Value = serde_json::from_str(body).map_err(|e| format!("invalid GitHub release response ({e})"))?;
        let entries = entries.as_array().ok_or("unexpected GitHub releases response")?;
        let mut releases = Vec::new();
        for entry in entries {
            if entry.get("draft").and_then(Value::as_bool).unwrap_or(false) {
                continue;
            }
            let Some(tag) = entry.get("tag_name").and_then(Value::as_str) else { continue };
            let Some(version_text) = tag.strip_prefix("figureide-v") else { continue };
            let Some(version) = craft_update::parse_version(version_text) else { continue };
            if format!("figureide-v{version_text}") != tag {
                continue;
            }
            if channel == Channel::Stable && !version.pre.is_empty() {
                continue;
            }

            // craft-update already validates release URLs, asset names and repository-scoped download
            // URLs. Present it the SemVer tag it understands, then restore FigureIDE's real tag.
            let mut normalized = entry.clone();
            if let Some(object) = normalized.as_object_mut() {
                object.insert("tag_name".into(), json!(version_text));
            } else {
                continue;
            }
            let Ok(mut release) = craft_update::parse_github_release(&normalized.to_string(), REPOSITORY) else { continue };
            release.tag = tag.to_string();
            release.version = Some(version);
            release.prerelease = !release.version.as_ref().is_some_and(|v| v.pre.is_empty());
            releases.push(release);
        }
        releases.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(releases)
    }

    pub fn newest_release(body: &str, channel: Channel, current_version: &str) -> Result<Option<Release>, String> {
        let current = craft_update::parse_version(current_version).ok_or("the installed FigureIDE version is invalid")?;
        Ok(parse_releases(body, channel)?
            .into_iter()
            .find(|release| release.version.as_ref().is_some_and(|version| craft_update::version::is_newer(version, &current))))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Package {
        WindowsX64,
        LinuxX86_64,
        MacOsArm64,
        MacOsX86_64,
    }

    impl Package {
        pub fn current() -> Option<Self> {
            #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
            {
                return Some(Self::WindowsX64);
            }
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            {
                return Some(Self::LinuxX86_64);
            }
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            {
                return Some(Self::MacOsArm64);
            }
            #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
            {
                return Some(Self::MacOsX86_64);
            }
            #[allow(unreachable_code)]
            None
        }

        pub fn asset_name(self, version: &str) -> String {
            match self {
                Self::WindowsX64 => format!("FigureIDE-v{version}-Windows-x64.zip"),
                Self::LinuxX86_64 => format!("FigureIDE-v{version}-Linux-x86_64.tar.gz"),
                Self::MacOsArm64 => format!("FigureIDE-v{version}-macOS-AppleSilicon.zip"),
                Self::MacOsX86_64 => format!("FigureIDE-v{version}-macOS-Intel.zip"),
            }
        }

        pub fn archive_name(self) -> &'static str {
            match self {
                Self::WindowsX64 => "ZIP",
                Self::LinuxX86_64 => "TAR.GZ",
                Self::MacOsArm64 | Self::MacOsX86_64 => "ZIP",
            }
        }
    }

    pub fn select_asset(release: &Release, package: Package) -> Option<craft_update::release::Asset> {
        let version = release.version.as_ref()?;
        let name = package.asset_name(&version.to_string());
        release.assets.iter().find(|asset| asset.name == name).cloned()
    }

    pub fn checksum_for_asset(sums: &str, asset_name: &str) -> Result<String, String> {
        let checksum = craft_update::release::parse_sha256sums(sums)
            .into_iter()
            .find(|(_, name)| name == asset_name)
            .map(|(hash, _)| hash.to_ascii_lowercase())
            .ok_or_else(|| format!("SHA256SUMS has no entry for {asset_name}"))?;
        if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("SHA256SUMS contains an invalid checksum for {asset_name}"));
        }
        Ok(checksum)
    }

    /// Downloads one exact FigureIDE package into its app-owned cache and verifies its published sum.
    pub fn download_verified(
        release: &Release,
        package: Package,
        stop: &craft_update::Stop,
        progress: &mut dyn FnMut(craft_update::http::Transfer),
    ) -> Result<DownloadedPackage, String> {
        let archive = select_asset(release, package).ok_or("This release has no package for the current platform.")?;
        let version = release.version.as_ref().ok_or("The release version is missing.")?;
        if archive.size == 0 || archive.size > MAX_ARCHIVE_BYTES {
            return Err("The release package has an invalid or excessive download size.".into());
        }
        let sums_asset = release.assets.iter().find(|asset| asset.name == "SHA256SUMS").ok_or("This release has no SHA256SUMS file.")?;
        let http = craft_update::Http::new(&format!("FigureIDE/{}", release.version_label()))?;
        let sums = http.get_text(&sums_asset.url)?.ok_or("The release SHA256SUMS file could not be downloaded.")?;
        let checksum = checksum_for_asset(&sums, &archive.name)?;
        let paths = craft_update::Paths::for_app(&FIGUREIDE_APP)?;
        let directory = paths.cache.join("figureide-downloads");
        std::fs::create_dir_all(&directory).map_err(|error| format!("Could not create the FigureIDE update cache: {error}"))?;
        let destination = directory.join(&archive.name);
        http.download(&archive.url, &destination, archive.size, &checksum, stop, progress)?;
        let actual = craft_update::http::sha256_file(&destination)?;
        if !actual.eq_ignore_ascii_case(&checksum) {
            let _ = std::fs::remove_file(&destination);
            return Err("Downloaded package failed SHA-256 verification and was removed.".into());
        }
        if let Err(error) = crate::updates_install::validate_archive(&destination, package, &version.to_string()) {
            let _ = std::fs::remove_file(&destination);
            return Err(format!("The verified archive failed FigureIDE package validation and was removed: {error}"));
        }
        Ok(DownloadedPackage { path: destination, sha256: actual })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn release(tag: &str, prerelease: bool, draft: bool, assets: &[&str]) -> Value {
            json!({
                "tag_name": tag,
                "name": tag,
                "body": "Release notes",
                "html_url": format!("https://github.com/{REPOSITORY}/releases/tag/{tag}"),
                "published_at": "2026-11-01T12:00:00Z",
                "prerelease": prerelease,
                "draft": draft,
                "assets": assets.iter().map(|name| json!({
                    "name": name,
                    "size": 12,
                    "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{name}")
                })).collect::<Vec<_>>()
            })
        }

        fn body(items: Vec<Value>) -> String {
            Value::Array(items).to_string()
        }

        #[test]
        fn beta_channel_finds_newest_beta_or_stable_and_does_not_trust_api_flag() {
            let answer = body(vec![
                release("figureide-v0.2.0-beta.2", false, false, &[]),
                release("figureide-v0.2.0-beta.1", true, false, &[]),
                release("figureide-v0.1.9", true, false, &[]),
            ]);
            let latest = newest_release(&answer, Channel::Beta, "0.2.0-beta.1").unwrap().unwrap();
            assert_eq!(latest.tag, "figureide-v0.2.0-beta.2");
            assert_eq!(latest.version.unwrap().to_string(), "0.2.0-beta.2");
        }

        #[test]
        fn beta_channel_offers_versions_newer_than_v0_2_1_beta_1() {
            let answer = body(vec![
                release("figureide-v0.2.1", false, false, &[]),
                release("figureide-v0.2.1-beta.2", true, false, &[]),
                release("figureide-v0.2.1-beta.1", true, false, &[]),
                release("figureide-v0.2.0", false, false, &[]),
            ]);

            let latest = newest_release(&answer, Channel::Beta, "0.2.1-beta.1").unwrap().unwrap();
            assert_eq!(latest.tag, "figureide-v0.2.1");
            assert_eq!(latest.version.unwrap().to_string(), "0.2.1");
        }

        #[test]
        fn stable_channel_excludes_semver_prereleases_even_if_github_flag_is_false() {
            let answer = body(vec![release("figureide-v1.0.0-beta.2", false, false, &[]), release("figureide-v0.9.0", true, false, &[])]);
            let latest = newest_release(&answer, Channel::Stable, "0.8.0").unwrap().unwrap();
            assert_eq!(latest.tag, "figureide-v0.9.0");
        }

        #[test]
        fn ignores_drafts_invalid_tags_and_other_repositories_and_never_downgrades() {
            let answer = body(vec![
                release("figureide-v9.0.0", false, true, &[]),
                release("figureide-vbad", false, false, &[]),
                release("v9.0.0", false, false, &[]),
                release("figureide-v0.2.0-beta.1", false, false, &[]),
            ]);
            assert!(newest_release(&answer, Channel::Beta, "0.2.0-beta.1").unwrap().is_none());
            assert!(parse_releases("{}", Channel::Beta).is_err());
        }

        #[test]
        fn selects_only_the_exact_package_for_each_supported_architecture() {
            let assets = [
                "FigureIDE-v0.2.0-beta.2-Windows-x64.zip",
                "FigureIDE-v0.2.0-beta.2-Linux-x86_64.tar.gz",
                "FigureIDE-v0.2.0-beta.2-macOS-AppleSilicon.zip",
                "FigureIDE-v0.2.0-beta.2-macOS-Intel.zip",
            ];
            let mut release = parse_releases(&body(vec![release("figureide-v0.2.0-beta.2", true, false, &assets)]), Channel::Beta).unwrap().remove(0);
            for package in [Package::WindowsX64, Package::LinuxX86_64, Package::MacOsArm64, Package::MacOsX86_64] {
                assert_eq!(select_asset(&release, package).unwrap().name, package.asset_name("0.2.0-beta.2"));
            }
            release.assets.retain(|asset| !asset.name.contains("AppleSilicon"));
            assert!(select_asset(&release, Package::MacOsArm64).is_none());
        }

        #[test]
        fn checksum_lookup_requires_the_exact_archive_name() {
            let sums = format!("{}  FigureIDE-v0.2.0-beta.2-Windows-x64.zip\n{}  other.zip\n", "a".repeat(64), "b".repeat(64));
            assert_eq!(checksum_for_asset(&sums, "FigureIDE-v0.2.0-beta.2-Windows-x64.zip").unwrap(), "a".repeat(64));
            assert!(checksum_for_asset(&sums, "FigureIDE-v0.2.0-beta.2-Linux-x86_64.tar.gz").is_err());
            assert!(checksum_for_asset("invalid\n", "file.zip").is_err());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
