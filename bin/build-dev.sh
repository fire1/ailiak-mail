#!/usr/bin/env bash
# Dev run: rebuild + launch AiliakMail.
# On Linux Wayland, force winit onto X11 so wry child webviews (Desk / HTML mail) can attach.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" == Linux* ]]; then
  export WINIT_UNIX_BACKEND="${WINIT_UNIX_BACKEND:-x11}"
fi

exec cargo run -p mailbox-gui "$@"
