# Screenshot and graphics plan

## What Play requires (checked by `tools/check-listing.sh`)

| Asset | Size | Count | Notes |
|---|---|---|---|
| App icon | 512 x 512 PNG (32-bit), up to 1 MB | 1 | `listing/en-US/images/icon.png`, made from the logo by `tools/make-graphics.sh`. Full-bleed square: Play rounds the corners itself |
| Feature graphic | 1024 x 500 PNG or JPEG, no alpha | 1 | `listing/en-US/images/featureGraphic.png` (same script). Keep text inside the middle: parts are cropped on some surfaces |
| Phone screenshots | PNG or JPEG, each side 320-3840 px, the long side at most 2 x the short side | 2 to 8 (Play needs at least 2; 4 or more can make the listing eligible for more placements) | `listing/en-US/images/phoneScreenshots/` |
| 7-inch and 10-inch tablet screenshots | same rules (16:9 or 9:16 recommended, 1080 px or more on the short side for 10 inch) | optional, up to 8 each | Needed only for the "large screen" badge; the app has a two-pane tablet layout |
| Promo video | YouTube URL | optional | skip for the first release |

## Which screens (in this order)

1. Home: the greeting and the composer (shows what the app is).
2. A chat with an answer and tool steps (the main use).
3. A question card (single choice with "Other": the thing that sets it apart).
4. An approval card (you stay in control).
5. The drawer: chats, projects, scheduled (shows breadth).
6. Settings: model, provider, personality.
7. "Run on my computer" (the desktop link).
8. The same chat in the light theme.

Rule: use a test account with invented content. No real e-mail addresses, no real API keys, no real chats.

## How to get them

* **Real device (best, and required before production)**: install the signed release build on a phone (or an emulator with a Pixel 8 profile), sign in with the test account, take each screenshot with the phone (power + volume down) or `adb exec-out screencap -p > shot.png` on an emulator. Phones with a 20:9 screen produce 1080 x 2400 (ratio 2.22): crop to 1080 x 2160 or add a bar so that the ratio is at most 2:1.
* **From the app's own screenshot tests (candidates)**: `./gradlew recordRoborazziDebug` renders the screens on the JVM into `app/src/test/screenshots` with fake data; `tools/make-screenshots.sh` turns seven of them into 1080 x 1920 candidates (the test images are 822 x 1720, ratio 2.09, which Play rejects, so the script pads them). These are not committed (the images in the repository predate the rename and some still say ZO-AI): re-record them after the last UI change, look at every one, and only then use them.
* Add a one-line caption on each screenshot only if it is readable at thumbnail size; otherwise leave them clean. No device frames are required.

## Localised listings

English (en-US) first. Hindi and others later: add `listing/hi-IN/...` with the same file names (the owner writes English only in code and docs; store text for other languages is a separate decision).
