# ZO-AI for Android (native)

Kotlin + Jetpack Compose, no WebView. Package `org.zohara.zoai`, minSdk 26. It replaces the Capacitor wrapper `apps/android-web` (legacy).
What it has to contain is listed in `docs/ANDROID-NATIVE-PARITY.md` (the parity matrix, the source of truth); the server API in
`apps/android-native/docs/API.md`; `docs/check-parity.py` checks the matrix.

## Build

Needs JDK 17 and the Android SDK (`sdk.dir` in `local.properties`, which is git-ignored). Keep Gradle caches in the default `~/.gradle`.

    ./gradlew assembleDebug testDebugUnitTest          # build and unit tests
    ./gradlew recordRoborazziDebug                     # rewrite the screenshots in app/src/test/screenshots
    ./gradlew verifyRoborazziDebug                     # compare against them

## Layout (`app/src/main/java/org/zohara/zoai`)

| Package | What |
|---|---|
| `core.design` | colour tokens (dark and light, from `crates/zo-ai/src/theme.rs`), type (Inter, Source Serif 4 in `res/font`), shapes, the logo, reusable components |
| `core.net` | OkHttp client with the Firebase ID token interceptor, `ZoApi` (one function per op), NDJSON `send` as a `Flow<SendEvent>`, error mapping |
| `core.auth` | Firebase sign-in (Google through Credential Manager, e-mail and password), verification, reset, consent, login screens |
| `core.store` | DataStore preferences, 30 s TTL cache |
| `core.markdown` | Markdown parser and Compose renderer (no WebView) |
| `core.session` | start sequence: whoami, consent gate, account state |
| `nav` | single activity, navigation graph (`Routes`, `ScreenRegistry`), modal drawer, top bar |
| `chat`, `pages`, `settings`, `media`, `link` | one screen file per route; each is a stub until its module fills it in |

`AppContainer` is the whole dependency graph (no DI framework); screens get it with `LocalAppContainer.current`.

## Firebase (what the owner has to do)

`google-services.json` is NOT in the repository. Without it the app builds and runs, and sign-in says "Sign-in is not configured in this build".
To turn sign-in on:

1. In the Firebase console add an Android app with package `org.zohara.zoai` to the ZO-AI project and add the SHA-1 and SHA-256 of the signing key
   (debug key for testing, the release key for the store build).
2. Download `google-services.json` into `apps/android-native/app/` (git-ignored). The google-services plugin is then applied automatically.
3. Put the project's Web client id in `apps/android-native/local.properties` as `ZO_WEB_CLIENT_ID=...` (or the environment variable); Google
   sign-in needs it for Credential Manager.

Push (FCM) is not part of the foundation.

## Server

Default `https://zo-ai-server.onrender.com`; Settings > Advanced overrides it (https only; plain http only for localhost in debug builds).
No secrets, keys or tokens are written to the log.
