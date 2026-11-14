# Updates: how every Zohara Agent client finds out that a new version exists

Written 2026-11-14. Policy in one sentence: **every client checks, tells the person, and links to the update; nothing is ever downloaded or installed
silently.** (Zohara OS has its own system update flow; this page is about the Zohara Agent apps.)

## 1. The source of truth: a GitHub Release

A release of `Zohaib8090/zohara-agent` carries these files (made by `.github/workflows/release.yml`, section 6):

| File | What |
|---|---|
| `zohara-agent-<version>-1-x86_64.pkg.tar.zst` | the Arch package (desktop app, `zo`, `zo-mcp`) |
| `zohara-agent-<version>.apk` | the signed Android APK (sideload) |
| `zohara-agent-<version>.aab` | the signed Android App Bundle (upload to Google Play) |
| `latest.json` | what the clients read |

`latest.json`:

```json
{"version": "0.2.0", "date": "2026-11-20", "notes": "What is new ...", "min_supported": "0.1.0",
 "assets": {"apk": "zohara-agent-0.2.0.apk", "pkg": "zohara-agent-0.2.0-1-x86_64.pkg.tar.zst", "aab": "zohara-agent-0.2.0.aab",
            "sha256s": {"zohara-agent-0.2.0.apk": "<64 hex characters>"}}}
```

* `version`: a version in the form `1.2.3` (a leading `v`, a pre-release such as `-rc.1` and a build time as a fourth number are understood). Clients compare it with
  the version they run by the rules of semver (section 2). If it cannot be read, the release is ignored.
* `min_supported` (optional): a client older than this is told the update is **required** (still only told: it is never forced).
* `assets` holds file names (never addresses) and sha256 sums. Every client builds addresses itself: the release page is
  `https://github.com/<repo>/releases/tag/v<version>`.
* `notes`: plain text, cut to 4000 characters. Written from `docs/release-notes/<version>.md` if that file exists, else from the commit subjects since the previous tag.
* GitHub only calls a release "latest" when it is not a pre-release and not a draft. A tag with a dash (`v0.2.0-rc.1`) is published as a pre-release, so no client announces it.

If a release has no `latest.json` (an old or hand-made release), the server falls back to GitHub's release record (tag, date and text); `assets` is then empty.

## 2. The server: `GET /api/update/latest`

Public (no sign-in, no cookie, any origin), read-only, answers JSON:

```json
{"ok": true, "server_version": "0.1.0", "repo": "Zohaib8090/zohara-agent", "latest": {"version": "...", "date": "...", "notes": "...",
 "min_supported": "...", "assets": {...}, "release_url": "https://github.com/..."}, "checked_at": 1794600000, "stale": false}
```

* **No user data goes to GitHub.** The clients only ask the Zohara Agent server; the server asks GitHub (`https://github.com/<repo>/releases/latest/download/latest.json`,
  else `https://api.github.com/repos/<repo>/releases/latest`) with a fixed `User-Agent` and nothing else. GitHub sees the server's address only.
  The answer holds nothing about the caller either (a test checks the list of fields).
* **Cached for one hour** (`zo_core::update::Cache`): one request to GitHub per hour however many clients ask (a crowd of callers makes one request).
* **Fails gracefully.** The answer is always `200` with `ok: true`. If GitHub cannot be reached, answers an error, or limits requests (403/429), the last good answer
  is served with `"stale": true` and an `error` sentence; GitHub is left alone for five minutes after a failure. With no good answer yet, `latest` is `null` and `error` says why
  (for example `no release has been published yet`). A client shows that as "could not check", not as a failure of the server.
* **Settings** (operator): `ZO_UPDATE_REPO` (`owner/name`, default `Zohaib8090/zohara-agent`; a value that is not a plain `owner/name` is ignored). `ZO_UPDATE_BASE_URL` points the
  source at a mirror (tests use it); leave it unset.
* **One source of the version:** the Cargo workspace version (`Cargo.toml`, `[workspace.package]`). The server reports it as `server_version`; `zo version`, the desktop About
  page and the web About page show it; the Android `versionName` is that same value for a local build and the release tag in CI (`ZO_VERSION_NAME`).
  The release workflow refuses a tag that differs from `Cargo.toml`.

### Comparing versions

All clients use the same rules (`zo_core::update::Semver`, Kotlin `AppVersion`, JavaScript `semverCmp`; the same test vectors in all three):
up to four numbers compared as numbers (`0.10.0` is newer than `0.9.9`), then pre-releases (`1.0.0-rc.1` is older than `1.0.0`; `-beta.2` is older than `-beta.11`),
build metadata (`+abc`) ignored. A version that cannot be read is never "newer": a broken answer never raises a false alarm.

## 3. Android (native app, `apps/android-native`, package `org.zohara.agent`)

* **Settings > About** (`SettingsAboutScreen`): the version line (`App version 0.1.0 (build 1) · server version ...`), a **Check for updates** button, the result
  (*up to date*, *Version X is available* with the release notes, or *Could not check for updates: ...*), and, when a newer version exists, a **Download** button.
* **Download** opens the GitHub release page in a browser tab for a sideloaded install. When the install came from **Google Play** (installer package
  `com.android.vending`, read with `PackageManager.getInstallSourceInfo`) the button says **Open Google Play** and opens the Play listing
  (`market://details?id=org.zohara.agent`, or the Play web page if the Play app is missing) instead. The app never downloads, installs or sideloads anything itself,
  and asks for no install permission.
* **Check for updates automatically** (on by default; saved in the app's settings): WorkManager periodic work `zohara-agent-update-check`, every ~12 hours (flex window 3 hours),
  only with a network (`NetworkType.CONNECTED`) and a battery that is not low, exponential back-off, unique work (starting the app again does not push it back).
  WorkManager keeps the work in its own database and re-schedules it after a restart of the phone. A failed check is tried again twice, then waits for the next period.
  Turning the switch off cancels the work.
* **One notification per new version**, on its own channel **App updates** (low importance: no sound; the person can tune it in the system settings). The last announced version
  is saved; a newer one is announced again, the same one never. It is only posted when notifications are allowed (and then remembered). Tapping it opens
  `zohara-agent://about`, which opens Settings > About (after sign-in if the app was signed out).
* The update call (`UpdateApi`) is a plain OkHttp client with **no** sign-in token: it asks `GET <server>/api/update/latest` only.
* Version: `versionName` = `ZO_VERSION_NAME` or the Cargo workspace version; `versionCode` = `ZO_VERSION_CODE` (CI uses the run number: it always grows, which Google Play needs).
* Code: `org.zohara.agent.update` (`AppVersion`, `UpdateApi`/`UpdateChecker`, `UpdateNotifier`, `UpdateWorker`/`UpdateScheduler`, `InstallSource`, `UpdateController`);
  screen: `settings/SettingsAboutScreen.kt`.
* Tests (JVM): `AppVersionTest`, `UpdateCheckTest` (mock server: available, up to date, required, no release, every kind of failure, no auth header), `UpdateNotifierTest`
  (one per version, channel, tap intent, no permission), `UpdateWorkerTest` (worker, schedule constraints, unique work, the switch), `InstallSourceTest`, `AboutScreenTest`
  (Compose UI + screenshots `settings_about_*.png` in both themes), `DeepLinksTest`.

## 4. Desktop (`crates/zohara-agent`): notify only

* **Settings > About**: the version, **Check now**, the result, **Check for updates automatically** (on by default), and, when a newer version exists, the notes and
  **Update: open the release page** (opens `https://github.com/...` only) and **Show the pacman command** (a dialog with `sudo pacman -U <file>`, and a Copy button;
  it is shown only, never run). Zohara Agent never installs anything; on Zohara OS the update also arrives through the system updates.
* **In the background**, while the app runs (also when it only lives in the tray): after two minutes, then every 30 minutes, it looks whether the last automatic check was 12 hours ago
  (stored in `~/.config/zohara-agent/update-state.json`, with the switch and the last version told about) and if so asks the server. For a new version it shows ONE desktop
  notification ("Zohara Agent update available", body with the versions and the first line of the notes; clicking opens Settings > About, the button **Release page** opens the release);
  the same version is never announced again, a newer one is.
* Code: `crates/zohara-agent/src/updates.rs`, `crates/zo-core/src/update.rs`.

## 5. Web (`crates/zo-server/web/index.html`)

* **Settings > About** shows the **web app build** (`<workspace version> (<7 characters of the page hash>)`, swapped into the page by the server: it is the same hash that versions the service worker),
  the **server version**, a **Check** button and its result: whether a new page is waiting, and whether a newer app release exists (with notes and the release page).
* **Service worker flow**: `/sw.js` has a version that is a hash of the page, so every new page is a new worker. When a new worker is waiting the page shows the toast
  **New version available - Reload** (Reload activates it and reloads; it asks first if an answer is still being written). The page asks the browser to look for a new worker every hour,
  when the tab becomes visible again, and when the network comes back; **Check** does it on demand.
* The web page has no installer to start: it updates by itself when the server is redeployed, and this toast is how the person gets it.

## 6. The terminal (`zo`)

* `zo version` prints `zo <version>` (`zo --version` does the same).
* `zo update-check` asks the server (the Zohara Agent server, or the one given with `--remote`/`ZO_REMOTE_URL`) and prints the result: up to date, or the new version with its notes, the release page and,
  if the release has a package, `sudo pacman -U <file>`. Exit code `0` when the check worked, `1` when it could not be done (the server unreachable or without an update check, or no release found).
  It never downloads anything.

## 7. The release workflow (`.github/workflows/release.yml`)

Pushing a tag `vX.Y.Z` (the tag must equal the Cargo workspace version) builds and publishes the release:

1. **meta**: reads the tag, checks it against `Cargo.toml`, refuses a tag whose release already exists.
2. **arch**: tests, builds, `makepkg` (the same steps as `build.yml`, pinned to the approved Arch day), uploads the package.
3. **android**: unit tests, then `assembleRelease` and `bundleRelease` with the version from the tag, the Firebase file and the signing key from secrets; if the signing secrets are missing the
   APK and bundle are **not** built or attached (an unsigned one cannot be installed or uploaded) and a warning says so.
4. **release**: notes, `packaging/make-latest-json.sh` (file names and sha256 sums into `latest.json`), `gh release create` with every file (a tag with a dash becomes a pre-release).

Owner steps: add the secrets `ANDROID_KEYSTORE_BASE64` (`base64 -w0 release.jks`), `ANDROID_STORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD` and `GOOGLE_SERVICES_JSON_BASE64`, and the
variable `ZO_WEB_CLIENT_ID`; optionally the variable `ZO_MIN_SUPPORTED`. Never commit the keystore or `google-services.json`. To release: raise `version` in `Cargo.toml`, commit,
`git tag v0.2.0 && git push origin v0.2.0`. For Google Play, download the `.aab` from the release and upload it in the Play Console (`docs/ANDROID.md`).

## 8. What was verified, and what was not

Verified (2026-11-14, on the developer machine):

* Rust: semver order, `latest.json` parsing and cleaning, the one-hour cache, back-off and stale fallback, the failure messages against a mock HTTP server, the client call, the desktop
  state file and the 12-hour rule, the notify-once rule, the release script's output through the parser (`cargo test -p zo-core update::`); the public route and its failure shape over real HTTP
  (`cargo test -p zo-server the_update_check`); `zo version` and `zo update-check` (unit tests, and by hand against a real `zo-server` with a mock GitHub).
* Web: the About page in a real browser against a real `zo-server` with a mock GitHub (version rows, Check, result, notes, release link); the semver function against the same vectors in Node.
* Android: the JVM tests listed above (823 in the whole app, all passing), the release (R8) build, the merged manifest (`RECEIVE_BOOT_COMPLETED`, WorkManager's reschedule receiver).

Not verified:

* No real GitHub release exists yet, so nothing has been read from GitHub itself (the fallback path to the API and the redirect of `releases/latest/download` were tested against a mock).
* The service worker "New version available" toast in a browser that has a working worker (the in-app browser pane cannot register one); the flow is the existing worker code with new text.
* The GTK Settings > About page and the desktop notification (with its two actions) were compiled and their text logic tested, but never run on a screen.
* Android on a device: the periodic work really running at ~12 hours and after a restart, the notification on a real phone, the Play listing intent, `getInstallSourceInfo` on a Play install.
* The release workflow itself (never run: it needs a pushed tag and the owner's secrets); `actionlint` was not available.
