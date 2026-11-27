# FigureIDE v0.3.0

Release notes prepared for FigureIDE v0.3.0, covering improvements merged since v0.2.1-beta.1.
The coordinated release workflow is configured to build Windows x64, Linux x86_64, macOS Apple
Silicon, and macOS Intel packages from one source commit, validate each archive, and generate a
`SHA256SUMS` file before draft creation.

## Highlights

- **R Console and Script Editor improvements:** the Console supports session-aware completion with
  keyboard navigation and completion acceptance. Script Editor find and replace supports
  case-sensitive, whole-word, and regular-expression options, with replacements recorded for undo.
- **R function argument suggestions:** completion can suggest arguments for R functions.
- **R plot capture and history:** capture plots produced by base R and ggplot2 into the FigureIDE
  Plots history while preserving their aspect ratio for figure editing.
- **Automatic graph placement:** newly inserted R graphs are placed in the earliest available region
  of the artboard, scanning left to right and then top to bottom, with an 18 pt margin and spacing.
  Placement accounts for graph bounds and remains undoable (PR #33).
- **Graph Inspector:** categorized graph properties and a Copy as R Code action make graph styling
  and code reuse easier (PR #20).
- **Document tabs:** documents can be renamed inline or from the tab context menu (PR #18).
- **Desktop updates:** the updater detects newer releases, selects the matching platform package,
  validates it against `SHA256SUMS`, and supports staged updates with rollback for portable Windows
  and supported Linux installs (PR #25).

## Downloads

The validated release is expected to contain these assets:

- Windows x64 portable ZIP: `FigureIDE-v0.3.0-Windows-x64.zip`
- Linux x86_64 portable tar.gz: `FigureIDE-v0.3.0-Linux-x86_64.tar.gz`
- macOS Apple Silicon ZIP: `FigureIDE-v0.3.0-macOS-AppleSilicon.zip`
- macOS Intel ZIP: `FigureIDE-v0.3.0-macOS-Intel.zip`
- `SHA256SUMS`

## Installation and update limitations

- **Windows:** extract the portable ZIP and run `FigureIDE.exe`. Automatic updates are supported for
  the portable ZIP installation; installer-based or otherwise relocated installations require a
  manual update.
- **Linux:** extract the tar.gz and run `./FigureIDE` from its directory. Automatic updates require
  a supported portable installation whose parent directory is writable. System package-manager
  installs and locations requiring elevated privileges need manual updates. Linux remains
  experimental, and required desktop runtime libraries are not bundled.
- **macOS:** select the ZIP for the processor and move `FigureIDE.app` into Applications. The updater
  can detect and verify an update but leaves installation to the user. Packages are ad-hoc signed
  and not notarized; Gatekeeper may prevent them from opening.
- **R:** R and R packages are not bundled. Install R separately for analysis, R Console features,
  and R-linked plot reflow. Vector editing, `.fide` save/open, and ordinary SVG/PDF export work
  without R.

Updates are not installed silently and do not bypass operating-system security checks. Checksums
detect archive corruption or mismatch but do not independently authenticate the publisher.
