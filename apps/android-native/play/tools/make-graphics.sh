#!/usr/bin/env bash
# Makes the two Play graphics that are not screenshots from the app's own logo:
#   listing/en-US/images/icon.png            512 x 512, 32-bit PNG (the Play "App icon"; Play applies the rounded mask itself)
#   listing/en-US/images/featureGraphic.png  1024 x 500 (the Play "Feature graphic")
# Needs rsvg-convert (librsvg). Run from anywhere: bash apps/android-native/play/tools/make-graphics.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../listing/en-US/images"
mark="$here/../../../../crates/zohara-agent/data/zohara-agent-mark.svg"
mkdir -p "$out"
command -v rsvg-convert >/dev/null || { echo "rsvg-convert is needed (package librsvg)"; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# the mark's own drawing (without the <svg> wrapper), reused in both pictures
inner=$(sed -e '1d' -e '$d' "$mark" | grep -v '^\s*<!--')

# icon: full-bleed square (no rounded corners, Play masks it), dark tile #191919 as the Android launcher icon, mark at 60 %
cat > "$tmp/icon.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <rect width="512" height="512" fill="#191919"/>
  <g transform="translate(102.4,102.4) scale(2.8444)">$inner</g>
</svg>
SVG
rsvg-convert -w 512 -h 512 "$tmp/icon.svg" -o "$out/icon.png"

# feature graphic: the mark left, name and one line right, dark background; nothing important within 60 px of the edge
cat > "$tmp/feature.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="500" viewBox="0 0 1024 500">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#191919"/><stop offset="1" stop-color="#0B0E14"/></linearGradient>
  </defs>
  <rect width="1024" height="500" fill="url(#bg)"/>
  <g transform="translate(100,110) scale(2.5)">$inner</g>
  <text x="440" y="235" font-family="Inter, 'Helvetica Neue', Arial, sans-serif" font-size="74" font-weight="700" fill="#F2F2F2">Zohara Agent</text>
  <text x="444" y="300" font-family="Inter, 'Helvetica Neue', Arial, sans-serif" font-size="32" fill="#7EE0E0">Your AI agent, on every screen</text>
</svg>
SVG
rsvg-convert -w 1024 -h 500 "$tmp/feature.svg" -o "$out/featureGraphic.png"
file "$out/icon.png" "$out/featureGraphic.png"
