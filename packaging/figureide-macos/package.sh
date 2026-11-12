#!/usr/bin/env bash
# Package a native architecture-specific FigureIDE.app and ZIP without Developer ID signing.
set -euo pipefail

# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

case "${1:-}" in
  apple-silicon)
    TARGET="aarch64-apple-darwin"
    MACH_ARCH="arm64"
    ARTIFACT_ARCH="AppleSilicon"
    ;;
  intel)
    TARGET="x86_64-apple-darwin"
    MACH_ARCH="x86_64"
    ARTIFACT_ARCH="Intel"
    ;;
  *)
    echo "usage: $0 apple-silicon|intel" >&2
    exit 2
    ;;
esac

BIN="$CARGO_TARGET_DIR/$TARGET/release/vectorcraft"
LICENSE_REPORT="$CARGO_TARGET_DIR/THIRD-PARTY-LICENSES.md"
if [[ ! -x "$BIN" ]]; then
  echo "missing executable: $BIN (build the vectorcraft release target first)" >&2
  exit 1
fi
if [[ ! -s "$LICENSE_REPORT" ]]; then
  echo "missing third-party license report: $LICENSE_REPORT" >&2
  exit 1
fi

WORK="$CARGO_TARGET_DIR/figureide-macos-package/$ARTIFACT_ARCH"
APP="$DIST/FigureIDE.app"
ZIP="$DIST/FigureIDE-macOS-$ARTIFACT_ARCH.zip"
RESOURCES="$APP/Contents/Resources"
ICONSET="$WORK/FigureIDE.iconset"
ICON_SOURCE="$ROOT/assets/app-icon/figureide-app-icon.png"
rm -rf "$WORK" "$APP"
mkdir -p "$WORK" "$ICONSET" "$APP/Contents/MacOS" "$RESOURCES"
install -m 755 "$BIN" "$APP/Contents/MacOS/FigureIDE"

# Generate the required .icns from the original FigureIDE artwork supplied with the repository.
icon_image() {
  local name="$1" size="$2"
  sips -s format png -z "$size" "$size" "$ICON_SOURCE" --out "$ICONSET/$name" >/dev/null
}
icon_image icon_16x16.png 16
icon_image icon_16x16@2x.png 32
icon_image icon_32x32.png 32
icon_image icon_32x32@2x.png 64
icon_image icon_128x128.png 128
icon_image icon_128x128@2x.png 256
icon_image icon_256x256.png 256
icon_image icon_256x256@2x.png 512
icon_image icon_512x512.png 512
icon_image icon_512x512@2x.png 1024
iconutil -c icns "$ICONSET" -o "$RESOURCES/FigureIDE.icns"

python3 "$ROOT/packaging/figureide-macos/make_info.py" "$ROOT/packaging/macos/Info.plist.in" \
  "$APP/Contents/Info.plist" "$VERSION" "$VECTORCRAFT_BUILD_SHA"

install -m 644 "$LICENSE_REPORT" "$RESOURCES/THIRD-PARTY-LICENSES.md"
copy_docs "$RESOURCES"
while IFS= read -r -d '' license; do
  relative="${license#"$ROOT/assets/"}"
  install -D -m 644 "$license" "$RESOURCES/asset-licenses/$relative"
done < <(find "$ROOT/assets" -type f \( -name 'LICENSE*' -o -name 'OFL*' \) -print0)
printf 'APPL????' >"$APP/Contents/PkgInfo"

cat >"$RESOURCES/README-macOS.txt" <<'EOF'
FigureIDE for macOS

Move FigureIDE.app to Applications and open it. This app is ad-hoc signed only; it has no
Developer ID signature and has not been notarized. macOS Gatekeeper may warn about an app
downloaded from the internet.

R is an optional external dependency. Install R from https://cran.r-project.org/bin/macosx/ or
through Homebrew (`brew install r`). Make sure Rscript is available on PATH, then install the
plotting packages from R:

  install.packages(c("ggplot2", "svglite", "systemfonts"), repos = "https://cloud.r-project.org")

Vector editing, .fide files and ordinary SVG/PDF export work without R. R scripts and linked plot
refresh require R and the relevant R packages. See THIRD-PARTY-LICENSES.md and the adjacent license
files for dependency, project and asset terms.
EOF

plutil -lint "$APP/Contents/Info.plist"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")" == "FigureIDE" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$APP/Contents/Info.plist")" == "ai.storyteller.figureide" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$APP/Contents/Info.plist")" == "APPL" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIconFile' "$APP/Contents/Info.plist")" == "FigureIDE" ]]
[[ -x "$APP/Contents/MacOS/FigureIDE" ]]
[[ -s "$RESOURCES/FigureIDE.icns" ]]
[[ -s "$RESOURCES/LICENSE-MIT" && -s "$RESOURCES/LICENSE-APACHE" && -s "$RESOURCES/NOTICE" ]]
lipo -verify_arch "$MACH_ARCH" "$APP/Contents/MacOS/FigureIDE"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
"$APP/Contents/MacOS/FigureIDE" --version

rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/Info.plist'
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/MacOS/FigureIDE'
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/Resources/THIRD-PARTY-LICENSES.md'
echo "wrote $ZIP"
ls -lh "$ZIP"
