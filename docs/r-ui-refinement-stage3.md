# R UI refinement - Stage 3

Status: completed and verified (2026-10-28).

Scope: the main scientific workspace toolbar, pane proportions, dividers and existing VectorCraft Tools placement. Stages 1-2 changes were retained. Stages 4-5 are deferred. No commits or pushes are made.

## Baseline problems

- All commands and the full R detection/execution status shared a long unstructured wrapping row. A lengthy R installation path could increase toolbar height substantially.
- Source/Console's six-point divider had no visible grip or resize cursor. Its movement used the entire pane height rather than the editor's actual height budget.
- The horizontal layout reserved an approximate tool width but omitted implicit gaps between children, so the canvas allocation could extend past the intended right edge.
- A drag starting in a window-clamped workspace used the stored preferred width, causing an initial jump. Main-window resizing and explicit divider adjustment needed separate handling.
- Embedded VectorCraft Tools always rendered in one column and ignored the existing floating position. Startup sanitization also reduced saved R workspace widths above 900 points, despite the UI control accepting up to 1600.

Before images and the Stage 2 version of `scientific.rs` are preserved locally under `target/r-ui-refinement-stage3/before/`.

## Changes

- New/Open/Save/Save as and Run/Source remain directly accessible. Plots, R session and Workspace menus group less frequent existing actions. Run/Source and session actions reflect existing busy/session state. Keyboard shortcuts and File menu integration are unchanged.
- Session status uses concise Checking/Available/Ready/Running/Error/Disconnected text, with the full existing status available in a tooltip. Detection, execution and error reporting still use the same backend.
- Pane widths now account exactly for the divider and docked Tools width, with zero implicit horizontal gaps. Source retains its preferred width while the window temporarily clamps the visible allocation. Minimum pane sizes adapt to small windows; the vector workspace retains space to render its canvas.
- Both dividers have a ten-point hit area, visible line/grip, hover/focus styling and the correct resize cursor. Drags start from the displayed pane size and use total displacement. Arrow keys adjust the focused divider by 16 points and Shift+arrows by 64. Workspace > Reset pane sizes restores defaults. Source's vertical divider uses the actual editor budget and reserves space for utility tabs/Console. Maximize/restore remains available.
  Clicking/dragging explicitly focuses the divider. While it owns focus, workspace key routing prevents its arrows from also nudging artwork or activating canvas tools.
- Embedded Tools honors the existing single/double-column and floating/docking state. The normal and scientific workspaces share the existing floating-Tools implementation; tool order, flyouts and editing commands are preserved.
- Saved workspace-width validation now matches the supported 240-1600 point control range. Current viewport limits apply when rendering, without overwriting the stored preference merely because a window narrows.

No R session, IPC, evaluator, script format, SVG importer, linking model, Inspector property redesign or vector engine code was changed.

## Files

- `crates/ui-egui/src/scientific.rs`: extracted `show_workspace_toolbar`; added `workspace_status`, explicit workspace/editor size calculations, shared divider rendering/input and read-only workspace geometry. Source/Console internals retain their Stage 2 implementation.
- `crates/ui-egui/src/toolbar.rs`: `embedded_width`, `show_embedded`, and shared `show_floating_tools`; the normal toolbar keeps its behavior.
- `crates/ui-egui/src/state.rs`: aligned saved R workspace width bounds with UI controls.
- `crates/ui-egui/src/scientific_ui_tests.rs`: full production workspace and toolbar matrices, real multi-frame pointer drags, keyboard resizing, narrow/wide restoration, saved-width round trip, menu/busy state and existing Tools column/float/dock behavior.
- `scripts/verify-r-workspace-ui.ps1`: native multi-frame divider gestures, keyboard/reset actions, maximize/restore, whole-window widths and double/floating Tools checks, alongside the existing real R/Console/SVG/vector verification.
- This report and control/verification documentation.

## Verification and visual comparison

| Command | Result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib --quiet` | 2,296 passed; 0 failed; 0 ignored |
| `git diff --check` | Passed |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-r-workspace-ui.ps1 -Mode Auto -Artifacts target/r-ui-refinement-stage3/after` | Passed; 13 focused tests in each geometry/serial-capture pass; native Windows verification passed |

The final headless run generated 83 JSON measurements and 83 PNGs. The inherited panel/Source/Console tests and isolated toolbar controls report no control overlap; the new whole-workspace checks verify pane/canvas bounds and disjoint pane allocations. Whole-window cases cover 640x720, 900x720, 1280x900 and 1600x1000 at 100%/150%, alongside the existing 320/420/560/800 R-pane width matrix. Inspector internals are outside these Stage 3 geometry assertions and retain the narrow-width limitation below.

The presented Windows application generated 35 scenario captures at 200% scale. Measurements include:

| Interaction | Observed behavior in logical points |
|---|---|
| Drag R workspace +80 | Actual/pref width 420 -> 500; direction-key adjustment then reached 516 |
| Drag Source divider -50 | Editor interaction height 266.4375 -> 216.4375 |
| Reset pane sizes | Preferred width 420; editor fraction 0.62; maximization cleared |
| Whole window narrowed to 760 | Preferred R width remained 420; displayed R width 334; vector pane 360; canvas 188 |
| Two-column Tools | Docked Tools width 56 -> 88 |
| Floating Tools | Docked width became zero; canvas width grew from 718 to 806 |

Native verification also passed Source maximize/restore, 760/1000/1600 window resizing, two independent script buffers with Korean draft/dirty state, Source and shared Console execution, history/draft/Escape behavior, long output scrollback, and multiline execution returning 7. Two actual ggplots were discovered, imported as SVG and refreshed; an ordinary vector rectangle was created and moved. `desktop/result.json` records success, 35 captures and normal exit code 0. No test-owned instance remains running.

The current native width-drag/floating-Tools images, representative native vertical-drag/narrow-window/linked-artwork images, and headless toolbar/full-workspace/floating-Tools images were opened and inspected. The toolbar stays compact, both divider grips are visible, Tools icons render normally, and native linked screenshots display actual scatter/bar vector artwork. Not every generated PNG was individually reviewed. The separate image-review record is `target/r-ui-refinement-stage3/image-review.json`.

Local before/after evidence (generated artifacts are ignored by Git):

- [Stage 2 baseline](../target/r-ui-refinement-stage3/before/desktop/console-320px-scale200-source-first-tab.png).
- [Native width divider](../target/r-ui-refinement-stage3/after/desktop/console-500px-scale200-stage3-width-drag.png), [native height divider](../target/r-ui-refinement-stage3/after/desktop/console-420px-scale200-stage3-height-drag.png).
- [Native narrow window](../target/r-ui-refinement-stage3/after/desktop/console-420px-scale200-stage3-window760.png).
- [Native floating Tools](../target/r-ui-refinement-stage3/after/desktop/console-420px-scale200-stage3-tools-floating.png).
- [Native SVG artwork](../target/r-ui-refinement-stage3/after/desktop/linked-320px-scale200-fixture.png).
- [320-point toolbar](../target/r-ui-refinement-stage3/after/headless/console-320px-scale100-stage3-toolbar-density.png), [640-point whole workspace](../target/r-ui-refinement-stage3/after/headless/console-640px-scale100-stage3-main-workspace.png).

Cargo still prints the pre-existing unused-egui-patch warnings. The installed ggplot2 package's R-version warning remains visible; the real fixture completed successfully.

Native checks use the available Windows desktop at 200% scale; headless layouts cover 100%/150%. Generated files are ignored under `target/r-ui-refinement-stage3/after/`. Screenshot generation is recorded separately from actual image review. The driver closes its isolated test application normally.

The native driver uses the existing `ui.drag` control method: the application's raw-input hook delivers each pointer step on a separate settled native UI frame. UI Automation's client-origin conversion uses a temporary per-monitor DPI-aware thread context. The driver waits for both the requested width and the actual rendered pane width before using a divider's next position. The full-workspace headless harness installs the same SVG image loaders used by the app, so its tool icons render correctly.

An earlier verification attempt mixed operating-system mouse injection with queued control actions; its trace showed the correct Source-divider hover position but `primary_down = false`, so it could not verify a drag. The final driver consistently uses the app's supported control input queue and verifies actual pane movement in the presented Windows application.

## Remaining scope

Stages 4-5 have not been executed. Inspector/linked properties and the wider cross-panel design pass remain separate tasks. R and its packages are separately installed as before. A release/portable ZIP is not rebuilt by this UI verification task. R-specific presentation changes do not reassess Illustrator parity scores.

At the smallest 640-point whole-window headless size, the existing Inspector's narrow column truncates some property labels/buttons. Its contents were not redesigned in Stage 3; Inspector readability remains Stage 4 scope. The main toolbar, Source/utility tabs, dividers, Tools and canvas remain present in the tested layout.
