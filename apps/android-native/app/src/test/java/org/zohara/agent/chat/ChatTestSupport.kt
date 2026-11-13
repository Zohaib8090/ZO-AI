package org.zohara.agent.chat

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.core.net.ChatDetail
import org.zohara.agent.core.net.ChatEntry
import org.zohara.agent.core.net.ErrorKind
import org.zohara.agent.core.net.ProfileUseResp
import org.zohara.agent.core.net.ProfilesResp
import org.zohara.agent.core.net.QuestionAnswer
import org.zohara.agent.core.net.SelectResp
import org.zohara.agent.core.net.SendEvent
import org.zohara.agent.core.net.SendRequest
import org.zohara.agent.core.net.StatusInfo
import org.zohara.agent.core.net.TakeInfo
import org.zohara.agent.core.store.RunOn

/** A [ChatBackend] the tests drive: [events] feeds the stream of the next send, calls are recorded. */
class FakeBackend : ChatBackend, PillsBackend {
  val requests = mutableListOf<SendRequest>()
  var stream: Channel<SendEvent> = Channel(Channel.UNLIMITED)
  var sendError: ApiException? = null
  var chats = mutableMapOf<String, ChatDetail>()
  var chatError: ApiException? = null
  var switchResult: ChatDetail? = null
  var switchError: ApiException? = null
  val replies = mutableListOf<String>()
  var replyResult: () -> Boolean = { true }
  var replyError: ApiException? = null
  var replyErrorTimes = 0
  var compactError: ApiException? = null
  val compacted = mutableListOf<String>()
  val deleted = mutableListOf<String>()
  val pinned = mutableListOf<Pair<String, Boolean>>()
  var statusResult = StatusInfo(mode = "Manual", model = "claude-opus-5-5", provider = "claude-code", ready = true)
  var profilesResult = ProfilesResp()
  var modeResult: String? = null
  var modeError: ApiException? = null
  val calls = mutableListOf<String>()
  var computerStatusError: ApiException? = null

  /** the events of the next send; the stream ends when [Channel.close] is called */
  fun newStream(): Channel<SendEvent> { stream = Channel(Channel.UNLIMITED); return stream }

  override suspend fun chat(id: String): ChatDetail {
    chatError?.let { throw it }
    return chats[id] ?: throw ApiException(ErrorKind.Rejected, "that chat is gone")
  }
  override suspend fun switchTake(id: String, nth: Int, delta: Int): ChatDetail {
    switchError?.let { throw it }
    calls += "switch $id $nth $delta"
    return switchResult ?: chat(id)
  }
  override fun send(r: SendRequest): Flow<SendEvent> {
    requests += r
    val err = sendError
    if (err != null) return flow { throw err }
    return stream.receiveAsFlow()
  }
  override suspend fun replyStop(stream: Long): Boolean { replies += "stop $stream"; return true }
  override suspend fun replyApprove(stream: Long, id: Long, yes: Boolean): Boolean = reply("approve $stream $id $yes")
  override suspend fun replyAnswer(stream: Long, id: Long, answers: List<QuestionAnswer>): Boolean = reply("answer $stream $id ${answers.map { it.selected to it.other }}")
  override suspend fun replySkip(stream: Long, id: Long): Boolean = reply("skip $stream $id")
  private fun reply(s: String): Boolean {
    replies += s
    if (replyErrorTimes > 0 && replyError != null) { replyErrorTimes--; throw replyError!! }
    return replyResult()
  }
  override suspend fun compact(id: String) { compactError?.let { throw it }; compacted += id }
  override suspend fun deleteChat(id: String) { deleted += id }
  override suspend fun pinChat(id: String, on: Boolean) { pinned += id to on }

  override suspend fun status() = statusResult
  override suspend fun profiles() = profilesResult
  override suspend fun profileUse(id: String): ProfileUseResp { calls += "use $id"; return ProfileUseResp() }
  override suspend fun select(model: String): SelectResp { calls += "select $model"; return SelectResp(model = model) }
  override suspend fun mode(set: String?): String { modeError?.let { throw it }; calls += "mode $set"; return modeResult ?: set ?: "Manual" }
  override suspend fun computerStatus(device: String): StatusInfo { computerStatusError?.let { throw it }; return StatusInfo(mode = "Auto", model = "qwen", provider = "ollama", ready = true) }
  override suspend fun computerMode(device: String, set: String?): String { calls += "cmode $device $set"; return set ?: "Auto" }
  override suspend fun computerSelect(device: String, model: String) { calls += "cselect $device $model" }
}

class FakeEnv : ChatEnv {
  override var restricted = false
  override var messageLimit: Int? = null
  var run: RunOn = RunOn.Cloud
  var keep = false
  val toasts = mutableListOf<String>()
  val changed = mutableListOf<Long>()
  val needs = mutableListOf<String>()
  val done = mutableListOf<String>()
  var offline = 0
  override val attachmentCount = MutableStateFlow(0)
  var taken: Taken? = null
  var restored = 0
  override fun runOn() = run
  override fun keepRunning() = keep
  override fun takeForSend(typed: String): Taken = taken ?: Taken(typed.trim(), emptyList(), emptyList(), { restored++ })
  override fun toast(text: String) { toasts += text }
  override fun chatsChanged(delayMs: Long) { changed += delayMs }
  override fun needsYou(chatId: String?, kind: NeedsKind, streamId: Long?, id: Long, summary: String) { needs += "$kind $chatId $streamId $id $summary" }
  override fun needsYouDone(chatId: String?, kind: NeedsKind, id: Long) { done += "$kind $chatId $id" }
  override fun desktopOffline() { offline++ }
}

fun entry(kind: String, text: String, attach: List<org.zohara.agent.core.net.AttachInfo> = emptyList()) = ChatEntry(kind, text, attach)
fun detail(id: String, title: String, vararg e: ChatEntry, takes: List<TakeInfo> = emptyList()) = ChatDetail(id, title, e.toList(), takes)
