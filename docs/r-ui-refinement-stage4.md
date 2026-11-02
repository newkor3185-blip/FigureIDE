# R UI refinement - Stage 4

Status: completed and verified (2026-10-28). Only the Inspector and selected R-linked plot properties were changed. Stages 1-3 remain in the working tree; Stage 5 was not started. No commit or push was made.

## Before and after

The Stage 3 Windows capture showed a selected SVG plot and its R Plot Properties only below the Linked Plot Panel in the left utility area. The right Properties dock identified the object merely as a Group and began with its ordinary point-based Transform fields. The left plot size controls had no W/H labels, and the resize-method drop-down did not explain what R would rerender versus what VectorCraft would transform. On a long R status, the raw message could fill many Inspector lines.

Now a selected linked plot appears at the top of the native Properties dock as `R-linked plot · Panel A/B`, with its R variable, SVG-vector connection, labeled width and height in millimeters, aspect lock, and explicit Reflow/Scale choices. Reflow renders the R object at the chosen device dimensions; Scale resizes the existing vector artwork without running R. The adjacent explanation changes with the choice. `Update from R` refreshes the linked SVG at its stored size, while `Apply Resize` applies the chosen dimensions and method. Both use the existing rendering/resize paths and are disabled as appropriate when R is busy or its plot object is unavailable. Ordinary Transform remains below, explicitly labeled as a vector transform in document points. The same compact controls remain accessible in the left Linked Plot Panel when the Properties dock is hidden.

The property status is one short line with the full diagnostic in its tooltip. A link metadata change updates untouched mm fields and resize mode; a user's uncommitted input draft is retained until they apply it or leave/reselect the plot. Selecting ordinary artwork still shows the original VectorCraft Properties sections. Panel IDs, R variable bindings, generated SVG group, label group and vector editing commands are unchanged.

## Files changed

- `crates/ui-egui/src/panels/properties.rs`: inserts the linked-plot section before the ordinary Transform section only for a selected R link in the scientific workspace.
- `crates/ui-egui/src/scientific.rs`: shares property controls between Inspector and Linked Plot Panel; adds connection/size/method/action feedback and draft-to-metadata synchronization. The existing `render_plot`, `PanelAction::Refresh`/`Reflow`, `object.resizeRPanel`, and linked-panel persistence paths remain the action implementations.
- `crates/ui-egui/src/scientific_ui_tests.rs`: builds a persistent linked group in the complete workspace harness; checks 900/1280-point layouts at 100%/150%, selection/ordinary-object visibility, Scale without an R session, stored link identity/dimensions, metadata synchronization and draft retention.
- `scripts/verify-r-workspace-ui.ps1`: selects a real SVG plot in the native app, captures its Inspector before/after, clicks Inspector `Update from R`, checks the completed backend status and resumes the existing linked-panel workflow.
- This report and `docs/r-workspace-ui-verification.md`.

## Verification and visual evidence

The final `-Mode Auto` run exercised the existing 320/420/560/800-point R-pane matrix, the Stage 3 whole-workspace matrix and the new selected-Inspector cases. Its geometry and serial PNG runs each passed all 16 focused tests and produced 87 headless PNGs. The native driver used the actual Windows application and R/ggplot2, imported two SVG plots, refreshed one through the new Inspector button and another through the existing Linked Plot Panel button, exercised Console/Source and moved an ordinary vector rectangle. It recorded 37 native captures and exited normally with code 0. The final native capture shows a selected `plot_A`, millimeter dimensions, Reflow/Scale and an `Update from R` completion message next to the editable SVG artwork.

Images opened and inspected: [Stage 3 native baseline](../target/r-ui-refinement-stage3/after/desktop/linked-320px-scale200-fixture.png), [Stage 4 selected Inspector after real R update](../target/r-ui-refinement-stage4/verified/desktop/console-420px-scale200-stage4-inspector-linked-after-update.png), [Stage 4 Linked Plot Panel](../target/r-ui-refinement-stage4/verified/desktop/linked-320px-scale200-fixture.png), and [900-point headless selected Inspector](../target/r-ui-refinement-stage4/verified/headless/console-900px-scale100-stage4-inspector-linked-selection.png). Captures are ignored local artifacts. Screenshot generation alone is not counted as inspection.

| Check | Result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib --quiet` | 2,299 passed; 0 failed |
| `scripts/verify-r-workspace-ui.ps1 -Mode Auto -Artifacts target/r-ui-refinement-stage4/verified` | Both headless modes and native Windows mode passed |
| `git diff --check` | Passed |

## Remaining issues and limits

At a 640-point whole-window width, the existing dock may shrink below the space required by the generic VectorCraft Transform/Document controls; some of those labels and numeric fields remain clipped. The new R section was visually inspected at 900 points and on the full native desktop, but this task does not claim to solve every generic Inspector layout at extreme window widths. A contextual vector quick-action bar can cover part of the dock in a narrow headless screenshot while a group is selected. The R properties remain in the scrollable dock and in the Linked Plot Panel. These broader cross-panel and overlay issues belong to Stage 5.

Native captures use the available Windows desktop at 200% scale; headless cases cover 100% and 150%. R and its packages remain external runtime dependencies. No release/portable ZIP, full workspace `cargo xtask ci`, or WASM build was run for this scoped UI task. Existing Cargo unused-egui-patch warnings and the installed ggplot2 R-version warning do not prevent the verified workflow.
