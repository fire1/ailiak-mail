#!/usr/bin/env bash
# Dev run: rebuild + launch AiliakMail.
# On Linux Wayland, force winit onto X11 so wry child webviews (Desk / HTML mail) can attach.
#
# Options:
#   --omarchy <N>, -o <N>  On Omarchy/Hyprland: build, switch to workspace N, then run
#                          (so the app opens on that workspace instead of the current one).
#                          Example: -o 3  → workspace 3. Default N is 3 if omitted.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

OMARCHY_WS=""
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --omarchy|-o)
      if [[ $# -ge 2 && "$2" =~ ^[0-9]+$ ]]; then
        OMARCHY_WS="$2"
        shift 2
      else
        OMARCHY_WS=3
        shift
      fi
      ;;
    --omarchy=*|-o=*)
      OMARCHY_WS="${1#*=}"
      if [[ ! "$OMARCHY_WS" =~ ^[0-9]+$ ]]; then
        echo "error: workspace must be a number (got: $OMARCHY_WS)" >&2
        exit 1
      fi
      shift
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ "$(uname -s)" == Linux* ]]; then
  # wry child WebKitGTK needs X11 for both the window (winit) and GDK.
  # WINIT_UNIX_BACKEND is ignored by winit ≥0.29 — eframe forces X11 in main.rs.
  export GDK_BACKEND="${GDK_BACKEND:-x11}"
fi

if [[ -n "$OMARCHY_WS" ]]; then
  if ! command -v hyprctl >/dev/null 2>&1; then
    echo "error: --omarchy/-o requires hyprctl (Omarchy/Hyprland)" >&2
    exit 1
  fi
  # Prebuild so we don't sit on an empty workspace while cargo compiles.
  cargo build -p mailbox-gui "${ARGS[@]}"
  # Omarchy/Hyprland Lua config: classic `dispatch workspace N` is invalid.
  hyprctl dispatch "hl.dsp.focus({ workspace = \"$OMARCHY_WS\" })"
  exec cargo run -p mailbox-gui "${ARGS[@]}"
fi

exec cargo run -p mailbox-gui "${ARGS[@]}"
