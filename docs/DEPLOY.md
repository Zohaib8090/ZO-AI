# Putting the Zohara Agent server on Render (owner steps)

Written 2026-11-12, updated for the public multi-user service the same day: see `PUBLIC.md` (design, threat model, limits, cost, checklist). Anyone may now sign in with Google; only the addresses in `ZO_ADMIN_EMAILS` are admins, everybody else is restricted. What was checked, and what was not, is listed at the end.

## What is in the image

`Dockerfile` builds `zo-server` and `zo-mcp` in a Rust image (the desktop app is not built, so no GTK is needed), then copies them
into `node:22-bookworm-slim` with `git`, `ssh`, `curl`, `tini` and Claude Code (`npm install -g @anthropic-ai/claude-code`). About 690 MB.
It runs as the user `zo` (not root). Data, config and the socket live in `/home/zo/.local/share`, `/home/zo/.config` and `/tmp/zo-run`
(all owned by `zo`; the socket directory is short on purpose, unix socket paths are limited to about 100 characters).
Render's `PORT` is read by the server. `zo-mcp` must sit next to `zo-server` (Claude Code mode starts it), so both are in `/usr/local/bin`.

## Steps

1. Firebase console (project `zo-ai-144`):
   * Authentication > Sign-in method > **Google: on** and **Email/Password: on** (without it nobody can create an e-mail account).
   * Firestore Database: create it (production mode). Rules can stay `allow read, write: if false;`, the server uses the service account.
   * Project settings > Service accounts > Generate new private key. This downloads a JSON file. **Never commit it**; you paste it into Render below.
2. Render dashboard > New > Blueprint > connect the GitHub repository `Zohaib8090/zohara-agent` > branch `main`. Render reads `render.yaml`
   and asks for the values that are not in the file (they are marked secret there with `sync: false`):

   | Field | What to put | Secret? |
   |---|---|---|
   | `FIREBASE_SERVICE_ACCOUNT` | the whole content of the JSON key file, as one value | yes |
   | `ZO_ADMIN_EMAILS` | your Google address (several: separated by commas). Admins get every tool; everyone else is restricted | no |
   | `ZO_CONTACT_EMAIL` | the address people write to for access and deletion requests; shown in the privacy notice and the terms (the server does not start without it) | no |
   | `ZO_OPERATOR_COUNTRY` | whose law the terms name (for example Germany) | no |
   | `CLAUDE_CODE_OAUTH_TOKEN` | optional, see "Claude Code" below. Leave empty if you do not use Claude Code mode | yes |

   Optional: `ZO_UPDATE_REPO` (`owner/name`, default `Zohaib8090/zohara-agent`): the GitHub repository whose newest release `GET /api/update/latest` announces to the apps (cached one hour; see `UPDATES.md`).
   `ZO_SERVER_SECRET` is created by Render (`generateValue: true`). `ZO_MAX_USERS` (200), `ZO_MAX_RUNS` (8), `ZO_HOSTING_REGION` and `ZO_TENANT_ROOT` are set in `render.yaml`;
   optional `ZO_BLOCKED_EMAILS` (comma list refused at sign-in) you add yourself. There is no `ZO_ACCESS_TOKEN` any more: the desktop app and `zo` sign in (`LOGIN.md`) and the server issues their device credentials.
   `ZO_PUBLIC_URL` is in `render.yaml` with a placeholder: you set it in step 6 once you know the address.
3. Press Apply. The first build compiles Rust inside Docker: expect roughly 5 to 15 minutes (Render's build command limit is 120 minutes,
   so that is no problem; I could not find a documented memory or image size limit for builds).
4. **First-deploy order (the address is only known after the service exists):**
   1. Create the service (the steps above). `render.yaml` already sets `ZO_PUBLIC_URL` to the placeholder `https://zohara-agent.duckdns.org`. It wins over Render's own `RENDER_EXTERNAL_URL`
      (the fallback when `ZO_PUBLIC_URL` is empty), so if your service gets a different address the approval links are wrong until you fix it in step 4.
      The server refuses to start (exit code 2) in login mode if neither value is a valid `https://host` address.
   2. Read the address Render gives the service (for example `https://zohara-agent.duckdns.org`).
   3. Firebase console > Authentication > Settings > **Authorized domains** > Add domain: that address without `https://`
      (for example `zohara-agent.duckdns.org`). Without this the Google pop-up refuses to open.
   4. Render > the service > Environment > set `ZO_PUBLIC_URL` to the full address with `https://` and no path (`https://zohara-agent.duckdns.org`). It is not a secret.
      It is the base of the approval link people open to sign in the desktop app and `zo` (`<address>/device`). On a custom domain use that domain, and add it to Authorized domains too.
   5. Redeploy (saving the variable does it; or Manual Deploy).
   The desktop app and `zo` find the server by themselves: their default address is `https://zohara-agent.duckdns.org`. If your service has another
   address, people set `ZO_SERVER_URL` (or the address in the desktop Settings) to it, or change `DEFAULT_SERVER_URL` in `crates/zo-core/src/login.rs` before building.
5. Check the sign-in: open `https://<address>/device` (it loads the web app); then on a computer run `zo login` or use the desktop app's Settings > Web sync > Sign in,
   and see that the web address it shows begins with the address you set in step 4.4.
6. Reading the generated value: Render > the service > Environment > `ZO_SERVER_SECRET` (click the eye icon). Do not paste it into chat, a file in the repository or an issue.
   **Keep a private copy**: every user's API key in Firestore is encrypted with a key derived from it; a new secret means everybody enters their API keys again.
   Then work through the operator checklist in `PUBLIC.md` (Firebase billing alert, Firestore rules, privacy and terms details (the three settings above), a test with a second non-admin account).
7. Verify:
   * Open `https://<address>/healthz`: it says `ok`. (The first call after sleep takes about a minute.)
   * Open `https://<address>/`, sign in with Google (your address is admin). Sign in with a second Google account and with an e-mail and password account (a verification e-mail comes first): both must work and be restricted (web search, page fetch and memory only; no machines, MCP, Claude Code or Bypass). `/privacy` and `/terms` must open.
   * Render > Logs should show `zo-server: fetched N chats and N memory notes from Firestore` and `listening on 0.0.0.0:...`.
   * Settings > pick a provider and enter its API key; send a message. In Firestore, `users/<tenant id>/chats` gets a document within a few seconds.
   * Redeploy once (Manual Deploy) and check that the chat is still there: that proves the Firestore sync.

## Which Render plan

For a public service use a **paid always-on instance**: `render.yaml` says `starter` (about 7 USD/month, 512 MB, tight) and recommends `standard` (about 25 USD/month, 1 CPU, 2 GB) for real traffic, region Frankfurt. The free plan below is only for a private test (switch `plan: free` in the blueprint); reasons and costs are in `PUBLIC.md`.

## Render free plan: what is true

Verified on Render's own pages (render.com/docs/free and /docs/deploys, 2026-11-12):

* A free web service **spins down after 15 minutes without inbound traffic** (HTTP requests, WebSocket messages); spinning up again takes about a minute.
* **750 free instance hours per month per workspace.** One service awake all month is about 744 hours (31 days), so it uses almost all of
  them; a second free service in the same workspace would run the hours out. When they run out the services stop until the next month.
* **The filesystem is ephemeral**: lost at every redeploy, restart and spin-down. That is why everything is kept in Firestore. Render may
  also restart a free service at any time. Anything Claude Code stores under `/home/zo/.claude` (its saved sessions) is lost the same way.
* No persistent disks, no SSH or shell into the service, a single instance only, no outbound mail ports (25, 465, 587).
* Render may suspend a free service that starts an unusually high volume of traffic. A ping every 8 minutes is tiny, so this does not apply.
* Build command limit: 120 minutes.

Not verified (I could not find it in the pages I read; check it in your dashboard): the free instance's RAM and CPU. From memory it is
512 MB RAM and a fraction of a CPU: enough for the server and one Claude Code process, but slow and tight if several run at once.

### Does the self-ping keep it awake?

The server calls its own `/healthz` every 8 minutes through `RENDER_EXTERNAL_URL`, the public address. Render counts inbound requests, and
this one arrives through Render's public address like any other, so it **should** keep the service awake. But Render's pages do not say
whether such a request counts, and nothing promises it, so I treat it as **not reliable until you have watched it**: after deploying,
leave the page alone for 30 minutes, then open it; if the first load takes about a minute, the ping did not work.
This section matters only on the free plan; a paid instance does not sleep. Better and independent of how Render counts: let an **outside pinger** call `https://<address>/healthz` every 5 to 10 minutes
(UptimeRobot or cron-job.org, both have a free plan; they only need the address, never a secret). Keep the self-ping too, it costs nothing.
Even with a pinger Render can restart the service now and then; the server then comes back with its data from Firestore.
If you would rather not keep it awake, do nothing: it sleeps and wakes with the first visit (a minute's wait), and uses fewer hours.

## Claude Code on the server (admin only)

Only admins (`ZO_ADMIN_EMAILS`) can use Claude Code mode unless the operator sets `ZO_CLAUDE_CODE_USERS=all` (see TERMINAL-AND-CLAUDE-LOGIN.md: a person then signs in to Claude with their OWN account on the page and uses it with Zohara Agent's safe tools only; Anthropic's terms do not allow third-party products to offer Claude subscription logins, so leave it on `admins` unless you accept that). The owner's own `claude setup-token` can be done from the web page (Settings > Model > Claude Code > Log in). The cloud terminal (admins only) can be switched off with `ZO_TERMINAL=off`.

Zohara Agent's Claude Code mode runs the real `claude` program (found on the PATH, `/usr/local/bin`) and connects it to `zo-mcp`.
On your computer you sign in with a browser; a server has none, so it needs a token from outside.

1. On **your own computer**, where Claude Code is installed and you are signed in: run `claude setup-token`. It opens the browser,
   then prints a long-lived token (valid one year according to Anthropic's docs). It does not save it anywhere. It needs a Pro, Max, Ultra,
   Team or Enterprise plan, and it can only make model requests (no Remote Control, no claude.ai connectors; local MCP servers such as `zo-mcp` work).
2. Render > the service > Environment > `CLAUDE_CODE_OAUTH_TOKEN` > paste > Save (this redeploys). The server passes its environment on to
   `claude`, so the variable reaches it (tested: with a made-up token, `claude` answered "401 OAuth access token is invalid", which shows it
   read the variable; I could not test a real token).
3. In the web app: Settings > provider Claude Code. Send a message.
4. After a year the token stops working (error "login expired" or 401): run `claude setup-token` again and replace the value.

Other ways: `ANTHROPIC_API_KEY` (a Console API key, pay per use) also works with `claude` and is the route Anthropic documents for
programs and servers; for Zohara Agent you can simply choose the Anthropic provider and enter the key in Settings, which needs no Claude Code at all.
Claude Code keeps its login and sessions in `~/.claude` and `~/.claude.json` (here `/home/zo`); with the token variable it needs no login file.

**Terms, honestly.** Anthropic's page (code.claude.com/docs/en/legal-and-compliance, read 2026-11-12) says subscription (OAuth) login is for
ordinary use of Claude Code by the person who bought the plan; developers building products must use API keys; nobody may offer
claude.ai login to other people or route their requests through a subscription, and developers may not collect or store Claude.ai
credentials. Using your own token for your own use, in the unmodified `claude` program, on your own server, is what that page leaves
room for, and Zohara Agent never reads or stores the token (Render's environment holds it). It does not say anything specific about an
always-on server, so Anthropic could still treat heavy automated use differently. Do not share the address or the token with other people. If you
want to be certain, use an API key.

## What was tested here (2026-11-12, with podman on the owner's computer)

* `podman build` of this Dockerfile with `--locked` works: `Cargo.lock` did not need to change, and `cargo build -p zo-server -p zo-mcp`
  needs no GTK or libadwaita (the Rust image has none).
* Container run with `ZO_SERVER_NO_AUTH=1`: `/healthz` answers `ok`, `/` serves the web page, `POST /api/op {"op":"status"}` answers,
  the server process is user 10001 (`zo`, not root), the data, config and socket directories are writable, `claude --version` (2.1.322),
  `ssh -V`, `git` work, and `podman stop` ends it in 0.2 seconds (tini).
* Claude Code mode inside the container with a fake `CLAUDE_CODE_OAUTH_TOKEN`: the message reached `claude`, which rejected the token with 401.
  So the variable, the writable `HOME` and `zo-mcp` lookup work; only a real token was not tried.
* Not tested: Render itself, a real Firebase key (login, Firestore) from the container, a real Claude Code token, the self-ping (needs `RENDER_EXTERNAL_URL`),
  the Docker daemon (`docker` was not running, podman builds the same Dockerfile; the `HEALTHCHECK` line is ignored in podman's default image
  format, so it was not exercised), and the real binding on `0.0.0.0` (in no-login mode the server listens on 127.0.0.1 only, so the checks
  ran from inside the container; real mode binds 0.0.0.0).

## Needs a change in the code (for whoever merges)

* `crates/zo-core/src/ai/claude_code.rs`: no change is needed for the token (the child inherits the environment).
* Its error texts tell the user to run `claude auth login`; on a server the useful hint is to set `CLAUDE_CODE_OAUTH_TOKEN`.
* `run` stores the Claude session id and next time starts `claude --resume <id>`. The sessions live in `/home/zo/.claude`, which is
  lost at every redeploy or sleep, so after that the resume fails. It should retry without `--resume` (sending the chat history as context).
* `main.rs` only listens on `0.0.0.0` with the login on; `ZO_BIND` (planned in the sync branch) would allow testing the image from outside the container.

## The public address (2026-11-14)

The service answers at **https://zohara-agent.duckdns.org** (a free DuckDNS name pointed at Render: Render > the service > Settings > Custom Domains, and the A record DuckDNS holds for the name). All built-in defaults (desktop app, `zo`, web page, Android app, sitemap) now use it.

- The old Render address `zo-ai-server.onrender.com` keeps working as long as it stays in Render and in Firebase > Authentication > Settings > Authorized domains. Installs made before this date saved or defaulted to it; remove it only after those have updated.
- Set `ZO_PUBLIC_URL` to `https://zohara-agent.duckdns.org` in Render.
- Add `zohara-agent.duckdns.org` to Firebase Authorized domains (host only).
- The DuckDNS token is a secret: keep it in Render's environment, never in the repository.
- A DuckDNS name is not owned by us: log in to DuckDNS now and then so the name is not released, and move to a real domain before promoting the service widely (point the new domain at Render, add it to Firebase, then change the defaults again).

## Pages Google Play needs from the server

The Play listing links to `https://zohara-agent.duckdns.org/privacy` (privacy policy) and `https://zohara-agent.duckdns.org/delete-account` (account deletion page). Both are served without sign-in
and their contact line comes from `ZO_CONTACT_EMAIL`, so that variable must be a real, read inbox before Play review. See `docs/PLAY-STORE.md`.
