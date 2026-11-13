# MCP in Zohara Chat: what other apps do, and what Zohara Chat does

Written 2026-11-08 from the MCP specification (version 2026-07-28), Claude's connector documentation and the ChatGPT/Codex MCP
documentation. Zohara Chat is an **MCP client** (it uses other people's servers: `crates/zo-core/src/mcp_client.rs`, `mcp_oauth.rs`) and an
**MCP server** (`zo-mcp`: lets Claude Code or any client control this computer).

## How the apps connect

| | Claude (web, Desktop, mobile) | ChatGPT / Codex | Zohara Chat |
|---|---|---|---|
| Local program (stdio) | Claude Desktop: `claude_desktop_config.json` (`command`, `args`, `env`) or a `.mcpb` extension | `command`, `args`, `env`, `cwd` | `command` + `env`; "Paste a config" reads the same JSON |
| Remote server | Streamable HTTP, by URL ("custom connectors") | Streamable HTTP (older HTTP+SSE also), by URL | Streamable HTTP, by URL |
| Static key | `Authorization` / `X-API-Key` headers | bearer token, `http_headers` | `headers` |
| Sign-in | OAuth 2.0 with PKCE | OAuth 2.0 with PKCE | OAuth 2.0 with PKCE |

## OAuth and the redirect address

The redirect address belongs to the **app that signs in**, not to the server:

* Claude's hosted apps use one fixed address, `https://claude.ai/api/mcp/auth_callback` (also `claude.com`).
* ChatGPT uses `https://chatgpt.com/connector_platform_oauth_redirect`.
* A desktop program (Claude Code, Codex, Zohara Chat) opens a small web address **on your own computer**, `http://127.0.0.1:<port>/callback`
  (RFC 8252), and the browser sends the answer there. Servers should accept any port for these.

How a client gets known to a server (newest first in the 2026-07-28 spec): a **Client ID Metadata Document** (the client's id is an
https address that describes it), a client **registered by hand** (id and secret typed in), or **Dynamic Client Registration**
(RFC 7591, now marked old but still the most common). Zohara Chat does registered-by-hand and Dynamic Client Registration. A metadata
document needs a public https address for Zohara Chat; that is not set up yet.

The steps Zohara Chat follows: the server answers 401 with a `WWW-Authenticate` header that names the page describing it (RFC 9728) → that
page names the authorization server → its metadata (RFC 8414) gives the addresses → register → open the browser with PKCE and the
`resource` of the server (RFC 8707) → catch the code on the local address → trade it for tokens, which are kept in the keyring.
Scopes: the ones you typed, else the ones the 401 named, else everything the server lists, plus `offline_access` when allowed.
Access tokens are renewed shortly before they run out (Claude renews 5 minutes before) and again on a 401.

## What Zohara Chat supports

* Tools, including servers that hand out their tool list in pages. A tool with `readOnlyHint` runs without asking in Auto mode, the
  rest ask (like Codex's "writes" mode).
* Resources and prompts: through four tools per server (`zo_resources_list`, `zo_resources_read`, `zo_prompts_list`, `zo_prompts_get`).
* A server's own `instructions` are given to the model.
* Remote: `MCP-Protocol-Version` and `Mcp-Session-Id` headers, a new session when the server forgot the old one, the session closed
  at the end, replies as JSON or as an event stream, a site address without `/mcp` (it is found), custom headers, a pasted token.
* Sign-in: automatic registration, or a client id and secret you registered; a fixed local port; your own scopes; token in the form
  or in a Basic header; `invalid_grant` is reported as "sign in again".
* Adding: by hand, by pasting the JSON other apps use (`mcpServers`, `servers`, a bare server), or by asking the AI
  (`zohara_ai_add_mcp_server`, `zohara_ai_add_mcp_config`).

## Not supported yet

* The old HTTP+SSE transport (servers that only have a `/sse` address).
* Client ID Metadata Documents; step-up sign-in when a call is refused for a missing scope (403 `insufficient_scope`).
* Server-initiated requests: sampling (the server asks the model), roots, elicitation (the server asks you a question), and
  notifications such as "the tool list changed" (the list is read again at the start of each message, so this only matters
  inside one message).
* Showing resources and prompts in the interface (the model can use them; there is no `@` picker or `/` list yet).
* Extensions in the `.mcpb` format.
