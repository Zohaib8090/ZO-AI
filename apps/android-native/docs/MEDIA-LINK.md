# Media and link module (org.zohara.chat.media, org.zohara.chat.link)

Attachments, paste, camera, share-in, voice in, read aloud, notifications and push, and the Run on control with linked computers.
Matrix rows: module `media` and `link`, plus the NOTIF-* rows and INTER-15, BG-05 of the notifications work.

## How it plugs into the chat

`ZoApp` calls `ChatPlug.install(container)` once (inside the composition, so the container is there). That fills the chat's slots in `ChatHooks`:

| Slot | What the media-link module draws |
|---|---|
| `composerHost` | wraps the composer card; makes the one `ComposerMedia` (attachments, dictation, sheets, toasts) and shares it with the slots below through `LocalComposerCtx` |
| `attachmentChips` | the strip of chips (pictures, files, busy, links, addresses found in the text) and the Run on note |
| `attachButton` | the "+" button (menu: Upload images or files, Take a photo, Add a link, Paste from clipboard) |
| `micButton` | the microphone; the words go into the box as they are heard |
| `fieldModifier`, `textEdited` | keyboard pictures and paste, drag and drop, Esc and Ctrl+Shift+M; a pasted address becomes a chip; typing stops dictation; typed addresses get previews |
| `runOnPill` | the Run on pill with the presence dot, and the 20 s poll while the screen is visible |
| `readAloudButton`, `readAloud` | the speaker / stop button under an answer |
| `prefill` | text shared in from another app |

`ChatEnv` (implemented by `AppChatEnv`) asks the module for: `takeForSend` (attachments and the link text), `attachmentsBusy` (COMP-19), `beforeSend`
(COMP-20: look at the computer first; the text stays in the box when it is not reachable), `answerFinished` (NOTIF-01 notification when the app is in the
background, and VOICE-11 read aloud), `needsYou` / `needsYouDone` (INTER-12 notifications), `keepRunning` (NOTIF-05).
The model and the mode sheets on a computer are the link module's (`DesktopModelSheet`, `ModeRows`): `desktop_op` with the whitelisted ops only.
Settings uses `MediaModule.of(container).push`, `.choices`, `.setClosedPush` (Notifications page) and the routes `SETTINGS_COMPUTERS` (My computers).

## Attachments (media/)

* `AttachmentsController`: one worker reads files one after another (ATT-14); limits 4 pictures, 6 files, 400 KB of text in all, 9 MB of pictures; refusals are toasts
  (`messages`); `take()` gives the `/api/send` payload (`Attachment.Image`, `Attachment.TextFile`), the bubble facts and the link text; `restore()` puts them back after a failed send.
* `ImageProcessor`: decode honouring EXIF rotation, at most 1600 px, re-encode (drops EXIF), PNG kept for small transparent ones, thumbnail at most 20,000 characters.
* `TextProcessing`: text/code (UTF-8, BOM out, NUL = not text, 200 KB), PDF (PdfBox-Android, 500 pages, `[Page N]` markers; the library is loaded only when a PDF is added),
  ZIP (25 MB, 200 entries, 2 MB unpacked counted from the real bytes, 100 KB an entry, archives inside not opened, text-like only).
* `MediaRules`: kinds, refusals, names, link addresses, the 5 link limit, `safeThumb`.
* `ShareActivity` / `ShareIntake`: SEND and SEND_MULTIPLE for text, images, PDF, ZIP, JSON; files are copied to the cache at once (the sender's grant ends with the activity).
* No storage or camera permission: `OpenMultipleDocuments`, `TakePicture` with a `FileProvider` in the cache.

## Voice (media/)

`VoiceController` over a `SpeechEngine` (Android `SpeechRecognizer` in `AndroidSpeechEngine`): partial results fill the box without duplicates, listens again after each
phrase unless auto-send is on, typing or Back or pause or a new answer cancels, error sentences in `VoiceError`. `RECORD_AUDIO` is asked with the explanation sheet.
`ReadAloud` over `TtsEngine` (`TextToSpeech`): plain words only (`VoiceText.plain`), chunks of at most 200 characters, one reading at a time, Stop on the same button.

## Notifications (media/notify/)

* Channels `answers` (default) and `needs_you` (high) are made by `core.util.NotificationChannels` at start.
* `ZoNotifier`: `answerFinished`, `approval` (Allow / Deny through `ApprovalReceiver` -> `/api/reply`), `question` (Answer opens the chat), `fromPush`.
  Nothing is shown while the app is visible, without the permission, or when that kind is switched off. Text is generic unless "Show details" is on (NOTIF-11).
* `PushRegistrar`: `push_config` then `push_subscribe {kind:"fcm"}` after sign-in, `push_list` at every start, account switch drops the old registration, a new token
  replaces the old one (`ZoMessagingService.onNewToken`), sign-out and the switch off call `push_unsubscribe`. Device label "Android: <model>".
* `NotificationAskOnce`: the one-time explanation after sign-in (INTER-15); "Turn on" asks Android for `POST_NOTIFICATIONS` and switches the closed-app notices on.
* The notification tap opens `zohara-chat://chat/<id>` (the old `zoai://` is still accepted) (DeepLinks).

### Firebase (owner)

`google-services.json` is not in the repo. Download it from the Firebase console for package `org.zohara.chat`, add the SHA-1 and SHA-256 of the signing key, and put it in
`apps/android-native/app/`. The build applies the google-services plugin only when the file exists; without it push and Google sign-in are off (`BuildConfig.FIREBASE_CONFIGURED`).

## Link (link/)

`LinkModule.of(container)`: `repo` (`/api/link/status`, at most 30 computers), `runOn` (the choice, restored at start, back to the cloud when the computer is gone, a look before
sending), `desktop` (`desktop_op`: status, providers, profiles, profile_use, models, select, mode), `claude` (LINK-CC status). Screens: `RunOnPill`, `RunOnSheet`, `RunNote`
("Check again"), `WaitingApprovalRow`, `MyComputersScreen`, `ClaudeCodeCard`, `DesktopModelSheet`.

## Tests

JVM only (no device): MockWebServer for the payloads and ops, fake engines for voice and TTS, Robolectric for the manifest, the share parser, the real composer with the
slots plugged in, and Roborazzi screenshots (`src/test/screenshots/media_*`, `link_*`; record with `./gradlew recordRoborazziDebug`).
Not covered by any test: a real microphone, a real camera, the system pickers, a real Firebase sign-in, real FCM delivery, a real linked computer.
