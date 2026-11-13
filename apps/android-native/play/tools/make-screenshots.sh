#!/usr/bin/env bash
# Builds CANDIDATE phone screenshots for the Play listing from the app's own Roborazzi test screenshots (app/src/test/screenshots).
# The test screenshots are 822 x 1720 (aspect 2.09), which Play rejects (the long side may be at most twice the short side), so each one is
# scaled to height 1920 and centred on a 1080 x 1920 (9:16) canvas painted with the screenshot's own background colour.
# These come from the JVM renderer with fake data, not from a phone: replace them with real device captures before production (docs/PLAY-STORE.md).
# Needs ImageMagick 7 (magick). Run: bash apps/android-native/play/tools/make-screenshots.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
src="$here/../../app/src/test/screenshots"
out="$here/../listing/en-US/images/phoneScreenshots"
mkdir -p "$out"
rm -f "$out"/*.png
# order = order in the store listing: home, chat answer, question card, approval, drawer, settings, light theme
n=0
for f in app_home_dark app_chat_answer_dark chat_question_single_dark_large chat_approval_dark_large app_drawer_dark app_settings_list_dark app_chat_answer_light; do
  n=$((n + 1))
  bg=$(magick "$src/$f.png" -format '%[pixel:p{2,2}]' info:)
  magick "$src/$f.png" -resize x1920 -background "$bg" -gravity center -extent 1080x1920 -alpha remove -alpha off "$out/$(printf '%02d' $n)_$f.png"
done
file "$out"/*.png | sed 's/^.*images\///' | cut -c1-90
