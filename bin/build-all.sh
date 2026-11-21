#!/usr/bin/env bash
# Build whatever this host can produce, and print how to get the rest.
#
# Usage:
#   ./bin/build-all.sh
#   ./bin/build-all.sh --package
set -euo pipefail
# shellcheck source=./_common.sh
source "$(cd "$(dirname "$0")" && pwd)/_common.sh"

ARGS=("$@")
HOST="$(host_os)"

echo "Host OS: $HOST"
echo

case "$HOST" in
  linux)
    ./bin/build-linux.sh "${ARGS[@]}"
    echo
    echo "Also available:"
    echo "  ./bin/build-windows.sh --cross   # experimental"
    echo "  macOS / Windows installers → push a v* tag (GitHub Actions)"
    ;;
  macos)
    ./bin/build-macos.sh "${ARGS[@]}"
    echo
    echo "Linux / Windows installers → push a v* tag (GitHub Actions)"
    ;;
  windows)
    ./bin/build-windows.sh "${ARGS[@]}"
    echo
    echo "Linux / macOS installers → push a v* tag (GitHub Actions)"
    ;;
  *)
    echo "Unsupported host. Use GitHub Actions (.github/workflows/release.yml)." >&2
    exit 1
    ;;
esac
