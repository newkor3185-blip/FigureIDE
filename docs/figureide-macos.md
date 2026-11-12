# FigureIDE macOS build

The macOS workflow builds separate native application bundles for Apple Silicon (`arm64`) and Intel
(`x86_64`) on GitHub-hosted macOS runners. Each ZIP contains a `FigureIDE.app`, a generated `.icns`
from the supplied FigureIDE icon, project and dependency license notices, and installation notes.
R and R packages remain external dependencies.

The generated app is ad-hoc signed so its bundle structure can be validated. It is not signed with a
Developer ID certificate and is not notarized. Gatekeeper may block an app downloaded from the
internet. The workflow does not create a public release or a signed installer.

The CI job runs Cargo checks and tests, an R-backed ggplot integration regression using the built-in
`iris` data, validates the `.app` metadata and executable architecture, then uploads the ZIP as a
GitHub Actions artifact. These automated checks do not verify Mac-specific visual details or
peripheral interaction. Manual launch and GUI testing on physical Apple Silicon and Intel Macs is
still required before a stable release.

The R session locates `Rscript` through `PATH`, `R_HOME`, `/opt/homebrew/bin` (Apple Silicon
Homebrew), `/usr/local/bin` (Intel Homebrew), and
`/Library/Frameworks/R.framework/Resources/bin` (CRAN framework installations). Install `ggplot2`,
`svglite` and `systemfonts` in the user's R library to use linked plots.
