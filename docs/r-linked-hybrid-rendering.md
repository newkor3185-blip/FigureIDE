# R-linked plot hybrid display

Status: implemented and measured on Windows, 2026-11-02. Revision: initial implementation.

Large SVG plots (at least 1,000,000 source bytes) now enter a linked panel as one `PlacedDocument` child with the original SVG embedded in the document. The existing panel ID, R variable metadata, physical size, label and GraphStyle remain on the parent. The on-screen renderer parses and rasterizes that SVG on a background worker, then shares power-of-two zoom bitmaps under its existing 512 MiB LRU budget. A changed SVG has a different content-hash key, so Reflow, Graph settings and Update invalidate the old preview automatically. Only one render job per R source runs at a time. Smaller plots retain native vector children and their supported direct-edit preservation.

SVG/PDF output expands the embedded SVG through VectorCraft's SVG importer; the display bitmap is not exported. `.fide` saves the original SVG bytes. Reopening does not require R to display the plot: the preview is regenerated from those bytes. Confirmed Unlink expands the source to editable native artwork within the existing unlink undo group. Cancel changes nothing.

## Measurements

The 10k, 50k and 100k svglite fixtures were generated earlier from synthetic points on this Windows machine. Before figures are the preceding native-object audit; after figures use the same profile test and fixtures. Peak memory is sampled process working set, so fast insert-only runs can be underestimated. The former and current zoom measurements use different harnesses; the warm exact control below uses the same process and view as the cached measurement.

| Points | Native insert | Hybrid insert | Native peak insert | Hybrid peak insert | Hybrid peak with preview | Cached 1440 x 900 zoom |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 10k | 267 ms | 8–9 ms | 66 MB | 17 MB | 46 MB | 1.9 ms |
| 50k | 1,337 ms | 36–41 ms | 235 MB | 58 MB | 121 MB | 1.7–2.2 ms |
| 100k | 2,691 ms | 70–72 ms | 433 MB | 101 MB | 217 MB | 1.8–3.0 ms |

The 100k same-process warm exact vector frame took 109.9 ms; its cached progressive frame took 3.0 ms. The first exact frame, including deferred SVG import, took 1,616.7 ms. The hybrid 100k Reflow test took 66.6 ms, preserved panel identity, passed Undo/Redo, and saved a 14.7 MB `.fide` with the original SVG. The corresponding old native-art project was 105.5 MB. These figures are focused samples, not an FPS guarantee.

Explicit Unlink of the 100k panel took 1,628 ms, produced more than 10,000 native editable document nodes, and passed Undo/Redo. Peak working set for that process was 363 MB.

## Verification and limits

Focused tests cover embedded source save/reopen, native vector expansion, alpha-transparent bitmap display, unlink Cancel/Confirm with Undo/Redo, reflow, invalid refresh preservation, and the pre-existing editable text/gradient path for small plots. A Windows desktop screenshot of the reopened 100k `.fide` showed the coloured points, axes, labels and legend. The first smoke test exposed a false missing-file dialog for the embedded source; after excluding this source from external-link checks, a second screenshot showed the plot without the dialog. The screenshots and detailed benchmark logs are ignored under `target/umap_perf/`.

An actual 15k-point ggplot2 object in the persistent R session passed insertion, Graph title styling, Reflow to 100 x 80 mm, source-byte replacement, and last-valid-art preservation after removing its R variable. The test verified `serialize(plot_B, NULL)` was unchanged by styling and Reflow.

The reopened 100k file exported SVG (32.2 MB) and PDF (10.5 MB) with no warnings or missing links. The SVG contained 100,004 translucent point paths, no image elements, and the same 100,004 point-colour sequence as the source circles. The PDF contained no image XObjects in this fixture. The new persistent R session correctly reported the plot variable as missing for Update; viewing the saved art did not require it.

The preview itself is an in-memory cache and is regenerated after reopening; only the authoritative SVG is persisted. At zoom levels needing more than 4096 pixels on one side, the interactive bitmap is capped and may appear softer. Explicit Unlink converts large SVGs synchronously through the existing engine command, so a 100k conversion can briefly pause the UI. External SVG features unsupported by VectorCraft's importer retain the same export/unlink limitations as native SVG import. R must be running with the source variable restored for a later Update, though the saved artwork is viewable without R.
