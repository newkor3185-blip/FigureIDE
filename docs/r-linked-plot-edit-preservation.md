# Linked R plot edit preservation: first slice

Status: first slice complete in `feature/r-linked-plot-user-edits`, 2026-10-29.

## Design

The generated SVG group and the user-owned panel label remain separate children of the linked panel. For new panels, `object.markRGenerated` saves a versioned baseline fingerprint and a semantic text key on each generated node using the document model's existing SVG `data-*` attributes. The group also records its descendant count and bounds. These attributes survive native project save/open without adding a document format field or changing the R object.

The semantic key is a deterministic fingerprint of the original text content. It is accepted only when unique in both the previous and new render. It does not depend on shape order or temporary document node IDs. A user may change the label's text, size, fill, or position; the saved original key still identifies it on the next refresh. The replacement command copies that one edited native text node into the new vector group after fitting the new SVG to the panel. The outer panel and its label keep their existing IDs, position, dimensions, and grouping.

Before replacing artwork, the engine checks the group count, bounds, each group's saved child-ID structure, and every node's baseline. The child-ID structure detects manual regrouping or stacking changes; it is not used to match objects across renders. If a shape or group changed, a label key is missing or duplicated, the group geometry changed, or a pre-existing panel has no baseline, protected refresh reports an error and retains the prior artwork. This first slice deliberately does not infer correspondence between changed data marks. SVG gradients and unedited shapes are imported normally.

`object.replaceGroupContents` and `object.resizeRPanel` accept an optional `preserveRUserEdits` flag. Their existing behavior is unchanged when that flag is absent. Protected replacement and the retained text happen in the command's existing single undoable document edit. Marking a newly created plot occurs within the existing insertion undo group.

## Limits and follow-up

- Text with the same original content in multiple places is ambiguous if one instance was edited; refresh keeps the old art.
- Text moved beyond the generated group's original bounds changes its layout footprint; refresh keeps the old art to avoid resizing the panel.
- A reflow that would leave a retained text edit outside the new plot bounds keeps the old art instead of changing the requested panel dimensions.
- Object-level color or path edits are detected but cannot yet be safely reapplied without a stable SVG element identity; refresh keeps the old art.
- Older linked panels without stored baselines still open. Their protected refresh is refused rather than discarding possible edits; re-adding the plot opts into tracking.
- If an R plot changes or removes the original text key while the user has edited that label, refresh keeps the old art.
- Existing Scale operations are not yet tracked as semantic edits. A later protected refresh may refuse if scaling changed the saved generated-group baseline; it retains the scaled artwork.

The next slice can use explicit SVG element IDs where available to carry selected path style overrides while retaining new geometry. It should remain limited to elements with unique IDs and tested gradient semantics.

## Smart Resize regression correction (2026-10-29)

Canvas resizing changes the generated group's bounds before `PanelAction::Reflow` reaches `object.resizeRPanel`. The first version of `reconcile` compared those bounds directly with the insertion baseline, rejected the intentional resize, and left the stretched old SVG in place. Reflow now normalizes that canvas transform only while checking prior edits. It uses geometric bounds for the scale ratio so stroke widths do not distort the comparison; older tracked panels without the new geometry marker use their saved visual size and current stroke reach. The replacement still comes from R at the requested mm device size. After reinstating an edited text node, nested group child-ID markers are refreshed so a later Update can succeed. Refresh without a resize remains strict, and unsupported shape edits still keep the old artwork.

The focused regression covers a 70 × 50 mm R plot, canvas resize and reflow at 100 × 80 mm, edited text followed by a second reflow at 120 × 90 mm, Update at that size, undo/redo, and render failure recovery. It checks the raw R SVG device dimensions and native vector layout as well as metadata. The Scale path still calls `object.resizeRPanel` without a source SVG from `scientific/inspector.rs`; it does not call `render_plot`.

Verification of this correction: `cargo fmt --all`, `cargo check --offline -p vectorcraft`, six focused engine tests, two linked-plot SVG/R tests, the existing ggplot and pheatmap integration tests, and the existing vector Scale undo test passed. `cargo xtask ci` passed fmt and workspace Clippy, then stopped at the same four previously observed UI tests (`r_session` ×3 and `shortcuts` ×1); 2313 UI library tests passed. No GUI screenshot was taken because the native R renderer, imported SVG geometry, and document commands were exercised headlessly.

## Verification

- `cargo fmt --all`: passed.
- `cargo check --offline -p vectorcraft`: passed on Windows.
- Four focused engine tests: passed, including native save/open, edit preservation with undo/redo, and safe fallback.
- Synthetic SVG import/refresh test: passed; editable text, panel bounds and label, mm metadata, and native gradient artwork remained present.
- Existing real ggplot registry/import test and pheatmap link/refresh/reflow test: passed with local Rscript, ggplot2, svglite, and pheatmap.
- Native GUI screenshots were not captured; this is a document/refresh behavior change covered by headless command and SVG import tests.
- The repository CI gate exposed two pre-existing FigureIDE rebrand issues: a needless `return` in Windows icon loading and a desktop `--help` test expecting the old product name. The icon expression and test expectation were corrected without changing app behavior; all seven desktop argument tests then passed.
- `cargo xtask ci`: `fmt` and workspace Clippy passed. The full test stage stopped on four existing UI tests outside the changed modules: three `r_session` SVG assertion failures and one `shortcuts` Cmd+Enter expectation. The same four failures were observed on the prior baseline; no R-session or shortcut behavior was changed here. Later CI stages were therefore skipped by the gate.
