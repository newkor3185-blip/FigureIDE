# R UI refinement - Stage 2

Status: completed and verified (2026-10-28).

This task covers Source Editor and Console presentation only. Existing Stage 1 working-tree changes are retained. Stages 3-5 are deferred; no commits or pushes are made.

## Baseline and changes

The baseline is the completed Stage 1 working tree. Existing Windows and headless Source/Console screenshots were preserved in `target/r-ui-refinement-stage2/before/` before editing.

- Source tabs previously used independent selected labels and close buttons, with unrestricted filename width. Tabs now group the filename and close action, bound and truncate long names, retain complete paths in tooltips, distinguish active/inactive text and surfaces, and underline the active document with the existing accent token. A leading `*` keeps the dirty indicator visible when a long name is truncated. Tab overflow scrolls horizontally; switching documents reveals the selected tab and focuses its editor. The existing unsaved-close confirmation remains.
  The complete active tab, including its close action, is also revealed when the viewport narrows. Ordinary manual tab-strip browsing does not snap back.
- The duplicate active-filename heading becomes a compact Source caption and existing maximize/restore action. Source and Console share local 13-point monospace typography and 2-point vertical spacing. The global theme, main toolbar and splitters are unchanged.
- Source has an 8-point inset and a separate line-number gutter. Gutter width follows the number of source lines, and its numbers use the actual text layout positions, including blank lines and Korean fallback fonts. Code scrolls vertically and horizontally without inserting soft line breaks into the document. Horizontal code movement leaves the gutter fixed. Cursor movement is forwarded from the inner horizontal scroller to the vertical parent so Ctrl+Home/End and execution-driven cursor changes can remain visible.
- Console remains a **single continuous scrolling transcript**, with the editable `>` / `+` line directly after output. It has no detached input pane or divider. Only output rows are virtualized; the actual input widget is always created, even when clipped during scrollback. This retains its identity, draft and keyboard focus. A large multiline paste gets a bounded inner scroll area instead of expanding the whole workspace.
- Transcript rows use fixed spacing and horizontal scrolling for long lines; soft wrapping no longer invalidates virtual-row height calculations. Browsing old output does not force a jump to new output. Editing the live input or explicitly selecting Console reveals it; running Source does not request Console focus. Prompt and status colors use existing theme tokens; errors and warnings remain visible.
  The live editor's width is captured before long output expands the scrolling content; reveal targets the prompt's left edge. This prevents alignment to a long output line's right-hand end and keeps the first transcript column visible. History navigation and Escape also reveal the live prompt.
- Console captures Escape through the existing egui event-filter API, allowing the existing input-cancel action to execute instead of losing focus first. Enter, parser continuation, multiline evaluation, history/draft restoration, Ctrl+L and copy/paste continue through the existing execution/input paths.

No R bridge, persistent session, `.GlobalEnv`, file decoding/saving, IPC, evaluator, SVG importer, linking model or VectorCraft engine code was modified. Autocomplete was not reintroduced. User R files were not edited.

## Files and functions

- `crates/ui-egui/src/scientific.rs`: extracted `show_source_editor` from `show`; added local `code_style` and shared code metrics; refined `show_script_tabs`, `show_bottom_tabs`, `show_console` / `show_console_contents`, and `show_console_status`. `Workspace.pending_console_focus` is presentation state. `inspect_workspace` additionally reports script identities/dirty flags and the Console follow-output flag for read-only verification.
- `crates/ui-egui/src/scientific_ui_tests.rs`: production Source/Console harness; width/scale matrix, selected tab appearance, scrollable tab reachability, focus and keyboard switching, 150-line navigation, document scroll retention, dirty-close protection, existing two-space outdent, 500-line Console scrollback, bounded multiline input, Escape and draft/history regressions. Measurements explicitly clip intentional Source tab overflow and scrolled editor content to their viewports.
- `scripts/verify-r-workspace-ui.ps1`: extends the existing real Windows driver with a second script, Korean draft, dirty-state verification, keyboard switching, Source width captures, Console focus/history/draft cancellation, real long output, scrollback and multiline R execution. It retains real ggplot SVG import/refresh and vector editing checks.
- This report and the verification/control documentation.

## Verification

| Command | Observed result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib --quiet` | 2,292 passed; 0 failed; 0 ignored |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `git diff --check` | Passed |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-r-workspace-ui.ps1 -Mode Auto -Artifacts target/r-ui-refinement-stage2/after` | Passed; both geometry and serial PNG modes ran 9 focused tests; native Windows verification passed |

The final headless run generated 64 JSON measurements and 64 PNGs, with zero reported horizontal control overflow or control overlap in the measured viewports. The matrix covers 320/420/560/800 logical widths and 100%/150% scales. Intentionally scrollable Source tab/content bounds are measured after clipping to their viewports; interaction tests separately verify active-tab/close reachability and document navigation.

The native driver generated 27 scenario captures at 200% scale, using a real presented application window and the production debug executable. Two Source documents retained separate buffers and dirty state; keyboard switching and Korean input worked. The shared Console evaluated `numeric_value + 1` as 43, retained history and its draft, cancelled input, emitted 160 real R output lines, retained focus during scrollback, then evaluated a multiline function and returned 7. The expected function appeared in the shared environment. Both ggplots were discovered and imported as SVG, linked Update completed, and an ordinary vector rectangle was created and moved. The application exited normally with code 0; `desktop/result.json` reports success and no test-owned instance remains running.

Actual image review included the final native first/second Source tabs and linked artwork at 320 px, representative Source/gutter/scrollback/multiline headless images, and the baseline screenshots. The reviewed native Source tabs now retain their active close action at narrow widths; Console shows its first transcript column and live prompt following output. The linked screenshot displays actual scatter/bar vector artwork and selection handles. Not every generated PNG was individually opened. Review details are recorded separately in `target/r-ui-refinement-stage2/image-review.json`; screenshot generation alone is not counted as visual review.

Cargo continues to print the existing unused-egui-patch warnings. The installed ggplot2 package's R-version warning remains visible; the real R fixture completed successfully.

## Before/after visual evidence

The reviewed baseline Source has wrapping code, no gutter and separate filename/close controls. The updated Source shows numbered, unwrapped source lines, grouped tabs, a visible dirty marker and a clear active underline. The second script's Korean comment is readable. At 320 logical pixels the active tab and its close action remain reachable after narrowing from 800 pixels.

The Console remains continuous, with the live prompt following output. Long transcript lines now scroll horizontally instead of breaking virtual row positions. A 500-line headless transcript retains the live input identity and focus while browsing the beginning; new output does not force it to the end. Pasting 40 input lines then reveals a bounded, scrollable editor, with the original transcript still visible above it.

Local image links (ignored generated artifacts):

- Source/Console baseline: [native before](../target/r-ui-refinement-stage2/before/desktop/console-320px-scale200-fixture.png).
- Source tabs and gutter: [native first script](../target/r-ui-refinement-stage2/after/desktop/console-320px-scale200-source-first-tab.png), [native Korean second script](../target/r-ui-refinement-stage2/after/desktop/console-320px-scale200-source-second-tab.png).
- [Narrowed active tab and close action](../target/r-ui-refinement-stage2/after/headless/console-320px-scale100-source-narrowed-active-tab.png).
- [150-line document scrolled to its end](../target/r-ui-refinement-stage2/after/headless/console-320px-scale100-source-150-lines-scrolled.png).
- [Bounded multiline input](../target/r-ui-refinement-stage2/after/headless/console-320px-scale100-multiline-input-scroll.png).
- Real R output: [native last lines and prompt](../target/r-ui-refinement-stage2/after/desktop/console-320px-scale200-long-output.png), [native scrollback](../target/r-ui-refinement-stage2/after/desktop/console-320px-scale200-scrollback.png).

Artifacts are local and ignored by Git under `target/r-ui-refinement-stage2/after/`. Native Windows scale is the available monitor's 200%; headless checks cover 100% and 150%, at workspace widths 320, 420, 560 and 800 logical pixels. Captures are reviewed as images rather than treated as proof merely because PNG generation succeeds.

## Remaining scope

Stages 3-5 have not been executed. Main toolbar/workspace proportions and splitters, Inspector/linked property redesign, and a broader cross-panel design pass remain separate tasks. Existing externally installed R/packages and system CJK fallback font requirements remain unchanged.

The native verifier builds the debug `vectorcraft` binary. A release/portable ZIP was not rebuilt as part of this UI task. No Illustrator parity scores were reassessed for the R-specific presentation changes.
