#!/usr/bin/env bash
# Shared helpers for bin/build-*.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST="${DIST:-$ROOT/dist}"
mkdir -p "$DIST"

host_os() {
  case "$(uname -s)" in
    Linux*)  echo linux ;;
    Darwin*) echo macos ;;
    MINGW*|MSYS*|CYGWIN*) echo windows ;;
    *)       echo unknown ;;
  esac
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

ensure_rust() {
  need_cmd cargo
  need_cmd rustup
}

ensure_target() {
  local target="$1"
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "→ rustup target add $target"
    rustup target add "$target"
  fi
}

copy_bin() {
  local src="$1"
  local dest_name="$2"
  if [[ ! -f "$src" && ! -f "${src}.exe" ]]; then
    echo "error: binary not found: $src" >&2
    exit 1
  fi
  if [[ -f "${src}.exe" ]]; then
    install -m 755 "${src}.exe" "$DIST/${dest_name}.exe"
    echo "✓ $DIST/${dest_name}.exe"
  else
    install -m 755 "$src" "$DIST/$dest_name"
    echo "✓ $DIST/$dest_name"
  fi
}

echo "Mailbox build — repo: $ROOT"
echo "Output:            $DIST"
echo
