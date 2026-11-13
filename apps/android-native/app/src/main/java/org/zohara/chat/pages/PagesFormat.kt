package org.zohara.chat.pages

import java.text.DateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import org.zohara.chat.core.net.ActivityItem
import org.zohara.chat.core.net.ArtifactItem
import org.zohara.chat.core.net.ChatEntry
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.core.net.ClientSeen
import org.zohara.chat.core.net.ScheduledTask

/**
 * The wording and the small rules of the pages module, as pure functions so unit tests can pin them down.
 * Every string here is the desktop's or the web's own (docs/ANDROID-NATIVE-PARITY.md rows LIST, PROJ, ART, SCHED, ACT).
 */
object PagesFormat {
  // ------------------------------------------------------------ time

  /** The desktop's `model::relative` (crates/zo-core/src/store/projects.rs): "just now", "5 min ago", "1 hour ago", "3 days ago", then "Nov 4". */
  fun relative(ts: Long, now: Long, zone: TimeZone = TimeZone.getDefault()): String {
    val secs = maxOf(0L, now - ts) / 1000
    return when {
      ts <= 0 -> ""
      secs < 60 -> "just now"
      secs < 3600 -> "${secs / 60} min ago"
      secs < 86_400 -> (secs / 3600).let { h -> if (h == 1L) "1 hour ago" else "$h hours ago" }
      secs / 86_400 < 30 -> (secs / 86_400).let { d -> if (d == 1L) "1 day ago" else "$d days ago" }
      else -> {
        val cal = java.util.Calendar.getInstance(zone, Locale.ENGLISH).apply { timeInMillis = ts }
        val months = arrayOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")
        "${months[cal.get(java.util.Calendar.MONTH)]} ${cal.get(java.util.Calendar.DAY_OF_MONTH)}"
      }
    }
  }

  /** The web's `when()`: a medium date and a short time in the person's locale ("" for 0). */
  fun whenText(ms: Long, locale: Locale = Locale.getDefault(), zone: TimeZone = TimeZone.getDefault()): String {
    if (ms <= 0) return ""
    val f = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, locale)
    f.timeZone = zone
    return f.format(Date(ms))
  }

  /** The running clock of an Activity row (desktop `clock`): "12 s", "3 min 5 s", "1 h 2 min". */
  fun clock(ms: Long): String {
    val s = maxOf(0L, ms) / 1000
    return when {
      s < 60 -> "$s s"
      s < 3600 -> "${s / 60} min ${s % 60} s"
      else -> "${s / 3600} h ${(s % 3600) / 60} min"
    }
  }

  // ------------------------------------------------------------ chats

  const val RECENTS_LIMIT = 8
  /** the service lists the newest 60 chats and has no paging (LIST-13) */
  const val SERVICE_CHAT_LIMIT = 60
  const val OLDER_CHATS_NOTE = "Older chats can be found with Search"

  fun title(c: ChatSummary) = c.title.ifBlank { "Untitled" }

  /** NAV-09, NAV-10: pinned chats by last update (all of them), and the newest 8 unpinned ones. */
  data class Sections(val pinned: List<ChatSummary>, val recents: List<ChatSummary>)

  fun sections(chats: List<ChatSummary>): Sections = Sections(
    pinned = chats.filter { it.pinned }.sortedByDescending { it.updated },
    recents = chats.filter { !it.pinned }.sortedByDescending { it.updated }.take(RECENTS_LIMIT),
  )

  /** LIST-12: the plain text of a conversation for the share sheet. */
  fun transcript(title: String, entries: List<ChatEntry>): String = buildString {
    if (title.isNotBlank()) append(title).append("\n\n")
    for (e in entries) {
      when (e.kind) {
        "msg-user" -> append("You: ").append(e.text.trim()).append("\n\n")
        "msg-ai" -> append("Zohara Chat: ").append(e.text.trim()).append("\n\n")
        "msg-error" -> append("Error: ").append(e.text.trim()).append("\n\n")
        // tool lines are working notes, not the conversation
      }
    }
  }.trimEnd()

  // ------------------------------------------------------------ projects (PROJ-01)

  data class Project(val id: String, val name: String, val goal: String, val memoryNotes: String, val extra: String) {
    /** the card's second line: every column after the name joined with " · " (as the web does) */
    val meta: String get() = listOf(goal, memoryNotes, extra).filter { it.isNotBlank() }.joinToString(" · ")
  }

  /**
   * The `projects` op answers with text: one line per project, the columns separated by vertical bars: id, name, then the goal and
   * "N memory notes". "There are no projects." means none. Lines with fewer than two columns are ignored (as the web does).
   */
  fun parseProjects(text: String): List<Project> {
    val t = text.trim()
    if (t.isEmpty() || Regex("no projects", RegexOption.IGNORE_CASE).containsMatchIn(t)) return emptyList()
    return t.lines().map { l -> l.split("|").map { it.trim() } }.filter { it.size >= 2 }.map { c ->
      val rest = c.drop(2)
      val notes = rest.firstOrNull { Regex("""^\d+ memory notes?$""").matches(it) }.orEmpty()
      val others = rest.filter { it != notes && it.isNotBlank() }
      Project(id = c[0], name = c[1], goal = others.firstOrNull().orEmpty(), memoryNotes = notes, extra = others.drop(1).joinToString(" · "))
    }
  }

  /** PROJ-03: the client-side filter of the Projects page: by name and goal, case-insensitive; a blank query keeps all. */
  fun filterProjects(list: List<Project>, query: String): List<Project> {
    val q = query.trim()
    if (q.isEmpty()) return list
    return list.filter { it.name.contains(q, ignoreCase = true) || it.goal.contains(q, ignoreCase = true) }
  }

  /** LINK-15: the name shown when the linked computer's name is not known (web `devName` fallback) */
  const val COMPUTER_FALLBACK = "Your computer"

  // ------------------------------------------------------------ artifacts (ART-01)

  /** the sub line: the path, or "N lines of code", then the relative time */
  fun artifactSubline(a: ArtifactItem, now: Long, zone: TimeZone = TimeZone.getDefault()): String {
    val what = when {
      a.path.isNotBlank() -> a.path
      !a.text.isNullOrEmpty() -> "${lineCount(a.text)} lines of code"
      else -> ""
    }
    val rel = relative(a.created, now, zone)
    return listOf(what, rel).filter { it.isNotBlank() }.joinToString(" · ")
  }

  /** the desktop counts with Rust's `lines()`: a final newline does not start another line */
  fun lineCount(text: String): Int = text.lines().let { if (it.size > 1 && it.last().isEmpty()) it.size - 1 else it.size }

  /** What the viewer sheet shows and copies: the code, else the path. */
  fun artifactBody(a: ArtifactItem): String = a.text?.takeIf { it.isNotEmpty() } ?: a.path

  // ------------------------------------------------------------ scheduled (SCHED-01, SCHED-04)

  /** the card's title: the task's title, or its prompt */
  fun taskTitle(t: ScheduledTask) = t.title.ifBlank { t.prompt }

  /** "Next: <date> · <repeat>" or "Off · <repeat>" (the service does not report the last run, so "Done" cannot be told) */
  fun taskWhen(t: ScheduledTask, locale: Locale = Locale.getDefault(), zone: TimeZone = TimeZone.getDefault()): String {
    val rep = t.repeat.ifBlank { "once" }
    return if (t.enabled) "Next: ${whenText(t.nextRun, locale, zone)} · $rep" else "Off · $rep"
  }

  /** the repeat choices of the New task sheet, label -> value for `schedule_add` */
  val REPEAT_CHOICES = listOf("Once" to "once", "Every hour" to "hourly", "Every day" to "daily", "Every week" to "weekly", "Every month" to "monthly")

  /** the own interval wins over the choice (SCHED-04) */
  fun repeatValue(choice: String, own: String): String = own.trim().ifEmpty { choice }

  // ------------------------------------------------------------ activity (ACT-*)

  fun kindWord(kind: String): String = when (kind) {
    "answer" -> "Chat"
    "claude_code" -> "Claude Code"
    "scheduled" -> "Scheduled task"
    "memory_job" -> "Memory"
    "command" -> "Command"
    "terminal" -> "Terminal"
    "sync" -> "Sync"
    "client" -> "Client"
    else -> "Task"
  }

  fun sourceWord(source: String): String = when (source) {
    "desktop" -> "this app"
    "phone" -> "the phone"
    "web" -> "the web app"
    "web-link" -> "a web session"
    "cli" -> "the command line"
    "scheduler" -> "the scheduler"
    "background" -> "in the background"
    else -> source
  }

  /** "Chat from the web app", with the detail after it */
  fun activitySub(i: ActivityItem): String {
    val from = if (i.source.isBlank()) kindWord(i.kind) else "${kindWord(i.kind)} from ${sourceWord(i.source)}"
    return listOf(from, i.detail).filter { it.isNotBlank() }.joinToString(" · ")
  }

  /** clients are not "running": they are counted as "N connection(s) open" (ACT-08) */
  fun running(items: List<ActivityItem>) = items.filter { it.kind != "client" }
  fun clients(items: List<ActivityItem>) = items.filter { it.kind == "client" }
  fun connectionsLine(n: Int) = "$n connection${if (n == 1) "" else "s"} open"

  /** NAV-15: the avatar's content description is built by the avatar; this is the title text of the web */
  fun runningTitle(n: Int) = "$n ${if (n == 1) "thing is" else "things are"} running"

  /**
   * The elapsed time of a running item at local time [localNow]: the server's own clock difference (`now_ms` - `started_ms`) plus what has passed here
   * since the answer arrived at [fetchedAt], so a phone whose clock is wrong still counts right.
   */
  fun elapsedMs(item: ActivityItem, serverNow: Long, fetchedAt: Long, localNow: Long): Long {
    if (item.startedMs <= 0) return 0
    val base = if (serverNow > 0) serverNow - item.startedMs else fetchedAt - item.startedMs
    return maxOf(0L, base + (localNow - fetchedAt))
  }

  /** ACT-06: how the web app and the command line are doing, from `clients_seen` */
  fun clientsSeenText(seen: List<ClientSeen>, serverNow: Long): String {
    val last = seen.maxOfOrNull { it.lastMs } ?: return "Not connected yet"
    if (last <= 0) return "Not connected yet"
    val ago = maxOf(0L, serverNow - last) / 1000
    return if (ago < 30) "Connected, last request $ago s ago" else "Last seen ${relative(last, serverNow)}"
  }

  val BACKGROUND_SERVICES = listOf(
    "Zohara Chat Service" to "Listening for the phone, the web app and the command line",
    "Memory agent" to "Active — summarizes and extracts memories between chats",
    "Sync loop" to "On — keeps chats and presets backed up to your account",
  )
  const val DESKTOP_LINK_STANDBY = "Standby — link a computer in Settings > My computers"

  // ------------------------------------------------------------ terminal (TERM-*)

  const val TERMINAL_WELCOME = "Zohara Chat Terminal\nConnected. Type a shell command below (e.g. ls, pwd, df -h, uname -a)."
  const val TERMINAL_ON_COMPUTER = "Commands on your own computer need your approval there: choose My computer under the chat box and ask in the chat (the desktop app asks you first)."
  const val TERMINAL_PLACEHOLDER = "Type a shell command..."

  // ------------------------------------------------------------ copy of the states (same words as the desktop and the web)

  const val CHATS_EMPTY_TITLE = "No chats yet"
  const val CHATS_EMPTY_TEXT = "Start one with New."
  const val SEARCH_PLACEHOLDER = "Search your chats"
  const val SEARCH_EMPTY_TITLE = "No chats yet"
  const val SEARCH_EMPTY_TEXT = "Start one and it will show up here."
  const val SEARCH_NONE_TITLE = "Nothing found"
  const val SEARCH_NONE_TEXT = "Try other words."
  const val PROJECTS_EMPTY_TITLE = "No projects yet"
  const val PROJECTS_EMPTY_TEXT = "Projects keep one long-running thing together — its files, notes and context — so Zohara Chat keeps working inside it."
  const val ARTIFACTS_EMPTY_TITLE = "Nothing made yet"
  const val ARTIFACTS_EMPTY_TEXT = "Files Zohara Chat writes, screenshots it takes and code it shows you are kept here."
  const val SCHEDULED_EMPTY_TITLE = "Nothing is scheduled"
  const val SCHEDULED_EMPTY_TEXT = "Ask Zohara Chat in a chat to do something later (for example \"every morning at 8, check my disk space\"), or press New task."
  const val ACTIVITY_EMPTY_TITLE = "Nothing is running"
  const val ACTIVITY_EMPTY_TEXT = "Chats being answered, scheduled tasks, the memory agent and long commands show up here while they work."
  const val DELETE_CHAT_TITLE = "Delete this chat?"
  const val DELETE_CHAT_TEXT = "The conversation is removed. This cannot be undone."
  const val CANCEL_TASK_TITLE = "Cancel this task?"
  const val CANCEL_TASK_TEXT = "It will not run again."
  const val CANCEL_TASK_BUTTON = "Cancel task"
}
