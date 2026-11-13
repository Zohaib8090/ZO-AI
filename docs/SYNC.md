# Laptop <-> server sync

Chats, memory notes and custom personalities written on the desktop show up on the server (and so in the web app), and the other way
round. Since 2026-11-12 (later the same day) the same goes for the **profiles** (with their pinned models and model addresses), the
picker preferences (which providers are hidden), the **remote MCP servers**, and, only if switched on, the **API keys** (see "More kinds" below). The phone talks to the desktop, so it follows. Built 2026-11-12. Code: `crates/zo-core/src/sync.rs` (planner, documents,
client), `crates/zo-server/src/sync_api.rs` (the server's side), Settings > Web sync in `crates/zohara-agent`.

## Setup (every person)

The desktop app **works fully without any login**: nothing is sent to the server until the person signs in, and signing in is optional. It only
turns on sync with the web. Nobody copies or pastes a token (see `LOGIN.md` for the flow, endpoints and security).

1. In Zohara Agent on the desktop: Settings > Web sync > **Sign in**. Choose Google (a browser opens the server's approval page: sign in, type the short
   code the app shows, check the device name and approve) or e-mail and password (typed into the app; a new account must click the link in the
   verification e-mail first). Sync switches on by itself and a round runs at once. "Sync now" runs one more; the line under the buttons says when it
   last worked or why it failed. **Sign out** revokes this device on the server and forgets the sign-in here; chats, memory and personalities stay.
2. On the web, sign in with the same account. After both are signed in, chats, memory and personalities match.
3. Behind the scenes the server issues this device a **device credential** (`zd_...`, only its hash is stored on the server). It is kept in the
   system keyring (key id `sync-token`), never in a file, never shown, and can be revoked on the web Account page under **signed-in devices** (a lost computer).
   The account's e-mail, the on/off switch and a server address chosen by hand are in `~/.config/zohara-agent/sync.json`.
4. Server address: the default is `https://zohara-agent.duckdns.org`; the environment variable `ZO_SERVER_URL` or the `url` in `sync.json` overrides it. Only
   `https://` addresses are accepted (plain `http://` only for 127.0.0.1 / localhost), because the credential is sent with every call.

## How it works

* Every 30 seconds (and on "Sync now") the desktop asks the server for its **manifest**: for chats, memory notes and personalities,
  `{id: last changed (ms)}`, plus the **tombstones** `{"chats:ID": time deleted}`. It builds its own, and a pure planner decides:
  * an item that only one side has is copied to the other, unless a tombstone says it was deleted;
  * on both sides, the **newest `updated` wins**; equal times: nothing happens;
  * a tombstone at or after an item's last change **deletes** it on both sides (a later edit brings it back);
  * a tombstone is also passed on to a side that never saw the item.
* The desktop then pulls (`/api/sync/get` -> applied locally), pushes (`/api/sync/put`) and sends/applies deletions (`/api/sync/delete`).
  The server applies a `put` only if the document is newer than what it has and than any tombstone, and answers `{"applied": true|false}`.
  It writes through zo-core's stores, so the Firestore push sees the change like any other.
* **Tombstones** are in `tombstones.json` in the data folder (the server's one also goes to Firestore like the other small files).
  They are written when the app deletes a chat, forgets a note, clears the memory or removes a personality, and kept for 90 days.
* **Safety**: an empty, failing or unreachable server never deletes anything here: only a tombstone deletes. If the server cannot be
  reached or does not accept the credential (revoked: sign in again), the round stops and changes nothing more. An error that belongs to one item (the server refuses
  it, it does not parse) skips that item and goes on; the status text says how many were skipped and the first reason. Manifests that do not look like manifests are errors, not "empty".
* **Not synced**: the keys file and, unless the person switched "Also sync my API keys" on, any API key; the chosen model and provider (they stay per device),
  the permission mode, projects, artifacts, scheduled tasks, MCP servers that start a program, MCP tokens and sign-ins,
  which personality is in use, and the memory agent's "read N messages" counters. Pinning a note or a chat does not change its `updated`,
  so a pin only travels with the next edit (a chat already on both sides keeps its own local pin).
* Personalities: only the person's own (built-in ones are in the program). They are never edited, so their time is the one in their id (`p<ms>`).
* Memory also has one pseudo item, `memory:_state` (memory on/off and how often the agent reads a chat), newest change wins.
* Only the newest 100 chats are kept on each side (the store's own limit): a chat older than all 100 of the other side is not copied, and
  that refusal is remembered so it is not retried every 30 seconds.
* **Clocks**: the server's manifest carries its `now`. The computer measures how far its clock is off (ignored under 5 seconds), compares
  all times on the server's clock, and moves the time inside a document when it sends or takes it. A change or deletion dated more than
  an hour ahead is refused (and tried again each round until its time has come), so a wrong clock cannot block an item for good. A
  deletion is never stamped before the item's own last change.
* A chat that is being answered is not overwritten by a sync and does not overwrite one: when the answer is saved and the stored chat
  grew meanwhile (another device added messages), the new messages are added after what is there. If the stored chat changed in another
  way (an edit), the answer is saved as a separate "(copy)" chat. The window of the desktop app is not covered yet: it saves its own chat
  as it holds it.

## More kinds: profiles, preferences, MCP servers, keys

The manifest has four more kinds beside `chats`, `memory` and `personalities` (an older server's manifest without them still reads: they are empty).
All of them use the same planner, `updated` times and tombstones. The id is the item's name in the store.

| Kind | Item id | Document | Rules |
|---|---|---|---|
| `profiles` | the profile id (`p<ms>`) | `{id, name, provider, model, base_url?, effort?, vision?, pinned:[...], updated}` | **no key inside** (unknown fields are dropped when a document comes in); name clashes get " (2)"; max 100; the provider must be known; `base_url` http(s), no spaces; a deletion also unchooses the profile and clears its own key here |
| `settings` | `main` | `{hidden_providers:[...], provider_prefs:{provider:{...}}, updated}` | one document, never deleted; **not** the current provider/model (those stay per device); file `ui-prefs.json` |
| `mcp` | the server name as a tool name | `{name, address, enabled, updated}` | **remote servers only** (an address, no user name inside); never a program, env, header, token, client secret or sign-in; this computer's own headers stay when an edit comes in; max 50; a **restricted account never accepts, lists or deletes them** |
| `keys` | `provider` or `provider@profile-id` | `{id, key, updated}` | **opt-in, off by default**; only provider keys (never Claude Code, the sign-in credential, SSH or MCP tokens); the time of each key is in `keys-times.json` (times only); a key that was there before times were kept has the time 1 (it is copied if the other side has none, and a different key on the other side never replaces it: set it again here to make them agree) |

Times for profiles, servers and the preferences are stamped by the store itself when it saves (`profiles::save`, `mcp_client::save_servers`, `ui_prefs`), by comparing with the file
before: only a real change gets a new time, and a removal leaves a tombstone. The sync writes with the other side's time (`save_as_is`) and never leaves a tombstone of its own.
A document of these kinds over 64 KB is refused.

**What the person chooses** (desktop: Settings > Account and sync, stored in `sync.json` as `profiles`, `mcp`, `keys`): "Sync my profiles and pinned models" (on; covers `profiles` and `settings`),
"Sync MCP servers (web addresses only)" (on), "Also sync my API keys" (**off**; the page says plainly that the keys then also live, encrypted with the server's secret, on the server).
A kind that is switched off is dropped from both sides before the plan is made, so nothing of it is sent, fetched or deleted; switching it on later makes one full round. Keys travel only over the signed-in
device's HTTPS connection (`clean_url`: https, or plain http for this computer only) to the account's own tenant.

**On the server**: a restricted account's `put`/`delete` of `mcp`, a profile whose `base_url` is not a public https address, and a Claude Code profile are refused with a message that starts
`Not for this account:`. The desktop remembers the refusal for that version of the item and does not list it as a problem (the item stays on the desktop). Everything else that fails is an item error as before.

**Service ops added for the screens** (desktop and web; `crates/zo-core/src/service.rs`): `select {provider?, model?, effort?, base_url?}` also saves the address (empty = the provider's default; checked; a restricted account only a public https one) and answers `{provider, model, effort, base_url}`;
`models {provider?, base_url?}` answers `{provider, models, current, efforts, effort, base_url, error?}` and a failed fetch is still ok with `models: []` and a friendly `error` (no key, bad address, 401, timeout, ...); the address in the request is tried without being saved;
`model_test {provider?, base_url?, model?}` answers `{works, message}` (with a model: one tiny real answer, 15 s at most; without: the model list), counted like an answer for the limits;
`providers` adds `needs_url`, `default_url`, `base_url`, `hidden` per provider and `hidden_providers`; `profiles` adds `base_url`, `vision`, `updated`; `profile_update {id, base_url?, effort?}`; `settings_get` and `settings_set {hidden_providers?, provider_prefs?}`.


## Server API (all need `Authorization: Bearer <device credential or Firebase ID token>`; bodies up to 20 MB; they act only on the caller's own tenant)

| Call | Body | Answer |
|---|---|---|
| `POST /api/sync/manifest` | `{}` | `{ok, chats:{id:ms}, memory:{id:ms}, personalities:{id:ms}, profiles:{..}, settings:{..}, mcp:{..}, keys:{..}, deleted:{"chats:ID":ms}, now}` |
| `POST /api/sync/get` | `{kind, id}` | `{ok, doc}` (400 if there is no such item) |
| `POST /api/sync/put` | `{kind, id, doc}` | `{ok, applied}` |
| `POST /api/sync/delete` | `{kind, id, at}` | `{ok, deleted}` (the tombstone is recorded either way) |

The service also answers `{"op":"sync_status"}` (enabled, url, last_ok, error, pulled, pushed, deleted, running; never the credential), for the phone.

## Local test server (no Firebase)

    ZO_SERVER_NO_AUTH=1 PORT=8799 zo-server                    # 127.0.0.1 only, no login, one fixed tenant, no Firestore

Use a private HOME / XDG_* / XDG_RUNTIME_DIR for it (the server shares the app's data folders otherwise).
`crates/zo-core/examples/syncctl.rs` is a small driver (add a chat or note, delete, list, one sync round with `ZOHARA_AGENT_KEY_SYNC_TOKEN`).

## Tested / not tested

Tested (profiles, preferences, MCP, keys): planner tests with every new kind, document tests (no key in a profile or server document, unknown fields dropped, wrong documents refused, newest wins, tombstones, a key's time is not moved by the sync), restricted-account refusals (mcp, private address, Claude Code, oversize), the service ops over a real socket (address saved and echoed, friendly model errors, the connection test, hidden providers, profile_update), and an HTTP round trip with the desktop driver `syncctl` against the server with the fake sign-in: a desktop profile with pins and a custom address appears for the account on the web, an edit on either side reaches the other, deleting a profile deletes it on the other side, a remote MCP server (never the program) and the hidden providers arrive, keys only travel after the toggle is switched on (and stop when it is switched off), and a restricted account gets no MCP server and no private address.

Tested: unit tests of the planner (copy, newest wins, ties, tombstones, resurrection by a later edit, empty server), of the tombstone
hooks, `put`/`apply_delete`, manifests and address checks (zo-core), and of the four calls (zo-server). An integration run with a
local test server in one sandbox and `syncctl` in another, over HTTP on 127.0.0.1 (with the earlier token set-up; the sign-in itself is tested separately, see `LOGIN.md`): chat and note created in B appear in A; an edit in A
reaches B; a delete in B removes it in A; a delete in A (service op) removes it in B; a forgotten note disappears in A; a conflicting
edit resolves to the newest in both directions; a personality and a note added in A arrive in B; a wrong credential or an unreachable server
changes nothing in B; the credential is not in the server log. The Settings page renders (headless, container `zs-ai-img`).
Not tested: against Render or a real Firestore, with the Firebase token (a device credential takes the same path in `who()`), the sign-in flows against real Firebase (`LOGIN.md`),
the 30-second background loop and the switch in a running app (only `sync_with` was driven), the keyring storage of the credential
(`secret-tool`; tests used `ZOHARA_AGENT_KEY_SYNC_TOKEN`), big chats (megabytes), two computers syncing at once, the `ZO_BIND` listen address.
The open chat list in the app is not refreshed live when a sync brings something: it shows after the list is rebuilt (new chat, switching view).

Not tested for the new kinds: a real Firestore round trip (the in-memory database stands in), adding an MCP server on the web against a live server (the web
connects to it first; the save itself is the same code that the toggle test covers), the desktop Account and sync page on a real display beyond a headless look, and the
connection test against real providers (only a local stand-in model server). The connection test with a model sends one small real request to the provider.
