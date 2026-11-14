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
VERSION="$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p && $1=="version"{gsub(/[" ]/,"",$3);print $3;exit}' "$ROOT/Cargo.toml")"
BUILD_SHA="${VECTORCRAFT_BUILD_SHA:-$(git -C "$ROOT" rev-parse HEAD)}"
BUILD_DATE="${VECTORCRAFT_BUILD_DATE:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"
if [[ -n "${FIGUREIDE_RELEASE_VERSION:-}" ]]; then
  if [[ "$FIGUREIDE_RELEASE_VERSION" != "$VERSION" ]]; then
    echo "release version $FIGUREIDE_RELEASE_VERSION does not match Cargo version $VERSION" >&2
    exit 1
  fi
  ARCHIVE_NAME="FigureIDE-v$VERSION-Linux-x86_64.tar.gz"
else
  ARCHIVE_NAME="FigureIDE-Linux-x86_64.tar.gz"
fi
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
cat >"$PACKAGE/BUILD_INFO.txt" <<EOF
version=$VERSION
commit=$BUILD_SHA
build_date=$BUILD_DATE
target=x86_64-unknown-linux-gnu
EOF

while IFS= read -r -d '' license; do
  relative="${license#"$ROOT/assets/"}"
  install -D -m 644 "$license" "$PACKAGE/asset-licenses/$relative"
done < <(find "$ROOT/assets" -type f \( -name 'LICENSE*' -o -name 'OFL*' \) -print0)

ARCHIVE="$DIST/$ARCHIVE_NAME"
tar -C "$STAGING" -czf "$ARCHIVE" FigureIDE-Linux-x86_64
if [[ -n "${GITHUB_ENV:-}" ]]; then
  printf 'FIGUREIDE_ARCHIVE=%s\n' "$ARCHIVE" >>"$GITHUB_ENV"
fi
echo "wrote $ARCHIVE"
