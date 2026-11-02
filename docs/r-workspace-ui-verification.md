# R Workspace UI verification

Last verified: 2026-10-28. This workflow distinguishes native Windows frame capture from headless egui layout checks.

Stage 1's subsequent panel refinement added selected-entry and expanded-Relink scenarios (44 captures). Its evidence is recorded in [R UI refinement - Stage 1](r-ui-refinement-stage1.md).

Stage 2 adds Source tabs/gutters, narrowing an active tab, document scroll/keyboard checks, and continuous Console scrollback/multiline-input checks. The driver additionally checks a second native script, dirty state, focus/history, long real R output and multiline execution. Its latest results and before/after images are recorded in [R UI refinement - Stage 2](r-ui-refinement-stage2.md). The original verification results below remain the baseline record.

## Repeating the verification

Stage 3 extends this driver with native divider drags/reset/keyboard controls, whole-window sizing, and existing Tools column/float/dock checks. Its scoped implementation and latest evidence are in [R UI refinement - Stage 3](r-ui-refinement-stage3.md).

Stage 4 verifies the selected R-linked plot in the native Inspector, its millimeter dimensions and Reflow/Scale controls, and a real `Update from R` action. The scoped implementation and screenshot review are in [R UI refinement - Stage 4](r-ui-refinement-stage4.md).

Stage 5 checks the completed workspace across panels and corrects the narrow scientific Inspector and Contextual Task Bar. Its native/headless screenshots, test results and limits are in [R UI refinement - Stage 5](r-ui-refinement-stage5.md).

From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/verify-r-workspace-ui.ps1 -Mode Auto
```

`-Mode Headless` runs without a desktop. It first checks geometry without creating a GPU device, then runs a separate optional offscreen PNG pass. `-Mode Desktop` builds and exercises the native application only. `-SkipBuild` uses an executable already built from the current sources. The default artifact directory is `target/r-workspace-ui/`, already ignored by Git; `-Artifacts PATH` selects another directory.

The desktop mode requires Windows UI Automation, an accessible input desktop, installed R, and ggplot2. It uses the existing `vectorcraft` Cargo package and binary, the existing loopback control channel, and the actual source editor, Console, Plot Registry and vector document commands. There is no alternate R evaluator.

## Detected environment

- The agent runs directly on Windows kernel build 10.0.26200, user session 12, with `UserInteractive=true`, window station `WinSta0`, and input desktop `Default`.
- An isolated application instance created a real HWND in that session. Windows UI Automation exposed egui controls and their bounds. `ui.screenshot` returned presented native frames, which were opened as images for review.
- The native renderer reported NVIDIA GeForce RTX 5060 Ti, DX12. The desktop scale was 200%: a 1600 x 1000 logical client produced 3200 x 2000 PNGs.
- An empty computer-use app inventory did not prove desktop access was unavailable. Direct Win32 probing, launching the app and capturing a presented frame established availability.

The agent environment supplies `LC_ALL`, `LC_CTYPE` and `LANG` as `C.UTF-8`. Windows R rejected those Unix locale overrides and initially displayed escaped `<U+...>` metadata. The verification launcher removes only unsupported `C.UTF-8` overrides in its isolated child process, preserving the caller's environment and other locale choices. R then reported `Korean_Korea.utf8`, and native Korean variable/value display was verified. The original inherited-locale evidence remains under `target/r-workspace-ui/inherited-agent-locale/`. No R session or bridge code was changed.

## Native desktop mode

The driver starts `target/debug/vectorcraft.exe` with a dynamically selected control port, `CREATE_NO_WINDOW` for the debug console subsystem, and a separate no-preferences profile. Its native GUI still opens. It does not read or overwrite the user's scripts or normal application profile.

The committed fixture `scripts/fixtures/r-workspace-ui.R` creates a data frame, numeric and character values, a Korean variable, long names/classes, twenty additional variables and two named ggplots. The driver:

1. Measures the source editor's current clipped interaction rectangle, focuses it, types the fixture and waits for the reported buffer length before using Source.
2. Executes `numeric_value + 1` in the same Console and checks `[1] 43`.
3. Captures Environment, Plot Registry and Console at each workspace width; focuses the Environment filter through UI Automation and captures the Korean variable separately.
4. Adds both real ggplots through the actual Add to Artboard buttons, captures linked panels, and uses Update to refresh SVG artwork.
5. Creates and moves a rectangle with the existing vector commands, records the document, and closes through the normal quit/discard flow.

UI Automation supplies button labels and physical bounds. The driver checks the HWND client origin, current pixels per point and viewport before converting bounds to logical coordinates. It waits for input/state changes instead of assuming that an enqueued key or text event was already applied.

## Headless mode

`scientific_ui_tests.rs` uses the existing egui_kittest infrastructure and the production panel functions, interface fonts, MediumDark theme and CJK font fallback. Its synthetic object metadata does not create another R session. It builds actual vector groups for linked-panel command tests.

| Coverage | Logical surface / scale |
|---|---|
| Environment, Plot Registry, Linked Plot Panel, Console | 320, 420, 560, 800 x 1000; 100% and 150% |
| Empty Environment and 25 objects | All four widths; 100% |
| Tab switching, Console focus/history/draft/clear | 320 px |
| Relink draft retention, Relink, Unlink keeping artwork, Delete Artwork | 320 px |
| Existing rectangle creation/movement and UI control parameter validation | Headless command/input checks |

The matrix contains 40 capture scenarios. Accessibility response rectangles are checked for horizontal overflow and control overlap. Actual text shapes are checked against neighboring action controls using their clip rectangles. A long list is scrolled to its final object to verify reachability; Korean filtering and action activation use real egui events. Each scenario writes dimensions, scale, control rectangles, clip bounds, glyph availability and capture status to JSON.

Geometry runs independently of optional GPU screenshots. A capture initialization error is recorded without discarding geometry results. A separate capture process failure is recorded in `offscreen-capture-result.json`. Capture tests run with `--test-threads=1`: an earlier parallel offscreen run aborted with Windows `STATUS_ACCESS_VIOLATION`; serial captures completed. That observation does not establish that every Windows GPU driver is reliable.

## Focused layout fixes

- Environment search, object names, class/type metadata and wrapping action rows have their own space.
- Plot Registry names and metadata are separated from Select/Add actions.
- Linked panels separate title, metadata/status, actions and relinking. Relink choices persist across frames until confirmed; Unlink and Delete clear the associated draft.
- Long names wrap, shortened class summaries retain full tooltips, and narrow bottom tabs wrap instead of hiding the active title. The global R toolbar also wraps.

The theme, R backend, shared `.GlobalEnv`, IPC, evaluator, SVG importer and vector engine are unchanged.

## Artifacts and results

- `target/r-workspace-ui/environment.json`: desktop audit and inherited locale overrides.
- `target/r-workspace-ui/headless/`: 40 scenario JSON files and optional PNGs, named by tab, logical width, scale and scenario.
- `target/r-workspace-ui/desktop/`: native PNGs and matching UI state JSON, `document.json`, `final-state.json`, `control.jsonl`, launch-environment information, process logs and `result.json`.
- Automation records `visuallyInspected=false`; image review is recorded separately, so generating a PNG does not count as examining it.

Final commands and observed results:

| Command | Result |
|---|---|
| `cargo fmt --all` | Passed |
| `cargo check --offline -p vectorcraft` | Passed |
| `cargo test --offline -p vectorcraft-ui-egui --lib` | 2,288 passed, 0 failed |
| `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` | Passed |
| `git diff --check` | Passed |
| `scripts/verify-r-workspace-ui.ps1 -Mode Auto` | Both modes passed; geometry pass and serial PNG pass each ran 5 focused tests |

All 40 headless scenarios produced a JSON measurement and PNG. They reported zero horizontal control overflow and zero control overlap; text-versus-control checks passed. The 36 nonempty Unicode scenarios reported Korean glyph coverage; the four empty cases did not paint Korean text. The native run produced 17 scenario captures (16 width/tab combinations plus the Korean filter), executed both ggplots through SVG import, refreshed a linked plot and exercised rectangle movement. `desktop/result.json` records success and normal exit code 0. No verification instance was left running.

The following latest PNGs were actually opened and inspected, rather than merely generated:

- Native 200%: `environment-320px-scale200-unicode-filter.png`, `plots-320px-scale200-fixture.png`, `linked-320px-scale200-fixture.png`, `console-320px-scale200-fixture.png`, `environment-800px-scale200-fixture.png`, `linked-800px-scale200-fixture.png`.
- Headless: `plots-320px-scale100-normal-unicode-long-metadata.png`, `linked-320px-scale150-normal-unicode-long-metadata.png`.

The reviewed R panes had readable Korean text, wrapping long names/classes, visible action buttons and no observed overlapping controls. Native linked captures show actual scatter/bar vector artwork and selection handles. The installed ggplot2 package reports that it was built under R 4.6.1 while the runtime is R 4.6.0; its warning remains visible and the fixture completed. A separate `target/r-workspace-ui/image-review.json` records the reviewed file list. Other generated screenshots were not individually reviewed.

## Remaining limits

Headless geometry checks and offscreen PNGs do not prove native Windows rendering is perfect. Native verification uses the available monitor's 200% scale; 100%/150% are headless checks. CJK fallback comes from installed fonts when craft-fonts is absent, so captures are evidence rather than cross-machine pixel baselines. Windows R still depends on a valid externally supplied locale; the test launcher handles the agent's incompatible Unix override without changing the application backend.

## Files changed

`crates/ui-egui/src/scientific.rs`, `crates/ui-egui/src/control.rs`, `crates/ui-egui/src/scientific_ui_tests.rs`, `scripts/verify-r-workspace-ui.ps1`, `scripts/fixtures/r-workspace-ui.R`, this report and `docs/control-protocol.md`.
