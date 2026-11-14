#!/usr/bin/env bash
# Checks the Play listing files against the store limits. Exit 1 when something is wrong. Run: bash apps/android-native/play/tools/check-listing.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
l="$here/../listing/en-US"
bad=0
chars() { python3 -I -c 'import sys; print(len(open(sys.argv[1], encoding="utf-8").read().rstrip("\n")))' "$1"; }
check() { # file limit
  local n; n=$(chars "$l/$1")
  if [ "$n" -gt "$2" ]; then echo "FAIL $1: $n characters (limit $2)"; bad=1; else echo "ok   $1: $n of $2"; fi
}
check title.txt 30
check short_description.txt 80
check full_description.txt 4000
check whats_new.txt 500
dim() { python3 -I - "$1" <<'PY'
import struct, sys
d = open(sys.argv[1], "rb").read(32)
w, h = struct.unpack(">II", d[16:24])
print(w, h)
PY
}
read -r w h < <(dim "$l/images/icon.png");           [ "$w $h" = "512 512" ]  && echo "ok   icon.png 512x512"            || { echo "FAIL icon.png is ${w}x${h}"; bad=1; }
read -r w h < <(dim "$l/images/featureGraphic.png"); [ "$w $h" = "1024 500" ] && echo "ok   featureGraphic.png 1024x500" || { echo "FAIL featureGraphic.png is ${w}x${h}"; bad=1; }
shots=0
for f in "$l"/images/phoneScreenshots/*.png; do
  [ -e "$f" ] || continue
  shots=$((shots + 1))
  read -r w h < <(dim "$f")
  short=$(( w < h ? w : h )); long=$(( w < h ? h : w ))
  if [ "$short" -lt 320 ] || [ "$long" -gt 3840 ] || [ $((long)) -gt $((short * 2)) ]; then echo "FAIL $(basename "$f"): ${w}x${h} (each side 320-3840 px, long side at most twice the short side)"; bad=1; fi
done
echo "phone screenshots found: $shots (Play needs 2 to 8; none are committed, see screenshots.md)"
exit $bad
