# R plot placement and Source navigation

Status: implemented and verified on Windows, 2026-10-28. Branch: `feature/r-execution-engine`.

## Confirmed causes and changes

| Area | Cause | Correction |
| --- | --- | --- |
| Go to Source | The caret was written to `TextEdit` state, but scrolling requested inside the nested horizontal `ScrollArea`. The outer vertical editor viewport did not receive the target. | Keep a pending reveal after changing the caret. Measure the target with the laid-out `TextEdit` galley, reveal its line start horizontally, and request the outer vertical scroll after the inner area closes. Clear the request only after the target becomes visible. The target is placed near the upper third when the document has enough content below it. |
| Initial plot position | The previous two-column placement and imported SVG offsets did not align the visible group with the artboard's left boundary. | Measure the imported panel's document bounds and move by `artboard.x0 - panel_bounds.x0` for the first column. Later plots fill rows, then adjacent 70 mm columns. Refresh and Reflow do not apply this initial placement. |
| Default plot size | New plots used a 177.8 × 127 mm rendering device. | New plots render at 70 × 50 mm. This keeps the previous 7:5 device aspect ratio at one third of A4 width. Existing linked panels retain their stored dimensions; old v2 metadata is restored without the new typography policy. |
| Empty canvas | Insertion assumed an active document and usable artboard. | Before import, validate the SVG. If no document exists, create a native 210 × 297 mm document with a white A4 artboard. If the active document has no artboard, create and select one with the native artboard command. The subsequent native SVG import, group creation, naming, move, and selection happen in one insertion path. |

New ggplots use a temporary rendering copy with an 8 pt base text size when the original ggplot has the default 11 pt base or no explicit base text size. The named object in `.GlobalEnv` is unchanged. Explicit nondefault base sizes are retained. This preserves ggplot's relative axis/title hierarchy, but automatic styling does not guarantee that every long label or legend fits a 70 mm figure. Pheatmap and other non-ggplot objects keep their own typography. The existing `svglite` and VectorCraft SVG import path remains in use, so supported plot text and shapes remain vector objects.

Native `edit.undoGroup` makes each insertion one undo step. Undo removes the inserted plot; redo restores it. When insertion creates a brand-new document, `file.new` establishes its first A4 artboard as the document's base state, and the engine leaves that empty artboard after undo. Undoing document creation itself is not supported by the current document model. Invalid SVG is rejected before creating that document or artboard.

## Files changed

- `crates/ui-egui/src/scientific.rs`: pending viewport reveal, 70 × 50 mm defaults, A4 creation, actual-bounds placement, grouped insertion, persistent v3 link metadata and regression tests.
- `crates/ui-egui/src/r_session.rs`: small-figure ggplot rendering flag and real R test for smaller SVG text without `.GlobalEnv` mutation.
- `crates/ui-egui/src/scientific_ui_tests.rs`: headless visible-viewport regression across 600 lines, both navigation directions and a tab switch.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo check --offline -p vectorcraft`: passed.
- Targeted Rust tests: 600-line visible Source navigation, new plot bounds and reuse, no-document A4 insertion, invalid SVG, temporary ggplot typography, real ggplot SVG insertion, and pheatmap linked refresh/Reflow passed during development. The no-document undo test initially exposed that the engine preserves the new document's base artboard; its assertion now reflects that model and verifies plot undo/redo.
- Native Windows GUI: the debug `vectorcraft` binary launched on the interactive desktop. A 555-line R script created `plot_A`; Plot Registry insertion produced one selected vector panel on a white A4 artboard. `document.inspect` reported the artboard at x=0 with width 595.27559 pt (210 mm), and the selected panel bounds at x=0 with width 198.42999 pt (70 mm). The Inspector showed 70 × 50 mm. After scrolling the editor to the top and clicking **Go to Source**, the screenshot showed the `plot_A` expression at line 552. The application stderr contained only the control-server listening message.
- Representative screenshots: `target/r-plot-placement-focused/desktop/plot-on-a4.png` and `target/r-plot-placement-focused/desktop/source-line-551.png`. The focused GUI test used one 1600 × 1000 window size. The full desktop regression matrix was intentionally skipped.

## Remaining limits

- The 7:5 default is the prior device aspect ratio. Plot-specific `theme(aspect.ratio=...)` and pheatmap layouts are not inferred for initial height.
- The new document's base A4 artboard remains after undoing its first plot insertion, as described above.
- Long text and dense legends may still need manual Inspector resizing or a user-authored ggplot theme.
