# FigureIDE v0.2.1-beta.1

**Beta update release.** This prerelease packages the same source commit for Windows x64, Linux
x86_64, macOS Apple Silicon, and macOS Intel. It includes the desktop update implementation and
continues the v0.2.0-beta.1 release line.

## Downloads

The release workflow attaches these four archives and a `SHA256SUMS` manifest only after every
platform package has built and passed archive validation:

- Windows x64 portable ZIP: `FigureIDE-v0.2.1-beta.1-Windows-x64.zip`
- Linux x86_64 portable tar.gz: `FigureIDE-v0.2.1-beta.1-Linux-x86_64.tar.gz`
- macOS Apple Silicon ZIP: `FigureIDE-v0.2.1-beta.1-macOS-AppleSilicon.zip`
- macOS Intel ZIP: `FigureIDE-v0.2.1-beta.1-macOS-Intel.zip`
- `SHA256SUMS`

## What's included

- FigureIDE checks the public FigureIDE release feed for newer beta or stable versions.
- Before offering an update, it selects the exact package for the current architecture and verifies
  its SHA-256 checksum against the release's `SHA256SUMS` file.
- The portable Windows and Linux packages support staged update installation with rollback. Windows
  installer installs and Linux system/package-manager installs continue to use manual updates.
- macOS Apple Silicon and Intel packages are detected and verified by the updater, then installed
  manually from the release page to preserve Gatekeeper behavior.
- R remains optional. Vector editing, `.fide` save/reopen, and ordinary SVG/PDF export work without
  R; R and its plotting packages are installed separately for analysis and R-linked plot rendering.

## Installation and security

- **Windows:** extract the x64 ZIP and run `FigureIDE.exe`.
- **Linux:** extract the x86_64 tar.gz and run `./FigureIDE` from the extracted folder. Install the
  runtime libraries described in the [Linux build notes](../figureide-linux.md).
- **macOS:** extract the ZIP for your processor and move `FigureIDE.app` to Applications. The
  packages are ad-hoc signed for bundle integrity; they do not have a Developer ID signature and
  are not notarized. Gatekeeper may block opening them; see [Apple's guidance for apps that are not
  notarized](https://support.apple.com/en-us/102445).

The updater does not install silently or bypass operating-system security. Save open documents
before installing. Review `SHA256SUMS` and the package license and attribution files when downloading
manually. Checksums detect mismatched or corrupted archives but do not independently authenticate
the release publisher.

Please report reproducible issues in
[FigureIDE GitHub Issues](https://github.com/newkor3185-blip/FigureIDE/issues).
