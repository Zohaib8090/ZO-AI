# ZO-AI Android store app

A thin Capacitor wrapper in `apps/android-web/` that loads the website (`server.url`), so the app updates whenever the site does.
Package `org.zohara.zoai`, name ZO-AI. It is separate from Zohara Link (`~/Documents/zohara-link`, a different app and package).

What the app adds over the browser (all in the `/* ---- native bridge (Capacitor) ---- */` block of
`crates/zo-server/web/index.html`, inert in a normal browser):

- native Google sign-in (account picker instead of the pop-up): `FirebaseAuthentication.signInWithGoogle({skipNativeAuth:true})`, then `signInWithCredential(auth, GoogleAuthProvider.credential(idToken))`;
- native speech recognition on the page's mic button (`#mic`; also `window.zoaiNativeMic.toggle(button, textarea)`); RECORD_AUDIO is explained and requested the first time;
- push notifications (FCM): after sign-in, asks once for POST_NOTIFICATIONS (Android 13+), registers the token with `/api/account/push_subscribe {kind:"fcm"}`, removes it on sign-out; tapping a notification opens the chat (`data.chat`), a same-site path (`data.url`) or the chat list; notification channel `answers`;
- share into the app (text, links, images, PDF/zip/json files from other apps) opens a new message; the native receiver is `ZoShare.java`, the page calls `window.zoaiShare({text, files})` if it exists, otherwise `addAttachmentFromFile(file)` plus the text box;
- Android back button: back in page history, then minimize; external links open in the system browser tab; status/navigation bars follow the theme; the "Install app" hint is hidden.

Versions: Capacitor 7.6 (Java sources are compiled at the running JDK's level when it is older than 21, see `android/build.gradle`), AGP 8.7.2, Gradle 8.11.1, compileSdk/targetSdk 35, minSdk 23. Capacitor 8 needs compileSdk 36 and Java 21, which this machine does not have (only the 36.1 platform and JDK 17/27), so 7.x is used; move to 8 when building on a machine with JDK 21 and the 36 platform.

## Owner steps

1. **Firebase app and google-services.json.** Firebase console > Project settings (project `zo-ai-144`) > Add app > Android. Package name `org.zohara.zoai`. Add the SHA-1 and SHA-256 of the debug key and of the release key:
   ```
   keytool -list -v -keystore ~/.android/debug.keystore -alias androiddebugkey -storepass android -keypass android | grep -E "SHA1|SHA256"
   keytool -list -v -keystore ~/zo-ai-release.keystore -alias zoai | grep -E "SHA1|SHA256"
   ```
   Download `google-services.json` into `apps/android-web/android/app/`. It is not secret (it ships inside the APK) but it names the owner's Firebase project, so it is git-ignored by default; remove that line from `apps/android-web/.gitignore` if you decide to commit it. In the Firebase console enable nothing else (Google sign-in is already on for the website). If you publish through Play, also add the SHA-1/256 of the Play app-signing key (Play Console > App integrity) to the same Firebase app.
   For push, the Firebase project also needs a service account key on the server (the zo-server push sender; that is a server setting, not part of this app).
2. **Release keystore.** Create once, never commit, back it up (password manager plus an offline copy: losing it means no more updates under the same key unless Play App Signing is used):
   ```
   keytool -genkeypair -v -keystore ~/zo-ai-release.keystore -alias zoai -keyalg RSA -keysize 4096 -validity 10000
   ```
   Signing config goes in `~/.gradle/gradle.properties` (not in the repo):
   ```
   ZOAI_KEYSTORE=/home/you/zo-ai-release.keystore
   ZOAI_KEYSTORE_PASSWORD=...
   ZOAI_KEY_ALIAS=zoai
   ZOAI_KEY_PASSWORD=...
   ```
   Optional: `-PZOAI_VERSION_CODE=2 -PZOAI_VERSION_NAME=1.0.1` (the Play Store needs a higher code for every upload).
3. **Build.**
   ```
   cd apps/android-web
   npm install
   npx cap sync android              # ZO_WEB_URL=https://your-server npx cap sync android to point elsewhere
   export ANDROID_HOME=$HOME/Android/Sdk
   cd android && ./gradlew assembleDebug     # app/build/outputs/apk/debug/app-debug.apk
   ./gradlew bundleRelease                   # app/build/outputs/bundle/release/app-release.aab (signed if step 2 is set)
   ```
   Icons and splash come from `crates/zo-ai/data/zo-ai.svg`; re-generate with `npm run assets` after a logo change.
4. **Play Console (short).** Create the app, upload the `.aab` to Internal testing first, add testers. The store listing needs the privacy policy URL `<site>/privacy`, the Data safety form (account e-mail, chat content sent to the server, optional microphone audio for dictation handled by the phone's recognizer), content rating, and target audience. Also fill "App access" with a test account.
5. **Zohara Link** is a different app (Kotlin, own package and signing key). This app does not talk to it.

## Without google-services.json

The debug APK builds and runs. E-mail sign-in works. The Google button shows "Google sign-in is not set up in this build of the app yet" and push stays off (the page checks `ZoShare.info().firebase`). With the file present, the Gradle `google-services` plugin is applied automatically.

## Not done / not verified

- Never run on a device or emulator (no adb, by the owner's rule). The bridge flows were tested only in a browser with fake Capacitor plugin stubs; real Google account, real microphone/recognizer, real FCM delivery and the share intents are untested.
- The server side of push (`push_subscribe`, sending through FCM) is built by another workflow; the app only registers.
- The page needs the `#mic` button from the voice work and `addAttachmentFromFile` / `window.zoaiShare` from the attachment work; without them the native hooks do nothing (share falls back to filling the text box).
- No release signing key, no Play listing, no screenshots, no `assetlinks.json`; the app does not handle `https://` deep links, only notification taps and share.
- Notification small icon and adaptive icon are generated, not hand-tuned; no tablet/landscape polish.
