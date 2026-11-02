# R Vector Studio / FigureIDE testing policy

This policy applies to all future R Vector Studio development in this repository, including work by Codex and Antigravity. It takes precedence over the general `cargo xtask ci` pre-commit gate in `AGENTS.md` for this scoped work. The default is **focused testing**. Never run the entire VectorCraft workspace suite merely because a feature, commit, merge, or PR is ready.

## Everyday verification

1. Inspect only files relevant to the change and use the existing focused tests first.
2. Run `cargo fmt --all --check`.
3. Run `cargo check --offline -p vectorcraft`.
4. Run exact test names or filters for the changed feature, for example `cargo test --offline -p vectorcraft-ui-egui --lib r_workspace_`. Add the smallest meaningful regression test if none exists.
5. Include adjacent tests only when the change affects shared behavior. For integrations between Codex and Antigravity work, test both modified components and their cross-module interaction.
6. Run one targeted native Windows GUI smoke test only for visible UI changes or interaction that headless tests cannot adequately verify. Do not run screenshot matrices or repeat an unchanged smoke test.

Examples: Source Editor changes use editor and navigation tests; Console changes use execution and shortcut tests; Graph Inspector changes use graph-style and linked-panel tests; Reflow changes use rendering, dimensions, linked-panel, and undo/redo tests; SVG changes use relevant import/export tests; persistence changes use save/load and compatibility tests.

For a documentation-only change, state which code checks were skipped and why. Do not claim the skipped checks passed.

## Full phase validation

Run a full applicable Rust and integration suite, R runtime tests, relevant SVG/PDF checks, native GUI end-to-end checks, and compatibility/persistence checks **only** when:

- the user explicitly requests all tests, full regression, or the entire suite;
- the user initiates completion of a development phase before advancing to the next phase; or
- the user explicitly authorizes an exception after its reason is explained.

Ordinary feature completion, commit creation, branch merging, and PR preparation are not phase-completion authorization. If a phase gate is still needed, report it as pending. Do not run a complete workspace suite on your own.

## Agent coordination and failures

- Each agent tests its assigned modules in its dedicated Git worktree. Do not launch native GUI automation concurrently with another agent.
- After merging agents' changes, run focused integration tests for the combined behavior. A merge alone does not trigger the full suite.
- Separate known baseline failures from failures relevant to the current change. Investigate relevant regressions immediately; report unrelated baseline failures without expanding the test scope automatically.
- Keep detailed logs on disk when useful. Report commands, counts, and relevant errors without pasting successful logs or repeatedly reading unchanged files.

## Completion report

Every R Vector Studio development report must include:

- **Implementation:** summary, modified files, commit hash (if committed).
- **Verification:** Cargo check PASS/FAIL/SKIPPED; focused tests PASS/FAIL with counts; GUI smoke PASS/FAIL/SKIPPED; full regression **NOT RUN (per testing policy)** unless explicitly authorized.
- **Outstanding issues:** relevant failures and risks, plus tests deferred to a phase-completion gate.

Never describe focused test success as full regression coverage. Never conceal skipped checks or unavailable dependencies.
