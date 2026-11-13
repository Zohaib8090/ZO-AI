package org.zohara.zoai.chat

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.Attachment
import org.zohara.zoai.core.net.Attachments
import org.zohara.zoai.core.net.ChatDetail
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.net.QuestionAnswer
import org.zohara.zoai.core.net.SendEvent
import org.zohara.zoai.core.net.SendRequest

/**
 * One conversation: its rows, the running answer, and every action on them. It lives in [ChatHub] (not in a screen), so an answer keeps
 * streaming while the person looks at another page. Screens only read [state] and call the functions.
 */
class ChatSession(
  private val backend: ChatBackend,
  private val scope: CoroutineScope,
  private val env: ChatEnv,
  private val windowSize: Int = 60,
  private val clock: () -> Long = System::currentTimeMillis,
) {
  private val _state = MutableStateFlow(ChatUiState())
  val state: StateFlow<ChatUiState> = _state.asStateFlow()

  /** the text of the composer (COMP-18, NAV-19) */
  val draft = MutableStateFlow("")

  private var keySeq = 0L
  private fun key() = ++keySeq

  private var sendJob: Job? = null
  private var loadJob: Job? = null
  private var activeGroup: Long? = null
  private var stepHasDeltas = false
  private var chatBegan = false
  private var lastOpenMs = 0L
  /** the approval or question whose notification is up; any later event of the answer means it was answered somewhere else (INTER-14) */
  private var needsOpen: Pair<NeedsKind, Long>? = null

  val chatId: String? get() = _state.value.chatId

  // ------------------------------------------------------------------ load

  /** Opens a saved chat. Does nothing when this session already shows it and is busy or has its rows (the answer keeps going). */
  fun open(id: String, force: Boolean = false) {
    val s = _state.value
    if (s.chatId == id && !force && (s.busy || s.items.isNotEmpty() || s.loading)) return
    loadJob?.cancel()
    _state.value = ChatUiState(chatId = id, loading = true)
    lastOpenMs = clock()
    loadJob = scope.launch { fetch(id, silent = false) }
  }

  /** Called when the screen comes back (CHAT-42): the server's version replaces ours unless an answer is running. */
  fun refreshIfIdle() {
    val s = _state.value
    val id = s.chatId ?: return
    if (s.busy || s.loading || s.questionOpen) return
    if (clock() - lastOpenMs < 2000) return
    lastOpenMs = clock()
    loadJob?.cancel()
    loadJob = scope.launch { fetch(id, silent = true) }
  }

  private suspend fun fetch(id: String, silent: Boolean) {
    try {
      applyDetail(backend.chat(id), id)
    } catch (e: CancellationException) {
      throw e
    } catch (e: ApiException) {
      if (!silent) _state.update { it.copy(loading = false, items = listOf(ErrorItem(key(), e.text, retry = false))) }
      else if (e.kind == ErrorKind.Rejected) _state.update { it.copy(loading = false) }
    }
  }

  private fun applyDetail(d: ChatDetail, fallbackId: String) {
    val items = Transcript.fromSaved(d.entries, d.takes, ::key)
    activeGroup = null
    _state.update {
      it.copy(
        chatId = d.id.ifEmpty { fallbackId }, title = d.title, items = items, takes = d.takes, loading = false,
        visibleFrom = (items.size - windowSize).coerceAtLeast(0),
      )
    }
  }

  fun showEarlier() = _state.update { it.copy(visibleFrom = (it.visibleFrom - windowSize).coerceAtLeast(0)) }

  // ------------------------------------------------------------------ sending

  /** The composer's Send. Returns true when the message went out (the caller then clears its box). */
  fun submit(text: String): Boolean {
    if (_state.value.busy) { env.toast("Wait for the current answer to finish."); return false }
    if (_state.value.questionOpen) return false
    if (text.isBlank() && env.attachmentCount.value == 0) return false
    val taken = env.takeForSend(text)
    val t = taken.text
    val limit = if (env.runOn().isCloud) env.messageLimit else minOf(env.messageLimit ?: Int.MAX_VALUE, 100_000).takeIf { it != Int.MAX_VALUE }
    val problem = when {
      t.isEmpty() && taken.attachments.isEmpty() -> ""
      limit != null && t.length > limit -> "That message is too long (at most $limit characters)."
      else -> Attachments.validate(taken.attachments)
    }
    if (problem != null) { taken.restore(); if (problem.isNotEmpty()) env.toast(problem); return false }
    startSend(t, taken, edit = null, keepUser = false, typed = text)
    return true
  }

  /** "Try again" (CHAT-08): asks the last message again as a new take. */
  fun retry() {
    val s = _state.value
    if (s.busy) { env.toast("Wait for the current answer to finish."); return }
    val last = s.lastUser
    if (last == null) { env.toast("Nothing to retry."); return }
    if (env.runOn().isCloud) startSend("", Taken.plain(""), edit = last.nth, keepUser = true)
    else startSend(last.text, Taken.plain(last.text), edit = null, keepUser = false)
  }

  /** Sends [text] as a new take of user message [nth] (CHAT-06). */
  fun edit(nth: Int, text: String) {
    if (_state.value.busy) { env.toast("Wait for the current answer to finish."); return }
    val t = text.trim()
    if (t.isEmpty()) return
    if (env.runOn().isCloud) startSend(t, Taken.plain(t), edit = nth, keepUser = false)
    else startSend(t, Taken.plain(t), edit = null, keepUser = false)
  }

  private fun startSend(text: String, taken: Taken, edit: Int?, keepUser: Boolean, typed: String = text) {
    val atts = taken.attachments
    val runOn = env.runOn()
    val before = _state.value
    stepHasDeltas = false
    chatBegan = before.chatId != null
    activeGroup = null
    _state.update { st ->
      var items = st.items.filterNot { it is ThinkingItem || it is WaitingItem }
      if (edit != null) {
        val idx = items.indexOfFirst { it is UserItem && it.nth == edit }
        if (idx >= 0) items = items.take(if (keepUser) idx + 1 else idx)
      }
      if (!keepUser) {
        val nth = items.count { it is UserItem }
        items = items + UserItem(key(), text, nth, taken.infos)
      }
      items = items + ThinkingItem(key())
      st.copy(items = items, busy = true, streamId = null, usage = null, computerName = null, visibleFrom = 0.coerceAtLeast(items.size - windowSize))
    }
    val req = SendRequest(
      text = text, chat = before.chatId, edit = edit, attachments = atts, keepRunning = env.keepRunning(),
      target = if (runOn.isCloud) null else "desktop", device = runOn.device,
    )
    sendJob = scope.launch {
      var lost = false
      try {
        backend.send(req).collect { ev -> if (handle(ev)) lost = true }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        failSend(e, typed, taken, edit, keepUser)
        return@launch
      }
      finishTurn(lost)
    }
  }

  /** The send was refused or failed on the way (CHAT-19, NET-07): before the chat began the message goes back into the box. */
  private suspend fun failSend(e: ApiException, text: String, taken: Taken, edit: Int?, keepUser: Boolean) {
    val began = chatBegan && _state.value.chatId != null && _state.value.items.any { it is AiItem || it is ToolGroupItem }
    val id = _state.value.chatId
    if (!began) {
      _state.update { st ->
        val items = st.items.filterNot { it is ThinkingItem || it is WaitingItem }.let { l ->
          if (!keepUser && l.lastOrNull() is UserItem) l.dropLast(1) else l
        }
        st.copy(items = items + ErrorItem(key(), e.text, code = e.kind.name), busy = false, streamId = null)
      }
      if (!keepUser) { if (edit == null || draft.value.isEmpty()) draft.value = text }
      taken.restore()
      if (edit != null && id != null) fetch(id, silent = true)
    } else {
      endTurnState()
      _state.update { it.copy(items = it.items + ErrorItem(key(), e.text, code = e.kind.name)) }
    }
  }

  /** Returns true when the connection was lost (no `done`). */
  private fun handle(ev: SendEvent): Boolean {
    needsOpen?.let { (kind, id) ->
      if (ev is SendEvent.Delta || ev is SendEvent.Text || ev is SendEvent.ToolStart || ev is SendEvent.ToolDone || ev is SendEvent.Done || ev is SendEvent.Error || ev is SendEvent.Stopped || ev is SendEvent.ConnectionLost) {
        needsDone(_state.value.chatId, kind, id)
      }
    }
    when (ev) {
      is SendEvent.Stream -> _state.update { st ->
        var items = st.items
        var name: String? = null
        if (ev.target == "desktop") {
          name = ev.device?.takeIf { it.isNotBlank() } ?: "your computer"
          items = items.filterNot { it is ThinkingItem } + NoticeItem(key(), "Running on your computer ($name)") + ThinkingItem(key())
        }
        st.copy(streamId = ev.id, items = items, computerName = name)
      }
      is SendEvent.Chat -> {
        chatBegan = true
        _state.update { it.copy(chatId = ev.id.ifEmpty { it.chatId }, title = ev.title.ifEmpty { it.title }) }
        env.chatsChanged()
      }
      is SendEvent.Delta -> mutate {
        dropTransient()
        endGroup()
        val last = lastOrNull()
        if (last is AiItem && last.live) this[lastIndex] = last.copy(text = last.text + ev.text) else add(AiItem(key(), ev.text, live = true))
        stepHasDeltas = true
      }
      is SendEvent.Text -> mutate {
        dropTransient()
        if (stepHasDeltas) { finalizeLive() } else if (ev.text.isNotBlank()) { endGroup(); add(AiItem(key(), ev.text)) }
        stepHasDeltas = false
      }
      is SendEvent.ToolStart -> mutate {
        dropTransient(); finalizeLive()
        addLine(ToolLine(LineKind.Start, Transcript.toolLineFromSummary(ev.name, ev.summary)))
      }
      is SendEvent.ToolDone -> mutate {
        dropTransient()
        if (ev.name == "vision" && !ev.ok) { add(NoticeItem(key(), ev.output)) }
        else addLine(ToolLine(if (ev.ok) LineKind.Ok else LineKind.Fail, ev.output.ifBlank { if (ev.ok) "done" else "failed" }))
      }
      is SendEvent.Approval -> {
        if (env.restricted) return false
        val computer = _state.value.computerName != null
        mutate {
          dropTransient()
          if (ev.local) add(WaitingItem(key(), ApprovalItem.firstLine(ev.summary, ev.tool)))
          else add(ApprovalItem(key(), ev.id, ev.tool, ev.summary, onComputer = computer))
        }
        if (!ev.local) { needsOpen = NeedsKind.Approval to ev.id; env.needsYou(_state.value.chatId, NeedsKind.Approval, _state.value.streamId, ev.id, ApprovalItem.firstLine(ev.summary, ev.tool)) }
      }
      is SendEvent.Question -> {
        mutate { dropTransient(); add(QuestionItem(key(), ev.id, ev.questions)) }
        needsOpen = NeedsKind.Question to ev.id
        env.needsYou(_state.value.chatId, NeedsKind.Question, _state.value.streamId, ev.id, ev.questions.firstOrNull()?.question.orEmpty())
      }
      is SendEvent.Usage -> _state.update { it.copy(usage = Usage(ev.input, ev.output)) }
      is SendEvent.Error -> {
        mutate {
          dropTransient(); finalizeLive(); endGroup(); expireCards()
          add(ErrorItem(key(), ev.message, code = ev.code, chooseModel = chooseModelHint(ev.message)))
        }
        if (ev.code == "desktop_offline") env.desktopOffline()
      }
      SendEvent.Stopped -> mutate { dropTransient(); finalizeLive(); endGroup(); expireCards(); add(StoppedItem(key())) }
      is SendEvent.ConnectionLost -> {
        mutate { dropTransient(); finalizeLive(); endGroup(); expireCards(); add(ErrorItem(key(), CONNECTION_LOST_TEXT(_state.value.computerName != null), code = "lost", retry = false)) }
        return true
      }
      SendEvent.Done, SendEvent.Ping, is SendEvent.Unknown -> Unit
    }
    return false
  }

  private fun chooseModelHint(msg: String) = msg.contains("No model is chosen", ignoreCase = true) || msg.contains("Choose a model", ignoreCase = true)

  /** The stream is over: tidy the rows, free the composer, refresh the lists (CHAT-41), and reload what the server saved. */
  private suspend fun finishTurn(lost: Boolean) {
    endTurnState()
    val id = _state.value.chatId
    env.chatsChanged()
    if (!env.runOn().isCloud) env.chatsChanged(6000)
    if (id != null) {
      // the takes numbers (and, after a lost connection, the whole chat) come from the server
      try {
        val d = backend.chat(id)
        if (lost) applyDetail(d, id) else _state.update { it.copy(takes = d.takes, title = it.title.ifEmpty { d.title }, items = attachTakes(it.items, d.takes)) }
      } catch (e: CancellationException) { throw e } catch (_: ApiException) { }
    }
  }

  private fun endTurnState() {
    _state.update { st ->
      val l = st.items.toMutableList()
      Edit(l).run { dropTransient(); finalizeLive(); endGroup(); expireCards() }
      st.copy(items = l, busy = false, streamId = null)
    }
  }

  private fun attachTakes(items: List<ChatItem>, takes: List<org.zohara.zoai.core.net.TakeInfo>): List<ChatItem> =
    items.map { if (it is UserItem) it.copy(take = takes.firstOrNull { t -> t.nth == it.nth && t.count > 1 }) else it }

  /** Stop (COMP-02, CHAT-18). */
  fun stop() {
    val s = _state.value
    if (!s.busy) return
    val id = s.streamId
    if (id == null) {
      sendJob?.cancel()
      scope.launch { finishAfterStop() }
      return
    }
    scope.launch {
      try {
        backend.replyStop(id)
        if (s.computerName != null) env.toast("Stop sent to your computer.")
      } catch (e: CancellationException) { throw e } catch (e: ApiException) { env.toast(e.text) }
    }
  }

  private fun finishAfterStop() {
    endTurnState()
    _state.update { st -> st.copy(items = st.items + StoppedItem(key())) }
  }

  // ------------------------------------------------------------------ cards

  fun approve(itemKey: Long, yes: Boolean) {
    val s = _state.value
    val item = s.items.firstOrNull { it.key == itemKey } as? ApprovalItem ?: return
    val stream = s.streamId ?: return
    if (item.state != ApprovalState.Pending || item.sending) return
    setItem(itemKey) { (it as ApprovalItem).copy(sending = true) }
    scope.launch {
      val outcome = reply { backend.replyApprove(stream, item.id, yes) }
      when (outcome) {
        Reply.Ok -> { setItem(itemKey) { (it as ApprovalItem).copy(sending = false, state = if (yes) ApprovalState.Allowed else ApprovalState.Denied) }; needsDone(s.chatId, NeedsKind.Approval, item.id) }
        Reply.Over -> { setItem(itemKey) { (it as ApprovalItem).copy(sending = false, state = ApprovalState.NoAnswer) }; needsDone(s.chatId, NeedsKind.Approval, item.id) }
        Reply.OnComputer -> replaceItem(itemKey, WaitingItem(itemKey, ApprovalItem.firstLine(item.summary, item.tool)))
        is Reply.Failed -> { setItem(itemKey) { (it as ApprovalItem).copy(sending = false) }; env.toast("Could not send your answer") }
      }
    }
  }

  fun answer(itemKey: Long, answers: List<QuestionAnswer>) {
    val s = _state.value
    val item = s.items.firstOrNull { it.key == itemKey } as? QuestionItem ?: return
    val stream = s.streamId ?: return
    if (item.state != QuestionState.Pending || item.sending) return
    setItem(itemKey) { (it as QuestionItem).copy(sending = true) }
    scope.launch {
      when (reply { backend.replyAnswer(stream, item.id, answers) }) {
        Reply.Ok -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false, state = QuestionState.Answered(QuestionLogic.summary(answers))) }; needsDone(s.chatId, NeedsKind.Question, item.id) }
        Reply.Over, Reply.OnComputer -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false, state = QuestionState.Expired) }; needsDone(s.chatId, NeedsKind.Question, item.id) }
        is Reply.Failed -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false) }; env.toast("Could not send your answer") }
      }
    }
  }

  fun skip(itemKey: Long) {
    val s = _state.value
    val item = s.items.firstOrNull { it.key == itemKey } as? QuestionItem ?: return
    val stream = s.streamId ?: return
    if (item.state != QuestionState.Pending || item.sending) return
    setItem(itemKey) { (it as QuestionItem).copy(sending = true) }
    scope.launch {
      when (reply { backend.replySkip(stream, item.id) }) {
        Reply.Ok -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false, state = QuestionState.Skipped) }; needsDone(s.chatId, NeedsKind.Question, item.id) }
        Reply.Over, Reply.OnComputer -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false, state = QuestionState.Expired) }; needsDone(s.chatId, NeedsKind.Question, item.id) }
        is Reply.Failed -> { setItem(itemKey) { (it as QuestionItem).copy(sending = false) }; env.toast("Could not send your answer") }
      }
    }
  }

  private fun needsDone(chatId: String?, kind: NeedsKind, id: Long) { needsOpen = null; env.needsYouDone(chatId, kind, id) }

  private sealed interface Reply {
    data object Ok : Reply
    data object Over : Reply
    data object OnComputer : Reply
    data class Failed(val e: ApiException) : Reply
  }

  /** `/api/reply`, tried twice (INTER-02); 404 means the stream is over, 409 `approvals_on_computer` that only the computer can answer. */
  private suspend fun reply(call: suspend () -> Boolean): Reply {
    var last: ApiException? = null
    repeat(2) { attempt ->
      try {
        return if (call()) Reply.Ok else Reply.Over
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        if (e.status == 409 || e.text.contains("approvals_on_computer")) return Reply.OnComputer
        last = e
        if (attempt == 0) delay(400)
      }
    }
    return Reply.Failed(last!!)
  }

  // ------------------------------------------------------------------ other actions

  fun switchTake(nth: Int, delta: Int) {
    val s = _state.value
    val id = s.chatId ?: return
    if (s.busy) return
    scope.launch {
      try { applyDetail(backend.switchTake(id, nth, delta), id) } catch (e: CancellationException) { throw e } catch (e: ApiException) { env.toast(e.text) }
    }
  }

  /** CTX-06. [done] gets null on success or the sentence to show. */
  fun compact(done: (String?) -> Unit) {
    val s = _state.value
    val id = s.chatId
    if (id == null) { done("No active chat to compact."); return }
    if (s.busy) { done("Wait until the answer is finished."); return }
    scope.launch {
      try {
        backend.compact(id)
        fetch(id, silent = true)
        done(null)
      } catch (e: CancellationException) { throw e } catch (e: ApiException) { done(e.text) }
    }
  }

  fun delete(done: (Boolean) -> Unit) {
    val id = _state.value.chatId ?: return done(false)
    scope.launch {
      try { backend.deleteChat(id); env.chatsChanged(); done(true) } catch (e: CancellationException) { throw e } catch (e: ApiException) { env.toast(e.text); done(false) }
    }
  }

  fun pin(on: Boolean) {
    val id = _state.value.chatId ?: return
    scope.launch {
      try { backend.pinChat(id, on); env.chatsChanged() } catch (e: CancellationException) { throw e } catch (e: ApiException) { env.toast(e.text) }
    }
  }

  // ------------------------------------------------------------------ list editing

  private fun setItem(k: Long, f: (ChatItem) -> ChatItem) = _state.update { st -> st.copy(items = st.items.map { if (it.key == k) f(it) else it }) }
  private fun replaceItem(k: Long, item: ChatItem) = setItem(k) { item }

  /** The list operations of one event, applied to a copy of the rows. */
  private inner class Edit(val l: MutableList<ChatItem>) {
    fun dropTransient() { l.removeAll { it is ThinkingItem || it is WaitingItem } }
    fun finalizeLive() { for (i in l.indices) (l[i] as? AiItem)?.let { if (it.live) l[i] = it.copy(live = false) } }
    fun endGroup() {
      val k = activeGroup ?: return
      val i = l.indexOfFirst { it.key == k }
      if (i >= 0) l[i] = (l[i] as ToolGroupItem).copy(active = false)
      activeGroup = null
    }
    fun expireCards() {
      for (i in l.indices) {
        when (val it = l[i]) {
          is ApprovalItem -> if (it.state == ApprovalState.Pending) l[i] = it.copy(state = ApprovalState.NoAnswer, sending = false)
          is QuestionItem -> if (it.state == QuestionState.Pending) l[i] = it.copy(state = QuestionState.Expired, sending = false)
          else -> Unit
        }
      }
    }
    fun addLine(line: ToolLine) {
      val k = activeGroup
      val i = if (k == null) -1 else l.indexOfFirst { it.key == k }
      if (i >= 0) { val g = l[i] as ToolGroupItem; l[i] = g.copy(lines = g.lines + line) }
      else { val g = ToolGroupItem(key(), listOf(line), active = true); activeGroup = g.key; l.add(g) }
    }
    fun add(item: ChatItem) { l.add(item) }
    fun lastOrNull() = l.lastOrNull()
    val lastIndex get() = l.lastIndex
    operator fun set(i: Int, v: ChatItem) { l[i] = v }
  }

  private fun mutate(block: Edit.() -> Unit) {
    _state.update { st ->
      val l = st.items.toMutableList()
      Edit(l).block()
      st.copy(items = l)
    }
  }

  companion object {
    fun CONNECTION_LOST_TEXT(onComputer: Boolean) = if (onComputer)
      "The connection to your computer was lost. The task may still be running there: check Running now on your computer."
    else "The connection was lost. The answer may still be finishing on the server: open the chat again in a moment."
  }
}
