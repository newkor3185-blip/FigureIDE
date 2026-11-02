# R UI refinement - Stage 5

Status: completed and verified (2026-10-28). This pass validates the Stage 1-4 R workspace UI together and makes two narrow-window corrections. No later stage was started, and no commit or push was made.

## Findings and corrections

The Stage 4 selected-plot captures showed two remaining conflicts at small whole-window widths. At 640 logical points, the VectorCraft Properties dock received less width than its existing Transform/Document controls need, clipping the controls at the right edge. A selected group's full Contextual Task Bar was wider than the vector canvas and painted over the Inspector. Both issues came from combining otherwise valid R and vector panels in one narrow window; the R execution and SVG paths were not involved.

- In the scientific workspace only, a Properties dock narrower than 220 points now has horizontal scrolling plus always-visible left/right buttons at its top. The controls and their existing commands are kept intact. Buttons move by 100 points; a focused control can still request scrolling. This makes the hidden half of a row reachable without changing the saved dock width or the normal VectorCraft workspace.
- When the Contextual Task Bar would be wider than the scientific canvas, it becomes a small on-canvas Selection actions menu. Ungroup, Isolate, Duplicate, Fill, Recolor, Lock and the bar's placement/visibility commands remain available in that menu. At wider canvas sizes, the original bar and its button order remain. The same compact branch handles selection/Artboard task items using their existing commands.
- The final native script selects a real linked SVG plot, narrows the whole window to 760 points, asserts that the task bar stays within the actual canvas rectangle, captures both Inspector scroll positions, restores the wide window and continues the existing linked-plot refresh/vector editing workflow.

No R bridge, persistent session, `.GlobalEnv`, script import/save path, Console evaluator, plot registry, SVG importer, linked-panel identity, vector engine command or theme token was changed. Earlier uncommitted Stage 1-4 work remains in place.

## Files changed in Stage 5

- `crates/ui-egui/src/canvas.rs`: adaptive Contextual Task Bar layout and compact action menu, gated by the scientific workspace and actual canvas width.
- `crates/ui-egui/src/panels/properties.rs`: scientific narrow-Inspector scrolling and visible navigation buttons; ordinary Inspector contents remain the same.
- `crates/ui-egui/src/scientific_ui_tests.rs`: regression cases for selected task-bar bounds/menu actions at 640/900/1280 points, and both sides of a 640-point Inspector row.
- `scripts/verify-r-workspace-ui.ps1`: real Windows narrow selected-plot resize, bar-bound assertion, scroll-button gestures and screenshots.
- This report, `docs/r-workspace-ui-verification.md`, and `docs/control-protocol.md`.

## Cross-stage verification

The final `-Mode Auto` workflow passed its 18 focused layout/interaction tests in geometry and serial offscreen-capture modes. It generated 91 headless PNGs at 100%/150% scale. The native Windows run used R 4.6.0 and installed ggplot2, generated 39 scenario captures at 200% scale, and exited with code 0. It verified two script buffers and Korean text, Source/Console execution in the shared R environment, history/draft and long scrollback, plot discovery, two actual ggplot SVG imports, linked refresh through both the Inspector and Linked Plot Panel, and ordinary vector rectangle movement. It also exercised the Stage 3 dividers, window sizes and Tools placement.

In the native 760-point window, `ui.inspect` reported canvas `[400, 101, 188, 838]` and compact task bar `[408, 412, 58, 44]` in logical points. The bar fits within the canvas's horizontal bounds. Native images show its menu button on the artboard and the Inspector's two scroll positions; the second position exposes `Apply Resize` and the ordinary W/H transform controls. Headless tests open the compact menu and find its retained group actions, then confirm the selected linked group remains in the document. The narrow Inspector test clicks both navigation buttons and verifies the corresponding Document Setup/Edit Artboards controls' centers enter the visible dock.

| Command | Result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib --quiet` | 2,301 passed; 0 failed; 0 ignored |
| `scripts/verify-r-workspace-ui.ps1 -Mode Auto -SkipBuild -Artifacts target/r-ui-refinement-stage5/verified` | Geometry, serial PNG and native Windows passes succeeded; `-SkipBuild` used the current-source debug binary built in the immediately preceding Auto run |
| `git diff --check` | Passed |

The first final Auto attempt built the current binary and completed the headless cases but stopped in the native driver because PowerShell 5 misdecoded literal arrow characters in the new script. The script now constructs those characters from Unicode code points. A subsequent Desktop run and the final Auto run both completed normally. This was a driver encoding issue, not an application crash.

## Before/after image review

Opened and inspected, rather than merely generated:

- [Stage 4 narrow selected baseline](../target/r-ui-refinement-stage4/verified/headless/console-900px-scale100-stage4-inspector-linked-selection.png): the full task bar extended across the narrow Inspector.
- [Stage 5 640-point selected headless view](../target/r-ui-refinement-stage5/verified/headless/console-640px-scale100-stage5-selection-actions-contained.png): compact bar on the canvas and Inspector navigation at the top.
- [Stage 5 native narrow Inspector before scrolling](../target/r-ui-refinement-stage5/verified/desktop/console-420px-scale200-stage5-narrow-selected-inspector.png) and [after scrolling](../target/r-ui-refinement-stage5/verified/desktop/console-420px-scale200-stage5-narrow-inspector-scrolled.png): left and right property controls can each be reached.
- [Native Korean Environment](../target/r-ui-refinement-stage5/verified/desktop/environment-320px-scale200-unicode-filter.png), [selected Plot Registry](../target/r-ui-refinement-stage5/verified/desktop/plots-320px-scale200-fixture.png), [second Source tab and Console](../target/r-ui-refinement-stage5/verified/desktop/console-320px-scale200-source-second-tab.png), and [linked SVG artwork and Inspector](../target/r-ui-refinement-stage5/verified/desktop/linked-320px-scale200-fixture.png): the Stage 1-4 presentation remains intact.

## Remaining limits

At 640/760-point whole-window widths, the dock is too narrow to show both halves of the existing VectorCraft property rows at once. The new visible navigation makes them reachable; the inactive side is intentionally clipped by the horizontal viewport. A larger redesign that simultaneously displays the R editor, useful canvas and full Inspector at that width would require a different pane/collapse policy. The native desktop was available only at 200% scale; 100%/150% were verified headlessly. Only the Windows Rust target is installed, so a WASM build could not be run. The scoped task did not run full `cargo xtask ci` or rebuild a release/portable ZIP. R/packages remain external dependencies. Cargo's pre-existing unused-egui-patch messages and ggplot2's installed-version warning remain visible.
