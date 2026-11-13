package org.zohara.agent.chat

import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.put
import org.zohara.agent.core.net.ChatDetail
import org.zohara.agent.core.net.ProfileUseResp
import org.zohara.agent.core.net.ProfilesResp
import org.zohara.agent.core.net.QuestionAnswer
import org.zohara.agent.core.net.SelectResp
import org.zohara.agent.core.net.SendEvent
import org.zohara.agent.core.net.SendRequest
import org.zohara.agent.core.net.StatusInfo
import org.zohara.agent.core.net.ZoApi
import org.zohara.agent.core.net.ZoJson

/** What the chat session needs from the server. [ZoChatBackend] is the real one; tests use a fake. */
interface ChatBackend {
  suspend fun chat(id: String): ChatDetail
  suspend fun switchTake(id: String, nth: Int, delta: Int): ChatDetail
  fun send(r: SendRequest): Flow<SendEvent>
  suspend fun replyStop(stream: Long): Boolean
  suspend fun replyApprove(stream: Long, id: Long, yes: Boolean): Boolean
  suspend fun replyAnswer(stream: Long, id: Long, answers: List<QuestionAnswer>): Boolean
  suspend fun replySkip(stream: Long, id: Long): Boolean
  suspend fun compact(id: String)
  suspend fun deleteChat(id: String)
  suspend fun pinChat(id: String, on: Boolean)
}

/** What the model and mode pills need. */
interface PillsBackend {
  suspend fun status(): StatusInfo
  suspend fun profiles(): ProfilesResp
  suspend fun profileUse(id: String): ProfileUseResp
  suspend fun select(model: String): SelectResp
  suspend fun mode(set: String?): String
  /** the same ops on the linked computer ([device]) */
  suspend fun computerStatus(device: String): StatusInfo
  suspend fun computerMode(device: String, set: String?): String
  suspend fun computerSelect(device: String, model: String)
}

class ZoChatBackend(private val api: ZoApi) : ChatBackend, PillsBackend {
  override suspend fun chat(id: String) = api.chat(id)
  override suspend fun switchTake(id: String, nth: Int, delta: Int) = api.switchTake(id, nth, delta)
  override fun send(r: SendRequest) = api.send(r)
  override suspend fun replyStop(stream: Long) = api.replyStop(stream)
  override suspend fun replyApprove(stream: Long, id: Long, yes: Boolean) = api.replyApprove(stream, id, yes)
  override suspend fun replyAnswer(stream: Long, id: Long, answers: List<QuestionAnswer>) = api.replyAnswer(stream, id, answers)
  override suspend fun replySkip(stream: Long, id: Long) = api.replySkipQuestion(stream, id)
  override suspend fun compact(id: String) = api.compact(id)
  override suspend fun deleteChat(id: String) = api.deleteChat(id)
  override suspend fun pinChat(id: String, on: Boolean) = api.pinChat(id, on)

  override suspend fun status() = api.status()
  override suspend fun profiles() = api.profiles()
  override suspend fun profileUse(id: String) = api.profileUse(id)
  override suspend fun select(model: String) = api.select(model = model)
  override suspend fun mode(set: String?) = api.mode(set)
  override suspend fun computerStatus(device: String): StatusInfo =
    ZoJson.decodeFromJsonElement(api.desktopOp(device, buildJsonObject { put("op", "status") }))
  override suspend fun computerMode(device: String, set: String?): String {
    val r: JsonObject = api.desktopOp(device, buildJsonObject { put("op", "mode"); if (set != null) put("set", set) })
    return (r["mode"] as? kotlinx.serialization.json.JsonPrimitive)?.content.orEmpty()
  }
  override suspend fun computerSelect(device: String, model: String) {
    api.desktopOp(device, buildJsonObject { put("op", "select"); put("model", model) })
  }
}
