//! FigureIDE-specific, non-blocking release checks and update notifications.

use serde_json::Value;

#[cfg(not(target_arch = "wasm32"))]
use craft_update::{Release, http::Fetched};
#[cfg(any(windows, target_os = "linux"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    sync::mpsc::{Receiver, TryRecvError, channel},
    sync::{Arc, Mutex},
};

use crate::VectorcraftApp;
#[cfg(not(target_arch = "wasm32"))]
use crate::updates_client::{self, Channel};

#[derive(Default)]
pub(crate) struct Runtime {
    #[cfg(not(target_arch = "wasm32"))]
    check_rx: Option<Receiver<Result<Option<Release>, String>>>,
    #[cfg(not(target_arch = "wasm32"))]
    check_requested: bool,
    #[cfg(not(target_arch = "wasm32"))]
    auto_check_started: bool,
    #[cfg(not(target_arch = "wasm32"))]
    repaint: Option<egui::Context>,
    #[cfg(not(target_arch = "wasm32"))]
    download_rx: Option<Receiver<Result<updates_client::DownloadedPackage, String>>>,
    #[cfg(not(target_arch = "wasm32"))]
    download_progress: Arc<Mutex<Option<craft_update::http::Transfer>>>,
    #[cfg(not(target_arch = "wasm32"))]
    download_stop: Option<Arc<craft_update::Stop>>,
    #[cfg(not(target_arch = "wasm32"))]
    verified_download: Option<updates_client::DownloadedPackage>,
    #[cfg(any(windows, target_os = "linux"))]
    install_rx: Option<Receiver<Result<(), String>>>,
    #[cfg(any(windows, target_os = "linux"))]
    installation_safe: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) manual_window_open: bool,
    pub(crate) status_message: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) available: Option<Release>,
}

impl VectorcraftApp {
    /// Enables or disables the optional FigureIDE background release check.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_startup_update_check(&mut self, on: bool) {
        self.ui.updates.automatic_checks = on;
        self.prefs_changed = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn startup_update_check(&self) -> bool {
        self.ui.updates.automatic_checks
    }
}

pub(crate) fn check(app: &mut VectorcraftApp) -> Result<Value, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.updates_rt.check_requested = true;
        app.updates_rt.manual_window_open = true;
        app.updates_rt.status_message = Some(tl!("Checking for updates…").to_string());
        if let Some(ctx) = &app.updates_rt.repaint {
            ctx.request_repaint();
        }
        Ok(Value::Null)
    }
    #[cfg(target_arch = "wasm32")]
    {
        app.updates_rt.manual_window_open = true;
        app.updates_rt.status_message = Some("Update checking is available in the desktop app.".into());
        Ok(Value::Null)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn begin_check(app: &mut VectorcraftApp, ctx: &egui::Context, requested: bool) {
    if app.updates_rt.check_rx.is_some() {
        app.updates_rt.check_requested |= requested;
        return;
    }
    let (tx, rx) = channel();
    let context = ctx.clone();
    let channel = app.ui.updates.channel;
    let version = env!("CARGO_PKG_VERSION").to_string();
    let spawn = std::thread::Builder::new().name("figureide-update-check".into()).spawn(move || {
        let result = std::panic::catch_unwind(|| fetch_latest(channel, &version))
            .unwrap_or_else(|_| Err("The FigureIDE update check failed unexpectedly.".into()));
        let _ = tx.send(result);
        context.request_repaint();
    });
    match spawn {
        Ok(_) => {
            app.updates_rt.check_rx = Some(rx);
            app.updates_rt.check_requested = requested;
            if requested {
                app.updates_rt.status_message = Some(tl!("Checking for updates…").to_string());
                app.updates_rt.manual_window_open = true;
            }
        }
        Err(error) => {
            app.updates_rt.status_message = Some(format!("Could not start the update check: {error}"));
            app.updates_rt.manual_window_open = requested;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_latest(channel: Channel, current: &str) -> Result<Option<Release>, String> {
    let http = craft_update::Http::new(&format!("FigureIDE/{current}"))?;
    from_api_response(http.get_api(updates_client::RELEASES_API, None)?, channel, current)
}

#[cfg(not(target_arch = "wasm32"))]
fn from_api_response(response: Fetched, channel: Channel, current: &str) -> Result<Option<Release>, String> {
    match response {
        Fetched::Body { text, .. } => updates_client::newest_release(&text, channel, current),
        Fetched::RateLimited { reset } => Err(reset.map_or_else(
            || "GitHub's API rate limit was reached. Try again later.".to_string(),
            |at| format!("GitHub's API rate limit was reached (try again after Unix time {at})."),
        )),
        Fetched::NotModified | Fetched::NotFound => Ok(None),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_download(app: &mut VectorcraftApp, ctx: &egui::Context, release: Release) {
    if app.updates_rt.download_rx.is_some() {
        return;
    }
    let Some(package) = updates_client::Package::current() else {
        app.updates_rt.status_message = Some("Automatic downloads are not supported on this platform.".into());
        return;
    };
    if updates_client::select_asset(&release, package).is_none() {
        app.open_url(&release.html_url);
        return;
    }
    app.updates_rt.verified_download = None;
    let (tx, rx) = channel();
    let stop = Arc::new(craft_update::Stop::default());
    let worker_stop = Arc::clone(&stop);
    let progress = Arc::new(Mutex::new(None));
    let worker_progress = Arc::clone(&progress);
    let repaint = ctx.clone();
    let spawned = std::thread::Builder::new().name("figureide-update-download".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            updates_client::download_verified(&release, package, &worker_stop, &mut |state| {
                if let Ok(mut latest) = worker_progress.lock() {
                    *latest = Some(state);
                }
                repaint.request_repaint();
            })
        }))
        .unwrap_or_else(|_| Err("The update download failed unexpectedly.".into()));
        let _ = tx.send(result);
        repaint.request_repaint();
    });
    match spawned {
        Ok(_) => {
            app.updates_rt.download_rx = Some(rx);
            app.updates_rt.download_progress = progress;
            app.updates_rt.download_stop = Some(stop);
            app.updates_rt.status_message = Some("Downloading and verifying the FigureIDE package…".into());
        }
        Err(error) => app.updates_rt.status_message = Some(format!("Could not start the download: {error}")),
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn safe_to_replace(app: &VectorcraftApp) -> bool {
    !crate::unsaved::any_dirty(app) && !app.scientific.operation_running() && !crate::opening::long_operation_busy(app)
}

#[cfg(target_os = "linux")]
fn start_install_linux(app: &mut VectorcraftApp, ctx: &egui::Context, release: Release, archive: PathBuf, sha256: String) {
    if app.updates_rt.install_rx.is_some() {
        return;
    }
    if !safe_to_replace(app) {
        app.updates_rt.status_message =
            Some(tl!("Save your documents and finish active R, rendering, or export work before installing.").to_string());
        return;
    }
    let version = release.version_label();
    let (tx, rx) = channel();
    let repaint = ctx.clone();
    let safe = Arc::clone(&app.updates_rt.installation_safe);
    let spawn = std::thread::Builder::new().name("figureide-update-install".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paths = craft_update::Paths::for_app(&updates_client::FIGUREIDE_APP)?;
            let plan = crate::updates_install::prepare_linux_install(&archive, &sha256, &version, &paths.cache)?;
            if !safe.load(std::sync::atomic::Ordering::Acquire) {
                let _ = std::fs::remove_dir_all(plan.stage_path());
                let _ = std::fs::remove_file(plan.plan_path());
                return Err("Save your documents and finish active operations before installing.".into());
            }
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            match std::process::Command::new(executable).arg("--figureide-finish-update").arg(plan.plan_path()).spawn() {
                Ok(_) => Ok(()),
                Err(error) => {
                    let _ = std::fs::remove_dir_all(plan.stage_path());
                    let _ = std::fs::remove_file(plan.plan_path());
                    Err(format!("Could not start the FigureIDE update helper: {error}"))
                }
            }
        }))
        .unwrap_or_else(|_| Err("The Linux update installer failed unexpectedly.".into()));
        let _ = tx.send(result);
        repaint.request_repaint();
    });
    match spawn {
        Ok(_) => {
            app.updates_rt.install_rx = Some(rx);
            app.updates_rt.status_message = Some(tl!("Preparing the portable update. FigureIDE will restart after the helper finishes.").to_string());
        }
        Err(error) => app.updates_rt.status_message = Some(format!("Could not start the installer: {error}")),
    }
}

#[cfg(windows)]
fn start_install(app: &mut VectorcraftApp, ctx: &egui::Context, release: Release, archive: PathBuf, sha256: String) {
    use std::os::windows::process::CommandExt;
    if app.updates_rt.install_rx.is_some() {
        return;
    }
    if !safe_to_replace(app) {
        app.updates_rt.status_message =
            Some(tl!("Save your documents and finish active R, rendering, or export work before installing.").to_string());
        return;
    }
    if !crate::updates_install::windows_portable_install() {
        app.updates_rt.status_message = Some(tl!("Automatic replacement is available only for the Windows portable ZIP installation. Use the release page for other installation types.").to_string());
        return;
    }
    let (tx, rx) = channel();
    let repaint = ctx.clone();
    let safe = Arc::clone(&app.updates_rt.installation_safe);
    let spawn = std::thread::Builder::new().name("figureide-update-install".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let paths = craft_update::Paths::for_app(&updates_client::FIGUREIDE_APP)?;
            let plan = crate::updates_install::prepare_windows_install(&archive, &sha256, &release.version_label(), &paths.cache)?;
            if !safe.load(std::sync::atomic::Ordering::Acquire) {
                crate::updates_install::discard_windows_plan(&plan);
                return Err("Save your documents and finish active operations before installing.".into());
            }
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            match std::process::Command::new(executable).arg("--figureide-finish-windows-update").arg(&plan).creation_flags(0x08000000).spawn() {
                Ok(_) => Ok(()),
                Err(error) => {
                    crate::updates_install::discard_windows_plan(&plan);
                    Err(format!("Could not start the FigureIDE update helper: {error}"))
                }
            }
        }))
        .unwrap_or_else(|_| Err("The update installation failed unexpectedly.".into()));
        let _ = tx.send(result);
        repaint.request_repaint();
    });
    match spawn {
        Ok(_) => {
            app.updates_rt.install_rx = Some(rx);
            app.updates_rt.status_message = Some(tl!("Preparing the verified update. FigureIDE will restart after the helper finishes.").to_string());
        }
        Err(error) => app.updates_rt.status_message = Some(format!("Could not start the installer: {error}")),
    }
}

pub(crate) fn tick(app: &mut VectorcraftApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.updates_rt.repaint = Some(ctx.clone());
        #[cfg(any(windows, target_os = "linux"))]
        app.updates_rt.installation_safe.store(safe_to_replace(app), std::sync::atomic::Ordering::Release);
        let now = craft_update::now();
        if !app.updates_rt.auto_check_started {
            app.updates_rt.auto_check_started = true;
            let due = app.ui.updates.last_successful_check.is_none_or(|last| now.saturating_sub(last) >= 24 * 60 * 60);
            if app.ui.updates.automatic_checks && due && app.updates_rt.check_rx.is_none() {
                begin_check(app, ctx, false);
            }
        }
        if app.updates_rt.check_requested && app.updates_rt.check_rx.is_none() {
            begin_check(app, ctx, true);
        }
        let result = app.updates_rt.check_rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("The FigureIDE update check stopped unexpectedly.".into())),
        });
        if let Some(result) = result {
            app.updates_rt.check_rx = None;
            let requested = std::mem::take(&mut app.updates_rt.check_requested);
            match result {
                Ok(available) => {
                    app.ui.updates.last_successful_check = Some(now);
                    app.prefs_changed = true;
                    match available {
                        Some(release) if app.ui.updates.skipped_version.as_deref() == Some(&release.tag) => {
                            app.updates_rt.available = None;
                            if requested {
                                app.updates_rt.manual_window_open = true;
                                app.updates_rt.status_message =
                                    Some(tl!("You skipped FigureIDE {version}.").replace("{version}", &release.version_label()));
                            }
                        }
                        Some(release) => {
                            let already_notified = app.ui.updates.notified_version.as_deref() == Some(&release.tag);
                            if requested || !already_notified {
                                app.ui.updates.notified_version = Some(release.tag.clone());
                                app.prefs_changed = true;
                                app.updates_rt.available = Some(release);
                                app.updates_rt.manual_window_open = true;
                                app.updates_rt.status_message = None;
                            }
                        }
                        _ if requested => {
                            app.updates_rt.available = None;
                            app.updates_rt.manual_window_open = true;
                            app.updates_rt.status_message = Some(
                                tl!("FigureIDE {version} is currently the newest version available.").replace("{version}", env!("CARGO_PKG_VERSION")),
                            );
                        }
                        _ => {}
                    }
                }
                Err(error) if requested => {
                    app.updates_rt.manual_window_open = true;
                    app.updates_rt.status_message = Some(format!("Could not check for updates:\n{error}"));
                }
                Err(error) => log::warn!("FigureIDE automatic update check failed: {error}"),
            }
        } else if app.updates_rt.check_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let downloaded = app.updates_rt.download_rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("The FigureIDE download stopped unexpectedly.".into())),
        });
        if let Some(result) = downloaded {
            app.updates_rt.download_rx = None;
            app.updates_rt.download_stop = None;
            match result {
                Ok(path) => {
                    app.updates_rt.status_message = Some(format!("SHA-256 verified. Package saved to {}", path.path.display()));
                    app.updates_rt.verified_download = Some(path);
                }
                Err(error) => app.updates_rt.status_message = Some(format!("Download or verification failed:\n{error}")),
            }
            app.updates_rt.manual_window_open = true;
        } else if app.updates_rt.download_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        #[cfg(any(windows, target_os = "linux"))]
        {
            let installed = app.updates_rt.install_rx.as_ref().and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("The FigureIDE installer stopped unexpectedly.".into())),
            });
            if let Some(result) = installed {
                app.updates_rt.install_rx = None;
                match result {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => {
                        app.updates_rt.status_message = Some(format!("Update installation failed:\n{error}"));
                        app.updates_rt.manual_window_open = true;
                    }
                }
            } else if app.updates_rt.install_rx.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if app.updates_rt.manual_window_open || app.updates_rt.available.is_some() {
        let mut open = true;
        egui::Window::new(tl!("FigureIDE Updates"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_width(420.0)
            .show(ctx, |ui| {
                if let Some(release) = app.updates_rt.available.clone() {
                    let new_version = release.version_label();
                    ui.heading(tl!("FigureIDE Update Available"));
                    ui.label(format!("Current version: {}", env!("CARGO_PKG_VERSION")));
                    ui.label(format!("New version: {new_version}"));
                    ui.label(format!("Update channel: {}", app.ui.updates.channel.label()));
                    ui.separator();
                    ui.label(tl!("What's new:"));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.label(if release.notes.is_empty() { tl!("No release notes were published.") } else { &release.notes });
                    });
                    if updates_client::Package::current().and_then(|p| updates_client::select_asset(&release, p)).is_none() {
                        ui.label(tl!("No package is published for this platform. Open the release page to install manually."));
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some((done, total)) = app.updates_rt.download_progress.lock().ok().and_then(|state| match *state {
                        Some(craft_update::http::Transfer::Bytes { done, total })
                        | Some(craft_update::http::Transfer::Reconnecting { done, total }) => Some((done, total)),
                        None => None,
                    }) {
                        let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
                        ui.add(egui::ProgressBar::new(fraction).show_percentage().text(format!("{done} / {total} bytes")));
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(download) = &app.updates_rt.verified_download {
                        ui.label(format!("SHA-256 verified: {}", download.path.display()));
                        #[cfg(any(windows, target_os = "linux"))]
                        if app.updates_rt.install_rx.is_some() {
                            ui.label(tl!("Installing. FigureIDE will close when the helper is ready."));
                        } else if ui.button(tl!("Install and Restart")).clicked() {
                            #[cfg(windows)]
                            start_install(app, ctx, release.clone(), download.path.clone(), download.sha256.clone());
                            #[cfg(target_os = "linux")]
                            start_install_linux(app, ctx, release.clone(), download.path.clone(), download.sha256.clone());
                        }
                        #[cfg(not(any(windows, target_arch = "wasm32")))]
                        ui.label(tl!(
                            "Automatic installation is not yet supported for this platform. Open the release page for safe manual installation."
                        ));
                    }
                    #[cfg(windows)]
                    if app.updates_rt.install_rx.is_some() {
                        ui.label(tl!("Installation is waiting for the verified update helper."));
                    }
                    if let Some(message) = &app.updates_rt.status_message {
                        ui.label(message);
                    }
                    ui.horizontal(|ui| {
                        #[cfg(not(target_arch = "wasm32"))]
                        if app.updates_rt.download_rx.is_some() {
                            if ui.button(tl!("Cancel Download")).clicked()
                                && let Some(stop) = &app.updates_rt.download_stop
                            {
                                stop.cancel();
                            }
                        } else if app.updates_rt.verified_download.is_none()
                            && let Some(package) = updates_client::Package::current()
                            && updates_client::select_asset(&release, package).is_some()
                            && release.assets.iter().any(|asset| asset.name == "SHA256SUMS")
                            && ui.button(tl!("Download and Verify")).clicked()
                        {
                            start_download(app, ctx, release.clone());
                        }
                        if ui.button(tl!("Open Release Page")).clicked() {
                            app.open_url(&release.html_url);
                        }
                        if ui.button(tl!("Skip This Version")).clicked() {
                            app.ui.updates.skipped_version = Some(release.tag.clone());
                            app.prefs_changed = true;
                            app.updates_rt.available = None;
                            app.updates_rt.manual_window_open = false;
                        }
                        if ui.button(tl!("Later")).clicked() {
                            app.updates_rt.available = None;
                            app.updates_rt.manual_window_open = false;
                        }
                    });
                } else if let Some(message) = &app.updates_rt.status_message {
                    ui.label(message);
                    ui.horizontal(|ui| {
                        if ui.button(tl!("Check Again")).clicked() {
                            app.updates_rt.check_requested = true;
                            app.updates_rt.status_message = Some(tl!("Checking for updates…").to_string());
                        }
                        if ui.button(tl!("OK")).clicked() {
                            app.updates_rt.manual_window_open = false;
                        }
                    });
                }
            });
        if !open {
            app.updates_rt.available = None;
            app.updates_rt.manual_window_open = false;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn show(app: &mut VectorcraftApp, _ctx: &egui::Context) {
    if app.updates_rt.manual_window_open {
        egui::Window::new(tl!("FigureIDE Updates")).show(_ctx, |ui| {
            ui.label(tl!("Update checking is available in the desktop app."));
            if ui.button(tl!("OK")).clicked() {
                app.updates_rt.manual_window_open = false;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    #[test]
    fn default_settings_enable_beta_checks_without_blocking() {
        let app = VectorcraftApp::new(Session::new(), Default::default());
        assert!(app.ui.updates.automatic_checks);
        assert_eq!(app.ui.updates.channel, Channel::Beta);
        assert_eq!(app.ui.updates.last_successful_check, None);
    }

    #[test]
    fn check_reports_unavailable_on_wasm() {
        let mut app = VectorcraftApp::new(Session::new(), Default::default());
        let _ = check(&mut app);
        #[cfg(target_arch = "wasm32")]
        assert!(app.updates_rt.status_message.as_deref().is_some_and(|m| m.contains("desktop")));
        #[cfg(not(target_arch = "wasm32"))]
        assert!(app.updates_rt.check_requested);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn mocked_github_responses_surface_rate_limit_and_malformed_api_errors() {
        assert!(from_api_response(Fetched::RateLimited { reset: None }, Channel::Beta, "0.2.0-beta.1").unwrap_err().contains("rate limit"));
        assert!(from_api_response(Fetched::Body { text: "{}".into(), etag: None }, Channel::Beta, "0.2.0-beta.1").is_err());
        assert!(from_api_response(Fetched::NotFound, Channel::Beta, "0.2.0-beta.1").unwrap().is_none());
    }
}
