# Laptop <-> server sync

Chats, memory notes and custom personalities written on the desktop show up on the server (and so in the web app), and the other way
round. The phone talks to the desktop, so it follows. Built 2026-11-12. Code: `crates/zo-core/src/sync.rs` (planner, documents,
client), `crates/zo-server/src/sync_api.rs` (the server's side), Settings > Web sync in `crates/zo-ai`.

## Setup (owner)

1. On Render, `ZO_ACCESS_TOKEN` is made for you by `render.yaml` (a long random value). Open the service > Environment and copy it.
2. In ZO-AI on the desktop: Settings > Web sync. Server address = `https://<your service>.onrender.com`, Access token = the value, switch
   "Keep in step" on, Save. "Sync now" runs one round at once; the line under the buttons says when it last worked or why it failed.
   The token goes into the system keyring (key id `sync-token`), never into a file, and is not shown again. The address and the switch
   are in `~/.config/zo-ai/sync.json`.
3. Only `https://` addresses are accepted (plain `http://` only for 127.0.0.1 / localhost), because the token is sent with every call.

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
* **Safety**: an empty, failing or unreachable server never deletes anything here: only a tombstone deletes. Any error stops the round,
  keeps the status text and changes nothing more. Manifests that do not look like manifests are errors, not "empty".
* **Not synced**: API keys and the keys file, the chosen model, the permission mode, projects, artifacts, scheduled tasks, MCP servers,
  which personality is in use, and the memory agent's "read N messages" counters. Pinning a note or a chat does not change its `updated`,
  so a pin only travels with the next edit (a chat already on both sides keeps its own local pin).
* Personalities: only the person's own (built-in ones are in the program). They are never edited, so their time is the one in their id (`p<ms>`).
* Memory also has one pseudo item, `memory:_state` (memory on/off and how often the agent reads a chat), newest change wins.
* Only the newest 100 chats are kept on each side (the store's own limit): a chat older than all 100 of the other side is not copied, and
  that refusal is remembered so it is not retried every 30 seconds.
* A chat that is being answered right now has a newer `updated` than any copy, so a sync never overwrites it.

## Server API (all need `Authorization: Bearer <ZO_ACCESS_TOKEN or Firebase ID token>`; bodies up to 20 MB)

| Call | Body | Answer |
|---|---|---|
| `POST /api/sync/manifest` | `{}` | `{ok, chats:{id:ms}, memory:{id:ms}, personalities:{id:ms}, deleted:{"chats:ID":ms}}` |
| `POST /api/sync/get` | `{kind, id}` | `{ok, doc}` (400 if there is no such item) |
| `POST /api/sync/put` | `{kind, id, doc}` | `{ok, applied}` |
| `POST /api/sync/delete` | `{kind, id, at}` | `{ok, deleted}` (the tombstone is recorded either way) |

The service also answers `{"op":"sync_status"}` (enabled, url, last_ok, error, pulled, pushed, deleted, running; never the token), for the phone.

## Token-only server (testing, or a server without Firebase)

    ZO_ACCESS_TOKEN=<32+ characters> PORT=8799 zo-server      # 127.0.0.1 only, no login, no Firestore
    ZO_BIND=0.0.0.0 ... (with PORT)                            # to listen on other addresses on purpose

Use a private HOME / XDG_* / XDG_RUNTIME_DIR for it (the server shares the app's data folders otherwise).
`crates/zo-core/examples/syncctl.rs` is a small driver (add a chat or note, delete, list, one sync round with `ZOAI_KEY_SYNC_TOKEN`).

## Tested / not tested

Tested: unit tests of the planner (copy, newest wins, ties, tombstones, resurrection by a later edit, empty server), of the tombstone
hooks, `put`/`apply_delete`, manifests and address checks (zo-core), and of the four calls (zo-server). An integration run with a
token-only server in one sandbox and `syncctl` in another, over HTTP on 127.0.0.1: chat and note created in B appear in A; an edit in A
reaches B; a delete in B removes it in A; a delete in A (service op) removes it in B; a forgotten note disappears in A; a conflicting
edit resolves to the newest in both directions; a personality and a note added in A arrive in B; a wrong token or an unreachable server
changes nothing in B; the token is not in the server log. The Settings page renders (headless, container `zs-ai-img`).
Not tested: against Render or a real Firestore, with the Firebase token (the access token takes the same path in `who()`),
the 30-second background loop and the switch in a running app (only `sync_with` was driven), the keyring storage of the token
(`secret-tool`; tests used `ZOAI_KEY_SYNC_TOKEN`), big chats (megabytes), two computers syncing at once, the `ZO_BIND` listen address.
The open chat list in the app is not refreshed live when a sync brings something: it shows after the list is rebuilt (new chat, switching view).
