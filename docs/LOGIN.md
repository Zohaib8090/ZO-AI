# Signing in (desktop app, terminal client, web)

Written 2026-11-12. Nobody ever copies, pastes or sees a token. A person signs in with a **Google account** or with
**e-mail and password**, and the desktop app and the terminal client `zo` get a **device credential** from the server behind the scenes.
Code: `crates/zo-core/src/login.rs` (the client side), `crates/zo-server/src/loginapi.rs` and `devicelogin.rs` (the server side),
`crates/zo-server/src/auth.rs` (which Firebase sign-ins are accepted), `crates/zo-server/web/index.html` (the web app and its `/device` screen).

## What the person sees

* **Desktop app (Zohara Chat)**: works fully **without any login**. Chats, memory, personalities, tools and the phone all work, and **nothing
  is sent to the sync server** until the person chooses to sign in (Settings > Web sync). Signing in is optional; it only turns on sync with
  the web. After signing in on the desktop and on the web, chats, memory and personalities match (`SYNC.md`).
* **Web app**: needs a sign-in (Google or e-mail and password). A first-time e-mail account must click the link in the verification e-mail.
* **Terminal client `zo`**: `zo login` signs in the same way as the desktop app; `zo logout` signs out. After that `zo --remote` works
  without any token file or environment variable.

## Two ways in

### A. Browser (Google, or e-mail and password in the browser): the device flow

Used for Google sign-in on the desktop and in the terminal, and available for e-mail accounts too.

    device (desktop / zo)                      server                               person, in a browser (any device)
    ---------------------                      ------                               ---------------------------------
    POST /api/auth/device_start  ----------->  makes device_code (secret, 43 chars)
      {name, kind}                             and user_code "ABCD-EFGH"
                               <-----------    {device_code, user_code, verify_url, expires_in 600, interval 3}
    shows:  "Open <verify_url> and type ABCD-EFGH"
    (the device_code is never shown)                                                  opens <public base>/device
                                                                                      signs in (Google or e-mail)
                                                                                      TYPES the code ABCD-EFGH
                                               <-----------  POST /api/auth/device_lookup {user_code}  (Firebase token)
                                               ----------->  {name, kind, requested_at, ip}  -> shown on the screen
                                                                                      reads it, presses Approve or Deny
                                               <-----------  POST /api/auth/device_approve {user_code, approve}
                                               creates the credential for this person's account
    POST /api/auth/device_poll  every 3 s --->  {status:"pending"}  ... until:
                               <-----------    {status:"approved", credential:"zd_...", email}   (exactly once, then forgotten)
    keeps the credential in the key store, e-mail in sync.json, sync starts

Other poll answers: `slow_down` (polling faster than about every 2.5 s: the client slows down), `denied`, `expired` (the client says so and the
person starts again).

### B. E-mail and password in the app: Firebase REST plus `exchange`

For the desktop app and `zo`, when the person types e-mail and password into the app itself.

1. The app calls Firebase (`https://identitytoolkit.googleapis.com/v1/accounts:signInWithPassword`, or `accounts:signUp` for a new account),
   with the project's public web API key. The password goes to Google only, never to the Zohara Chat server.
2. New account: Firebase sends a verification e-mail (`accounts:sendOobCode`, `VERIFY_EMAIL`); the account works once the link was clicked.
   Forgot the password: `accounts:sendOobCode` with `PASSWORD_RESET` (Firebase sends the mail).
3. The app sends the resulting Firebase ID token (short-lived, not kept) to `POST /api/auth/exchange {name, kind}` and gets
   `{credential, email}` in the answer. The ID token is then dropped.

## Endpoints (all `POST`, JSON in, `{ok:true,...}` or `{ok:false,error}` out)

| Endpoint | Who may call | Body | Answer |
|---|---|---|---|
| `/api/auth/device_start` | anybody (rate limited) | `{name (1..60 chars), kind: "desktop"\|"cli"}` | `{device_code, user_code, verify_url, expires_in: 600, interval: 3}` |
| `/api/auth/device_poll` | anybody who knows the `device_code` | `{device_code}` | `{status: "pending"\|"slow_down"\|"denied"\|"expired"}` or `{status:"approved", credential, email}` (once) |
| `/api/auth/device_lookup` | Firebase sign-in, verified e-mail, terms accepted | `{user_code}` | `{name, kind, requested_at, ip}` |
| `/api/auth/device_approve` | same | `{user_code, approve: true\|false}` | `{}` |
| `/api/auth/exchange` | same | `{name, kind}` | `{credential, email}` |
| `/api/auth/signout` | the device credential itself, or a Firebase sign-in | `{}` (a Firebase caller may add `{id}` of one of their own devices) | `{ok, revoked}`: a credential revokes itself |
| `/api/account/devices_list` | Firebase sign-in | `{}` | the signed-in devices: id, name, kind, created, last used |
| `/api/account/device_revoke` | Firebase sign-in | `{id}` | `{}` |

`GET /device` serves the web app, which shows the approval screen. `/api/account/device_create` no longer exists (404): a credential is
only ever made by approving a device request or by `exchange`. Kind `desktop` gets scope `sync` (only `/api/sync/*`); kind `cli` gets scope
`full` (also `/api/op`, `/api/send`, `/api/reply`). A person may have at most 10 signed-in devices; at the limit `exchange` and
`device_approve` answer 409 and say to remove one under "signed-in devices" (the waiting request stays waiting).
`verify_url` is `<public base>/device`, where the public base is `ZO_PUBLIC_URL`, else `RENDER_EXTERNAL_URL`. A server that has neither (or an
invalid one: it must be `https://host[:port]` with no path) refuses to start in login mode (exit code 2).

If the server answers 403 with `terms_required` (a first-time person who has not accepted the privacy notice and terms), the app says where to accept them: the person opens the web app
once, accepts there, and signs in again.

## Security model

* **`device_start` and `device_poll` are for programs only**: they send no cross-site permission, and a request with an `Origin` header (which only a browser sends) is refused,
  so another web page cannot start a sign-in from a visitor's own address. Per-address limits count an IPv6 address by its /64, and the last fifth of the waiting-request room is
  kept for networks that have nothing waiting. The approval screen shows the account being approved, says when the request came from another network, and asks for a tick
  (always for a terminal) before Approve.

* **The person never handles a secret.** The `zd_` credential is 32 random bytes (base64url) made by the server. It travels from the
  server to the device once, over HTTPS, and is never shown, logged or put in an error text.
* **On the server only a hash is kept** (sha256, in Firestore `users/<tenant>/meta/devices` and an index by hash). A stolen database does not
  give out working credentials. A credential belongs to one account's tenant and cannot reach another person's data.
* **On the device** the credential is in the system keyring / key store (id `sync-token` for the desktop app, `cli-token` for `zo`: each program has its own
  credential, so signing out of one never signs out the other, and the app's narrower `sync` credential is never used where `zo` needs a `full` one). Where there is
  no keyring (no `secret-tool`, a server shell) the credential is kept in `~/.config/zohara-chat/credentials.json`, mode 0600, instead; a keyring that does not answer within
  8 seconds is given up on. If it cannot be stored at all, the new credential is revoked on the server again, so no orphan device is left.
  Older setups (a pasted token with syncing on but no account) are switched off when the app starts: nothing is sent for them. `~/.config/zohara-chat/sync.json`
  holds only the server address (if chosen by hand), the on/off switch and the account's e-mail address.
* **Device-code phishing** (an attacker starts a request on their own device and tricks a victim into approving it, which would give the
  attacker's device the victim's account). Defences:
  * the approval page makes the person **type** the code shown on their own device; the link (`/device`) never carries the code, so an
    attacker cannot send a pre-filled link, and a person has to read a code that an attacker would have to get them to type;
  * the page shows the **device name, its kind and the requesting network address**, and warns: "Only approve if you started this on your own device";
  * codes **expire after 10 minutes** and are **single use**; the secret `device_code` that collects the credential is a different value that is
    never displayed and is stored on the server only as a hash;
  * **anti-guessing**: at most 5 wrong codes per person in 10 minutes (counted across lookup, approve and deny; a locked person is refused even with
    the right code); the code has 32^8 possibilities;
  * **caps**: 10 requests per minute per source address, 5 waiting per address, 1000 waiting overall; polling is limited too;
  * a credential that was approved but never collected is revoked when its request expires.
* **Verified e-mail**: an e-mail and password account must have a verified address (Firebase `email_verified`), like a Google account. Anonymous,
  custom-token, phone and unverified-password sign-ins are refused. This assumes Firebase's default of one account per e-mail address, so a Google
  and a password sign-in with the same verified address are the same person.
* **Restricted by default**: signing in only proves who the person is. Everybody except the operator's `ZO_ADMIN_EMAILS` addresses stays
  restricted on the server (`PUBLIC.md`). Computer-control tools run only in the person's own desktop app.
* **Pending requests live in the server's memory.** A restart only makes people start again. If a poll answer is lost on the way, the next poll says `expired`
  and the credential that was made is revoked at expiry. (This assumes one server instance.)

## What is stored where

| What | Where |
|---|---|
| Google / e-mail identity, the password hash (e-mail accounts) | Google Firebase Authentication (United States) |
| Hash of each device credential, device name, kind, created, last used | Firestore `users/<tenant>/meta/devices` and the hash index |
| A waiting device request (hash of the device code, the user code, name, kind, address, time) | the server's memory, at most 10 minutes |
| The credential | the device's key store only (id `sync-token`) |
| Account e-mail, server address, on/off switch | `~/.config/zohara-chat/sync.json` on the device |
| Chats, memory, personalities | on the device always; on the server and Firestore only after the person signed in |

## Signing out, and a lost device

* **Normal sign-out**: desktop Settings > Web sync > Sign out, or `zo logout`. The server is told to revoke that device's credential, and the
  device forgets it. Chats, memory and personalities on the device stay. If the server cannot be reached, the device says so and still signs out locally: remove
  the device afterwards as below.
* **Lost or stolen device**: sign in to the web app, Account > **Signed-in devices**, press Revoke next to the device. From that moment its credential stops working
  (the next call gets 401; the device notices on its next sync round, forgets the sign-in and shows the signed-out screen). Then change the password (e-mail account) or review the account's security at Google, if the
  device was unlocked and someone could have used it.
* **Delete the account**: Account > Delete account removes the account, all devices and the data on the server (`PRIVACY.md`).
* **Operator**: setting an address in `ZO_BLOCKED_EMAILS` refuses its sign-ins.

## Setup for the operator

Firebase console (project `zo-ai-144`): Authentication > Sign-in method: **Google** and **Email/Password** on; the Render address under Authorized domains;
Render environment: `ZO_PUBLIC_URL` set to the Render address (see `DEPLOY.md`). Without the Email/Password provider no e-mail account can exist.

## Tested / not tested

The server side has unit tests (the store, the limits, expiry, single use, scopes, the endpoints; `cargo test -p zo-server`) and the client side has unit tests
against a mock server on this computer (`cargo test -p zo-core login`). This page was checked against the code in those files. **Not tested** against the real
Firebase, Render or Google sign-in: e-mail and password sign-up and the verification e-mail, the approval page in a real browser with a real Google account,
the keyring on a real desktop, two devices at once. The wording of buttons in the apps may differ slightly from the names used here.
