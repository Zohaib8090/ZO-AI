# Zohara Chat as a public service

Written 2026-11-12. The server is not a personal site: every person who uses Zohara OS / Zohara Chat may sign in with Google or with e-mail and password and use it from
the web, the desktop app (sync) and the terminal client `zo`. The web version needs a sign-in; the desktop app works fully without one and sends nothing until the person signs in (`LOGIN.md`). This page is the design, the threat model, the limits, the cost and the
operator checklist. Privacy and terms texts: `PRIVACY.md`, `TERMS.md` (templates, not legal advice). Cost and quota figures come from
the vendors' pages read on 2026-11-12 (sources at the end); the per-user usage numbers are my assumptions, not measurements.

## Who is who

* **Admin**: a verified address (Google or e-mail account) listed in `ZO_ADMIN_EMAILS` (the owner). Unrestricted tools, Claude Code mode, SSH machines, Bypass mode.
* **Restricted user**: everybody else. Bring-your-own-key chat plus web search, page fetch and memory. Nothing that touches a computer.
* `ZO_BLOCKED_EMAILS` refuses sign-in; `ZO_MAX_USERS` (default 200) caps registrations.

## Architecture

* **Tenant** = the first 32 hex characters of `sha256("zo-tenant:" + firebase_uid)`. Raw uids and e-mails never appear in paths.
  Disk: `$ZO_TENANT_ROOT/<tenant>/{config,data}` (mode 0700). Firestore: `users/<tenant>/{chats,memory,files,meta}`.
* **Isolation in zo-core** (`zo_core::tenant`): a thread-local tenant context. Every store, the audit log and every thread started for
  a request, an agent or a memory job runs inside it (`tenant::spawn`). With `ZO_MULTI_TENANT=1` the service refuses any request that
  has no valid `"tenant"` on its first line; it never falls back to the default folders. Without it (desktop app, phone) nothing changes.
* **Server**: verifies the Firebase token, derives the tenant, applies limits, keeps one sync manager per tenant (pulled from
  Firestore on first use, pushed by a loop, evicted from local disk after 30 minutes idle with nothing unsynced).
* **Keys**: each user's own API key, in `<config>/keys.enc`, AES-256-GCM, key = `sha256("zo-keys" || ZO_SERVER_SECRET || tenant)`;
  the ciphertext syncs to Firestore. There is no shared model key.
* **Device credentials** replace the old global `ZO_ACCESS_TOKEN`: the desktop app and `zo` get one by signing in (nobody copies a token); see `LOGIN.md` and "Account API".

## What is restricted, and why

| Restricted users cannot | Why |
|---|---|
| use any tool except `zohara_web_search`, `zohara_web_fetch`, `zohara_memory_search`, `zohara_memory_save`, `zohara_memory_forget` | shell, files, screenshots and SSH on the server would be remote code execution for strangers. The tools are removed from the list the model sees and refused again in `tools::call` |
| use Claude Code mode | it runs a program with the owner's login on the server (no operator switch gives it to others; they use their own computer through the desktop link, `LINK.md`) |
| load MCP servers (stdio or remote) | stdio means running programs; remote means arbitrary outbound calls |
| use custom or local provider addresses unless they are public `https` hosts | SSRF: `localhost`, private networks and the cloud metadata address must be unreachable (same host check as the fetch tool) |
| add machines, schedules, projects with paths, or pick Bypass mode | server-side execution, background work and unattended approval |

Quotas: message text up to 20,000 characters, tenant data folder up to 25 MB (writes beyond it are refused with a clear message),
8 tool steps per answer, 120 seconds per answer.

## The desktop link and Claude Code (see `LINK.md`)

A person's own computer can run what they write on the web ("My computer"): the desktop app polls the server (`/api/link/*`), the server relays jobs and events per tenant, nothing is stored, and the desktop asks its owner before it acts. The server's secrets (`FIREBASE_SERVICE_ACCOUNT`, `ZO_SERVER_SECRET`, `CLAUDE_CODE_OAUTH_TOKEN`) are removed from its environment at start so no child process or `/proc/<pid>/environ` shows them.

## Threat model

| A signed-in malicious user could try | Defence | Remaining risk |
|---|---|---|
| reading or writing another user's chats, memory or keys | the tenant comes only from the verified token or device token; per-tenant folders and Firestore paths; the service refuses requests without a tenant | a bug in the tenant scoping: needs tests of the cross-tenant case and review of every new store |
| SSRF through fetch, provider addresses or MCP | public-https-only host check; MCP and custom providers off for restricted users | DNS rebinding and redirects must be checked in the fetch code |
| running a shell or reading server files | no such tool for restricted users, enforced twice | the tool list is the one line of defence; keep it a short allow-list, never a deny-list |
| stealing keys (their own are theirs; others' or the operator's) | keys encrypted at rest with a per-tenant derived key; the secret exists only in Render's environment; processes the AI starts (admin only) do not inherit secrets | the operator holds `ZO_SERVER_SECRET` and can decrypt: said plainly in the privacy notice |
| using up CPU, memory, disk, model or Firestore quota | 60 requests/minute per user, 2 concurrent `/api/send` per user, `ZO_MAX_RUNS` global, body limits (1 MB, 20 MB for sync), 25 MB per tenant, `ZO_MAX_USERS`, 100 new Firebase accounts/hour/IP | many real accounts still add up; watch the Firestore quota (see capacity) |
| a page the model reads makes it leak the user's notes through a web address, or erase them | inside one answer a restricted account's notes and the web are kept apart: once the notes were read the web tools are closed until the next message, once the web was read notes cannot be erased; addresses and searches are at most 500 characters | a page can still make the model save a false note (it is the user's own data and can be forgotten); one answer cannot do "read notes, then search the web" (ask in two messages) |
| a saved chat names a file of the server as a "picture" | for restricted accounts picture paths are dropped when a chat is synced in and never read when a request is built; for everyone else the kind comes from the file name, not the saved mime | an admin's own chats can still name their own files (they own the computer) |
| prompt injection: a fetched page tells the model to save false or hostile "memories", or to leak earlier chat text | restricted tools cannot reach anything but the user's own memory, so the harm stays inside that user's own data | a poisoned memory misleads later answers for that user; memory is visible and can be forgotten; a fetch result is data, not instructions (keep that wording in the prompt) |
| abusing memory, export or delete | the export contains only the caller's chats, memory and personalities, never keys; delete needs `{"confirm":"DELETE"}` and a Firebase token (not a device token) | a stolen browser session can delete; there is no undo, which is stated in the UI |
| a desktop syncs a model address or an MCP server into a restricted account | the sync kinds `profiles`, `settings`, `mcp` and `keys` (see `SYNC.md`) go through the same checks as the web: `mcp` is never accepted, listed or deleted for a restricted account; a profile's `base_url` must pass the public-https rule (`public_https_url`) and Claude Code profiles are refused; a document of these kinds over 64 KB, or past 100 profiles / 50 servers, is refused; the refusals are marked `Not for this account:` so the desktop does not list them as problems | an admin (own computer) account takes all of them |
| API keys carried by the sync | only the opt-in kind `keys` (off by default, switched on in the desktop's Account and sync page); stored in the tenant's own encrypted `keys.enc`, answered only to that tenant's credential; never inside a profile or MCP document; Claude Code, sign-in and MCP-token entries are not syncable | the server's secret opens every tenant's `keys.enc` (as for keys typed on the web); a person who switches this on accepts that |
| stealing a device credential | the person never sees or copies it; only the hash is stored on the server; kept in the device's key store; revocable (signed-in devices); scope `sync` reaches only `/api/sync/*`; never logged | a leaked credential works until revoked |
| device-code phishing: tricking a victim into approving the attacker's sign-in request | the approval page needs the code to be **typed** (links never carry it), shows device name, kind and requesting address with a warning, codes last 10 minutes and are single use, 5 wrong codes per person per 10 minutes, caps on requests per address (10/min, 5 waiting) and overall (1000 waiting) | a person who types a code an attacker dictated and ignores the warning; see `LOGIN.md` |
| using the server as a free relay to attack the web | fetch is rate limited by the per-user bucket and runs only from the model's tool loop | a user can still make the server fetch public pages; the 8-step cap bounds it |

## Attachments (pictures, files, links)

The web composer takes pictures and files from the clipboard (Ctrl+V), the "+" menu (upload, camera on phones, a link) and drag and drop. Nothing leaves the
browser until Send. The browser does the work: pictures are shrunk to at most 1600 px and re-drawn (this drops EXIF and location data), JPEG, or PNG/WebP when
they have transparency; text and code files are read as UTF-8 (200 KB each); PDF text is read with pdf.js and ZIP files with JSZip, both loaded on demand from
cdnjs at a pinned version with a checksum (the CSP allows `cdnjs.cloudflare.com` for scripts and `connect`, and `worker-src 'self' blob:` for the PDF worker, which is
fetched with its checksum and started from a local blob). ZIP guards: at most 200 entries, 2 MB unpacked in all (declared sizes and the bytes that really come
out are both counted), 100 KB per entry, archives inside are not opened, files that are not text are listed by name only. Other types (Office files, programs,
video, audio) are refused with a message, never dropped silently.

`/api/send` takes `"attachments": [{"kind":"image","mime","name","data_b64","thumb"?}, {"kind":"text","name","text"}]`. This is the only route with a bigger body limit
(14 MB, 90 s to arrive, at most 16 such bodies being read at once on the whole server). The server and the service (`zo_core::attach::parse`, the same rules twice)
check: at most 4 pictures and 6 files; a picture is png, jpeg, webp or gif by its mime **and** by its first bytes, strict base64, at most 3 MB decoded (9 MB for all);
a text is at most 200 KB (400 KB for all) and has no control characters other than tab and line breaks; names lose control characters, slashes, quotes and angle
brackets and are cut to 120 characters; the preview (`thumb`) is kept only if it is a data address of a real png/jpeg/webp of at most 20,000 characters. The server forwards
only what it rebuilt from the checked fields.

The model gets a picture as inline data (OpenAI-compatible `image_url` data address, Anthropic base64 source, Gemini `inlineData`), never as a path: a restricted
account never has a file read from the server's disk, so a picture in a saved chat that names a path is ignored for them. The picture is for that question only: after
the answer the turn in the saved history becomes text with `[image: name]`. The text of a file is put into the message between `<attachment name="...">` tags
(a closing tag inside the text is defused), and the system prompt says that attachments and web pages are data, not instructions. A model that cannot see pictures
gets none and is told so, and the person sees "this model cannot look at pictures" (the question is still answered). Claude Code mode (owner only) gets the pictures as
files in a private temp folder that is deleted after the answer. The saved chat entry keeps only `attach: [{kind, name, size, thumb}]` (previews are at most 20 KB each and
count against the 25 MB of a restricted account); chats that arrive through the sync have these fields cleaned the same way before they are stored, because the web page draws them.

Links: a link added with "+" (or a pasted address) is a chip and is added to the message as a line for the model, which reads it with its web tool. The service operation
`link_preview {url}` returns `{title, description, host}` for the chip (same public-address-only fetch as the fetch tool, redirects included, 300 KB at most, 12 a minute per person,
allowed for restricted accounts, never needed to send).

| A signed-in malicious user could try | Defence | Remaining risk |
|---|---|---|
| a huge or malformed picture or a base64 bomb | body limit, length checked before decoding, magic bytes, size caps, nothing is decoded by an image library on the server | the model provider decodes the picture (their parser) |
| a file that talks to the model ("ignore the person and ...") | tagged blocks, closing tag defused, the prompt says attachments are data | prompt injection is reduced, not removed; the harm stays inside the person's own data (see above) |
| a file name or preview that attacks the page (script in a name, `javascript:` thumbnail) | names cleaned on the server, the page writes names with `textContent`, previews only if a real small picture, sync cleans them too | none known |
| using the 14 MB route to hold memory | per-person limits are unchanged, 16 big bodies at once overall, 90 s deadline | many accounts together |
| a link preview used to probe the network | the fetch tool's public-address check, 12 a minute | a public page can still be fetched on a person's behalf |

## Notifications

The web app can be installed, can notify when an answer is ready, and (with the person's consent) can have the server finish an answer after the tab closed and push a generic "answer ready" message. Web Push goes only to the allow-listed push services the browser named, at most 5 devices and 30 pushes an hour per account. Details, privacy and limits: [NOTIFICATIONS.md](NOTIFICATIONS.md).

## Activity

The service operations `activity` and `activity_stop` list and stop what is running, scoped to the calling tenant: a user sees and stops only their own answers, memory jobs and open client connections (restricted users cannot have anything else). Details: [ACTIVITY.md](ACTIVITY.md).

## Limits (defaults)

60 requests/minute per user, 8 requests at once per user, 2 concurrent answers per user (ops that call a model, `compact`,
`memory_edit`, `personality_generate`, `models`, `model_test`, count as an answer), `ZO_MAX_RUNS` (8) answers at once overall, 2 sync requests at once per
user and 6 overall, bodies 1 MB (`/api/op`, `/api/reply`), 14 MB (`/api/send`, because of attachments) and 20 MB (`/api/sync/*`) that must arrive within 30 s (90 s for send, 120 s for sync), idle
client connections are dropped after 20 s (a patched `tiny_http` in `vendor/`), `429` with `Retry-After` when over. People who are not
registered yet may knock 10 times a minute; when the registry is full they are refused for 30 s at a time without a database read.
Sign-in requests (`device_start`): 10 a minute and 5 waiting per source address, 1000 waiting overall. Device-credential lookups: 60 a minute per source address (the last `X-Forwarded-For` entry), 1200 overall; a real token that was seen in the last
hour still works when a guesser has used up the allowance. Restricted: see the quotas above. Not done: nothing reclaims the places of
accounts that are never used again (watch the count; `ZO_MAX_USERS` and `ZO_BLOCKED_EMAILS` are the tools).

## Account API

POST, Firebase-authenticated, under `/api/account/`: `whoami`, `devices_list` (shown as "signed-in devices"), `device_revoke {id}`, `accept_terms {version}`, `export`, `delete {confirm:"DELETE"}`, and the notification devices `push_config`, `push_subscribe`, `push_unsubscribe`, `push_list` ([NOTIFICATIONS.md](NOTIFICATIONS.md)). Until a person has accepted the current version of the
notice and terms (recorded in `users/<tenant>/meta/terms`), everything but `whoami`, `accept_terms`, `export` and `delete` answers 403 with
`terms_required`. A blocked address can still `export` and `delete`. Delete writes a marker (`deleted/<tenant>`, time and whether the
login was removed) first, so a sign-in token that is still valid cannot bring the account back for two hours (for good when the login could
not be removed), even after a restart; the answer says `login_removed` and the page tells the person plainly when it is false. Device credentials are not made here any more (`device_create` was removed): they come from the sign-in flows in `LOGIN.md` (`/api/auth/*`, at most 10 per person). Scope `sync` (desktop) may call only
`/api/sync/*`; `full` (`zo`) also `/api/op`, `/api/send`, `/api/reply` (for `zo`). Account deletion must also remove the user from Firebase
Authentication (Firebase then purges it from backups within 180 days); confirm this in the code before launch.

## Capacity and cost (honest numbers)

Assumptions per active user per day (mine, to be checked against real traffic): about 40 Firestore writes, 150 reads and 4.5 MB of
Firestore data read.

* **Spark (no cost)**: 1 GiB stored, 50,000 reads/day, 20,000 writes/day, 20,000 deletes/day, 10 GiB/month outbound. By the numbers:
  writes allow about 500 daily active users, reads about 330, egress about 75. **Expect roughly 50 to 150 daily active users**,
  and a few hundred registered ones. Render is outside Google Cloud, so every Firestore read counts as internet egress. When a daily
  quota is used up, calls fail until it resets around midnight Pacific (general knowledge, not re-checked). Firebase Authentication with
  Identity Platform on Spark is limited to 3,000 daily active users.
* **Blaze (pay as you go)**, eur3 list prices: writes 0.18 USD per 100,000, reads 0.06 USD per 100,000, egress 0.12 USD per GiB after
  10 GiB free; the free quotas stay. At 1,000 daily active users with the same assumptions: about 18 to 20 USD a month, about 15 of it
  egress. Larger chats raise it in proportion. **Budgets only send alerts; they do not stop spending**, so set a budget alert and watch it.
* Cheaper at any size: load only a chat list on start and the chat body when opened, keep a tenant cached longer when nothing is unsynced, compress chat JSON.
* **Render**: the **free plan is not suitable** for a public service. It spins down after 15 minutes idle (about a minute to wake), loses
  its disk at every restart, redeploy and spin-down (unsynced data is lost), may be restarted at any time (running answers die),
  has 512 MB RAM, may be suspended for unusual outbound traffic, and Render's docs say not to use it for production. The cheapest paid
  always-on instance is **Starter, about 7 USD/month (512 MB, 0.5 CPU)**: the minimum, tight. **Standard, about 25 USD/month (1 CPU,
  2 GB)** is the sensible launch size; start with `ZO_MAX_RUNS` 4 there, 2 on Starter. Region Frankfurt. A paid instance does not sleep,
  so the self-ping and an outside pinger are not needed. Firestore stays the only source of truth.
* Total honest budget to start: about 25 USD/month for Render Standard, Firebase free until the Spark quotas bite, then about 20 USD/month per 1,000 daily users.
* Model costs are paid by each user's own key, not by the operator.

## Operator checklist

Firebase (project `zo-ai-144`):
* Authentication > Sign-in method: **Google and Email/Password** on. Authentication > Settings > Authorized domains: the Render address and any custom domain.
* Firestore created in **eur3**; rules closed (`allow read, write: if false;`), the server uses the service account.
* Optional: App Check later. Do not load Google Analytics (the measurement id in the web config stays inert); if analytics is ever added, a consent banner becomes necessary.
* Billing: choose Spark or Blaze knowingly (above), and **create a budget alert** in Google Cloud Billing. Check the Firestore usage page weekly at first.
* Confirm the Firebase / Google Cloud data processing terms in the console and keep a record.

Render env vars: `FIREBASE_SERVICE_ACCOUNT` (secret), `ZO_ADMIN_EMAILS` (secret not needed, but set), `ZO_SERVER_SECRET` (generated; keep a
private copy: changing it makes every stored API key unreadable and users must enter them again), `ZO_MAX_USERS`, `ZO_MAX_RUNS`,
`ZO_TENANT_ROOT`, `ZO_PUBLIC_URL` (the Render address, `https://...`, for the sign-in approval link; `RENDER_EXTERNAL_URL` is the fallback), `ZO_CONTACT_EMAIL`, `ZO_OPERATOR_COUNTRY`, `ZO_HOSTING_REGION`, optional `ZO_BLOCKED_EMAILS`, optional admin-only `CLAUDE_CODE_OAUTH_TOKEN`. `ZO_ACCESS_TOKEN` is gone.

Before launch: set `ZO_CONTACT_EMAIL`, `ZO_OPERATOR_COUNTRY` and `ZO_HOSTING_REGION` (the pages are filled from them and the server does not start without them; `TERMS_VERSION` in `pages.rs` is the pages' date, change it when the texts change), have them reviewed, check the pages are
served at `/privacy` and `/terms`; sign in with a second, non-admin Google account and with an e-mail and password account (verification e-mail arrives, sign-in works only after the click) and confirm it is restricted (no shell, no files, no MCP,
no machines); try to read the first account's chats from it; check limits return 429; try an export and a delete.

Incident handling: (1) stop the bleeding: set `ZO_BLOCKED_EMAILS`, or suspend the service in Render. (2) Rotate: new `FIREBASE_SERVICE_ACCOUNT` key
(and delete the old one in Google Cloud), new `ZO_SERVER_SECRET` if keys may be exposed (users re-enter keys), ask users to revoke provider keys and sign out lost devices (Account > signed-in devices).
(3) Look at Render logs (they contain no message text, keys or tokens by design). (4) If personal data was exposed, tell the users and,
where the law requires, the data protection authority within 72 hours. (5) Write down what happened and fix the cause.

## Claude and other providers' terms

Bring-your-own-key matches Anthropic's rule that each end user authenticates with their own API key and the operator does not resell or
intermediate usage (Commercial Terms A.1 and D.4, code.claude.com/docs/en/legal-and-compliance). The service must never accept, store or
route a Claude.ai login or subscription token for other people: Claude Code mode stays admin-only on the owner's own token. The owner sends it once from the desktop app (`claude setup-token`, see `LINK.md`); it is stored encrypted for that account only and `claude` runs on the server with a minimal environment. Everybody else signs in to Claude Code on their own computer and the web page reaches it through the desktop link ("My computer"), so no other person's subscription is ever on the server. Anthropic asks
that users are told outputs may be wrong (D.3): the Terms say it. OpenRouter requires users to be 18+ and makes the key holder responsible
for charges (openrouter.ai/terms). OpenAI forbids sharing or reselling keys (3.1, 3.3(g)); a user pasting their own key is not clearly
addressed, so the consent wording is "Zohara Chat may use your key only for your own requests".

## Not verified

* Exact eur3 price of stored data and plain deletes (assumed near 0.18 USD/GiB-month and 0.02 USD per 100,000).
* The per-user usage numbers above; what exactly happens on Spark when a daily quota is exceeded; whether Auth without Identity Platform has any daily cap.
* Which Render plans include the GDPR data processing agreement; the free plan's suspension threshold.
* Minimum ages by country; whether the "strictly necessary" exemption covers Firebase's browser storage in every EU country; legal sufficiency of the texts.
* Whether pasting one's own OpenAI key into a third-party service conflicts with OpenAI's terms.
* Attachments: tested in the built-in browser pane against a local server (a pasted picture from a stubbed clipboard event, the "+" menu, a text file, a generated PDF, ZIPs including one that unpacks to 4 MB and one with 250 entries, drag and drop, link chips, 390 px and light/dark) and in Rust tests against fake model servers. Not tested: Ctrl+V with a real system clipboard, a real phone camera or file picker, Safari and Firefox, HEIC conversion, a real vision model, and a ZIP whose headers lie about sizes (the bytes that really come out are counted, but no such file was built).
* None of this was tested against a real Render, Firebase or Firestore; the code of the multi-tenant design is written by other agents and this page describes the agreed design, not verified behaviour.

Sources (read 2026-11-12): firebase.google.com/docs/firestore/quotas, /pricing, firebase.google.com/pricing, /docs/auth/limits,
/support/privacy; cloud.google.com/firestore/pricing, cloud.google.com/identity-platform/pricing; render.com/docs/free, /pricing,
/docs/regions; gdpr-info.eu (Art. 8, 13); anthropic.com/legal/commercial-terms; code.claude.com/docs/en/legal-and-compliance;
openai.com/policies/business-terms; openrouter.ai/terms; edpb.europa.eu Guidelines 2/2023.
