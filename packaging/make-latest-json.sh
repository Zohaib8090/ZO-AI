#!/usr/bin/env bash
# Writes the latest.json of a release to stdout (docs/UPDATES.md): the file every client's update check reads (through the server).
#
#   packaging/make-latest-json.sh <version> <dist-dir> <notes-file> > latest.json
#
# <dist-dir> holds the release files: the first *.apk, *.pkg.tar.zst and *.aab found are listed (file names, with their sha256).
# Environment: RELEASE_DATE (default: today, UTC), MIN_SUPPORTED (a version; clients older than it are told the update is required).
set -euo pipefail

version="${1:?usage: make-latest-json.sh <version> <dist-dir> <notes-file>}"
dist="${2:?usage: make-latest-json.sh <version> <dist-dir> <notes-file>}"
notes_file="${3:?usage: make-latest-json.sh <version> <dist-dir> <notes-file>}"
version="${version#v}"
date="${RELEASE_DATE:-$(date -u +%F)}"
min="${MIN_SUPPORTED:-}"

first() { find "$dist" -maxdepth 1 -type f -name "$1" -printf '%f\n' | LC_ALL=C sort | head -n1; }
apk="$(first '*.apk')"
pkg="$(first '*.pkg.tar.zst')"
aab="$(first '*.aab')"

sums='{}'
for f in "$apk" "$pkg" "$aab"; do
  [ -n "$f" ] || continue
  h="$(sha256sum "$dist/$f" | cut -d' ' -f1)"
  sums="$(jq -c --arg f "$f" --arg h "$h" '. + {($f): $h}' <<<"$sums")"
done

# the notes are cut well below the 4000 characters the clients keep
notes="$(head -c 3500 "$notes_file")"

jq -n \
  --arg version "$version" --arg date "$date" --arg notes "$notes" --arg min "$min" \
  --arg apk "$apk" --arg pkg "$pkg" --arg aab "$aab" --argjson sums "$sums" '
  {version: $version, date: $date, notes: $notes}
  + (if $min != "" then {min_supported: $min} else {} end)
  + {assets: (
      ({apk: $apk, pkg: $pkg, aab: $aab} | with_entries(select(.value != ""))
      ) + {sha256s: $sums})}'
