#!/usr/bin/env bash
# Build Mailbox for macOS (native arm64 and/or x86_64 + optional .app/.dmg).
#
# Usage:
#   ./bin/build-macos.sh
#   ./bin/build-macos.sh --package    # also cargo-packager (app, dmg)
#   ./bin/build-macos.sh --universal  # arm64 + x86_64 lipo (on Apple Silicon)
set -euo pipefail
# shellcheck source=./_common.sh
source "$(cd "$(dirname "$0")" && pwd)/_common.sh"

PACKAGE=0
UNIVERSAL=0
for arg in "$@"; do
  case "$arg" in
    --package|-p) PACKAGE=1 ;;
    --universal|-u) UNIVERSAL=1 ;;
    -h|--help)
      echo "Usage: $0 [--package] [--universal]"
      exit 0
      ;;
  esac
done

ensure_rust

if [[ "$(host_os)" != "macos" ]]; then
  echo "error: macOS builds must run on a Mac (host=$(host_os))." >&2
  echo "       Cross-compiling egui/winit for macOS from Linux is not practical." >&2
  echo "       Push a v* tag to build via GitHub Actions, or run this on macOS." >&2
  exit 1
fi

ARCH="$(uname -m)"
if [[ "$ARCH" == "arm64" ]]; then
  NATIVE_TARGET="aarch64-apple-darwin"
else
  NATIVE_TARGET="x86_64-apple-darwin"
fi

build_one() {
  local target="$1"
  ensure_target "$target"
  echo "→ cargo build -p mailbox-gui --release --target $target"
  cargo build -p mailbox-gui --release --target "$target"
}

if [[ "$UNIVERSAL" -eq 1 ]]; then
  build_one aarch64-apple-darwin
  build_one x86_64-apple-darwin
  need_cmd lipo
  OUT="$DIST/mailbox-macos-universal"
  lipo -create \
    -output "$OUT" \
    "target/aarch64-apple-darwin/release/mailbox" \
    "target/x86_64-apple-darwin/release/mailbox"
  chmod 755 "$OUT"
  echo "✓ $OUT"
else
  build_one "$NATIVE_TARGET"
  copy_bin "target/${NATIVE_TARGET}/release/mailbox" "mailbox-macos-${NATIVE_TARGET}"
fi

if [[ "$PACKAGE" -eq 1 ]]; then
  if ! command -v cargo-packager >/dev/null 2>&1; then
    echo "→ installing cargo-packager"
    cargo install cargo-packager --locked
  fi
  echo "→ cargo packager --release --formats app,dmg"
  cargo packager --release --formats app,dmg
  echo "✓ packages in dist/"
fi

echo
echo "Done."
