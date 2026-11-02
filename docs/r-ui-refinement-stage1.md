# R UI refinement - Stage 1

Completed 2026-10-28. Only Environment, Plot Registry and Linked Plot Panel presentation was refined. Stages 2-5 of the roadmap were not executed. The existing verification workflow was run as required for Stage 1.

## Problems found in the baseline

- Environment entries put names, class/type strings, dimensions, previews and actions on many equally weighted lines. Long technical classes consumed space needed for browsing objects.
- Plot Registry selection highlighted only the name. Metadata and the selected-item footer repeated information, while actions had the same visual weight as destructive operations elsewhere.
- Linked panels always displayed a variable selector and disabled Relink confirmation. These controls made normal entries tall, while dimensions and resize mode were absent from the entry.
- Headings, name weights, metadata sizes, button heights and entry padding differed across the three panels.

Baseline source: `f217a195415f193bbfba09f77a75cca0da43aa4d`. Before editing, existing headless and Windows artifacts were preserved under `target/r-ui-refinement-stage1/before/`.

## Implemented changes

- Shared panel spacing, entry padding, 13-point name hierarchy, smaller secondary metadata and compact action buttons use the existing VectorCraft fonts and color tokens. Their scopes end at the panel; the global theme is unchanged.
- Environment shows a count and compact Refresh/Clear All controls above filtering. Each entry prioritizes its full wrapping name, category, primary class, and dimensions/length. Class vectors and complete preview metadata remain available in tooltips. Scalar previews use a single truncated display line with the complete value in a tooltip; data-frame previews are available in the metadata tooltip and View Data.
- Plot Registry uses the same entry styling, a short ggplot classification, linked count, and stable Select/Add actions. The complete selected entry receives the native selection fill and accent outline. The create-another-instance policy is on the Add tooltip.
- Linked entries combine label and connection state, show the full R variable and read-only `width x height mm | Reflow/Scale`, and group normal actions separately from Unlink/Delete. Relink opens its existing selector on demand, retains its draft, and closes after confirmation. Panel IDs remain in the variable tooltip.
- Remove, Clear All, Delete Artwork and destructive confirmations use the theme's error text color. Existing confirmations and commands are preserved.

No changes were made to R evaluation, IPC, the persistent session, SVG representation or linking identities. `show_r_plot_properties` remains outside the compact panel styling scope. No Stage 2 editor/Console work, Stage 3 toolbar/splitter work, or Stage 4 Inspector/property-control work was implemented.

## Files and functions changed

- [scientific.rs](../crates/ui-egui/src/scientific.rs): `show_environment` / `show_environment_entries`, `show_plot_registry` / `show_plot_registry_entries`, `show_linked_panels` / `show_linked_panel_entries`; shared `compact_panel_style`, `panel_caption`, `panel_action`, `panel_entry`, `panel_name`; `object_metadata_summary`, `environment_metadata_summary`, `environment_metadata_tooltip`. `Workspace.panel_relink_open` controls presentation only. `inspect_workspace` exposes selected plot/object state for verification.
- [scientific_ui_tests.rs](../crates/ui-egui/src/scientific_ui_tests.rs): the Relink interaction now opens the compact details first; expanded/selected captures and tests check full-row selection, destructive text colors, clipping and existing link commands.
- [verify-r-workspace-ui.ps1](../scripts/verify-r-workspace-ui.ps1): select a real registry plot through UI Automation and wait for that selection before width captures.
- This report; a follow-up link in [r-workspace-ui-verification.md](r-workspace-ui-verification.md).

## Before/after observations

The same 320-logical-pixel, 100% headless fixture produced these measurements from accessibility response rectangles:

| Measurement | Before | After |
|---|---:|---:|
| Environment: first object name to fifth object's final action | 568 | 379 |
| Plot Registry: first plot name to second plot's final action | 213 | 136 |
| Linked panels: distance between the first and second Select/Focus rows | 201 | 123 |

These are fixture layout heights in logical pixels, not performance benchmarks. Measurements for 420/560/800 px are in `target/r-ui-refinement-stage1/comparison.json`.

The inspected after-images show clearer names, shorter technical summaries, aligned small actions and visible selection backgrounds. Korean names/values remain readable. Long variable names wrap without displacing actions. Relink controls remain reachable when expanded. The native Linked Plot Panel now shows dimensions/mode while occupying less vertical space.

Local screenshot evidence (generated files are ignored by Git):

- Plot Registry: [before](../target/r-ui-refinement-stage1/before/desktop/plots-320px-scale200-fixture.png), [after](../target/r-ui-refinement-stage1/after/desktop/plots-320px-scale200-fixture.png).
- Linked panels: [before](../target/r-ui-refinement-stage1/before/desktop/linked-320px-scale200-fixture.png), [after](../target/r-ui-refinement-stage1/after/desktop/linked-320px-scale200-fixture.png).
- [Native Korean filter after](../target/r-ui-refinement-stage1/after/desktop/environment-320px-scale200-unicode-filter.png).
- [Expanded Relink after](../target/r-ui-refinement-stage1/after/headless/linked-320px-scale100-relink-expanded.png).

Actual image review included baseline headless Environment/Plot Registry/Linked panels and baseline native Plot Registry/Linked panels at 320 px; after-review included headless Environment, selected Plot Registry, normal/expanded Linked panels, and native Korean filtering, selected Plot Registry and Linked panels at 320 px. Other captures were measured automatically but not individually opened.

## Verification performed

| Command | Observed result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib` | 2,289 passed; 0 failed |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `git diff --check` | Passed |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-r-workspace-ui.ps1 -Mode Auto` | Passed; 6 focused tests in geometry mode and 6 in serial capture mode; native verification passed |

All 320, 420, 560 and 800 logical widths were checked. Headless 100%/150% verification generated 44 JSON/PNG scenarios, including new selected-entry and expanded-Relink cases, with zero reported horizontal control overflow or control overlap. Korean filtering, long names/classes, selection state, destructive styling, draft retention, Relink, Unlink keeping artwork and Delete Artwork passed.

The real Windows run used 200% scale and generated 17 scenario captures. Actual R source execution, shared Console execution, ggplot discovery, two SVG additions, linked Update, vector rectangle movement and clean application exit all passed. Existing real-R, import, canvas and editor/Console regressions are included in the full lib suite. No test-owned native instance was left running.

## Remaining work

Native visual checks cover the available monitor's 200% scale; 100%/150% are headless checks. System CJK fallback fonts remain part of the existing setup. No known overflow remains in the tested Stage 1 scenarios.

Stage 2 remains deferred: source tab styling and unsaved indicators, editor padding/line numbers, editor/Console typography, prompt/input focus and transcript/scroll refinements need their own task. Stages 3, 4 and 5 also remain deferred. No commits or pushes were made.
