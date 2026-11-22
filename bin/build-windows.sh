#!/usr/bin/env bash
# Build Mailbox for Windows.
#
# Preferred: run on Windows (Git Bash / MSYS2) or use GitHub Actions.
# Optional: cross-compile from Linux with mingw-w64 (experimental for egui).
#
# Usage:
#   ./bin/build-windows.sh
#   ./bin/build-windows.sh --package   # NSIS installer (Windows host + NSIS)
#   ./bin/build-windows.sh --cross     # try Linux → windows-gnu cross build
set -euo pipefail
# shellcheck source=./_common.sh
source "$(cd "$(dirname "$0")" && pwd)/_common.sh"

PACKAGE=0
CROSS=0
for arg in "$@"; do
  case "$arg" in
    --package|-p) PACKAGE=1 ;;
    --cross|-c) CROSS=1 ;;
    -h|--help)
      echo "Usage: $0 [--package] [--cross]"
      exit 0
      ;;
  esac
done

ensure_rust

TARGET="x86_64-pc-windows-msvc"
HOST="$(host_os)"

if [[ "$HOST" == "windows" ]]; then
  TARGET="x86_64-pc-windows-msvc"
  ensure_target "$TARGET"
  echo "→ cargo build -p mailbox-gui --release --target $TARGET"
  cargo build -p mailbox-gui --release --target "$TARGET"
  copy_bin "target/${TARGET}/release/ailiakmail" "ailiakmail-windows-x86_64"
elif [[ "$CROSS" -eq 1 ]]; then
  TARGET="x86_64-pc-windows-gnu"
  echo "→ experimental cross-compile to $TARGET"
  if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "error: install mingw-w64 first, e.g.:" >&2
    echo "  sudo apt install mingw-w64" >&2
    exit 1
  fi
  ensure_target "$TARGET"
  export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
  echo "→ cargo build -p mailbox-gui --release --target $TARGET"
  if ! cargo build -p mailbox-gui --release --target "$TARGET"; then
    echo >&2
    echo "Cross-build failed (common with egui/winit). Prefer:" >&2
    echo "  • GitHub Actions release workflow" >&2
    echo "  • Building on a Windows machine: ./bin/build-windows.sh" >&2
    exit 1
  fi
  copy_bin "target/${TARGET}/release/ailiakmail" "ailiakmail-windows-x86_64"
else
  echo "error: Windows build should run on Windows (host=$HOST)." >&2
  echo "       Options:" >&2
  echo "         ./bin/build-windows.sh --cross     # experimental mingw from Linux" >&2
  echo "         git tag v0.1.0 && git push --tags   # GitHub Actions" >&2
  exit 1
fi

if [[ "$PACKAGE" -eq 1 ]]; then
  if [[ "$HOST" != "windows" ]]; then
    echo "warning: --package (NSIS) requires a Windows host with makensis; skipping." >&2
  else
    if ! command -v cargo-packager >/dev/null 2>&1; then
      echo "→ installing cargo-packager"
      cargo install cargo-packager --locked
    fi
    echo "→ cargo packager --release --formats nsis"
    cargo packager --release --formats nsis
    echo "✓ installer in dist/"
  fi
fi

echo
echo "Done."
