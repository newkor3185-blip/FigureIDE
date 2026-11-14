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

if [[ -n "${FIGUREIDE_RELEASE_VERSION:-}" && "$FIGUREIDE_RELEASE_VERSION" != "$VERSION" ]]; then
  echo "release version $FIGUREIDE_RELEASE_VERSION does not match Cargo version $VERSION" >&2
  exit 1
fi
BUILD_DATE="${VECTORCRAFT_BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

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
if [[ -n "${FIGUREIDE_RELEASE_VERSION:-}" ]]; then
  ZIP="$DIST/FigureIDE-v$VERSION-macOS-$ARTIFACT_ARCH.zip"
else
  ZIP="$DIST/FigureIDE-macOS-$ARTIFACT_ARCH.zip"
fi
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
cat >"$RESOURCES/BUILD_INFO.txt" <<EOF
version=$VERSION
commit=$VECTORCRAFT_BUILD_SHA
build_date=$BUILD_DATE
target=$TARGET
EOF
while IFS= read -r -d '' license; do
  relative="${license#"$ROOT/assets/"}"
  destination="$RESOURCES/asset-licenses/$relative"
  mkdir -p "$(dirname "$destination")"
  install -m 644 "$license" "$destination"
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
[[ -s "$RESOURCES/LICENSE-MIT" && -s "$RESOURCES/LICENSE-APACHE" && -s "$RESOURCES/NOTICE" && -s "$RESOURCES/ASSETS.md" ]]
[[ -s "$RESOURCES/THIRD-PARTY-LICENSES.md" && -s "$RESOURCES/BUILD_INFO.txt" ]]
[[ -n "$(find "$RESOURCES/asset-licenses" -type f -print -quit)" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")" == "${VERSION%%-*}" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$APP/Contents/Info.plist")" == "${VERSION%%-*}" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :FigureIDEVersion' "$APP/Contents/Info.plist")" == "$VERSION" ]]
lipo "$APP/Contents/MacOS/FigureIDE" -verify_arch "$MACH_ARCH"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
REPORTED_VERSION="$("$APP/Contents/MacOS/FigureIDE" --version)"
[[ "$REPORTED_VERSION" == "FigureIDE $VERSION" ]]

rm -f "$ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/Info.plist'
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/MacOS/FigureIDE'
unzip -l "$ZIP" | grep -Fq 'FigureIDE.app/Contents/Resources/THIRD-PARTY-LICENSES.md'
echo "wrote $ZIP"
ls -lh "$ZIP"
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'FIGUREIDE_MACOS_ZIP=%s\n' "$ZIP" >>"$GITHUB_ENV"
fi
