# Large R-linked UMAP insertion and navigation

Measured on 2026-11-02 in the Codex Windows worktree, AMD Ryzen 5 7500F (12 logical processors), 16 GB RAM. The integration baseline `origin/feature/r-execution-engine` was an ancestor of this branch. Tests used the same local deterministic ggplot SVGs with 10,000, 50,000, and 100,000 translucent points before and after the changes. These are synthetic UMAP-like plots, not single-cell analysis data. Detailed logs and screenshots are ignored under `target/umap_perf/`.

## Bottlenecks and changes

The preceding optimization already removed a second SVG parse and raised the bounded generated-node tracking limit. This audit measured the remaining creation path. At 100k points, `clipboard.importSvg` (XML/usvg parse and native document construction) took 1.45 s, `object.markRGenerated` (per-node edit baselines) took 0.98 s, paste/group 0.16 s, and naming/positioning 0.20 s. The insertion path already uses one grouped undo operation, not one undo step per point.

Creation now keeps imported SVG coordinates in the clipboard and positions the panel once, rather than centering all imported nodes at the origin and then moving them. It also reuses the measured generated-group and label bounds when placing the panel. Generated-baseline stamping reuses its first visual-bounds result. These are local reductions to redundant work; the SVG importer and edit-baseline traversal remain the dominant costs.

The canvas already has a single full-frame texture, background render worker, and reprojection during view changes. For a linked plot whose last frame took over 8 ms, rapid native view changes now defer new exact worker renders until the view has been still for 200 ms. The last frame is reprojected during movement; the exact original vectors are then rendered at the settled zoom. Document edits still invalidate the frame immediately. This uses the existing bounded canvas texture, without a second raster panel cache or changes to vector export.

For ordinary Selection-tool clicks, an intact `RVS_PANEL` is selected as one group after normal visibility, stacking, and reach checks. The hit tester no longer descends through all generated point paths for that click. Direct Selection and Select Behind retain their original child-level paths, and an ordinary object stacked above the plot remains selectable first.

## Before and after

Single-process insertion samples from the stage-profile test (ms):

| Points | Before | After | After SVG import | After baseline marking |
| ---: | ---: | ---: | ---: | ---: |
| 10k | 272 | 267 | 135 | 103 |
| 50k | 1,412 | 1,337 | 679 | 494 |
| 100k | 2,798 | 2,691 | 1,351 | 963 |

Repeat insertions varied: the final independent run measured 299 / 1,391 / 2,735 ms. The small insertion improvement overlaps run-to-run variation; it is not a reliable speedup claim. The main measured benefit is interaction scheduling and linked-panel selection. Sampled peak process working set during insertion was 62.2 / 213.5 / 395.9 MB before (previous audit) and 66.4 / 234.5 / 433.2 MB after. Memory **did not improve** in these samples; allocator and test-build variation were not isolated. A 100k insert plus Reflow remains a high-memory operation.

The CLI's exact vector rerender times at 1440 × 900, 8 threads (ms):

| Points | Zoom before | Zoom after | Repeated frame before | Repeated frame after |
| ---: | ---: | ---: | ---: | ---: |
| 10k | 20.5 | 22.1 | 17.1 | 17.3 |
| 50k | 96.0 | 100.1 | 81.5 | 82.4 |
| 100k | 190.6 | 195.7 | 161.1 | 168.4 |

The renderer itself was not changed, and exact zoom-frame time did **not** improve. The change avoids requesting every intermediate exact frame during continuous linked-plot zoom. A focused five-step rapid-zoom test requested zero intermediate renders and one after settling, versus five immediate requests on the earlier native path. A 50k linked-panel hit at the tested point fell from 25 ms/click to below the 0.01 ms display resolution; hit time depends on the point and child stacking.

## Fidelity and regression evidence

- The native Windows app reopened a 100k `.fide` project. At 100% and settled 200% zoom, the plot showed individual coloured, translucent points, axes, labels, and legend. Selection highlighted one parent panel, and the right-click menu contained **Unlink from R**. Zoom command responses in the smoke test ranged from 4 to 16 ms after startup; an exact 200% background render took about 264 ms. These are short smoke observations, not a sustained FPS benchmark.
- The 100k linked project reflowed, undid, redid, and saved/reopened with its panel identity and R metadata. A deliberately invalid refresh left the last valid document revision intact. Two 10k panels remained independent. Existing ggplot, pheatmap, and original airway PCA/MA tests passed.
- The 100k `.fide` exported to a 35.8 MB SVG and 9.6 MB PDF without embedded image objects. All 100,004 source `<circle>` points corresponded to 100,004 exported paths with `fill-opacity="0.45"`; their fill-colour sequence matched the source order. This checks count, alpha, and colour order, though it is not a full geometric diff of every path.
- The direct-hit regression test checks the linked panel, child-level direct hit, and an ordinary vector object above the panel. The existing R-generated baseline tests cover project persistence, supported edits, and reflow integrity.

## Remaining limits

- The 1.35 s SVG import and 0.96 s baseline stamping dominate 100k insertion. They still run during UI application of the R result. The 200,000-node tracking cap remains explicit.
- Exact 100k vector renders still take roughly 0.17–0.27 s depending on view and machine load. During motion the previous frame is reprojected and may look soft until the 200 ms settle interval and exact render finish.
- The saved `.fide` restores vector artwork and link metadata, but the R variable must be recreated in a new persistent R session before Update or GraphStyle rendering can run. GraphStyle and Update were covered by focused existing R tests, not by this reopened-project GUI smoke.

Verification: `cargo fmt --all --check` and `cargo check --offline -p vectorcraft` passed; focused document, engine, UI/R, large-plot, export and native Windows GUI checks passed. Full regression was **NOT RUN (per the R Vector Studio testing policy)**.
