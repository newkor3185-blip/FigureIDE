# FigureIDE v0.2.0-beta.1

**Early public beta.** This prerelease packages the same source commit for Windows x64, Linux
x86_64, macOS Apple Silicon, and macOS Intel. It is intended for evaluation and feedback, not as a
stable production release.

## Downloads

The release workflow attaches these four archives and a `SHA256SUMS` manifest after every package
has built and passed archive validation:

- [Windows x64 portable ZIP](https://github.com/newkor3185-blip/FigureIDE/releases/download/figureide-v0.2.0-beta.1/FigureIDE-v0.2.0-beta.1-Windows-x64.zip)
- [Linux x86_64 portable tar.gz](https://github.com/newkor3185-blip/FigureIDE/releases/download/figureide-v0.2.0-beta.1/FigureIDE-v0.2.0-beta.1-Linux-x86_64.tar.gz)
- [macOS Apple Silicon ZIP](https://github.com/newkor3185-blip/FigureIDE/releases/download/figureide-v0.2.0-beta.1/FigureIDE-v0.2.0-beta.1-macOS-AppleSilicon.zip)
- [macOS Intel ZIP](https://github.com/newkor3185-blip/FigureIDE/releases/download/figureide-v0.2.0-beta.1/FigureIDE-v0.2.0-beta.1-macOS-Intel.zip)

## What is included

- R source editing, an interactive R console, Plot Registry, and linked ggplot figures.
- A redesigned Graph Inspector with categorized graph settings and **Copy as R Code**.
- Folder-based projects with editable `.fide` figures, vector editing, and SVG/PDF export.
- One Windows, Linux, and macOS package per supported architecture, all built from one source SHA.
- Project, third-party dependency, and bundled asset license notices in every package.

## Install

- **Windows:** extract the x64 ZIP and run `FigureIDE.exe`.
- **Linux:** extract the x86_64 tar.gz and run `./FigureIDE` from the extracted folder. Install the
  runtime libraries described in the [Linux build notes](../figureide-linux.md).
- **macOS:** extract the ZIP for your processor and move `FigureIDE.app` to Applications. These
  builds are ad-hoc signed for bundle integrity only; they have no Developer ID signature and are
  not notarized. Gatekeeper may block opening them; see [Apple's guidance for apps that are not
  notarized](https://support.apple.com/en-us/102445).

R is not bundled. Install R separately to run scripts or re-render linked plots, then install
`ggplot2`, `svglite`, and `systemfonts` from R. Vector editing, `.fide` save/reopen, and ordinary
SVG/PDF export work without R.

## Limitations and validation

- This is an early beta. Linux packaging and runtime behavior remain experimental and depend on
  host graphics, font, and desktop libraries.
- macOS packages are unsigned with Developer ID and unnotarized.
- Automated release validation checks package startup metadata, source SHA, licenses, archive
  contents, R/ggplot2 SVG generation, and architecture-specific app bundles. Headless Graph
  Inspector and vector workflow tests run in CI. These checks do not replace manual GUI testing on
  physical Windows, Linux, Apple Silicon, and Intel systems.
- R packages and Linux system libraries are user-installed dependencies.

Please report reproducible issues and feedback in
[FigureIDE GitHub Issues](https://github.com/newkor3185-blip/FigureIDE/issues).
