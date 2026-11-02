#!/usr/bin/env bash
# Build ZO-AI and install it for the current user (~/.local). The packaged install is packaging/PKGBUILD.
set -euo pipefail
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
D=crates/zo-ai/data

printf 'building (release)\n'
cargo build --release -p zo-ai -p zo-mcp

printf 'installing to %s\n' "$PREFIX"
install -Dm755 target/release/zo-ai  "$PREFIX/bin/zo-ai"
install -Dm755 target/release/zo-mcp "$PREFIX/bin/zo-mcp"
install -Dm644 $D/zo-ai.desktop            "$PREFIX/share/applications/zo-ai.desktop"
install -Dm644 $D/zo-ai-quick.desktop      "$PREFIX/share/applications/zo-ai-quick.desktop"
install -Dm644 $D/zo-ai-background.desktop "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/zo-ai-background.desktop"
install -Dm644 $D/org.zohara.ZoAi.metainfo.xml "$PREFIX/share/metainfo/org.zohara.ZoAi.metainfo.xml"
install -Dm644 $D/zo-ai.svg "$PREFIX/share/icons/hicolor/scalable/apps/zo-ai.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true

printf 'done. Start it now with: zo-ai --background   (Ctrl+Super+C or the Copilot key opens the popup after the next sign-in)\n'
printf 'Connect Claude Code to your computer: claude mcp add zohara -- %s/bin/zo-mcp\n' "$PREFIX"
