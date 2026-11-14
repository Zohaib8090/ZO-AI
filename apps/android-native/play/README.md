# Play Store material for Zohara Agent (`org.zohara.agent`)

Everything the Play Console asks for that can live in the repository. The step-by-step guide, with every action the owner has to do in the console, is `docs/PLAY-STORE.md`.

| Path | What |
|---|---|
| `listing/en-US/title.txt` | App name (30 characters at most): `Zohara Agent` |
| `listing/en-US/short_description.txt` | Short description (80 at most) |
| `listing/en-US/full_description.txt` | Full description (4000 at most) |
| `listing/en-US/whats_new.txt` | "What's new" of the release (500 at most); update it for every upload |
| `listing/en-US/images/icon.png` | 512 x 512 app icon (made by `tools/make-graphics.sh` from the logo) |
| `listing/en-US/images/featureGraphic.png` | 1024 x 500 feature graphic (same script) |
| `listing/en-US/images/phoneScreenshots/` | Screenshots go here (not committed, see `screenshots.md`); `tools/make-screenshots.sh` makes candidates |
| `data-safety.md` | Answers for the Data safety form |
| `permissions.md` | Every permission, why it is there, and the text for the Play permission declarations |
| `content-rating.md` | Answers for the content rating questionnaire (IARC) |
| `screenshots.md` | Screenshot and graphics plan: sizes, which screens, how to take them |
| `tools/` | `make-graphics.sh`, `make-screenshots.sh`, `check-listing.sh` (length and size checks) |

The layout follows fastlane `supply` (`listing/<locale>/title.txt`, `short_description.txt`, `full_description.txt`, `images/...`), so a later move to
fastlane is only a copy; the docs do not need fastlane, the files are pasted into the Play Console by hand.
