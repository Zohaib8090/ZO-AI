package org.zohara.chat.pages

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.chat.core.net.ActivityItem
import org.zohara.chat.core.net.ArtifactItem
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.core.net.ClientSeen
import org.zohara.chat.core.net.ScheduledTask
import org.zohara.chat.core.net.SearchHit

/** Loads one list; calls overlapping a running load are dropped (a pull while a poll runs does not start a second call). */
abstract class ListHolder<T>(protected val scope: CoroutineScope) {
  private val _state = MutableStateFlow(Loadable<T>())
  val state: StateFlow<Loadable<T>> = _state.asStateFlow()
  private var job: Job? = null

  protected abstract suspend fun load(): T

  fun refresh() {
    if (job?.isActive == true) return
    _state.update { it.copy(loading = true) }
    job = scope.launch {
      val r = attempt { load() }
      _state.update { cur ->
        r.fold({ Loadable(it, false, null) }, { Loadable(cur.value, false, it.sentence()) })
      }
    }
  }

  protected fun setValue(v: T) { _state.update { it.copy(value = v) } }
  protected val current: T? get() = _state.value.value
}

// ------------------------------------------------------------------ chats (LIST-01..04, 08, 09, 12; NAV-09..12)

/** The chat list, shared by the Chats page, Search (recents) and the sidebar. */
class ChatsHolder(private val api: PagesApi, scope: CoroutineScope) : ListHolder<List<ChatSummary>>(scope) {
  override suspend fun load() = api.chats()

  /** Deletes a chat: it leaves the list at once, comes back when the server refuses. Returns the error sentence, or null. */
  suspend fun delete(id: String): String? {
    val before = current
    if (before != null) setValue(before.filter { it.id != id })
    val r = attempt { api.deleteChat(id) }
    return r.fold({ refreshNow(); null }, { if (before != null) setValue(before); it.sentence() })
  }

  /** Pins or unpins a chat (LIST-08); same optimistic rule. */
  suspend fun pin(id: String, on: Boolean): String? {
    val before = current
    if (before != null) setValue(before.map { if (it.id == id) it.copy(pinned = on) else it })
    val r = attempt { api.pinChat(id, on) }
    return r.fold({ refreshNow(); null }, { if (before != null) setValue(before); it.sentence() })
  }

  private fun refreshNow() = refresh()

  /** LIST-12: the transcript of a chat as text, or an error sentence */
  suspend fun transcript(id: String): Result<String> =
    attempt { api.chat(id).let { PagesFormat.transcript(it.title, it.entries) } }
}

// ------------------------------------------------------------------ search (LIST-05, 06)

data class SearchUi(val query: String = "", val loading: Boolean = false, val hits: List<SearchHit>? = null, val error: String? = null)

class SearchHolder(private val api: PagesApi, private val scope: CoroutineScope, private val debounceMs: Long = 200) {
  private val _ui = MutableStateFlow(SearchUi())
  val ui: StateFlow<SearchUi> = _ui.asStateFlow()
  private var job: Job? = null

  /** A new text: waits [debounceMs] after the last key, then searches. An empty text shows the recent chats (hits null). */
  fun setQuery(q: String) {
    job?.cancel()
    if (q.isBlank()) {
      _ui.value = SearchUi(query = q)
      return
    }
    _ui.update { it.copy(query = q, loading = true, error = null) }
    job = scope.launch {
      delay(debounceMs)
      val r = attempt { api.search(q.trim()) }
      _ui.update { cur ->
        if (cur.query != q) cur
        else r.fold({ cur.copy(loading = false, hits = it, error = null) }, { cur.copy(loading = false, error = it.sentence()) })
      }
    }
  }
}

// ------------------------------------------------------------------ projects (PROJ-01, 02, 05)

class ProjectsHolder(private val api: PagesApi, scope: CoroutineScope) : ListHolder<List<PagesFormat.Project>>(scope) {
  override suspend fun load() = PagesFormat.parseProjects(api.projects())

  /** Creates a project; returns null when it worked (the list reloads), else the server's sentence. */
  suspend fun create(name: String, goal: String): String? =
    attempt { api.projectCreate(name.trim(), goal.trim()) }.fold({ refresh(); null }, { it.sentence() })
}

// ------------------------------------------------------------------ artifacts (ART-01..05)

class ArtifactsHolder(private val api: PagesApi, scope: CoroutineScope) : ListHolder<List<ArtifactItem>>(scope) {
  override suspend fun load() = api.artifacts()
}

// ------------------------------------------------------------------ scheduled (SCHED-01..04)

/** The fields of the New task sheet. */
data class TaskForm(val prompt: String = "", val whenText: String = "", val repeatChoice: String = "once", val ownInterval: String = "") {
  val valid: Boolean get() = prompt.isNotBlank() && whenText.isNotBlank()
}

class ScheduledHolder(private val api: PagesApi, scope: CoroutineScope) : ListHolder<List<ScheduledTask>>(scope) {
  override suspend fun load() = api.scheduled()

  /** Cancels a task; the card leaves at once. Returns an error sentence or null. */
  suspend fun cancel(id: String): String? {
    val before = current
    if (before != null) setValue(before.filter { it.id != id })
    return attempt { api.scheduleCancel(id) }.fold({ refresh(); null }, { if (before != null) setValue(before); it.sentence() })
  }

  /** Adds a task. Returns Result of the server's message; the list reloads on success. */
  suspend fun add(f: TaskForm): Result<String> =
    attempt { api.scheduleAdd(f.prompt.trim(), f.whenText.trim(), PagesFormat.repeatValue(f.repeatChoice, f.ownInterval).takeIf { it != "once" }) }
      .onSuccess { refresh() }
      .map { it }
}

// ------------------------------------------------------------------ activity (ACT-*, NAV-14, NAV-15)

data class ActivityUi(
  val items: List<ActivityItem> = emptyList(),
  val clientsSeen: List<ClientSeen> = emptyList(),
  val serverNow: Long = 0,
  /** the local time at which the answer arrived */
  val fetchedAt: Long = 0,
  val loaded: Boolean = false,
  val error: String? = null,
  /** ids whose Stop was sent and that are still listed */
  val stopSent: Set<Long> = emptySet(),
) {
  val running: List<ActivityItem> get() = PagesFormat.running(items)
  val clients: List<ActivityItem> get() = PagesFormat.clients(items)
}

class ActivityHolder(
  private val api: PagesApi,
  private val scope: CoroutineScope,
  private val clock: () -> Long = System::currentTimeMillis,
  /** receives the number of running items after every read (the avatar badge) */
  private val onCount: (Int) -> Unit = {},
) {
  private val _ui = MutableStateFlow(ActivityUi())
  val ui: StateFlow<ActivityUi> = _ui.asStateFlow()
  private var job: Job? = null

  fun refresh() {
    if (job?.isActive == true) return
    job = scope.launch { fetch() }
  }

  /** Reads once and waits for it (tests, and Stop which wants the fresh list). */
  suspend fun fetch() {
    val r = attempt { api.activity() }
    r.fold(
      { a ->
        _ui.update { cur ->
          val ids = a.items.map { it.id }.toSet()
          ActivityUi(a.items, a.clientsSeen, a.nowMs, clock(), true, null, cur.stopSent.filter { it in ids }.toSet())
        }
        onCount(PagesFormat.running(a.items).size)
      },
      { e -> _ui.update { it.copy(loaded = true, error = e.sentence()) } },
    )
  }

  /** ACT-03: sends Stop; returns null on success or the sentence ("Could not stop" when the server gives none). */
  suspend fun stop(id: Long): String? {
    val r = attempt { api.activityStop(id) }
    return r.fold(
      { _ui.update { it.copy(stopSent = it.stopSent + id) }; fetch(); null },
      { it.sentence().ifBlank { "Could not stop" } },
    )
  }

  /** Reads every [everyMs] until the caller's coroutine is cancelled (the screen leaves, the app goes to the background). */
  suspend fun poll(everyMs: Long) {
    while (true) {
      fetch()
      delay(everyMs)
    }
  }
}

// ------------------------------------------------------------------ terminal (TERM-*)

data class TerminalUi(val log: String = PagesFormat.TERMINAL_WELCOME + "\n", val running: Boolean = false)

class TerminalHolder(private val api: PagesApi, private val scope: CoroutineScope) {
  private val _ui = MutableStateFlow(TerminalUi())
  val ui: StateFlow<TerminalUi> = _ui.asStateFlow()
  private val history = ArrayList<String>()
  private var cursor = 0

  fun clear() { _ui.update { it.copy(log = "") } }

  /** TERM-02: Up goes to the older command. [draft] is what is typed now (kept for Down). */
  fun historyUp(): String? {
    if (history.isEmpty()) return null
    cursor = maxOf(0, cursor - 1)
    return history[cursor]
  }

  fun historyDown(): String? {
    if (history.isEmpty()) return null
    cursor = minOf(history.size, cursor + 1)
    return if (cursor >= history.size) "" else history[cursor]
  }

  /** TERM-02..04: appends "$ cmd" and then the output, or "Error: ..."; on a computer nothing is sent (TERM-04). */
  fun run(cmd: String, onComputer: Boolean) {
    val c = cmd.trim()
    if (c.isEmpty() || _ui.value.running) return
    history.add(c)
    cursor = history.size
    append("\$ $c\n")
    if (onComputer) {
      append("Error: ${PagesFormat.TERMINAL_ON_COMPUTER}\n")
      return
    }
    _ui.update { it.copy(running = true) }
    scope.launch {
      val r = attempt { api.terminalRun(c) }
      r.fold(
        { out -> append(out.trimEnd().let { if (it.isEmpty()) "" else it + "\n" }) },
        { e -> append("Error: ${e.sentence()}\n") },
      )
      _ui.update { it.copy(running = false) }
    }
  }

  private fun append(s: String) { _ui.update { it.copy(log = it.log + s) } }
}
