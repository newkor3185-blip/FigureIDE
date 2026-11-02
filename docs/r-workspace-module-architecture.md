# R Workspace module architecture

Status: Phase 1 architecture refactor, 2026-10-29. Work branch: `refactor/r-workspace-ui-modules` (based on `feature/r-execution-engine`). No commit, push, or merge was made.

## Dependency map and module tree

Before this refactor, `scientific.rs` contained the workspace layout, all lower panes, source navigation, console presentation, Inspector controls, and plot insertion alongside shared state and R dispatch. `r_session.rs` already owned the persistent R process and file IPC; `r_editor.rs` already owned highlighting and text helpers. The canvas and Properties panel entered through the public `scientific` interface.

```text
crates/ui-egui/src/
  scientific.rs                 shared state, UI command bridge, script I/O, R message dispatch
  scientific/
    workspace.rs               split layout, pane sizing, lower-tab routing
    source_editor.rs           script tabs, text editor, source reveal, expression navigation
    console.rs                 transcript and interactive Console presentation
    environment.rs             Environment, History, Help, Packages presentation
    plot_registry.rs           named-plot list and insertion controls
    linked_plots.rs            linked-plot list, SVG import, A4 placement, refresh/Reflow insertion
    inspector.rs               selected R plot properties and resize controls
    status.rs                  R status labels and colors
  r_editor.rs                  R text-processing helpers (unchanged)
  r_session.rs                 persistent R process and IPC (unchanged)
  scientific_ui_tests.rs       shared UI harness and regression tests (unchanged)
```

`scientific.rs` owns `Workspace`, `ScriptDocument`, `RLinkedPanel`, `RMessage`, and the application command/async dispatch path. Child modules import only the shared types and functions they use. The parent exposes the existing `show`, Inspector and app-bar hooks to the rest of the UI. `chrome.rs`, `lib.rs`, and `panels/properties.rs` continue to use those hooks without signature changes.

The UI dependencies flow from `workspace.rs` to the pane modules and from `linked_plots.rs` to its plot properties in `inspector.rs`. Source navigation stays in `source_editor.rs`; linked-plot controls request it through the shared parent API. Plot rendering still calls the existing `r_session.rs` service, and SVG import still calls the existing document commands. No new R process, parser, format, or state representation was introduced.

## File ownership for concurrent work

| Owner | Primary files | Scope |
| --- | --- | --- |
| Codex | `plot_registry.rs`, `linked_plots.rs` | Plot discovery UI, linked panels, SVG insertion, physical-mm geometry, refresh and Reflow UI. |
| Antigravity | `workspace.rs`, `source_editor.rs`, `console.rs`, `environment.rs`, `status.rs` | Layout, source navigation, Console, Environment and status presentation. |
| Coordinate first | `scientific.rs`, `inspector.rs`, `scientific_ui_tests.rs`, `state.rs`, `r_session.rs`, `r_editor.rs`, `menus.rs`, Cargo manifests | Shared state and APIs, Inspector coupling, test harness, execution semantics and command registration. |

Add a new presentation function to the file that owns its pane. Keep module-local helpers private. Use `pub(super)` only when the parent or a sibling needs a function; retain the established crate-facing hooks through `scientific.rs`. Discuss changes to shared types, command contracts, or R message variants before editing the coordinating files.

## Verification and follow-up

Baseline on the integration branch: `cargo check --offline -p vectorcraft` passed and 20 `r_workspace_` UI tests passed. After extraction: `cargo fmt --all`, `cargo check --offline -p vectorcraft`, and the same 20 focused UI tests passed. The full `vectorcraft-ui-egui --lib` suite ran 2,315 tests: 2,311 passed and four failed (`r_session` ggplot gradient, ggplot dimensions, pheatmap paths; `shortcuts` Cmd+Enter). Each failed alone too. A detached worktree at the unmodified integration commit `8f56b025` produced the **same 2,311 pass / 4 fail result with the identical failing test names**. The four failures predate this refactor; they were not fixed in this architecture-only phase. Native GUI screenshots were skipped because no layout or behavior was intentionally changed and the existing layout tests cover the relocated panes.

Recommended next step: keep `scientific.rs` focused on shared state, script I/O and asynchronous orchestration. If future changes make either area a frequent source of conflicts, extract it as a separate coordinated follow-up; that is outside this behavior-preserving Phase 1.
