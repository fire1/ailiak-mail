#!/usr/bin/env bash
# Install AiliakMail icons + .desktop so the dock/panel shows the proper icon.
# Linux does not read icons out of the ELF binary; FreeDesktop needs these files.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMG="$ROOT/crates/mailbox-gui/assets/img"
NAME="com.ailiak.AiliakMail"
# Previous id — remove so the panel does not keep a mismatched class/icon.
LEGACY_NAME="com.ailiak.Mailbox"
ICON_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
APP_BASE="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

mkdir -p "$APP_BASE" "$BIN_DIR"

# Drop stale desktop + icon names from the Mailbox → AiliakMail rename.
rm -f "$APP_BASE/${LEGACY_NAME}.desktop"
for size in 16 24 32 48 64 128 256 512; do
  rm -f "$ICON_BASE/${size}x${size}/apps/${LEGACY_NAME}.png"
done
# Old binary name (keep if user still wants it, but prefer ailiakmail).
if [[ -L "$BIN_DIR/mailbox" ]] || [[ -f "$BIN_DIR/mailbox" ]]; then
  echo "note: leaving $BIN_DIR/mailbox in place; panel/launcher now uses ailiakmail"
fi

for size in 16 24 32 48 64 128 256 512; do
  src="$IMG/ailiak-icon-${size}.png"
  if [[ -f "$src" ]]; then
    dest_dir="$ICON_BASE/${size}x${size}/apps"
    mkdir -p "$dest_dir"
    install -m 644 "$src" "$dest_dir/${NAME}.png"
    echo "installed ${size}x${size} icon"
  fi
done

# User hicolor trees often lack index.theme; without it icon caches stay empty
# and some panels never resolve Icon=com.ailiak.AiliakMail.
if [[ ! -f "$ICON_BASE/index.theme" ]]; then
  cat >"$ICON_BASE/index.theme" <<'EOF'
[Icon Theme]
Name=Hicolor
Comment=Fallback icon theme
Directories=16x16/apps,24x24/apps,32x32/apps,48x48/apps,64x64/apps,128x128/apps,256x256/apps,512x512/apps

[16x16/apps]
Size=16
Context=Applications
Type=Fixed

[24x24/apps]
Size=24
Context=Applications
Type=Fixed

[32x32/apps]
Size=32
Context=Applications
Type=Fixed

[48x48/apps]
Size=48
Context=Applications
Type=Fixed

[64x64/apps]
Size=64
Context=Applications
Type=Fixed

[128x128/apps]
Size=128
Context=Applications
Type=Fixed

[256x256/apps]
Size=256
Context=Applications
Type=Fixed

[512x512/apps]
Size=512
Context=Applications
Type=Fixed
EOF
  echo "wrote $ICON_BASE/index.theme"
fi

# Prefer a freshly built release binary if present, else debug.
if [[ -x "$ROOT/target/release/ailiakmail" ]]; then
  install -m 755 "$ROOT/target/release/ailiakmail" "$BIN_DIR/ailiakmail"
  echo "installed binary → $BIN_DIR/ailiakmail (release)"
elif [[ -x "$ROOT/target/debug/ailiakmail" ]]; then
  install -m 755 "$ROOT/target/debug/ailiakmail" "$BIN_DIR/ailiakmail"
  echo "installed binary → $BIN_DIR/ailiakmail (debug)"
else
  echo "warning: no built ailiakmail binary found; desktop Exec=ailiakmail must be on PATH" >&2
fi

desktop_out="$APP_BASE/${NAME}.desktop"
sed "s|^Exec=ailiakmail|Exec=$BIN_DIR/ailiakmail|" \
  "$ROOT/packaging/com.ailiak.AiliakMail.desktop" >"$desktop_out"
chmod 644 "$desktop_out"
echo "installed desktop entry → $desktop_out"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APP_BASE" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true
fi

echo
echo "Done. Restart the app (or log out/in). Launch with:  gtk-launch $NAME"
echo "Window icon is also embedded at runtime via eframe."
