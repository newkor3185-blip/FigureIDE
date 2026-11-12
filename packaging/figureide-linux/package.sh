#!/usr/bin/env bash
# Package the validated Linux x86_64 executable as a portable archive.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARCH="$(uname -m)"
if [[ "$ARCH" != "x86_64" ]]; then
  echo "FigureIDE Linux packaging currently supports x86_64; found $ARCH" >&2
  exit 2
fi

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
BIN="$TARGET_DIR/release/vectorcraft"
LICENSE_REPORT="$TARGET_DIR/THIRD-PARTY-LICENSES.md"
DIST="${DIST:-$ROOT/dist}"
if [[ ! -x "$BIN" ]]; then
  echo "missing executable: $BIN (build with cargo build --locked --release -p vectorcraft)" >&2
  exit 1
fi
if [[ ! -s "$LICENSE_REPORT" ]]; then
  echo "missing dependency license report: $LICENSE_REPORT (generate it with cargo about)" >&2
  exit 1
fi

STAGING="$(mktemp -d)"
PACKAGE="$STAGING/FigureIDE-Linux-x86_64"
mkdir -p "$PACKAGE/asset-licenses" "$DIST"
install -m 755 "$BIN" "$PACKAGE/FigureIDE"
install -m 644 "$ROOT/LICENSE-MIT" "$PACKAGE/LICENSE-MIT"
install -m 644 "$ROOT/LICENSE-APACHE" "$PACKAGE/LICENSE-APACHE"
install -m 644 "$ROOT/NOTICE" "$PACKAGE/NOTICE"
install -m 644 "$ROOT/ASSETS.md" "$PACKAGE/ASSETS.md"
install -m 644 "$LICENSE_REPORT" "$PACKAGE/THIRD-PARTY-LICENSES.md"
install -m 644 "$ROOT/docs/figureide-linux.md" "$PACKAGE/README-Linux.md"
cp -R "$ROOT/licenses" "$PACKAGE/licenses"

while IFS= read -r -d '' license; do
  relative="${license#"$ROOT/assets/"}"
  install -D -m 644 "$license" "$PACKAGE/asset-licenses/$relative"
done < <(find "$ROOT/assets" -type f \( -name 'LICENSE*' -o -name 'OFL*' \) -print0)

ARCHIVE="$DIST/FigureIDE-Linux-x86_64.tar.gz"
tar -C "$STAGING" -czf "$ARCHIVE" FigureIDE-Linux-x86_64
echo "wrote $ARCHIVE"
