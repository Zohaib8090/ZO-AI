#!/usr/bin/env bash
# Build Zohara Agent and install it for the current user (~/.local). The packaged install is packaging/PKGBUILD.
set -euo pipefail
cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
D=crates/zohara-agent/data

printf 'building (release)\n'
cargo build --release -p zohara-agent -p zo-mcp

printf 'installing to %s\n' "$PREFIX"
install -Dm755 target/release/zohara-agent  "$PREFIX/bin/zohara-agent"
install -Dm755 target/release/zo-mcp "$PREFIX/bin/zo-mcp"
install -Dm755 target/release/zohara-chat "$PREFIX/bin/zohara-chat"   # the old command names: print a hint and start zohara-agent
install -Dm755 target/release/zo-ai "$PREFIX/bin/zo-ai"
# The launcher is named after the app's id (org.zohara.Agent), which is how Plasma ties the window in the task bar to its icon.
install -Dm644 $D/org.zohara.Agent.desktop    "$PREFIX/share/applications/org.zohara.Agent.desktop"
# Old names (before the renames): remove the old launchers so there is one entry.
rm -f "$PREFIX/share/applications/org.zohara.ZoAi.desktop" "$PREFIX/share/applications/org.zohara.Chat.desktop" "$PREFIX/share/applications/zo-ai-quick.desktop" "$PREFIX/share/applications/zohara-chat-quick.desktop"
if [ -f /usr/share/applications/zo-ai.desktop ]; then
    printf '[Desktop Entry]\nType=Application\nName=Zohara Agent\nHidden=true\n' > "$PREFIX/share/applications/zo-ai.desktop"
fi
install -Dm644 $D/zohara-agent-quick.desktop      "$PREFIX/share/applications/zohara-agent-quick.desktop"
# Autostart: the person's own old entries (zohara-chat-background.desktop, zo-ai-background.desktop) are switched off, not deleted, so the app
# can still see whether it was off (the app carries the choice over to the new entry on its first start, newest old entry first); a new
# entry is only written when the newest old one was not switched off.
AUTO="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
newest=""
for old in zohara-chat-background.desktop zo-ai-background.desktop; do
    if [ -f "$AUTO/$old" ]; then newest="$AUTO/$old"; break; fi
done
if [ -n "$newest" ] && grep -qi '^Hidden=true' "$newest"; then
    printf 'old autostart entry was switched off: keeping autostart off\n'
else
    install -Dm644 $D/zohara-agent-background.desktop "$AUTO/zohara-agent-background.desktop"
    for old in zohara-chat-background.desktop zo-ai-background.desktop; do
        if [ -f "$AUTO/$old" ] && ! grep -qi '^Hidden=true' "$AUTO/$old"; then printf 'Hidden=true\n' >> "$AUTO/$old"; fi
    done
fi
install -Dm644 $D/org.zohara.Agent.metainfo.xml "$PREFIX/share/metainfo/org.zohara.Agent.metainfo.xml"
install -Dm644 $D/zohara-agent.svg "$PREFIX/share/icons/hicolor/scalable/apps/zohara-agent.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$PREFIX/share/applications" || true

printf 'done. Start it now with: zohara-agent --background   (Ctrl+Super+C or the Copilot key opens the popup after the next sign-in)\n'
printf 'Connect Claude Code to your computer: claude mcp add zohara -- %s/bin/zo-mcp\n' "$PREFIX"
