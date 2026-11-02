# R Workspace text verification

## Findings

The display corruption was in `crates/ui-egui/src/scientific.rs`, not in the R object bridge. Strict UTF-8 inspection found eight U+CA0C characters used as separators in Environment, Plot Registry, linked-panel status/metadata, and R Plot Properties strings. Three U+D69E characters were used where dimensions should show an `x`. No U+FFFD replacement characters were present.

## Changes

- Replaced the corrupted separators with the ASCII `|` separator and the corrupted dimension marker with ASCII `x`.
- Added shared display helpers for object summaries, full class/type tooltips, linked-panel metadata, and linked-panel status text.
- Long class lists show at most two classes in the row, with the omitted count; hovering shows the complete class list and R type. Metadata rows in Environment, Plot Registry, and Linked Plot Panels wrap so controls have room at narrower widths.
- Formatting is display-only. R variable names, classes, types, dimensions, lengths, previews, ggplot detection, and session behavior remain unchanged.
- Added tests for the ASCII metadata format, full tooltips, Korean variable/value preservation, linked-panel status strings, and absence of the known corrupted codepoints.

## Verification

- `cargo fmt --all`
- `cargo check --offline -p vectorcraft`
- `cargo test --offline -p vectorcraft-ui-egui --lib`
- Strict UTF-8 decoding of `scientific.rs`, `r_session.rs`, and `r_editor.rs` succeeded; none contains U+FFFD.
- The known corrupted codepoints were not found in those R workspace source files after the fix.

The desktop inventory exposed no running app or browser surface, so a Windows screenshot and narrow-width visual review were unavailable. R bridge, IPC, evaluator, and persistent session code were not changed.
