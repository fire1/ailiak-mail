#!/usr/bin/env bash
# Dev run: rebuild + launch AiliakMail.
# On Linux Wayland, force winit onto X11 so wry child webviews (Desk / HTML mail) can attach.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" == Linux* ]]; then
  # wry child WebKitGTK needs X11 for both the window (winit) and GDK.
  # WINIT_UNIX_BACKEND is ignored by winit ≥0.29 — eframe forces X11 in main.rs.
  export GDK_BACKEND="${GDK_BACKEND:-x11}"
fi

exec cargo run -p mailbox-gui "$@"
