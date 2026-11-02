# R-linked UMAP performance audit

**Measured:** 2026-11-02 on Windows, AMD Ryzen 5 7500F (12 logical processors), 16 GB RAM, StarDesk virtual display. Results are single-run wall-clock samples on this machine, not throughput guarantees. Fixtures are deterministic synthetic four-cluster ggplots with 1,000, 10,000, 50,000 and 100,000 translucent points, rendered by R 4.6.0, ggplot2 and svglite. Large SVG and project files stay under ignored `target/umap_perf/`.

## Finding and change

The linked-plot creation path parsed every SVG twice: once to validate it and again through `clipboard.importSvg`. At 50,000 nodes, edit-baseline stamping rejected the plot even though ordinary SVG import succeeded. The render worker already keeps UI input responsive during pan and zoom; the first visible linked plot and the hard node limit were the effective bottlenecks for this task.

Creation now loads the SVG into the existing Clipboard import command **before** creating a document, then pastes that parsed vector art. The command can load a clipboard without an open document; invalid SVG still leaves the workspace without a new document. The bounded generated-node traversal limit is 200,000, with its depth limit unchanged. No raster replacement or renderer rewrite was introduced.

## Measurements

R SVG creation produced 0.15, 1.47, 7.34 and 14.68 MB files for the four sizes; device rendering took 590, 380, 780 and 830 ms respectively. The 1,000-point run includes first-use initialization.

| Points | Linked insertion before | Linked insertion after | Peak working set before / after |
| ---: | ---: | ---: | ---: |
| 1,000 | 77.5 ms | 46.9 ms | sampling too short to compare |
| 10,000 | 553.4 ms | 298.2 ms | 63.6 / 62.2 MB |
| 50,000 | failed at 2.94 s, 50k-node limit | 1.58 s, succeeded | 213.7 / 213.5 MB |
| 100,000 | failed at 5.54 s, 50k-node limit | 3.05 s, succeeded | 379.6 / 395.9 MB |

The before and after insertion timings use the same SVGs and ignored Rust insertion benchmark in separate processes. The 50k and 100k before entries are **failure times**, so they are not comparable completion speedups. Memory is sampled process working set, not a precise allocation profile.

The existing CLI benchmark was extended to expose open, first render, repeated render, pan, zoom, hit-test, save and reopen timings. On the 100k SVG, final measured open was 1.42 s, first 1440×900 render 278 ms, repeated render 146 ms/frame with 8 threads, pan rerender 145 ms, zoom rerender 182 ms, hit-test 31.9 ms/click, save 207 ms and reopen 558 ms. Before the change, the same fixture measured open 2.04 s, repeated render 236 ms/frame, hit-test 52.7 ms/click, save 312 ms and reopen 854 ms. **The renderer and hit-test code did not change**, so these observed differences are run-to-run variation and are not attributed to this patch.

After insertion, a 50k linked panel reflowed in 1.83–1.90 s; undo and redo took 5.6 and 7.3 ms. A 100k linked panel reflowed in 4.04–4.71 s; undo and redo took 10.9 and 17.6 ms. Saving and reopening retained the linked panel metadata and native nodes. The 100k insert-plus-reflow test peaked near **855 MB working set** because the old and replacement art coexist during reconciliation.

## Fidelity and interaction checks

- Existing real ggplot, pheatmap, and original airway PCA/MA tests passed. A two-panel synthetic UMAP test preserved each panel's independent identity and dimensions after reflow.
- The 50k linked project exported to SVG with 50,026 paths, 50,004 opacity attributes and no `<image>` elements. PDF export produced a 5.29 MB file without image XObjects. Artwork remains editable vector content.
- Invalid SVG before insertion created no document. A failed refresh left the document revision unchanged. Large reflow, undo/redo, save/reopen and association checks passed.
- A native Windows GUI smoke test sourced a 50k-point ggplot in the persistent R session, inserted it in 2.58 s, reflowed width and height in about 2.7 s each, and updated it from R in 2.42 s. Right-click selected the parent linked panel and offered Unlink. The visible plot retained translucent point colours. The UI reported about 60 FPS at fit view and 33 FPS at a zoomed view on the virtual display; background render times were about 135 and 78 ms. These are two snapshots, not a sustained FPS study.

## Remaining limits

- Full-view 100k-point rerenders still take roughly 0.15–0.28 s, and click hit-testing takes tens of milliseconds. The UI's existing frame cache and worker keep navigation responsive while the new frame is built, but the final sharp frame is delayed.
- Large reflow has high transient memory use. The 200,000-node tracking cap remains explicit; larger plots report an error rather than silently losing edit baselines.
- No permanent raster preview, batching of translucent points, or new spatial index was added. Those require a separate measured renderer task with overlap-opacity fidelity tests.
- The native smoke used a synthetic UMAP-like ggplot; no real single-cell dataset was available. GUI screenshots and detailed logs are local in `target/umap_perf/`.

**Verification:** `cargo fmt --all --check`, `cargo check --offline -p vectorcraft`, focused engine and UI/R tests, 50k/100k ignored integration benchmarks, CLI SVG/PDF export, and one Windows GUI smoke test. The full Rust suite was not run under the focused R Vector Studio testing policy.
