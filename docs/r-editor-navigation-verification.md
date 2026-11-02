# R Source Editor navigation verification

Status: verified on Windows (2026-10-28)

## Root causes and changes

| Issue | Cause | Correction |
| --- | --- | --- |
| Column 1 caret | The code viewport began directly beside the fixed gutter, and the horizontal scroll target did not explicitly reveal the caret. | `show_source_editor` reserves a fixed 6 pt gap outside the horizontal scroller and asks both nested scroll areas to reveal the caret with horizontal breathing room. The gutter remains fixed. |
| Last-line Ctrl+Enter | The prior completion handler moved to the byte after the executed expression but did not create an empty line. | The completion result carries a source revision and EOF flag. After successful submission, it appends one normalized `\n` if needed, records the edit in egui's undoer, and navigates to the new empty line. Encoding and line-ending metadata stay unchanged for save. |
| Skipping comments | The prior cursor destination was the next physical line, even if it contained only comments or whitespace. | The persistent R parser's `EXPR` response now includes the next top-level expression's source line. This uses R's own parsing of strings, raw strings, comments and multiline expressions. Selection execution leaves the selection intact. |
| Linked plot source | Panel linkage stored the R variable but no executed source identity; string search would confuse comments and references with definitions. | Source and current-expression execution now record verified script, top-level assignment line and source revision. `Go to Source` uses this when current; otherwise an asynchronous `DEFS` R parser request searches open scripts for top-level `<-`, `=`, `<<-`, `->` and `->>` assignments. A unique candidate opens with an explicit unknown-origin status. Ambiguous, closed and edited sources report status instead of guessing. |

`navigate_source` shares UTF-8-safe character indexing, tab activation, deferred egui cursor placement, focus and scroll reveal. Canvas selection and R panel IDs are not changed by navigation. `selected_plot` syncs when the selected linked panel changes.

## Files changed

- `crates/ui-egui/src/r_session.rs`: R `EXPR` next-expression line and `DEFS` top-level assignment lookup.
- `crates/ui-egui/src/scientific.rs`: cursor advancement, EOF undoable edit, source provenance/navigation, linked-panel action and control inspection field.
- `crates/ui-egui/src/scientific_ui_tests.rs`: scale, focus, tab and horizontal recovery UI test.
- `scripts/verify-r-workspace-ui.ps1`: native Source jump and Ctrl+Enter assertions/screenshots.
- This report.

## Cursor rules

- No selected range: Ctrl+Enter runs the R expression at or after the caret, exactly once. On success, the caret moves to the first column of the next parsed top-level expression.
- At the final expression: an existing trailing newline is reused; otherwise one newline is inserted. Running again from the resulting blank EOF line is a no-op.
- A selected range is run once and remains selected. No automatic source edit occurs.
- A parse or submission failure leaves the source untouched. If the buffer changes while a background execution runs, its stale cursor/edit result is not applied.

## Source provenance limits

Origins are retained for verified top-level assignments from an executed open script or current expression. Arbitrary Console execution, session restart and commands with possible side effects clear recorded origins. Definitions created dynamically through `assign()`, nested functions or external sourcing have no definitive script origin. The open-script candidate lookup is syntax-aware but is labeled as a candidate; multiple matches do not trigger a jump. Provenance is in memory for this session and does not alter saved artwork.

## Verification

- `cargo fmt --all`: passed.
- `cargo check --offline -p vectorcraft`: passed.
- `cargo test --offline -p vectorcraft-ui-egui --lib -- --test-threads=1 --quiet`: 2,305 passed. The first parallel run had one Windows R IPC temporary-file access-denied failure in the existing encoding test; that test passed alone and the entire suite passed serially.
- `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings`: passed after addressing type and style findings.
- `scripts/verify-r-workspace-ui.ps1 -Mode Auto -Artifacts target/r-editor-navigation-final`: headless geometry and offscreen capture passed (19 tests), followed by native Windows desktop launch and R/SVG/vector workflow verification. The native extension clicked `Go to Source`, confirmed tab/focus/plot linkage, executed two expressions with Ctrl+Enter, checked the skipped-line and EOF cursor positions and byte lengths through `ui.inspect`, and saved native screenshots.
- Inspected [100%](../target/r-editor-navigation-final/headless/console-560px-scale100-source-column-one-navigation.png), [150%](../target/r-editor-navigation-final/headless/console-560px-scale150-source-column-one-navigation.png) and [200% Source screenshots](../target/r-editor-navigation-final/headless/console-560px-scale200-source-column-one-navigation.png), plus [native linked source jump](../target/r-editor-navigation-final/desktop/linked-source-jump-native.png), [native next-expression caret](../target/r-editor-navigation-final/desktop/source-next-expression-native.png), and [native EOF caret](../target/r-editor-navigation-final/desktop/source-eof-caret-native.png). The first-column caret is visibly separated from the gutter at all three scales; the last native screenshot shows the caret at column 1 of line 5. The native linked-source screenshot shows editable SVG plot artwork and the selected linked panel still present.

Tests cover parser expression boundaries, comment/string disambiguation, assignment discovery including reverse assignment, EOF edit/undo/dirty/format, repeated Ctrl+Enter on blank EOF, real R execution and character-safe tab navigation at 100%, 150% and 200% scale.
