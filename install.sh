#!/usr/bin/env bash
# Build Zohara Chat and install it for the current user (~/.local). The packaged install is packaging/PKGBUILD.
set -euo pipefail
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
D=crates/zohara-chat/data

printf 'building (release)\n'
cargo build --release -p zohara-chat -p zo-mcp

printf 'installing to %s\n' "$PREFIX"
install -Dm755 target/release/zohara-chat  "$PREFIX/bin/zohara-chat"
install -Dm755 target/release/zo-mcp "$PREFIX/bin/zo-mcp"
install -Dm755 target/release/zo-ai "$PREFIX/bin/zo-ai"   # the old command name: prints a hint and starts zohara-chat
# The launcher is named after the app's id (org.zohara.Chat), which is how Plasma ties the window in the task bar to its icon.
install -Dm644 $D/org.zohara.Chat.desktop    "$PREFIX/share/applications/org.zohara.Chat.desktop"
# Old names (before the rename): hide the old launchers so there is one entry, and drop the old autostart copy.
rm -f "$PREFIX/share/applications/org.zohara.ZoAi.desktop" "$PREFIX/share/applications/zo-ai-quick.desktop" "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/zo-ai-background.desktop"
if [ -f /usr/share/applications/zo-ai.desktop ]; then
    printf '[Desktop Entry]\nType=Application\nName=Zohara Chat\nHidden=true\n' > "$PREFIX/share/applications/zo-ai.desktop"
fi
install -Dm644 $D/zohara-chat-quick.desktop      "$PREFIX/share/applications/zohara-chat-quick.desktop"
install -Dm644 $D/zohara-chat-background.desktop "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/zohara-chat-background.desktop"
install -Dm644 $D/org.zohara.Chat.metainfo.xml "$PREFIX/share/metainfo/org.zohara.Chat.metainfo.xml"
install -Dm644 $D/zohara-chat.svg "$PREFIX/share/icons/hicolor/scalable/apps/zohara-chat.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true

printf 'done. Start it now with: zohara-chat --background   (Ctrl+Super+C or the Copilot key opens the popup after the next sign-in)\n'
printf 'Connect Claude Code to your computer: claude mcp add zohara -- %s/bin/zo-mcp\n' "$PREFIX"
