# FigureIDE branding and Windows distribution

Status: Windows x64 ZIP built and verified on an interactive desktop (2026-10-28).

FigureIDE is the product name for the existing R and vector editing application. The previous R Vector Studio branding report, `docs/r-vector-studio-branding.md`, records the earlier distribution and remains as historical evidence.

## Application and assets

The native title, app header, Welcome screen, About dialog, menus, status messages, Windows ProductName/FileDescription, MSI display strings, and portable distribution use **FigureIDE**. The supplied `assets/app-icon/figureide.ico` remains the Windows executable and installer icon; `figureide-desktop-icon.png` supplies the runtime window/taskbar and in-app marks. The icon artwork is user supplied, GPT generated, and authorized for MIT OR Apache-2.0 redistribution. `ASSETS.md`, `NOTICE`, and `assets/app-icon/LICENSE-figureide.txt` record this provenance and license.

## Compatibility

The Cargo package and build artifact remain `vectorcraft` and `vectorcraft.exe`. The packager copies the build artifact to `FigureIDE.exe`; Windows VERSIONINFO `OriginalFilename` and `InternalName` still describe that build artifact. The app ID, document formats, `%APPDATA%\VectorCraft`, portable `VectorCraftData`, MSI upgrade identity, and control protocol retain their existing values so user data and automation continue to work. VectorCraft attribution and upstream resource names are preserved where they identify the source project.

## Portable build

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\packaging\windows\package.ps1 -Arch x64 -PortableOnly
```

The Windows x64 ZIP output is `dist/release/FigureIDE-Windows-x64.zip`, with a `FigureIDE/` root containing `FigureIDE.exe`, `vectorcraft-cli.exe`, `portable.txt`, and `docs/` license notices. `portable.txt` keeps settings beside the executable. R and its packages remain external installed dependencies. The regular package path retains MSI creation, with FigureIDE as its displayed product and shortcut name.

## Files changed

The product name was updated in `apps/vectorcraft/{build.rs,src/main.rs}`, the visible labels and associated tests in `crates/ui-egui/src/`, and Windows package and signing metadata in `packaging/windows/`. The artwork itself was kept. The icon notice was renamed to `assets/app-icon/LICENSE-figureide.txt`, and its paths and product name were updated in `ASSETS.md` and `NOTICE`. No existing R script, document format, profile path, Cargo crate name, or control method was changed. Existing uncommitted editor work remains in place.

## Verification

- `cargo fmt --all`, `cargo check --offline -p vectorcraft`, and `git diff --check` passed. `cargo test --offline -p vectorcraft-ui-egui --lib -- --test-threads=1 --quiet` passed all 2,305 tests. `cargo xtask assets` attributed all 335 asset files; the unrelated pre-existing root `Rplots.pdf` was temporarily moved for that check and restored.
- The exact `package.ps1 -Arch x64 -PortableOnly` command built both Windows x64 Release binaries with static CRT and required Windows resources. It confirmed GUI PE subsystem 2 for the app, console PE subsystem 3 for the CLI, and `vectorcraft-cli 0.17.0`. After capitalizing the packaged executable filename, `-SkipBuild` repackaged the same verified binary. The final ZIP is `dist/release/FigureIDE-Windows-x64.zip` (77,125,440 bytes, SHA-256 `33210DE98F95673588559F847A7C07DCE76472A0ABB982C97E0C8D82F960E5A6`). The final extracted executable SHA-256 matched the desktop-verified binary (`99BD09BBA0522DA2624903A677A51EDAC9CB3D5BBE7D098469816C95F41FD09D`).
- ZIP inspection found `FigureIDE/FigureIDE.exe`, `portable.txt`, `vectorcraft-cli.exe`, and `docs/` with project, icon and third-party license notices. There are no adjacent runtime DLLs. R and its packages are separately installed requirements.
- The ZIP was extracted under `%TEMP%`, outside the source repository. Windows FileVersionInfo reported ProductName `FigureIDE`, FileDescription `FigureIDE - R and SVG scientific graphics editor`, and version `0.17.0`. The extracted app opened a native window with OS title `FigureIDE` and nonzero big and small window-icon handles. Launch without a config override created `VectorCraftData` beside `FigureIDE.exe`.
- `scripts/verify-r-workspace-ui.ps1 -Mode Desktop -SkipBuild -Executable <extracted exe> -WorkingDirectory <extracted folder>` passed. On the interactive Windows desktop it ran real R code, populated Console and Plot Registry, inserted two ggplot SVGs as linked vector panels, refreshed and selected plot artwork, navigated to source, and created and moved a rectangle. The process exited cleanly. Thirty-nine native screenshots were captured under `target/figureide-ui/desktop/`; the startup and linked-plot screenshots were visually inspected and show FigureIDE and actual plotted vector artwork.

## Remaining limits

The MSI requires WiX and was not rebuilt for this portable-only task. Windows signing credentials were not configured, so the ZIP binaries are unsigned. macOS and Linux bundle IDs/icons still use the upstream VectorCraft packaging. The actual Windows taskbar surface was not separately captured; the native window-icon handles and in-app screenshot were checked. The old R Vector Studio ZIP and report are historical artifacts, not the current FigureIDE release.
