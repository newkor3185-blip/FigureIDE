# R Vector Studio branding and Windows portable build

Status: Windows x64 ZIP built and verified on an interactive desktop (2026-10-28).

## Supplied artwork

The user supplied `assets/app-icon/figureide.ico`, `figureide-app-icon.png`, `figureide-desktop-icon.png`, and `figureide-logo.png`. The ICO contains 16, 24, 32, 48, 64, 128 and 256 px 32-bit entries. The two square PNGs are 1254 × 1254 RGB; the logo is 1983 × 793 RGBA. The supplied desktop curve mark is used as the native runtime window/taskbar icon and the in-app/About mark. The ICO is embedded as the Windows executable resource and is used by the MSI. No substitute artwork was generated or original file overwritten.

The user stated that the artwork was generated with GPT and expressly authorized MIT OR Apache-2.0 distribution. Each asset is listed in `ASSETS.md`; the dedicated notice is `assets/app-icon/LICENSE-r-vector-studio.txt`, summarized in `NOTICE`. The original VectorCraft artwork keeps its separate license and attribution.

## Product name and compatibility

The native window title, app header, About window, Welcome text, app menu, update/status copy and Windows VERSIONINFO ProductName/FileDescription display **R Vector Studio**. Document titles still retain the document name and dirty `*` marker. The Windows ICO resource, runtime viewport icon and Windows big taskbar icon use the supplied artwork.

Cargo package and binary names remain `vectorcraft`. Windows VERSIONINFO `OriginalFilename` and `InternalName` remain `vectorcraft.exe` / `vectorcraft`, describing the original build artifact. The portable packager copies this verified binary as `r-vector-studio.exe`; no second Rust target is introduced. `cargo run -p vectorcraft --release` and the control channel remain supported.

The app ID, document extensions, `%APPDATA%\VectorCraft` profile, `VectorCraftData` portable profile, installer registry keys and existing release upgrade identity remain unchanged to retain user preferences and saved documents. VectorCraft source attribution and format names remain visible where they identify the upstream project or `.vectorcraft` files. The MSI keeps the existing internal executable path and installation identity while showing R Vector Studio in its product name and shortcuts.

## Packaging

Build the x64 ZIP on Windows without WiX:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\packaging\windows\package.ps1 -Arch x64 -PortableOnly
```

The script rebuilds both binaries for `x86_64-pc-windows-msvc` with static CRT and required Windows resources, checks PE machine/subsystem fields, signs when configured, and creates `dist/release/R-Vector-Studio-Windows-x64.zip`. The ZIP root is `R-Vector-Studio/`, containing `r-vector-studio.exe`, `vectorcraft-cli.exe`, `portable.txt`, and `docs/` with project and icon license notices. The marker activates the unchanged `VectorCraftData` portable profile beside the renamed executable. The normal packaging path still builds the existing MSI and then this ZIP; `-PortableOnly` skips WiX and installer artwork.

## Files changed

`apps/vectorcraft/build.rs` embeds the supplied ICO and Windows product metadata. `apps/vectorcraft/src/main.rs` supplies the runtime window icon, title and remaining startup status copy. The in-app mark and visible product labels were updated in `crates/ui-egui/src/{brand,canvas,chrome,menus,native_menu,prefs_dialog,scientific,tests_home,tests_nativemenu,updates_ui}.rs` and `crates/ui-egui/src/dialogs/{about,recovery}.rs`, with focused UI test expectations in `crates/ui-egui/src/i18n/tests.rs`. `packaging/windows/{package.ps1,portable.txt,vectorcraft.wxs}` handles the renamed ZIP and MSI labels. `scripts/verify-r-workspace-ui.ps1` accepts an external executable and working directory for extracted-package testing. The four supplied images, `ASSETS.md`, `NOTICE`, and the dedicated icon license notice record the artwork and redistribution permission. Existing staged R editor changes were retained.

## Verification

- `cargo fmt --all`, `cargo check --offline -p vectorcraft`, `cargo build --release -p vectorcraft`, and `git diff --check`: passed. `cargo test --offline -p vectorcraft-ui-egui --lib -- --test-threads=1 --quiet`: 2,305 tests passed. `cargo xtask assets`: all 335 asset files attributed. The pre-existing root `Rplots.pdf` was moved temporarily for this asset check and restored afterward.
- The exact documented `package.ps1 -Arch x64 -PortableOnly` command completed. It built `vectorcraft` and `vectorcraft-cli` with `--release --locked` for `x86_64-pc-windows-msvc`, required Windows resources, and static CRT. The script confirmed x64 PE machine type, GUI subsystem 2 for the app, console subsystem 3 for the CLI, and `vectorcraft-cli 0.17.0`. A documentation-only correction in `ASSETS.md` was repackaged with `-SkipBuild`; the final ZIP is `dist/release/R-Vector-Studio-Windows-x64.zip` (77,127,029 bytes; SHA-256 `7A6FD04CECCD7D1AFD46417E562E1680E19495528D61A5DA5EF3BCD40BB34FFC`). The final ZIP was extracted again and its executable SHA-256 matched the desktop-verified binary (`21684D1A60E2C19898D79CE085F704030D48E0A49420562B7C51ECB0BFA3370D`).
- ZIP inspection confirmed `R-Vector-Studio/r-vector-studio.exe`, `vectorcraft-cli.exe`, `portable.txt`, and `docs/` containing the project and third-party license notices. There are no adjacent runtime DLLs. R and its packages remain separately installed requirements.
- The final ZIP was extracted under `%TEMP%`, outside the repository. Windows FileVersionInfo reports ProductName `R Vector Studio`, FileDescription `R Vector Studio - R and SVG scientific graphics editor`, and version `0.17.0`. The extracted executable opened a real native window with OS title `R Vector Studio`. Its big and small window-icon handles were present; the big icon was extracted and visually checked against the supplied artwork. The embedded PE icon was also extracted and inspected. The native UI screenshot shows the new in-app mark.
- Launching the extracted app with no config override created `VectorCraftData` next to `r-vector-studio.exe`, confirming that `portable.txt` activates the existing portable profile. The GUI PE subsystem and native launch produced no parent console window.
- `scripts/verify-r-workspace-ui.ps1 -Mode Desktop -SkipBuild -Executable <extracted exe> -WorkingDirectory <extracted folder>` passed against the final ZIP. Its native desktop run executed real R code, populated the Console, Environment and Plot Registry, inserted two ggplot SVGs as linked vector panels, refreshed a panel, navigated to source, and created/moved a rectangle with the VectorCraft command engine. It exited cleanly and saved 39 native captures under `target/r-vector-studio-final-ui/desktop/`. The linked-panel capture was visually inspected: the plotted points and bars appear as artwork, and a plot group is selected with handles.

## Remaining limits

The ZIP is unsigned because Windows signing credentials are not configured. The MSI/WiX path was kept but not built on this machine. The macOS bundle and Linux launcher still carry their existing VectorCraft package IDs/icons; this focused Windows distribution update does not migrate bundle IDs or user settings. The update service and original upstream community links remain VectorCraft resources. The actual Windows taskbar surface was not captured separately; the operating-system window-icon handles and extracted icon image were checked.
