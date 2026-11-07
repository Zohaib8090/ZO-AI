#!/usr/bin/env bash
# ZO-AI installer — puts the module on the system so the Zohara settings app
# (and the app grid) can find it. Run from the rust/ directory.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/scalable/apps"
META="$PREFIX/share/metainfo"

say() { printf '  %s\n' "$*"; }

printf '\nZO-AI installer\n'

if ! command -v cargo >/dev/null 2>&1; then
  printf 'cargo not found. Install Rust first: https://rustup.rs\n' >&2
  exit 1
fi

say "building release binary"
cargo build --release --quiet

say "installing to $PREFIX"
mkdir -p "$BIN" "$APPS" "$ICONS" "$META"
install -m 0755 target/release/zo-ai "$BIN/zo-ai"
install -m 0644 data/org.zohara.ZoAi.desktop "$APPS/org.zohara.ZoAi.desktop"
install -m 0644 data/org.zohara.ZoAi.metainfo.xml "$META/org.zohara.ZoAi.metainfo.xml"
[ -f data/zo-ai.svg ] && install -m 0644 data/zo-ai.svg "$ICONS/zo-ai.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

say "done — launch with: zo-ai"
printf '\n'
