# R execution engine verification

> **Status:** Verified · **Last updated:** 2026-10-28 · **Change:** unified R evaluation across buffered and streamed requests; verified script decoding, R execution and Plot Registry updates on Windows.

## Scope

This review checks R script decoding and execution from disk through the persistent R process, plus consistency between buffered and streamed execution. It does not change the VectorCraft graphics engine or replace the persistent `Rscript` child process.

## Execution architecture

`RSession` starts one persistent `Rscript` child and serializes requests through its session mutex. Rust hex-encodes UTF-8 request payloads; the embedded bridge decodes them and evaluates user code in the shared `.GlobalEnv`.

Both `EXEC` and `EXEC_STREAM` now use the bridge's `evaluate_source()` function. It parses the complete source before evaluating any expression, then evaluates expressions in order with `withVisible()`. Visible results are printed once; invisible assignments and `invisible()` results are not printed. A parse error evaluates nothing. A runtime error stops later expressions while leaving earlier side effects in `.GlobalEnv`. Both modes write output, messages, and warnings to the same temporary UTF-8 output file. Rust either returns that output after completion or drains it incrementally to the Console. The UI action still controls command echo separately.

## Script import path

The file pathway is `open_script_path()` → strict byte decoding → normalized editor buffer → `source_active_script()` or `run_current_expression()` → `run_r()` → `RSession::execute_streaming()` → the shared R evaluator. UTF-8, UTF-8 BOM, UTF-16 LE/BE, and line ending metadata are retained for saving. A non-UTF-8 byte stream that is valid as CP949/EUC-KR now asks the user to select the encoding before loading; invalid data is never silently replaced.

`decode_hex()` marks bytes recovered from IPC as UTF-8 before R parses or evaluates them. A bridge-level regression request echoes decoded payload bytes as hex so tests can compare them byte-for-byte without printing user source.

Set `VECTORCRAFT_R_SOURCE_DIAGNOSTICS=1` to log file-import and IPC-send byte lengths, FNV-1a 64-bit hashes, selected encoding and the first differing byte offset during round-trip checks. Source contents are not logged. Diagnostics are disabled by default.

## Verification results

### Historical import reproduction

The workspace's untracked `test.R` was left unchanged. Its bytes are 17 bytes: ASCII `library(Seurat)` followed by CRLF, with no BOM. `Rscript --vanilla test.R` exited successfully and loaded Seurat. The app test opened that exact path through `open_script_path()` backed by the real filesystem, verified the editor contains `library(Seurat)\n`, sourced it through the persistent app session, and verified `loadedNamespaces()` contains `Seurat`. The test passed.

On the installed R runtime, `parse(text = "library(Seurat)\r\n")` returns `unexpected invalid token`; the import normalizer converts CRLF to LF before the editor buffer is executed. This confirms why passing raw Windows line endings to the text parser is unsafe. The editor buffer and execution source are verified in the import test. Separately, the bridge now tags hex-decoded payloads as UTF-8 before returning them to the R parser. A bridge round-trip test compared the recovered bytes for Korean comments and strings, escapes, and an R raw string literal against the original UTF-8 source; they matched exactly.

### Execution consistency and session recovery

`buffered_and_streaming_execution_share_visibility_output_and_error_semantics` runs the same source through both request modes and asserts identical output. It covers multiple visible expressions, invisible results, messages, warnings, `cat()`, `print()`, evaluation ordering, partial state after a later runtime error, no evaluation after a parse error, and successful execution after errors. `streaming_keeps_partial_utf8_sequences_until_the_next_file_read` verifies that a multibyte character split across filesystem reads is emitted intact.

`imported_r_scripts_decode_execute_and_round_trip_supported_encodings` writes actual `.R` fixtures to a temporary directory, opens them through the app's `open_script_path()` and filesystem reader, checks the editor buffer, executes them, verifies the `.GlobalEnv` and Plot Registry, saves, and reopens them. Coverage includes UTF-8, UTF-8 BOM, UTF-16 LE/BE, explicitly selected CP949/EUC-KR, CRLF/LF/CR, Korean comments and strings, a backtick Unicode identifier, Windows paths, an R raw string, escaped quotes and backslashes, and a multiline ggplot. Separate tests cover empty files, missing final newlines and refusal to silently replace invalid bytes.

`file_import_manual_source_and_native_rscript_produce_the_same_result` compares a real temporary `.R` file run by installed `Rscript --vanilla`, the same file loaded and sourced by the app, and identical source typed in a new editor tab. All produced `[1] 16`; the app retained the expected shared-environment object. Existing real ggplot tests verify Plot Registry discovery, SVG rendering and linked-panel behavior.

Commands run on Windows with installed R 4.6.0:

- `Rscript --vanilla test.R` — exit code 0; Seurat loaded.
- `cargo test --offline -p vectorcraft-ui-egui --lib` — **2,275 passed, 0 failed**; includes real R integration tests, the exact `test.R` import regression and the differential execution tests.
- `cargo check --offline -p vectorcraft` — passed.
- `cargo clippy --offline -p vectorcraft-ui-egui --all-targets -- -D warnings` — passed.
- `cargo fmt --all` and `git diff --check` — passed.

The native window was launched with the control channel and visually inspected from a captured 3840 × 2064 screenshot. The R Vector Studio workspace rendered and reported `Rscript (R) version 4.6.0`. The app was then closed through its control channel. The file-import and execution interactions themselves were validated by app integration tests, not by driving the OS file-picker dialog.

The installed R runtime reports that its `C.UTF-8` locale categories cannot be set. Plain non-ASCII identifiers therefore fail in this environment even in native `Rscript`; the regression uses a valid backtick Unicode identifier, which native R and the app both accept. UTF-8 comments and string literals are exercised. A different R installation with a UTF-8 locale may accept unquoted Unicode identifiers.

## Console limits

The backend is a non-interactive `Rscript` process, not a native terminal. `readline()` and interactive package prompts are not supported. Safe interruption of a running R expression is not currently implemented; stopping or restarting the session is an explicit user action and discards that session's environment. GUI visual verification is reported separately from backend tests.
