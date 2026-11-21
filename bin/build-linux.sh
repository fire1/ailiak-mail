#!/usr/bin/env bash
# Build Mailbox for Linux (native release + optional .deb / AppImage).
#
# Usage:
#   ./bin/build-linux.sh
#   ./bin/build-linux.sh --package    # also run cargo-packager (deb, appimage)
set -euo pipefail
# shellcheck source=./_common.sh
source "$(cd "$(dirname "$0")" && pwd)/_common.sh"

PACKAGE=0
for arg in "$@"; do
  case "$arg" in
    --package|-p) PACKAGE=1 ;;
    -h|--help)
      echo "Usage: $0 [--package]"
      exit 0
      ;;
  esac
done

ensure_rust

if [[ "$(host_os)" != "linux" ]]; then
  echo "warning: this script is meant to run on Linux (host=$(host_os))." >&2
  echo "          Cross-building egui for Linux from elsewhere is not supported here." >&2
  echo "          Use GitHub Actions (.github/workflows/release.yml) instead." >&2
  exit 1
fi

TARGET="${CARGO_BUILD_TARGET:-x86_64-unknown-linux-gnu}"
ensure_target "$TARGET"

echo "→ cargo build -p mailbox-gui --release --target $TARGET"
cargo build -p mailbox-gui --release --target "$TARGET"

BIN="target/${TARGET}/release/mailbox"
# Fallback if cargo put it in target/release (no explicit target dir layout quirk)
[[ -f "$BIN" ]] || BIN="target/release/mailbox"
copy_bin "$BIN" "mailbox-linux-x86_64"

if [[ "$PACKAGE" -eq 1 ]]; then
  if ! command -v cargo-packager >/dev/null 2>&1; then
    echo "→ installing cargo-packager"
    cargo install cargo-packager --locked
  fi
  echo "→ cargo packager --release --formats deb,appimage"
  cargo packager --release --formats deb,appimage
  echo "✓ packages in dist/ (and packager out-dir)"
fi

echo
echo "Done. Run:  $DIST/mailbox-linux-x86_64"
echo "Dock icon:  ./scripts/install-linux-icon.sh"
