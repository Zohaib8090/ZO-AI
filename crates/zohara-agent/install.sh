#!/usr/bin/env bash
# Zohara Agent installer — puts the module on the system so the Zohara settings app
# (and the app grid) can find it. Run from the rust/ directory.
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/scalable/apps"
META="$PREFIX/share/metainfo"

say() { printf '  %s\n' "$*"; }

printf '\nZohara Agent installer\n'

if ! command -v cargo >/dev/null 2>&1; then
  printf 'cargo not found. Install Rust first: https://rustup.rs\n' >&2
  exit 1
fi

say "building release binary"
cargo build --release --quiet

say "installing to $PREFIX"
mkdir -p "$BIN" "$APPS" "$ICONS" "$META"
install -m 0755 target/release/zohara-agent "$BIN/zohara-agent"
install -m 0755 target/release/zohara-chat "$BIN/zohara-chat"   # the old command names: print a hint and start zohara-agent
install -m 0755 target/release/zo-ai "$BIN/zo-ai"
rm -f "$APPS/org.zohara.ZoAi.desktop" "$APPS/org.zohara.Chat.desktop"
install -m 0644 data/org.zohara.Agent.desktop "$APPS/org.zohara.Agent.desktop"
install -m 0644 data/org.zohara.Agent.metainfo.xml "$META/org.zohara.Agent.metainfo.xml"
[ -f data/zohara-agent.svg ] && install -m 0644 data/zohara-agent.svg "$ICONS/zohara-agent.svg"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true
fi

say "done — launch with: zohara-agent"
printf '\n'
