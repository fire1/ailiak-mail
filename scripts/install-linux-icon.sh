#!/usr/bin/env bash
# Install Ailiak icons + .desktop so the dock/launcher shows the proper icon.
# Linux does not read icons out of the ELF binary; FreeDesktop needs these files.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="$ROOT/crates/mailbox-gui/assets/img"
NAME="com.ailiak.Mailbox"
ICON_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
APP_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

mkdir -p "$APP_BASE" "$BIN_DIR"

for size in 16 24 32 48 64 128 256 512; do
  src="$IMG/ailiak-icon-${size}.png"
  if [[ -f "$src" ]]; then
    dest_dir="$ICON_BASE/${size}x${size}/apps"
    mkdir -p "$dest_dir"
    install -m 644 "$src" "$dest_dir/${NAME}.png"
    echo "installed ${size}x${size} icon"
  fi
done

# Prefer a freshly built release binary if present, else debug.
if [[ -x "$ROOT/target/release/mailbox" ]]; then
  install -m 755 "$ROOT/target/release/mailbox" "$BIN_DIR/mailbox"
  echo "installed binary → $BIN_DIR/mailbox (release)"
elif [[ -x "$ROOT/target/debug/mailbox" ]]; then
  install -m 755 "$ROOT/target/debug/mailbox" "$BIN_DIR/mailbox"
  echo "installed binary → $BIN_DIR/mailbox (debug)"
else
  echo "warning: no built mailbox binary found; desktop Exec=mailbox must be on PATH" >&2
fi

desktop_out="$APP_BASE/${NAME}.desktop"
sed "s|^Exec=mailbox|Exec=$BIN_DIR/mailbox|" \
  "$ROOT/packaging/com.ailiak.Mailbox.desktop" >"$desktop_out"
chmod 644 "$desktop_out"
echo "installed desktop entry → $desktop_out"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_BASE" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
fi

echo
echo "Done. Log out/in or run:  gtk-launch $NAME"
echo "Window icon is also embedded at runtime via eframe."
