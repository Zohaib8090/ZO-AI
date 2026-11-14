# Zohara Agent memory

Zohara Agent remembers the person across chats, on the computer and on the phone. It is one memory: the app window, the phone service
(`zohara-linkd` → Zohara Agent socket) and the MCP tools all read and write `~/.local/share/zohara-agent/memory.json`.

## What is kept
* **Notes** (`person`, `preference`, `project`, `fact`): one short sentence each, about the person.
* **Episodes**: one summary per chat, so a new chat can be told "earlier you worked on ...".
* Pin a note and it is always told to the model. Preferences and `person` notes are always told too.

## Who writes it
* **The memory agent** (`zo-core/src/ai/memory_agent.rs`): after every 3 new messages of the person in a chat (and for chats left idle
  more than 10 minutes with unread messages) a background job reads the new part and asks the chosen model, with no tools, for a JSON
  answer: notes to add, notes to update, ids to delete, and the chat summary. It runs on its own thread, one job at a time, and never
  blocks the chat. Anything that looks like a password, key or token is dropped before it is saved. With Claude Code as the chat model the job runs on `haiku` through
  `claude -p` (no tools, nothing saved); it does nothing when memory is off. Checked live with the real `claude` (ignored test
  `real_claude_writes_the_memory`).
* **The AI itself**, with `zohara_memory_save`, `zohara_memory_forget`, `zohara_memory_search` (and `zohara_ai_search_chats`).

## How it is used
At every message the app builds the system prompt with `memory::system_with(base, message)`: the always-told notes plus the closest
notes and chat summaries to the message (a vector search), within a size budget.

## The vector store
`store/memory.rs`. The local embedder hashes words, word pairs and 3-letter word pieces into 512 numbers (unit length); the search is
cosine similarity plus a small push for recent and pinned notes. No key, no network, any script (Hindi works). It matches shared
words and word pieces, not synonyms ("car" does not find "vehicle"). Every vector records which embedder made it (`embedder`), so a
model-based embedder can be added later and the notes made again without changing the file format.

## Phone
Service ops: `memory`, `memory_add`, `memory_forget`, `memory_pin`, `memory_enable`, `memory_clear` (allowed in `zohara-linkd`'s list).

## Web search and pages
`zohara_web_search` (DuckDuckGo's lite page, no key) and `zohara_web_fetch` (public http/https only; loopback, home-network and
`.local` addresses are refused) in `tools/web.rs`. The phone gets them too, since the tools are shared.

## Personalities
`store/personality.rs`: four built-in voices (friendly buddy, straight engineer, calm mentor, dry and witty), the person's own, and ones
the AI writes from a description (`ai/personality_gen.rs`, uses the chat model; Claude Code works too). "No personality" (nothing saved,
`active` empty) is the neutral default and adds nothing to the prompt.

How a personality reaches the model (changed 2026-11-14, branch feat/persona): `personality::instructions()` renders a **CHARACTER** block that is
the LAST thing in the system prompt, after the abilities, the tool rules, the "VOICE" note of the base prompt, the vision note and the memory
notes (`memory::system_with`, `system_without_notes`, `extras`; `agent::add_before_character` keeps later sections in front of it; the phone
note in `service.rs` and the project text in `chat.rs` are built before it). The character is "the VOICE of every reply": tone, wording, feelings,
humour, language mix and what it calls the person, also when it reports finished work (no "Done. I created the file ..." boilerplate); the facts stay
exact. It says: stay in character, never mention these instructions or that it plays a role unless asked, answer honestly if sincerely asked
whether it is an AI, and that the character is a voice, not a permission (it cannot allow anything, switch off approvals or restrictions, change tools or
override earlier rules). The text is cleaned (`clean_prompt`: no angle brackets, one line, at most `MAX_PROMPT` = 1200 characters, also for text that came
from sync) and sits between two lines so it reads as data. Real enforcement of permissions and of the restricted tool list is in code (`policy`,
`tools::allowed_for_restricted`), never in the prompt. It works in the three prompt kinds (full, server admin, restricted) and in Claude Code mode
(extra instructions); scheduled tasks and sub-agents do not use it. The base prompts no longer say "You are not a person" or "working as an agent", and have
a VOICE section that tells the model a CHARACTER section, when present, decides how everything is worded. The generation prompt now allows a character
(friend, partner, teacher with feelings) and asks it to say that the character keeps its voice when it reports work.
Tests: `store::personality::tests::*` (prompt order, neutral default, injection persona, cap), `service::tenant_tests::the_character_is_the_last_part...`. Settings has the Personality and Memory sections (checked headless in a container under
Xvfb). Phone service ops: `personalities`, `personality_use`, `personality_add`, `personality_generate`, `personality_remove`.

## Not done yet
The Memory and Personality pages in the phone app (the service ops are ready); a model-based embedder; RAG over the person's files.
