# FigureIDE Linux validation

Status: experimental Linux x86_64 build and core R/figure workflow validated on Ubuntu 26.04 LTS, 2026-11-12. This is not a stable release certification.

## Tested host

- Ubuntu 26.04 LTS, x86_64; 12 logical CPUs, 13 GiB memory, 59 GiB free disk at audit time.
- XFCE desktop over XRDP, X11 display `:11.0`.
- The SSH shell did not inherit display variables; the active XFCE session was reached through its X11 socket and user authorization file.
- Mesa selected llvmpipe through Vulkan (`Cpu` adapter). XRDP reported no DRI3 device, so hardware acceleration was not exercised.
- Rust 1.99.1 was installed in the user account. No system packages were installed on the host.
- The host lacked `libxkbcommon-x11-0` and `libxcb-xkb1`. For the launch check, their Ubuntu packages were unpacked under `/tmp` and supplied with `LD_LIBRARY_PATH`.

## Results

- `cargo build --locked --release -p vectorcraft`: passed in a Debian 13 container with the Linux build headers. The resulting x86_64 GNU executable also launched on the Ubuntu host.
- `cargo fmt --all --check`: passed.
- `cargo check --locked --offline -p vectorcraft`: passed.
- `cargo clippy --locked -p vectorcraft --all-targets -- -D warnings`: passed.
- `cargo test --locked -p vectorcraft`: passed, 164 unit tests and 7 integration tests.
- The real XFCE/XRDP window launched and rendered at 1440×900. Menus, tool rail, canvas, R workspace and FigureIDE welcome view were visible. Window resizing to 1200×800 was verified through X11.
- R 4.5.0 and ggplot2 were tested in a disposable Debian container attached to the same X11 session. FigureIDE detected `Rscript`; source code ran against the built-in `iris` data; the ggplot appeared in Plot Registry; Add to Artboard created a linked vector panel; Graph Inspector showed the selected graph and linked settings; updating the R plot and refreshing the linked panel updated the artwork.
- A linked `.fide` was saved and reopened. The reopened document contained the updated title and axis labels and retained its vector artwork. R workspace save produced an `.RData` file. SVG and PDF exports were created successfully.
- The PDF exporter reported that DejaVu Sans Regular was substituted with the closest installed style, DejaVu Sans Book.
- The XDG portal was unavailable in the XFCE image. FigureIDE used its `zenity` fallback, and the file picker window opened when `zenity` was supplied locally. Choosing a file through that fallback was not verified.

The test-generated `.fide`, SVG, PDF, R workspace and screenshots were kept under `/tmp/figureide-linux-smoke`; they are local validation artifacts and are not part of the repository.

## Not yet verified

- GPU acceleration and performance on a physical GPU.
- Wayland sessions and other Linux distributions.
- Selecting and opening a file through a desktop file picker on this XFCE image.
- Clipboard exchange with another desktop application.
- Linux package installation through a system package manager, desktop integration, and upgrades.
- Every Windows path after the Linux-only icon selection change. The Windows code path and packaging workflow were not modified.

The manual workflow at [`.github/workflows/figureide-linux-release.yml`](../.github/workflows/figureide-linux-release.yml) uploads an archive as a GitHub Actions artifact only. No public release is created.
