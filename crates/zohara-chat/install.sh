#!/usr/bin/env bash
# Zohara Chat installer — puts the module on the system so the Zohara settings app
# (and the app grid) can find it. Run from the rust/ directory.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/scalable/apps"
META="$PREFIX/share/metainfo"

say() { printf '  %s\n' "$*"; }

printf '\nZohara Chat installer\n'

if ! command -v cargo >/dev/null 2>&1; then
  printf 'cargo not found. Install Rust first: https://rustup.rs\n' >&2
  exit 1
fi

say "building release binary"
cargo build --release --quiet

say "installing to $PREFIX"
mkdir -p "$BIN" "$APPS" "$ICONS" "$META"
install -m 0755 target/release/zohara-chat "$BIN/zohara-chat"
install -m 0755 target/release/zo-ai "$BIN/zo-ai"   # the old command name: prints a hint and starts zohara-chat
rm -f "$APPS/org.zohara.ZoAi.desktop"
install -m 0644 data/org.zohara.Chat.desktop "$APPS/org.zohara.Chat.desktop"
install -m 0644 data/org.zohara.Chat.metainfo.xml "$META/org.zohara.Chat.metainfo.xml"
[ -f data/zohara-chat.svg ] && install -m 0644 data/zohara-chat.svg "$ICONS/zohara-chat.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

say "done — launch with: zohara-chat"
printf '\n'
