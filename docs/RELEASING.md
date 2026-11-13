# Releasing Zohara Agent (GitHub Releases)

A release is a git tag. Pushing the tag `v1.2.3` makes `.github/workflows/release.yml` build everything and publish a GitHub Release. This is separate from the
alpha channel: every push to `main` still goes to the alpha channel through `build.yml` and `publish-alpha.yml`, and beta and stable are still chosen on purpose
from the release admin site. A GitHub Release is the public download page (website, Android APK, anyone without Zohara OS), and `latest.json` is what the
update checkers in the apps read. Nothing in the repository creates a tag or a release by itself.

## What a release contains

| File | What |
|---|---|
| `zohara-agent-<version>-x86_64.pkg.tar.zst` | the Arch Linux package (`pacman -U`), built like the alpha package from `packaging/PKGBUILD` |
| `zohara-agent-<version>-linux-x86_64.tar.gz` | the programs `zo`, `zo-server`, `zohara-agent`, `zo-mcp`, README and INSTALL.txt, built in the Arch container (the desktop app needs gtk4, libadwaita, libsecret, webkitgtk-6.0) |
| `zohara-agent-<version>.apk` and `.aab` | Android release build, signed with the release key. When the signing secrets are missing both are named `zohara-agent-<version>-unsigned.apk` / `-unsigned.aab` (an unsigned APK does not install; it is only there to prove the build works) |
| `zohara-agent-<version>-mapping.txt` | the R8 mapping, to read crash stack traces |
| `SHA256SUMS` | `sha256sum` format, every file above |
| `latest.json` | `{version, date, notes, assets[{name,url,size,sha256}], min_supported, sha256{name: hex}}` |

The release is marked **pre-release** when the tag has a suffix (`-alpha.N`, `-beta.N`, `-rc.N`). The notes are the commit subjects since the previous tag.

## Versioning

Tags are `vMAJOR.MINOR.PATCH` or `vMAJOR.MINOR.PATCH-(alpha|beta|rc).N` (checked by `packaging/release/version.py`; anything else fails the run at once).

* `versionName` (Android), the crates' version (`zo --version`, the package) and the release name are the tag without the `v`. CI sets the crate versions with `packaging/release/set-version.py` on its own checkout; nothing is committed back.
* Android `versionCode = MAJOR*1000000 + MINOR*10000 + PATCH*100 + STAGE`, where STAGE is `alpha.N` -> N, `beta.N` -> 30+N, `rc.N` -> 60+N, final -> 99 (MINOR and PATCH below 100, N from 1 to 29). Examples: `v0.1.0` = 10099, `v0.1.0-beta.2` = 10032, `v0.1.1-alpha.1` = 10101, `v1.0.0` = 1000099. Every later version has a higher code, which is what Play requires for each upload, and a stable release always beats its own pre-releases.
* Arch `pkgver` cannot contain `-`: `0.2.0-beta.1` becomes `0.2.0beta.1`, which pacman sorts after `0.2.0alpha.9`, after `0.1.x`, and before `0.2.0` (checked with `vercmp`).
* `packaging/release/MIN_SUPPORTED` holds the oldest version that may still run (the `min_supported` of `latest.json`). Raise it only when an old version can no longer work with the server.

Print the strings for a tag: `python3 packaging/release/version.py v0.2.0-beta.1`.

## Cut a release

1. Make sure `main` is what you want to ship and CI (Build & Publish Zohara Agent) is green on it.
2. Do a dry run first: GitHub > Actions > Release > Run workflow, leave "Dry run" ticked, enter a version such as `0.2.0-beta.1`. It builds everything and keeps the files as the workflow artifact `release-all` (download it and look; `latest.json` and `SHA256SUMS` are in it). It publishes nothing.
3. Tag and push (the owner does this; the tooling never does):

       git tag -a v0.2.0-beta.1 -m "Zohara Agent 0.2.0-beta.1"
       git push origin v0.2.0-beta.1

4. Watch the run. The Release appears when the `publish` job finishes. Check that the files and `SHA256SUMS` are there and the notes read well; edit the notes in GitHub if you want.
5. For Android on Google Play, upload the `.aab` from the release by hand (docs/PLAY-STORE.md).

A mistaken release: delete the GitHub Release and the tag (`git push origin :refs/tags/v0.2.0-beta.1`) and cut a new version number. Never reuse a version number that reached Play.

## Secrets to add (repository Settings > Secrets and variables > Actions)

All optional for a dry run. None may ever be committed or pasted into chat. The workflow never prints them (it writes the keystore to a temporary file, deletes it at the end, and passes passwords only as environment variables to the Gradle step).

| Secret | What |
|---|---|
| `ZO_STORE_BASE64` | the release keystore, base64 on one line: `base64 -w0 zohara-agent-release.jks` |
| `ZO_STORE_PASSWORD` | the keystore password |
| `ZO_KEY_ALIAS` | the key alias (`zohara-agent` in the command below) |
| `ZO_KEY_PASSWORD` | the key password |
| `ZO_GOOGLE_SERVICES_JSON_BASE64` | `google-services.json` of the Firebase app `org.zohara.agent`, base64 (`base64 -w0 google-services.json`). Without it the APK builds but has no sign-in and no push (the build prints a warning) |
| `ZO_WEB_CLIENT_ID` | the Web client id of the Firebase project (Google sign-in) |

If any of the four signing secrets is missing, the Android files are built unsigned and named `*-unsigned`; the Linux files and the release are not affected.
`PACKAGES_DISPATCH_TOKEN` (alpha publishing) is a different thing and is not used here.

Safety built in: the workflow starts only from a tag push (or by hand); the publish job is the only one with `contents: write`; it never publishes from a fork or when "Dry run" is on or when started from a branch; actions are pinned to major tags of GitHub's own actions only; secrets are not available to pull requests from forks (the workflow does not run on pull requests at all).

## Make the release keystore (once)

    keytool -genkeypair -v -keystore ~/zohara-agent-release.jks -alias zohara-agent \
      -keyalg RSA -keysize 4096 -validity 10000

Keep the keystore and both passwords in a password manager **and** an offline copy outside the repository (`*.jks` and `*.keystore` are git-ignored). Print the fingerprints for Firebase:

    keytool -list -v -keystore ~/zohara-agent-release.jks -alias zohara-agent | grep -E "SHA1|SHA256"

If you use Play App Signing (recommended, docs/PLAY-STORE.md), this key is the **upload key**: Google holds the real app-signing key. Then there are two signatures to know about:

* Add the SHA-1/SHA-256 of **both** the upload key and Google's app-signing key (Play Console > App integrity) to the Firebase Android app, or Google sign-in fails in one of the two builds.
* The APK downloaded from GitHub (signed with the upload key) and the app installed from Play (signed with Google's key) have different signatures: Android will not update one with the other. Tell people to pick one source, or uninstall before switching.

To build the same release locally (the same variables):

    cd apps/android-native
    ZO_VERSION_CODE=10099 ZO_VERSION_NAME=0.1.0 ZO_STORE_FILE=$HOME/zohara-agent-release.jks \
    ZO_STORE_PASSWORD=... ZO_KEY_ALIAS=zohara-agent ZO_KEY_PASSWORD=... ./gradlew bundleRelease assembleRelease

## How Play uploads work

The workflow does not upload to Google Play. The `.aab` from the GitHub Release is uploaded by hand in the Play Console (Testing > Internal testing > Create new release > upload the `.aab`, paste "What's new", Save, Review, Roll out). The first release of a personal developer account has to go through a closed test before production; the whole path, including every owner action, is in `docs/PLAY-STORE.md`. A later step can automate the upload with a service account and the Play Developer API; it is not set up because it needs a Google Cloud service account key, which is one more secret to guard.

## What this was tested with

The scripts (`version.py`, `set-version.py`, `make-latest.py`) were run locally; the Android steps (`assembleRelease`, `bundleRelease`) were run offline with a throw-away keystore and the result checked with `apksigner`, `jarsigner`, `aapt2` and `zipalign`. The workflow file itself can only really be tested by GitHub: run the dry run before the first real tag.
