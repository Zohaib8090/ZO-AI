# Notifications, the installable web app and background push

Written 2026-11-12 for the web version of ZO-AI (`crates/zo-server`). Three layers, each one optional for the person:

1. **Installable app (PWA).** The web page has a manifest, icons and a service worker, so Chrome, Edge, Safari (iOS: Share, Add to Home Screen) and Firefox for Android can install it. Settings > *Install and notifications* has the *Install app* button (it appears when the browser offers installation) and the iOS hint.
2. **Notifications from the open page** (tab in the background). Local to the browser: nothing is sent to any server for this.
3. **Background push** (tab or browser closed). The server finishes the answer and sends a Web Push (or an FCM message for the Android app).

## What is sent, to whom

| Thing | Goes to | Content |
|---|---|---|
| Layer 2 notice | nobody (made in the browser by the service worker) | "Your answer is ready", or the chat title if the person switched *Show the chat title* on |
| Web Push (layer 3) | the push service the browser itself named when it subscribed: Google (`fcm.googleapis.com`), Mozilla (`updates.push.services.mozilla.com`), Apple (`*.push.apple.com`, `web.push.apple.com`), Microsoft (`*.notify.windows.com`) | an encrypted record (RFC 8291, aes128gcm; the push service cannot read it). Plain content: `{"v":1,"type":"answer","chat":"<chat id>"}` and, only for a device whose owner switched the title on, `"title"` cut to 60 characters |
| FCM (Android app) | `fcm.googleapis.com/v1/projects/<project>/messages:send` with the Firebase service account the server already has (scope `firebase.messaging`) | notification "ZO-AI: Your answer is ready", data `chat` (and `title` under the same rule). FCM is not end-to-end encrypted |
| Registration | this server | the browser's push address and two keys (needed to encrypt for it), a label such as "Chrome on Linux", the title choice. Kept in Firestore `users/<tenant>/meta/push`, deleted with the account (`delete` wipes the whole `users/<tenant>` tree) |

Never sent: the question, the answer, tool output, the API key, the e-mail address. The chat id is a random-looking number (`c<time>`), not content.

The browser stores its choices in `localStorage["zo-notify"]` (which toggles are on, the id of its registration on the server, and which account it belongs to). A browser that was registered for one account and is used by another is cut loose (its push subscription is dropped) so one person's notices never reach the next person's session.

## Keys and the server

* **VAPID key pair** (RFC 8292): made once, stored in Firestore `server/vapid`, the private part encrypted with AES-256-GCM under a key derived from `ZO_SERVER_SECRET`. A redeploy keeps it (a new key would silently end every subscription). If `ZO_SERVER_SECRET` changes the stored key cannot be opened: the server does **not** make a new one (that would end all subscriptions without anybody noticing) and says so in the log and in `push_config` (empty `vapid_public_key`). Restore the old secret, or delete the document `server/vapid` (every device must then subscribe again). Without `ZO_SERVER_SECRET` there is no background push at all.
* The VAPID token's `sub` is `mailto:<ZO_CONTACT_EMAIL>` (else `ZO_PUBLIC_URL`): push services use it to reach the operator.
* Crypto is the pure-Rust RustCrypto set (`p256`, `hkdf`, `aes-gcm`, `sha2`) and `ureq`: no OpenSSL, no system packages, the Dockerfile is unchanged.
* The FCM token for the service account is minted like the Firestore one (`firestore.rs`, another scope) and cached.

## Account API (POST, Firebase-authenticated, a signed-in person only; a device credential gets 403)

* `push_config` -> `{vapid_public_key, fcm}` (`fcm` is true when the server can send to the Android app).
* `push_subscribe` `{kind:"webpush", subscription:{endpoint, keys:{p256dh, auth}}, label?, show_title?}` or `{kind:"fcm", token, label?, show_title?}` -> `{id}`. Registering the same endpoint or token again replaces its entry (and keeps its id).
* `push_unsubscribe` `{id}` -> `{ok}` (404 for an id that is not the caller's).
* `push_list` -> `{ok, subscriptions:[{id, kind, label, created}]}` (never the addresses or keys).

## Limits and protections

* At most **5 devices** per account (409 after that).
* **Endpoint allow-list** (SSRF): https only, host from the list above, no port but 443, no user name, no IP number, at most 1024 characters, no control characters; checked at registration and again at every send. Redirects are never followed. Keys must be a valid P-256 point (65 bytes) and a 16-byte auth secret; label at most 60 characters; FCM tokens 20 to 4096 characters of `[A-Za-z0-9_:.-]`.
* **30 pushes per hour per account** (in memory, per process); more are dropped and logged.
* **404 and 410** from a push service (and FCM `UNREGISTERED`) remove the subscription at once. Other failures (5xx, network) are logged and the subscription stays.
* Payloads are generic; a chat id that is not `[A-Za-z0-9_-]{1,64}` is dropped; a title is stripped of control characters and cut to 60 characters (with an ellipsis).
* The TTL of a push is one hour (a push that cannot be delivered within it is dropped by the push service).

## keep_running (the answer survives a closed tab)

`POST /api/send {"text":..., "keep_running": true}` (the web app adds it whenever background push is switched on in that browser). The server honours it only if the account has at least one registered device; otherwise the field is dropped and a closed connection stops the answer as before.

* Browser gone, answer not finished: the connection to the service stays open, the answer is completed and the chat is saved (`zo-core` `service.rs`: the reader thread does not cancel when `keep_running` was asked on a multi-user server). The server tells the service `{"op":"detached"}` so that an approval that is asked meanwhile is refused at once instead of waiting 180 seconds (a restricted account has no approvals anyway).
* The **permits stay held until the answer ends** (the 2 answers per person and `ZO_MAX_RUNS` limits count it). It must finish within the existing answer limit: a restricted account's limit is 120 seconds in the service; the server cuts the connection after 130 seconds as a safety net (admins: 30 minutes). `/api/reply` with `stop` still works.
* When it ends and the browser is **gone**, one push is sent (only if the answer produced text and was not stopped). If the browser stayed connected until the end, **no push** is sent: the person saw it.
* One caveat that existed before: the server notices a closed browser only when it writes the next line, so a model that stays silent for a while (no streamed text) is cancelled or detached a little late.

## Web app behaviour (settings, notifications)

* Permission is asked only from the button, with an explanation. *Answer finished while this tab is in the background* is on after permission; *ZO-AI needs my approval* (admins only), *Show the chat title* (off by default), *Sound* (system sound; `silent` when off), *Send a test notification*.
* No notice while the tab is visible. The tab title shows `(1) ZO-AI` and the icon gets a dot while an answer is unseen; both reset when the tab is visible or focused again (open notifications are closed too).
* Notices go through the service worker (`registration.showNotification`), with a fallback to `new Notification`. Clicking one focuses an open ZO-AI window and opens that chat (message `zo-open-chat`), or opens `/?chat=<id>`. The same tag per chat de-duplicates.
* The service worker (`/sw.js`) caches only the page shell (`/`, manifest, icons), stale-while-revalidate, and **never** `/api/*`. Its version is a hash of the page, made by the server; a new page means a new worker, which waits, and the app shows *Update available - Reload*. Headers: `Content-Type: text/javascript`, `Service-Worker-Allowed: /`, `Cache-Control: no-cache` (also for the manifest). The CSP has `worker-src 'self'` and `manifest-src 'self'`.
* In the Android app (Capacitor) the web push parts are hidden; it registers with kind `fcm`.

## How to test

* `cargo test -p zo-server -p zo-core` covers: the RFC 8291 example byte for byte, a receiver that decrypts what is sent, the RFC 8292 example token, the allow-list, limits, expiry, rate limit, the VAPID key storage, `keep_running` over the real HTTP server and the service (answer finishes after the disconnect, chat saved, permits held and freed, no push when the browser stayed, caps unchanged), and the headers of the app files. A fake transport records the push requests, so nothing leaves the machine; the allow-list can be extended in tests only (`Allow::for_test`).
* By hand in a browser: start `ZO_SERVER_NO_AUTH=1 PORT=<n> zo-server` (use your own `HOME`, `XDG_*` and `ZO_TENANT_ROOT` folders), open `http://localhost:<n>/?noauth` in Chrome. DevTools > Application: manifest, service worker, installability (no errors). `Page.getInstallabilityErrors` over the DevTools protocol returns none. In Settings > *Install and notifications* grant permission, press the test button, put the tab in the background and send a message.
* Real push needs a real browser and a public https server; `localhost` can subscribe to Google's push service from a normal Chrome, but a headless Chrome cannot.
