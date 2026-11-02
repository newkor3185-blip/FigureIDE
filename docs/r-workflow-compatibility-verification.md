# R workflow compatibility verification

**Status:** implementation verified by real R integration tests, SVG import/export, VectorCraft PNG rendering and Windows desktop capture.  
**Date:** 2026-10-28  
**Branch:** `feature/r-execution-engine`

This report uses synthetic research data. The pasted request did not include an accessible RStudio screenshot or the user's original research script, so the visual reference and exact failing script could not be compared directly.

## 1. Session indicator

- **Reproduction/root cause:** The toolbar and Console used context colors for the status dot. The color could appear blue for Ready, and the two surfaces were not driven by one explicit color mapping.
- **Change:** `scientific.rs` maps actual session state to green Ready, red Running, amber Checking, distinct magenta Error and gray Disconnected, with text labels on both surfaces.
- **Verification:** The state/color regression test covers each state. The slow R test checks that Running remains active during evaluation. The native desktop capture shows a green `R: Ready` dot and label.

## 2. Ctrl+Enter source navigation

- **Reproduction/root cause:** The worker sent source navigation only after R evaluation returned; `Sys.sleep(3)` therefore held the caret at its old position.
- **Change:** `scientific.rs` sends `CurrentExpressionAccepted` immediately after the persistent R parser locates and validates the complete expression. The UI navigates on this message while evaluation continues. A busy request is rejected without moving the caret. EOF, comment skipping, source revision and focus behavior remain in the existing navigation path.
- **Verification:** A real `Sys.sleep(3)` R test measures the caret update with 10 ms polling and requires it within 2.5 seconds, while the three-second evaluation remains active; a busy request leaves it in place. This is an upper bound rather than a benchmark median. Existing multiline and EOF navigation tests pass.

## 3. Environment browser

- **Reproduction/root cause:** The previous Environment list showed generic type/class text without semantic Data, Values and Functions groups.
- **Change:** The R bridge in `r_session.rs` sends bounded category and description fields. Classification uses `is.function`, tabular inheritance, matrix/data frame checks and S4 dimensions. R formats compact data frame, matrix, vector, function and plot descriptions. Object size is sampled for S4 or large values and cached by a lightweight signature. `scientific.rs` shows collapsible counted sections with object name and description columns. Refresh, filter, View Data, Registry navigation, Remove and Clear All remain available.
- **Verification:** Real R integration tests cover data frames, matrices, lists, large vectors, functions and Korean names. UI tests cover the categorized layout. The native fixture verifies Data/Values/Functions categories through `ui.inspect`; its Environment screenshot shows the populated Data and Values sections. Functions is below the fold when 32 Values are expanded; the headless capture shows its section header. On Windows the R child no longer inherits the unsupported `C.UTF-8` locale from the development shell, which had prevented Korean metadata from round-tripping.
- **Limit:** Cached size is approximate and may be stale when an object changes contents without changing class, length or dimensions. Large metadata previews remain bounded.

## 4. Named pheatmap plots

- **Reproduction/root cause:** Registry filtering and R rendering accepted `ggplot` only. Installed `pheatmap` returns a named list with class `pheatmap` and a `gtable` component; ordinary lists have no reusable plot representation.
- **Change:** `r_session.rs` reports a `plot_kind` for ggplot or verified pheatmap objects. The persistent `.GlobalEnv` object is retrieved by name. pheatmap rendering calls `grid.newpage()` and `grid.draw(object[["gtable"]])` on the SVG device; ggplot continues through `print()`. `scientific.rs` uses `plot_kind` for the Registry, linked panels, refresh and reflow.
- **Verification:** Real R tests cover silent and normally drawn named pheatmaps, long labels, annotation, color legend, rejection of an ordinary list, two physical sizes, SVG import and export, and linked refresh/reflow. The generated SVG contains colored paths; the exported VectorCraft SVG retains blue and red and hundreds of vector paths. The native `workflow-heatmap-artboard.png` capture shows colored heatmap cells and editable selection handles. The existing ggplot tests also pass.
- **Limit:** Side-effect-only unnamed pheatmap calls have no reusable `.GlobalEnv` object and are intentionally absent from the Registry. Other grid plot classes are not yet supported.

## 5. Black ggplot artwork

- **Reproduction/root cause:** The synthetic `CellLine`/`Description` dot plot produces `#0000FF`, `#B38BFF`, `#FF9E81` and `#FF0000` in `ggplot_build`. With `svglite` unavailable, Windows `grDevices::svg` emits a Cairo SVG 1.2 arithmetic composition: the actual colored plot sits in referenced SVG groups inside `<defs>`, while the visible root is a filtered black rectangle. The previous SVG import could not resolve that filter and imported only the rectangle. The first color loss was at SVG import, not in ggplot or R's SVG output.
- **Change:** `crates/svg/src/import.rs` recognizes only Cairo's exact `k1=0, k2=1, k3=1, k4=0` source-over wrapper with two valid compositing groups. It replaces that wrapper with references to its destination and source vector groups, removing the destination's Cairo inverse mask, before the normal SVG parser runs. The first version kept that mask and still rendered black, which the PNG review exposed. Ordinary black SVG is untouched. The original R SVG is not recolored.
- **Verification:** The real R fixture, `target/r-workflow-color/source-r.svg`, contains colored points and the Cairo wrapper. After import/export, `target/r-workflow-color/imported-export.svg` retains the blue/red scale, white background, 146 paths and one image. `target/r-workflow-color/imported-render.png` was opened and visually inspected: the four colored points, axes, text and legends are visible on white. The native `workflow-gradient-artboard.png` capture shows its colored point, gradient legend and editable selection handles; previous plots occupy part of the same artboard and obscure some of this fixture. A pixel regression test checks white, red and blue areas in VectorCraft's own renderer. The image originates from Cairo's own rasterized legend gradient in its fallback SVG; points, axes and text remain vector shapes. A separate SVG importer regression test covers exact composition and ordinary black artwork. `svglite` was not installed, so only the existing Cairo fallback was exercised on this machine.
- **Limit:** The Cairo fallback itself embeds its continuous legend gradient as an image; VectorCraft preserves that source image, while the plot's paths remain editable. A `svglite` installation may produce a more fully vector legend.

## Files and checks

- `crates/ui-egui/src/scientific.rs`: status UI, immediate navigation, grouped Environment and plot-kind Registry.
- `crates/ui-egui/src/r_session.rs`: R metadata and plot dispatch; real R integration tests.
- `crates/ui-egui/src/scientific_ui_tests.rs`: UI state and layout expectations.
- `crates/svg/src/import.rs`: narrowly scoped Cairo composition import and regression test.
- `scripts/fixtures/r-workflow-compatibility.R` and `scripts/verify-r-workspace-ui.ps1`: synthetic native workflow fixture and captures.

Completed on the final source: `cargo fmt --all`; `cargo check --offline -p vectorcraft`; `cargo test --offline -p vectorcraft-ui-egui --lib` (**2,310 passed**); targeted `vectorcraft-svg` Cairo regression; `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings`; and `scripts/verify-r-workspace-ui.ps1 -Mode Auto` (19/19 headless geometry tests, 19/19 offscreen capture tests and native desktop pass). The final native captures were reopened and inspected. The earlier verifier's stale UI Automation filter reference was corrected and `-Mode Desktop -SkipBuild` also passed during the correction.

Native screenshots were inspected at `target/r-workspace-ui/desktop/environment-560px-scale200-workflow-categories.png`, `workflow-gradient-artboard.png` and `workflow-heatmap-artboard.png`. They show the actual Windows app, not a browser mockup.

Generated diagnostics are under `target/r-workflow-color/` and `target/r-workflow-pheatmap/`; they are outside tracked source. No commit or push was made.

The `wasm32-unknown-unknown` Rust target is not installed on this Windows machine, so a WASM build was not run. No unrelated VectorCraft format or renderer code changed.
