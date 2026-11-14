# Zohara Agent on Google Play: every step

Status: the app is built and the Play material is prepared; nothing has been uploaded and no Play account is set up. This guide is written so that the owner can follow it from top to bottom.
Marked **OWNER** = only the account holder can do it (identity, payment, legal declarations, passwords). Everything else is already in the repository.
Files: `apps/android-native/play/` (listing text, graphics, data safety, permissions, content rating, screenshot plan), `docs/RELEASING.md` (how the signed bundle is built), `docs/ANDROID.md` (the app).

Google changes the console and its rules often. The numbers below were right when this was written (2026-11-14); check each against the console and Play's help pages before you rely on it.

## 0. What is ready, what is not

| Ready | Not ready (needs a person) |
|---|---|
| Release build: R8 minify + resource shrinking, `allowBackup=false`, no cleartext, `targetSdk 36`, `minSdk 26`, 64-bit (arm64-v8a and x86_64 libraries, 16 KB aligned), adaptive + monochrome (themed) icon, versionCode/versionName from the tag | The Play developer account (**OWNER**), the release (upload) keystore, the Firebase Android app and `google-services.json` |
| `bundleRelease` builds a signed `.aab` (checked with a throw-away key) | Real screenshots from a device, a test account for "App access" |
| Store text, 512 icon, 1024x500 feature graphic (`play/listing/en-US`) | Review of the privacy notice by a lawyer (it is a template) |
| Privacy policy page `/privacy` and account-deletion page `/delete-account` on the server, link from the app | **In-app report/flag for AI answers (Play AI-Generated Content policy): not built, see section 8** |
| Data safety, permissions and content rating answers (`play/*.md`) | Closed test with 12 testers for 14 days (new personal accounts) |

## 1. The developer account (OWNER)

1. Go to play.google.com/console and sign up with the Google account that will own the app. Account type: **Personal** (you as an individual) or **Organization** (a registered company; needs a D-U-N-S number). Personal is simpler but triggers the testing requirement in section 6.
2. Pay the one-time registration fee (25 USD).
3. Verify your identity: legal name, address, a government ID, a phone and a contact e-mail; for a personal account also a device check with the Play Console app. Google reviews it (days).
4. Contact details: the address and e-mail you enter can be shown publicly on the store page (for developers who sell to EU users the "trader" rules apply). Use a business address or a P.O. box if you do not want a home address shown, and a role e-mail such as the `ZO_CONTACT_EMAIL` address.
5. Turn on 2-step verification on the Google account. Losing this account means losing the app.

## 2. Keys and Firebase (OWNER, once)

1. Make the upload keystore as in `docs/RELEASING.md` ("Make the release keystore"), back it up (password manager and an offline copy). Never commit it.
2. Firebase console, project `zo-ai-144`: add the Android app `org.zohara.agent`, add the SHA-1 and SHA-256 of the upload key, download `google-services.json` into `apps/android-native/app/` (git-ignored) and note the Web client id for Google sign-in. Details: `apps/android-native/README.md`.
3. After the first upload (section 5) Play shows the **app signing key** fingerprints (Release > Setup > App signing); add its SHA-1 and SHA-256 to the same Firebase app as well, or Google sign-in does not work in the Play build.
4. GitHub secrets for the workflow: `docs/RELEASING.md` ("Secrets to add").

### Play App Signing, explained

Play App Signing is mandatory for new apps (AAB uploads). You sign the bundle with the **upload key** (your keystore). Google re-signs the APKs it delivers with the **app signing key**, which Google keeps safe. Consequences:

* If you lose or leak the upload key you can ask Google to reset it (a support request); the app is not lost. That is why Play App Signing is safer than keeping one key forever.
* Users of the Play build get Google's signature; the APK from GitHub carries the upload key's signature. Android will not update one with the other (uninstall first). Firebase needs both fingerprints.
* Option: when creating the app you can instead upload your own app-signing key (exported with Google's PEPK tool) so that the GitHub APK and Play use the same signature. Decide this **before the first upload**; it cannot be changed afterwards.

## 3. Create the app in the console (OWNER)

Play Console > Create app:

| Field | Value |
|---|---|
| App name | Zohara Agent (`play/listing/en-US/title.txt`) |
| Default language | English (United States) |
| App or game | App |
| Free or paid | Free (this cannot be changed to paid later) |
| Declarations | tick the developer programme policies and US export laws |

## 4. App content and store listing (OWNER types it, text is ready)

Dashboard > "Set up your app". Each item below has its answers in the repository.

| Console section | Answer / source |
|---|---|
| Privacy policy | `https://zohara-agent.duckdns.org/privacy` (check it opens in a private window without signing in) |
| App access | Choose "All or some functionality is restricted" and give a **test account** (an e-mail + password account you create for the reviewers, verified, with a model API key saved, or with a note that chats need the reviewer's own key). Instructions must say how to sign in and where to add a key. Without this review is rejected |
| Ads | No, the app contains no ads |
| Content rating | `play/content-rating.md` |
| Target audience | 18 and over (`play/data-safety.md`, last section) |
| Data safety | `play/data-safety.md` |
| Government apps, financial features, health | No |
| News app | No |
| Advertising ID | The app does not use it: answer No (the manifest has no `AD_ID` permission) |
| Account deletion | in the app: Settings > Account; web URL `https://zohara-agent.duckdns.org/delete-account` (entered in the Data safety form) |
| Store settings | Category: Productivity (or Tools). Contact e-mail (public), website `https://zohara-agent.duckdns.org`, optional phone |
| Main store listing | paste `title.txt`, `short_description.txt`, `full_description.txt`; upload `images/icon.png`, `images/featureGraphic.png` and 2 to 8 phone screenshots (`play/screenshots.md`) |

Check the files first: `bash apps/android-native/play/tools/check-listing.sh`.

Permissions: no declaration forms are needed (`play/permissions.md`).

## 5. Build and upload the bundle

1. Tag a pre-release, for example `v0.1.0-beta.1` (`docs/RELEASING.md`). The workflow builds `zohara-agent-0.1.0-beta.1.aab`, signed with the upload key (the file name has `-unsigned` when the secrets are missing: do not upload that one).
2. Play Console > Testing > **Internal testing** > Create new release. Play App Signing: accept the default (or the custom key from section 2).
3. Upload the `.aab`. Release name: the version. Release notes: `whats_new.txt` (inside `<en-US>...</en-US>` tags if the console asks for language tags).
4. Review and roll out. Internal testing is available in minutes and does not need review for up to 100 testers.
5. Testers: Testing > Internal testing > Testers: create an e-mail list. Each tester opens the opt-in link on a device signed in with that Google account and installs from Play.
6. Every later upload needs a higher `versionCode`: the tag formula guarantees it.
7. Upload the `mapping.txt` from the release with the bundle (it is included in the bundle's metadata when built with R8; the separate file is the backup) so crash reports in the console are readable.

## 6. Testing tracks plan

| Step | Track | Who | Gate |
|---|---|---|---|
| 1 | **Internal testing** | the owner and up to 100 people on the list | the build starts, sign-in (Google and e-mail) works, a chat streams, notifications arrive |
| 2 | **Closed testing** | **at least 12 testers**, opted in for **14 days in a row** | required for a **new personal developer account** before production (rule in force since late 2024; check the current numbers in the console). Organization accounts are not subject to it. Testers must stay opted in; keep a few spare |
| 3 | **Production** | everyone | apply for production access in the console after the 14 days (Dashboard shows "Apply for production"); Google answers in about a week; then a normal review of the release |
| (opt.) | **Open testing** | anyone with the link | public beta; skip unless wanted |

Practical points for the closed test: find testers early (friends, the Zohara OS community; they need a Google account and an Android phone), ask them to actually open the app on some of the 14 days and to send feedback; Google asks you in the production application what you tested and what you changed. Use a pre-release tag (`-beta.N`) for these builds. Use a staged rollout (for example 20 % then 100 %) in production.

## 7. Pre-launch checklist

Tick every line before pressing "Apply for production".

- [ ] Developer account verified; 2-step verification on
- [ ] Upload keystore backed up in two places; fingerprints in Firebase (upload key and Play app-signing key)
- [ ] `google-services.json` and `ZO_WEB_CLIENT_ID` in the release build (otherwise no sign-in); Google and e-mail sign-in tried on the Play build
- [ ] Release build opened on at least two real phones (a small and a large screen, Android 8 and a recent one) and one tablet or the tablet emulator: first run, sign-in, send a message, a question card, an approval card, notification permission, microphone dictation, share into the app, deep link `zohara-agent://chat/<id>`, rotation, dark and light, large text
- [ ] No debug text, no test server address, no secrets in the APK: `strings` the release APK for keys; the server address is `https://zohara-agent.duckdns.org`
- [ ] `/privacy` and `/delete-account` open without signing in and show the real contact address (server env `ZO_CONTACT_EMAIL`); `docs/PRIVACY.md` mentions everything in the Data safety form (the Google display name is listed in the form but not yet in the notice)
- [ ] Privacy notice and terms reviewed by someone qualified
- [ ] Data safety form saved and matching the privacy notice; content rating done; target audience 18+
- [ ] Test account for "App access" works with a fresh install
- [ ] Real screenshots (2 to 8), feature graphic and icon uploaded; the screenshots show "Zohara Agent", not an old name
- [ ] Short and full description match what the app really does today
- [ ] **In-app report/flag for AI answers exists** (section 8)
- [ ] Pre-launch report in the console (automatic tests on Firebase Test Lab devices after each upload to a closed or open track) read; crashes fixed
- [ ] Target API: `targetSdk` meets the current Play requirement (36 now; Play requires the newest or one below within a year of each Android release: re-check each August) and 16 KB page sizes are supported (checked: the native libraries are aligned to 16 KB)
- [ ] The server can take the load: Render free plan sleeps and limits users (`ZO_MAX_USERS`); decide on the paid plan before a public launch (docs/PUBLIC.md, docs/DEPLOY.md)
- [ ] A support e-mail inbox that is read

## 8. Known blocker: reporting AI answers

Play's AI-Generated Content policy asks apps that generate content with AI to give people an in-app way to report or flag offensive or harmful output and to act on it. The Android app has no such control yet. It is not done in this branch because it needs a server operation to receive reports (or an agreed e-mail address in the app) and a design decision. Suggested: a "Report this answer" item in each answer's menu: it asks for an optional note, calls a new `report_answer` op that stores `{chat, message index, note, time}` for the operator (admin page), and tells the person it was received; the operator reviews reports regularly. Build it before applying for production access. See `play/content-rating.md`.

## 9. After release

* Staged rollout: Production > Create new release > Countries > "Staged rollout" percentage; raise it when crash-free rates look good (Android vitals).
* Updates: tag a new version, download the `.aab` from the GitHub Release, upload it to the track (internal -> closed -> production for risky ones, straight to production with a staged rollout for small fixes). Write the new "What's new".
* Reply to reviews; watch Android vitals (crashes, ANRs) and the pre-launch report.
* Each year: raise `targetSdk` when Play requires it; re-check the Data safety form after any feature that collects or shares something new (a new SDK counts).
* Keep `docs/PRIVACY.md`, `/delete-account`, `play/data-safety.md` and the Play form in step: change all four together.

## 10. Upload by hand (no Fastlane)

The whole flow needs only a browser: build the `.aab` (GitHub Release or locally: `docs/RELEASING.md`), open Play Console > the app > Testing or Production > Create new release > upload the `.aab` > release notes > Next > Save > Send for review/Roll out. The listing text and images are copied from `apps/android-native/play/listing/en-US/` into Grow > Store presence > Main store listing. The directory follows fastlane `supply`'s layout, so that tool can be adopted later with no restructuring, but it is not needed and not set up (it would need a service-account key as one more secret).
