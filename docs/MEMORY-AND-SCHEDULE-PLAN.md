# Memory feed, project memory and Scheduled v2 (owner's request, 2026-11-14)

Status: specified, not built. Build after the "updates / Play / greetings / persona / questions" workflow has merged.

## How things work today (read before changing)

**Memory engine.** `crates/zo-core/src/ai/memory_agent.rs`: after each saved answer `after_turn` runs. When the person has written N new messages in a chat (or an idle chat has unread messages) a background job asks the chosen model (Claude Code uses the small `haiku`) to read the new part of the chat and answer with JSON: notes to add, update or delete. `apply` writes them; a last guard drops anything that looks like a key or password. It does nothing when memory is off or no model is set. Notes live in `store/memory.rs` (kinds: fact, preference, person, project, episode; vector search).

**Scheduled tasks.** `crates/zo-core/src/store/schedules.rs`. A task = title + prompt (an instruction in words) + next run time + repeat (once, hourly, daily, weekly, monthly, or "every N minutes|hours|days|weeks|months", at least 5 minutes) + enabled + optional project + last run + the chat the last run left behind. Made on the Scheduled page or by asking in a chat ("remind me tomorrow 9am to ..."). Times are read by the system `date` ("tomorrow 9am", "next monday", "2026-11-05 18:30"; known bug: "in 2 hours" is rejected). **Only the desktop app runs tasks**: `crates/zohara-agent/src/scheduler.rs` wakes up regularly, takes every due task, opens a normal chat with the prompt (so tools, approvals and the model work as in any chat), saves the chat and marks the run (a repeating task moves to its next time, a one-off switches itself off). So a task never runs while the desktop app is closed or the computer is off; the cloud server and phone can add and cancel tasks but do not run them. The desktop has "Run now" and on/off; the service has no op for those, so web and phone cannot.

## 1. Memory feed ("Memory updated · View")

- When the memory engine changes anything, record a change entry: {time, chat id/title, added/updated/deleted notes with before and after text}, kept in a small log (last 200) next to the notes and synced like notes.
- Above the composer in every client show a slim chip: "Memory updated · View" with a round arrow button. It appears when a job finishes with changes, fades after about 12 s or when dismissed, and a click opens a sheet listing exactly what changed (grouped: added, updated, deleted) with an Undo per item and a link to the full memory page.
- Service: new event `memory_updated {summary, counts}` on the stream (and on desktop via the in-process hook), ops `memory_changes` (list) and `memory_undo {id}`.
- Setting: "Show memory updates" (default on). The engine's frequency (every 2-3 messages) stays a setting with default 3 and a floor of 2.
- Restricted/cloud people see only their own log. The log never contains secrets (same guard as the notes).

## 2. Memory per project

- Each project gets its own notes (scope = project id). Chats inside a project read and write project notes in addition to global ones; project notes never leak into other projects or chats outside the project.
- The memory page gets a scope switch (All / Global / one project). The engine decides scope per note (a fact about the project goes to the project scope, a fact about the person stays global); the feed shows the scope.
- Deleting a project deletes its notes (after a confirm); export/import include them.

## 3. Scheduled v2

- **Run now** and **on/off** for every client (service ops `schedule_run_now`, `schedule_enable {id,on}`), with the result shown (last run time, last chat, success/failed, last short output).
- **Task kinds**: (a) Task = a prompt run by the agent with tools (today); (b) Reminder = a notification only at the time ("remind me to call mom", repeat allowed), needs no model; (c) Browser task = a prompt that is allowed to use the browser/web tools (open pages, read, fill, collect) and reports back into a chat; each kind has its own permission mode (reminders none, tasks follow the chosen mode, browser tasks default to ask-first and are never allowed to buy or send anything without approval).
- **Where it runs**: pick "this computer" (needs the desktop app running; show the next-run time and a warning when it is off) or "cloud" (server-side, only for the allowed people, restricted tenants only get reminders and web search/fetch tasks, with per-person limits). A missed run (computer was off) runs once when it comes back, with a note "missed 09:00".
- **Notifications**: finished task -> notification with the result summary (desktop, web push, Android); failures say why.
- **Natural time**: fix "in N hours/minutes"; accept "every weekday at 9", "every day at 07:30", "on the 1st of each month"; show a preview "next run: Sat 14 Nov 09:00" before saving.
- **UI** on desktop, web and Android: list with next run, last result, on/off switch, Run now, Edit, History (last 20 runs, each linking to its chat).
- Write docs/SCHEDULED.md with all of this for people (what a task is, what runs where, examples).

## Order of work
1. Memory change log + feed chip (core, then desktop/web/Android).
2. Project memory scope.
3. Scheduled v2 core ops and the runner (desktop), then the cloud runner and the UI in all clients.
