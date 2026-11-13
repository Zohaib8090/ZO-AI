# Zohara Agent for Android (native)

Kotlin + Jetpack Compose, no WebView. Package `org.zohara.agent`, minSdk 26, targetSdk 36. It replaces the Capacitor wrapper `apps/android-web`
(legacy, kept as a reference). What the app has to contain is the parity matrix `docs/ANDROID-NATIVE-PARITY.md` (the contract: every row is
implemented AND verified by a named test before it is ticked; `python3 -I apps/android-native/docs/check-parity.py` checks the file). The server API
is `apps/android-native/docs/API.md`; the per-module notes are `docs/CHAT.md`, `docs/PAGES.md`, `docs/MEDIA-LINK.md`.

Renamed from "ZO-AI" (package `org.zohara.zoai`), then from "Zohara Chat" (package `org.zohara.chat`, never registered in Firebase and never released). The new package is a different app to Android: it installs next to the old one, nothing is migrated (Android
cannot move app data between packages; chats are on the server), and the Firebase app and `google-services.json` must be made again for `org.zohara.agent`.
Internal names that never showed the old product name (DataStore `zo_prefs`, SharedPreferences `zo_*`, notification channels `answers` and `needs_you`) are unchanged.

## Build, test, run

Needs JDK 17 and the Android SDK (`sdk.dir` in `local.properties`, git-ignored). Keep Gradle caches in the default `~/.gradle` (not on a RAM disk).

    ./gradlew assembleDebug                 # app/build/outputs/apk/debug/app-debug.apk
    ./gradlew testDebugUnitTest             # JVM tests: JUnit, MockWebServer, Robolectric, Compose UI tests (no device)
    ./gradlew lintDebug                     # Android lint (app/build/reports/lint-results-debug.html)
    ./gradlew assembleRelease               # minified + resource-shrunk; unsigned unless the signing values below are set
    ./gradlew recordRoborazziDebug          # rewrite the screenshots in app/src/test/screenshots (look at them!)
    ./gradlew verifyRoborazziDebug          # compare against them

Release signing is read from `~/.gradle/gradle.properties` or the environment, never from the repository:

    ZO_STORE_FILE=/home/you/zo-ai-release.keystore
    ZO_STORE_PASSWORD=...
    ZO_KEY_ALIAS=zoai
    ZO_KEY_PASSWORD=...            # optional, defaults to the store password

`ZO_VERSION_CODE` (environment) sets `versionCode` and the patch number of `versionName` (`0.1.<code>`); Play needs a higher code for every upload.
Release builds use R8 with the rules in `app/proguard-rules.pro` (kotlinx.serialization serializers, Firebase, Credential Manager, PdfBox, OkHttp).
`debuggable` is only true in debug. The debug build also allows plain `http://` to `localhost`, `127.0.0.1` and `10.0.2.2` (a development server,
`src/debug/res/xml/network_security_config_debug.xml`); the release build allows no cleartext at all.

## Architecture (`app/src/main/java/org/zohara/agent`)

One activity (`MainActivity`), everything else is Compose. `AppContainer` is the whole dependency graph (no DI framework): HTTP client, `ZoApi`,
DataStore preferences, caches, `Session`, `AuthRepository`. Screens get it with `LocalAppContainer.current` and navigate with `LocalNav.current`.

| Package | What |
|---|---|
| `core.design` | colour tokens (dark and light, from `crates/zohara-agent/src/theme.rs`), type (Inter, Source Serif 4 in `res/font`), shapes, the logo, icons, components (`ZoSwitch`, `SettingsRow`, `ZoBottomSheet`, `ZoTextField`, ...) |
| `core.net` | OkHttp client with the Firebase ID token interceptor, `ZoApi` (one function per op), NDJSON `send` as a `Flow<SendEvent>`, error mapping |
| `core.auth` | Firebase sign-in (Google through Credential Manager, e-mail and password), verification, reset, consent, the login screens |
| `core.store`, `core.session` | DataStore preferences and a 30 s TTL cache; the start sequence (`whoami`, consent gate, account state, global 401/403/410 handling) |
| `core.markdown` | Markdown parser and Compose renderer |
| `nav` | `Routes` (every screen), `ScreenRegistry` (route -> screen, checked by a test), the modal drawer (180 ms slide), the top bar, deep links (`DeepLinks`) |
| `chat` | home, thread, streaming, tool groups, approval and question cards, composer and its pills, context panel; fills `ChatHooks` slots |
| `pages` | Chats, Search, Projects, Artifacts, Scheduled, Activity, Terminal, the sidebar lists, "Running now" |
| `settings` | every Settings page, the model menu and sheet, profiles, pins, providers, memory, personality, MCP, machines, account, notifications |
| `media`, `link` | attachments, paste, camera, share-in, voice in, read aloud, notifications and push (FCM), Run on and My computers; plugged into the chat composer by `ChatPlug.install` |

Navigation: `ZoApp` (theme, sign-in gate, consent gate) -> `MainShell` (drawer + `NavHost`). The back stack is saved by `rememberNavController`; each chat
keeps its draft in saved state. Back closes a sheet, then the drawer, then goes up, then minimizes the app (it never ends a running answer). `zohara-agent://chat/<id>`
and the `chat` extra of a notification open a chat, `zohara-agent://new` is the launcher shortcut "New chat", `zohara-agent://device` opens "Approve a sign-in" (the older `zohara-chat://` and `zoai://` schemes are still accepted).
Edge to edge: the window follows the theme (status and navigation bars), the content is padded by the keyboard or the navigation bar, whichever is taller.

### Tests

`app/src/test` (about 740 tests, no device): per-module logic and API tests against MockWebServer with the answers captured live, Compose UI tests with
Robolectric, and `app/WholeAppTest` + `app/ScaleAndSmallScreenTest`, which run the real `ZoApp` (all modules together) against a mock server: start and
sign-in gate, drawer order and lists, navigation and back, deep links, saved state after process death, live theme switch, restricted accounts, the model
pill "Model and API key..." entry, a TalkBack-name audit of every clickable control on the main screens, and 320 dp wide with the font at 1.3x and 2.0x.
Screenshots (Roborazzi, both themes) are in `app/src/test/screenshots`; the names say module and state.

## Firebase (what the owner has to do)

`google-services.json` is NOT in the repository (git-ignored). Without it the app builds and runs, e-mail sign-in says "Sign-in is not configured in this
build", Google sign-in explains itself and push stays off. To turn it on:

1. Firebase console > project `zo-ai-144` > Project settings > Add app > Android, package `org.zohara.agent`.
2. Add the SHA-1 and SHA-256 of every key the app is signed with:

       keytool -list -v -keystore ~/.android/debug.keystore -alias androiddebugkey -storepass android -keypass android | grep -E "SHA1|SHA256"
       keytool -list -v -keystore ~/zo-ai-release.keystore -alias zoai | grep -E "SHA1|SHA256"

   With Play App Signing also add the SHA-1/SHA-256 of the Play app-signing key (Play Console > App integrity).
3. Download `google-services.json` into `apps/android-native/app/`. The google-services plugin is then applied automatically.
4. Authentication > Sign-in method: Google and E-mail/Password (both are already on for the website).
5. Put the project's **Web client id** (Authentication > Google > Web SDK configuration) in `apps/android-native/local.properties` as `ZO_WEB_CLIENT_ID=...`
   (or the environment variable); Credential Manager needs it for "Continue with Google".
6. Push (FCM): Cloud Messaging is on by default in the project; the sender is the zo-server (a service account key on the server, a server setting, not part
   of the app). The app registers its token with `push_subscribe {kind:"fcm"}` after the person agrees, and removes it on sign-out.

Release keystore (once, never commit, back it up):

    keytool -genkeypair -v -keystore ~/zo-ai-release.keystore -alias zoai -keyalg RSA -keysize 4096 -validity 10000

Play Console, short: create the app, upload the signed `.aab` (`./gradlew bundleRelease`) or APK to Internal testing, add testers. The listing needs the
privacy policy URL `<server>/privacy`, the Data safety form (account e-mail, chat content sent to the server, optional microphone audio for dictation handled by
the phone's recognizer), the content rating and a test account under "App access".

## Server

Default `https://zohara-agent.duckdns.org`; Settings > Account > Advanced overrides it (https only; plain http only for localhost and the emulator host in
debug builds). No secrets, keys, tokens or message text are written to the log.

## Not verified (no device was used)

Everything above was tested on the JVM. Never run on a real phone or emulator (the owner's rule): real TalkBack, a real hardware keyboard, the real share sheet
and Custom Tabs, the system file picker and camera, the POST_NOTIFICATIONS dialog, a real microphone and speech recognizer, real Google and Firebase sign-in,
real FCM delivery and notification buttons, rotation and process death on a real device, and a live server (the `settings_get`, `settings_set`, `model_test`
and `profile_update` ops are coded against docs/API.md and docs/SYNC.md). The remaining open rows of the matrix are listed in the commit message and in
`docs/ANDROID.md`.
