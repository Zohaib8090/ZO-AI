# Permissions audit

Read from the merged manifest of the release build (`aapt2 dump badging` of the built APK, version 0.1.0-beta.2): 10 permissions. Four are ours, the rest are added by libraries.
None is a dangerous permission except `POST_NOTIFICATIONS` and `RECORD_AUDIO`, which are asked at run time with an explanation. No location, contacts, SMS, call log, storage, camera, accessibility or "all files" permission.

| Permission | Where it comes from | Why | When it is asked |
|---|---|---|---|
| `INTERNET` | our manifest | every call to the Zohara Agent service (HTTPS only; `usesCleartextTraffic=false`, the release build allows no plain http at all) | install time (normal) |
| `ACCESS_NETWORK_STATE` | our manifest | tell "offline" from "server error" and retry when the network is back | install time (normal) |
| `POST_NOTIFICATIONS` | our manifest | notify when a background answer is finished or the agent needs an answer or an approval ("needs you") | at run time on Android 13+, after an explanation on the first-run notifications screen; the app works without it |
| `RECORD_AUDIO` | our manifest | voice dictation on the microphone button (the phone's own speech recognizer turns it into text; the app only ever receives the text) | at run time, only when the person starts dictating, with an explanation (VOICE-08); the app works without it |
| `USE_BIOMETRIC`, `USE_FINGERPRINT` | libraries (Firebase Auth / Google sign-in through Credential Manager) | merged in by the sign-in libraries; this app does not offer a fingerprint login and never calls the biometric prompt | install time (normal) |
| `WAKE_LOCK` | libraries (Firebase Messaging, WorkManager) | keeps the CPU awake for the short time it takes to handle a push message | install time (normal) |
| `com.google.android.c2dm.permission.RECEIVE` | Firebase Cloud Messaging | receive push messages (answers finished, approvals) | install time (normal) |
| `com.google.android.providers.gsf.permission.READ_GSERVICES` | Google Play services libraries | Play services configuration | install time (normal) |
| `org.zohara.agent.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION` | AndroidX (Android 13+ receiver safety) | internal to this app: stops other apps from calling its dynamic receivers | not user visible |

Features: camera and microphone are declared `required="false"`, so devices without them can install the app. There is no `CAMERA` permission: "Take a photo" opens the phone's camera app and the picture comes back through a `FileProvider`.

## Text for the Play Console

* **Sensitive permissions declarations**: none are needed (no SMS, call log, background location, `MANAGE_EXTERNAL_STORAGE`, accessibility, `QUERY_ALL_PACKAGES`, `SYSTEM_ALERT_WINDOW`, exact alarms or full-screen intents).
* **Foreground service permissions**: none are used.
* **Microphone**: if the console asks for a justification: "Voice dictation. The person presses the microphone button; the phone's speech recognizer converts speech to text; only the text is sent. Nothing is recorded in the background."
* **Notifications**: "Tells the person that a background answer is finished or that the agent is waiting for their answer or approval."

## Keep it this way

Before adding a permission, add a row here. A library update that adds a permission shows up in `aapt2 dump badging` of the release APK: check this table after every dependency upgrade.
