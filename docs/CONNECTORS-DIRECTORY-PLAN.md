# Connectors directory: browse and add remote MCP servers in the app (owner's request, 2026-11-14)

Status: specified, not built. Build after the memory feed / Scheduled v2 / migration items (docs/MEMORY-AND-SCHEDULE-PLAN.md, docs/VM-MIGRATION-PLAN.md).

## Why
Remote MCP servers (an `https://` address, like the Spotify one) are added by hand today. Claude's apps show a directory of connectors to add with one click. Zohara Agent should do the same.

## Source (checked 2026-11-14)
The official MCP registry has a public JSON API without a key: `https://registry.modelcontextprotocol.io/v0/servers?limit=N&cursor=...` (also `search=` text search where supported). Each entry: name (reverse-DNS id like `ac.mizan/registry`), title, description, version, websiteUrl, icons, `remotes` (type `streamable-http` or `sse`, url), `packages` (local npm/pypi/docker servers), and `_meta` with status and `isLatest`. Keep only: status active, isLatest true, and at least one remote with an `https://` url (remote servers only; local `packages` entries are listed in a second tab "Runs on this computer" and need Node/Python, see below). Plus a small hand-curated "Featured" list (well-known connectors: Notion, GitHub, Linear, Slack, Atlassian, Asana, Sentry, Stripe, Cloudflare, Figma, Intercom, PayPal, Zapier, Spotify...) that is shown first, each checked to exist in the registry or to have a documented remote address; no entry is shown that was not verified to answer.

## How it works
- The **server** fetches and caches the registry (1 hour, stale-while-error, size-limited, JSON only), and exposes `GET /api/connectors?q=&cursor=&kind=remote|local` (needs sign-in, rate limited). Clients never call the registry directly (privacy: the registry never sees user IPs or searches). The desktop app talks to the same endpoint, or to the registry directly when the person is not signed in (documented, off by default? decide: signed-out desktop uses the registry directly through a plain GET with no identifiers).
- **UI** on desktop (Settings > Connected apps > "Browse connectors"), web and Android: search box, categories (derived from keywords: dev, docs, chat, data, finance, media, search, other), cards with icon, title, publisher domain, description, and an **Add** button; already-added ones show "Added".
- **Add** reuses the existing flow: address check (public https only, same SSRF guard as today), try the server, sign in automatically if it asks (the existing OAuth for MCP), show the tool list, then save. Servers that need a key/header open a small form that names the header ("Authorization: Bearer ...") and stores it in the key store. Remote servers sync between devices by address as today (held until turned on on each device).
- **Local** entries (npm/pypi packages): shown with "Needs Node/Python on this computer or a VM"; Add writes the `npx -y <pkg>` / `uvx <pkg>` command after a confirmation that names what will run on the computer (it is third-party code). Never on the cloud server for non-owner accounts.
- **Trust**: show where it comes from (publisher domain from the id, websiteUrl), label "Not reviewed by Zohara Agent" except Featured; a confirm before adding that lists what the connector will be able to read (its tool names after connecting) and that tool calls still ask for approval in Manual/Auto mode.
- **Restricted accounts** on the cloud get no MCP at all (unchanged); the browse screen explains that.
- **Updates**: a connector whose registry address changed since it was added shows "Address changed" and is held until the person confirms (same rule as sync).

## Tests and docs
Parser tests with recorded registry JSON (including entries with several remotes, only packages, deprecated status), the filter, cache and failure fallback, SSRF rejection of non-public or http addresses, the Add flow with a fake MCP server, UI tests on the three clients. Docs: docs/MCP.md gets a "Browse connectors" section.
