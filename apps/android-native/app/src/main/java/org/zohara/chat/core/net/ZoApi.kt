package org.zohara.chat.core.net

import java.io.IOException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.channelFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.util.concurrent.TimeUnit

/** Body of `POST /api/send` (API.md section 3). */
data class SendRequest(
  val text: String,
  val chat: String? = null,
  /** re-ask from user message number `edit` (0-based); empty [text] asks the old text again */
  val edit: Int? = null,
  val attachments: List<Attachment> = emptyList(),
  /** `manual`, `auto` or `bypass` for this question only */
  val mode: String? = null,
  val keepRunning: Boolean = false,
  /** `desktop` runs it on the linked computer [device] */
  val target: String? = null,
  val device: String? = null,
)

/** One answer to a question card: the labels picked and an optional free text (`other`, at most 2000 characters). */
data class QuestionAnswer(val selected: List<String>, val other: String? = null)

/**
 * The typed client of the Zohara Chat server: one function per op of API.md. Every failure is an [ApiException] with a sentence for the person.
 * [baseUrl] is read on each call (the server address can change in Advanced settings).
 */
class ZoApi(
  private val http: OkHttpClient,
  private val baseUrl: () -> String,
  val json: kotlinx.serialization.json.Json = ZoJson,
  /** waits before a read-only retry; tests set it to 0 */
  private val retryDelayMs: (Int?) -> Long = { sec -> if (sec == null) 1000L else minOf(sec, 10) * 1000L },
) {
  private val jsonType = "application/json; charset=utf-8".toMediaType()
  private val opHttp: OkHttpClient by lazy { http.newBuilder().readTimeout(ZoHttp.OP_READ_SECONDS, TimeUnit.SECONDS).build() }
  private val streamHttp: OkHttpClient by lazy { http.newBuilder().readTimeout(ZoHttp.STREAM_SILENCE_SECONDS, TimeUnit.SECONDS).callTimeout(0, TimeUnit.SECONDS).build() }

  // ------------------------------------------------------------ plumbing

  /** Called for every error of every call (the session turns terms_required, 401, blocked, deleted and offline into screens and banners). */
  @Volatile var errorListener: ((ApiException) -> Unit)? = null
  /** Called after every call that worked (clears the offline banner). */
  @Volatile var successListener: (() -> Unit)? = null

  private fun report(e: ApiException): ApiException { errorListener?.invoke(e); return e }

  private fun url(path: String) = ZoHttp.normalizeServer(baseUrl()) + path

  private fun request(path: String, body: JsonObject): Request =
    Request.Builder().url(url(path)).post(body.toString().toRequestBody(jsonType)).header("Accept", "application/json").build()

  private suspend fun Call.await(): Response = suspendCancellableCoroutine { cont ->
    cont.invokeOnCancellation { runCatching { cancel() } }
    enqueue(object : Callback {
      override fun onFailure(call: Call, e: IOException) { if (cont.isActive) cont.resumeWithException(e) }
      override fun onResponse(call: Call, response: Response) { cont.resume(response) { _, _, _ -> response.close() } }
    })
  }

  /** POST [body] to [path]; returns the JSON object of the answer, or throws. HTTP 200 with `ok:false` is an error too (NET-02). */
  suspend fun post(path: String, body: JsonObject = JsonObject(emptyMap()), readOnly: Boolean = false): JsonObject =
    try {
      withRetry(readOnly) { postOnce(path, body) }.also { successListener?.invoke() }
    } catch (e: ApiException) {
      throw report(e)
    }

  private suspend fun postOnce(path: String, body: JsonObject): JsonObject {
    val response = try {
      opHttp.newCall(request(path, body)).await()
    } catch (e: CancellationException) {
      throw e
    } catch (e: Exception) {
      throw ApiErrors.fromThrowable(e)
    }
    val text = try {
      withContext(Dispatchers.IO) { response.use { it.body?.string().orEmpty() } }
    } catch (e: IOException) {
      throw ApiErrors.fromThrowable(e)
    }
    if (!response.isSuccessful) throw ApiErrors.fromHttp(response.code, text, response.header("Retry-After"))
    val obj = try {
      json.parseToJsonElement(text) as? JsonObject ?: throw ApiException(ErrorKind.Server, ApiErrors.GENERIC, response.code)
    } catch (e: ApiException) {
      throw e
    } catch (e: Exception) {
      throw ApiException(ErrorKind.Server, ApiErrors.GENERIC, response.code, cause = e)
    }
    if ((obj["ok"] as? JsonPrimitive)?.booleanOrNull == false) {
      throw ApiErrors.fromOpFailure((obj["error"] as? JsonPrimitive)?.contentOrNull.orEmpty())
    }
    return obj
  }

  private suspend fun <T> withRetry(readOnly: Boolean, block: suspend () -> T): T {
    try {
      return block()
    } catch (e: ApiException) {
      if (!readOnly) throw e
      when {
        e.kind == ErrorKind.Offline -> delay(retryDelayMs(null))
        (e.status == 429 || e.status == 503) -> delay(retryDelayMs(e.retryAfterSec ?: 5))
        else -> throw e
      }
      return block()
    }
  }

  private suspend inline fun <reified T> opAs(name: String, readOnly: Boolean = false, noinline fields: JsonObjectBuilder.() -> Unit = {}): T =
    json.decodeFromJsonElement<T>(op(name, readOnly, fields))

  /** `POST /api/op {op, ...}`. */
  suspend fun op(name: String, readOnly: Boolean = false, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject =
    post("/api/op", buildJsonObject { put("op", name); fields() }, readOnly)

  private suspend inline fun <reified T> postAs(path: String, readOnly: Boolean = false, noinline fields: JsonObjectBuilder.() -> Unit = {}): T =
    json.decodeFromJsonElement<T>(post(path, buildJsonObject(fields), readOnly))

  // ------------------------------------------------------------ 4.1 status, chats

  suspend fun status(): StatusInfo = opAs("status", true)
  suspend fun chats(): List<ChatSummary> = opAs<ChatsResp>("chats", true).chats
  suspend fun chat(id: String): ChatDetail = opAs("chat", true) { put("id", id) }
  /** the other take of user message [nth]: [delta] -1 older, +1 newer */
  suspend fun switchTake(id: String, nth: Int, delta: Int): ChatDetail = opAs("switch") { put("id", id); put("nth", nth); put("delta", delta) }
  suspend fun deleteChat(id: String) { op("delete_chat") { put("id", id) } }
  suspend fun deleteAllChats(): Int = opAs<DeletedCount>("delete_all_chats").deleted
  suspend fun pinChat(id: String, on: Boolean) { op("pin_chat") { put("id", id); put("on", on) } }
  suspend fun search(query: String): List<SearchHit> = opAs<SearchResp>("search", true) { put("query", query) }.hits
  suspend fun compact(id: String) { op("compact") { put("id", id) } }
  suspend fun linkPreview(url: String): LinkPreview = opAs("link_preview") { put("url", url) }
  suspend fun terminalRun(cmd: String): String = opAs<TextResp>("terminal_run") { put("cmd", cmd) }.output
  /** the saved mode (`Manual`, `Auto`, `Bypass`); with [set] (`manual`, `auto`, `bypass`) it also changes it */
  suspend fun mode(set: String? = null): String = opAs<ModeResp>("mode") { if (set != null) put("set", set) }.mode
  suspend fun activity(): ActivityResp = opAs("activity", true)
  suspend fun activityStop(id: Long) { op("activity_stop") { put("id", id) } }
  suspend fun syncStatus(): JsonObject = op("sync_status", true)

  // ------------------------------------------------------------ 4.2 models, keys, profiles

  suspend fun providers(): ProvidersResp = opAs("providers", true)
  suspend fun models(provider: String? = null, baseUrl: String? = null): ModelsResp = opAs("models") {
    if (provider != null) put("provider", provider)
    if (baseUrl != null) put("base_url", baseUrl)
  }
  suspend fun modelTest(provider: String? = null, model: String? = null, baseUrl: String? = null): ModelTestResp = opAs("model_test") {
    if (provider != null) put("provider", provider)
    if (model != null) put("model", model)
    if (baseUrl != null) put("base_url", baseUrl)
  }
  suspend fun select(provider: String? = null, model: String? = null, effort: String? = null, baseUrl: String? = null): SelectResp = opAs("select") {
    if (provider != null) put("provider", provider)
    if (model != null) put("model", model)
    if (effort != null) put("effort", effort)
    if (baseUrl != null) put("base_url", baseUrl)
  }
  /** The key is sent once and never logged or kept. */
  suspend fun setKey(provider: String, key: String) { op("set_key") { put("provider", provider); put("key", key) } }
  suspend fun profiles(): ProfilesResp = opAs("profiles", true)
  suspend fun profileCreate(name: String): String = opAs<IdResp>("profile_create") { put("name", name) }.id
  suspend fun profileUse(id: String): ProfileUseResp = opAs("profile_use") { put("id", id) }
  suspend fun profileUpdate(id: String, baseUrl: String? = null, effort: String? = null) {
    op("profile_update") { put("id", id); if (baseUrl != null) put("base_url", baseUrl); if (effort != null) put("effort", effort) }
  }
  suspend fun profileRename(id: String, name: String) { op("profile_rename") { put("id", id); put("name", name) } }
  suspend fun profileDelete(id: String) { op("profile_delete") { put("id", id) } }
  suspend fun profilePin(id: String, model: String, on: Boolean) { op("profile_pin") { put("id", id); put("model", model); put("on", on) } }
  suspend fun settingsGet(): SettingsResp = opAs("settings_get", true)
  suspend fun settingsSet(hiddenProviders: List<String>? = null, providerPrefs: Map<String, ProviderPref>? = null): SettingsResp = opAs("settings_set") {
    if (hiddenProviders != null) putJsonArray("hidden_providers") { hiddenProviders.forEach { add(JsonPrimitive(it)) } }
    if (providerPrefs != null) putJsonObject("provider_prefs") { providerPrefs.forEach { (k, v) -> putJsonObject(k) { if (v.order != null) put("order", v.order) } } }
  }

  // ------------------------------------------------------------ 4.3 memory, personalities

  suspend fun memory(): MemoryResp = opAs("memory", true)
  suspend fun memoryAdd(text: String, kind: String = "fact") { op("memory_add") { put("text", text); put("kind", kind) } }
  suspend fun memoryUpdate(id: String, text: String) { op("memory_update") { put("id", id); put("text", text) } }
  suspend fun memoryPin(id: String, on: Boolean) { op("memory_pin") { put("id", id); put("on", on) } }
  suspend fun memoryForget(id: String) { op("memory_forget") { put("id", id) } }
  suspend fun memoryEnable(on: Boolean) { op("memory_enable") { put("on", on) } }
  suspend fun memoryClear() { op("memory_clear") }
  suspend fun memoryEdit(text: String): String = opAs<TextResp>("memory_edit") { put("text", text) }.message
  suspend fun personalities(): PersonalitiesResp = opAs("personalities", true)
  /** `""` selects Plain */
  suspend fun personalityUse(id: String) { op("personality_use") { put("id", id) } }
  suspend fun personalityAdd(name: String, prompt: String): String = opAs<IdResp>("personality_add") { put("name", name); put("prompt", prompt) }.id
  suspend fun personalityGenerate(description: String): PersonalityGenerated = opAs("personality_generate") { put("description", description) }
  suspend fun personalityRemove(id: String) { op("personality_remove") { put("id", id) } }

  // ------------------------------------------------------------ 4.4 projects, artifacts, schedules, machines, MCP

  suspend fun projects(): String = opAs<ProjectsResp>("projects", true).text
  suspend fun projectCreate(name: String, goal: String = ""): String = opAs<TextResp>("project_create") { put("name", name); put("goal", goal) }.message
  suspend fun artifacts(): List<ArtifactItem> = opAs<ArtifactsResp>("artifacts", true).items
  suspend fun scheduled(): List<ScheduledTask> = opAs<ScheduledResp>("scheduled", true).tasks
  /** [`when`] is plain words; a leading "in " is rewritten to "now + " because the server parses it with GNU date (API.md 4.4 known bug) */
  suspend fun scheduleAdd(task: String, `when`: String, repeat: String? = null, title: String? = null): String = opAs<TextResp>("schedule_add") {
    put("task", task); put("when", fixWhen(`when`))
    if (!repeat.isNullOrBlank()) put("repeat", repeat)
    if (!title.isNullOrBlank()) put("title", title)
  }.message
  suspend fun scheduleCancel(id: String) { op("schedule_cancel") { put("id", id) } }
  suspend fun machines(): List<Machine> = opAs<MachinesResp>("machines", true).machines
  suspend fun machineAdd(name: String, host: String, user: String, port: Int, note: String, key: String) {
    op("machine_add") { put("name", name); put("host", host); put("user", user); put("port", port); put("note", note); put("key", key) }
  }
  suspend fun machineTest(name: String): String = opAs<TextResp>("machine_test") { put("name", name) }.message
  suspend fun machineRemove(name: String) { op("machine_remove") { put("name", name) } }
  suspend fun mcpList(): List<McpServer> = opAs<McpResp>("mcp_list", true).servers
  suspend fun mcpAdd(name: String, address: String, token: String? = null): String = opAs<TextResp>("mcp_add") {
    put("name", name); put("address", address); if (!token.isNullOrBlank()) put("token", token)
  }.message
  suspend fun mcpImport(config: String): String = opAs<TextResp>("mcp_import") { put("config", config) }.message
  suspend fun mcpToggle(name: String, enabled: Boolean) { op("mcp_toggle") { put("name", name); put("enabled", enabled) } }
  suspend fun mcpRemove(name: String) { op("mcp_remove") { put("name", name) } }

  /** 4.5: an op relayed to a linked computer. Returns the computer's own answer. */
  suspend fun desktopOp(device: String, inner: JsonObject): JsonObject {
    val r = op("desktop_op") { put("device", device); put("inner", inner) }
    return (r["result"] as? JsonObject) ?: r
  }

  // ------------------------------------------------------------ 5 account

  suspend fun whoami(): Whoami = postAs("/api/account/whoami", true)
  suspend fun acceptTerms(version: String): String = postAs<VersionResp>("/api/account/accept_terms") { put("version", version) }.version
  suspend fun devicesList(): List<AccountDevice> = postAs<DevicesResp>("/api/account/devices_list", true).devices
  suspend fun deviceRevoke(id: String) { post("/api/account/device_revoke", buildJsonObject { put("id", id) }) }
  /** the whole export as JSON text, ready to save as `zohara-chat-export-YYYY-MM-DD.json` (without `ok`) */
  suspend fun exportAccount(): String = JsonObject(post("/api/account/export").filterKeys { it != "ok" }).toString()
  /** irreversible; the caller signs out afterwards */
  suspend fun deleteAccount(): DeleteAccountResp = postAs("/api/account/delete") { put("confirm", "DELETE") }
  suspend fun pushConfig(): PushConfig = postAs("/api/account/push_config", true)
  suspend fun pushSubscribe(token: String, label: String, showTitle: Boolean = false): String =
    postAs<IdResp>("/api/account/push_subscribe") { put("kind", "fcm"); put("token", token); put("label", label); put("show_title", showTitle) }.id
  suspend fun pushUnsubscribe(id: String) { post("/api/account/push_unsubscribe", buildJsonObject { put("id", id) }) }
  suspend fun pushList(): List<PushSubscription> = postAs<PushListResp>("/api/account/push_list", true).subscriptions
  suspend fun claudeStatus(): ClaudeStatus = postAs("/api/account/claude_status", true)

  // ------------------------------------------------------------ 6, 7, 8 devices, link, sync

  suspend fun deviceLookup(userCode: String): DeviceLookup = postAs("/api/auth/device_lookup") { put("user_code", userCode) }
  suspend fun deviceApprove(userCode: String, approve: Boolean) { post("/api/auth/device_approve", buildJsonObject { put("user_code", userCode); put("approve", approve) }) }
  suspend fun linkStatus(): List<LinkDevice> = postAs<LinkStatusResp>("/api/link/status", true).devices
  suspend fun syncManifest(): JsonObject = post("/api/sync/manifest", readOnly = true)

  // ------------------------------------------------------------ 3.5 reply

  /** `POST /api/reply`. true when the line reached the answer; false when the stream is not yours or is over (404). */
  suspend fun reply(stream: Long, fields: JsonObjectBuilder.() -> Unit): Boolean {
    return try {
      post("/api/reply", buildJsonObject { put("stream", stream); fields() })
      true
    } catch (e: ApiException) {
      if (e.status == 404) false else throw e
    }
  }
  suspend fun replyStop(stream: Long): Boolean = reply(stream) { put("op", "stop") }
  suspend fun replyApprove(stream: Long, id: Long, yes: Boolean): Boolean = reply(stream) { put("op", "approve"); put("id", id); put("yes", yes) }
  suspend fun replyAnswer(stream: Long, id: Long, answers: List<QuestionAnswer>): Boolean = reply(stream) {
    put("op", "answer"); put("id", id)
    putJsonArray("answers") {
      answers.forEach { a ->
        add(buildJsonObject {
          putJsonArray("selected") { a.selected.forEach { add(JsonPrimitive(it)) } }
          put("other", a.other?.let { JsonPrimitive(sanitizeOther(it)) } ?: JsonNull)
        })
      }
    }
  }
  suspend fun replySkipQuestion(stream: Long, id: Long): Boolean = reply(stream) { put("op", "answer"); put("id", id); put("cancelled", true) }

  // ------------------------------------------------------------ 3 send (NDJSON stream)

  /** The JSON body of a send; public so tests can look at it. */
  fun sendBody(r: SendRequest): JsonObject = buildJsonObject {
    put("text", r.text)
    if (r.chat != null) put("chat", r.chat)
    if (r.edit != null) put("edit", r.edit)
    if (r.attachments.isNotEmpty()) put("attachments", Attachments.toJson(r.attachments))
    if (r.mode != null) put("mode", r.mode)
    if (r.keepRunning) put("keep_running", true)
    if (r.target != null) put("target", r.target)
    if (r.device != null) put("device", r.device)
    put("client", "phone")
  }

  /**
   * Sends a message and streams the answer. The flow ends after `done`, `error` + `done`, or [SendEvent.ConnectionLost] (no `done` came).
   * A refused request (400, 401, 403, 413, 429) throws an [ApiException] when the flow is collected. Cancelling the collector closes the
   * connection, which stops the answer on the server unless `keep_running` was set.
   */
  fun send(r: SendRequest): Flow<SendEvent> = channelFlow {
    val call = streamHttp.newCall(request("/api/send", sendBody(r)).newBuilder().header("Accept", "application/x-ndjson").build())
    val reader = launch(Dispatchers.IO) {
      var sawDone = false
      try {
        val response = try {
          call.execute()
        } catch (e: IOException) {
          if (isActive) throw ApiErrors.fromThrowable(e) else return@launch
        }
        response.use { resp ->
          if (!resp.isSuccessful) throw ApiErrors.fromHttp(resp.code, resp.body?.string().orEmpty(), resp.header("Retry-After"))
          successListener?.invoke()
          val source = resp.body?.source() ?: throw ApiException(ErrorKind.Server, ApiErrors.GENERIC, resp.code)
          val lines = NdjsonLineBuffer()
          val chunk = ByteArray(8192)
          loop@ while (isActive) {
            val n = try { source.read(chunk) } catch (e: IOException) {
              if (isActive) send(SendEvent.ConnectionLost(LOST)); sawDone = true; break@loop
            }
            if (n < 0) break
            for (line in lines.feed(chunk, n)) {
              val ev = SendEventParser.parse(line) ?: continue
              send(ev)
              if (ev is SendEvent.Done) { sawDone = true; break@loop }
            }
          }
          if (!sawDone && isActive) {
            for (line in lines.finish()) {
              val ev = SendEventParser.parse(line) ?: continue
              send(ev); if (ev is SendEvent.Done) sawDone = true
            }
            if (!sawDone) send(SendEvent.ConnectionLost(LOST))
          }
        }
        close()
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        close(report(e))
      } catch (e: Exception) {
        close(report(ApiErrors.fromThrowable(e)))
      }
    }
    awaitClose { call.cancel(); reader.cancel() }
  }

  companion object {
    const val LOST = "The connection was lost. The answer may still be finishing: reopen the chat in a moment."

    /** "in 2 hours" -> "now + 2 hours" (the server parses `when` with GNU date) */
    fun fixWhen(w: String): String {
      val t = w.trim()
      return if (t.lowercase().startsWith("in ")) "now + " + t.substring(3).trim() else t
    }

    /** question answers: control characters removed and at most 2000 characters (INTERACTION.md) */
    fun sanitizeOther(s: String): String = s.filter { !it.isISOControl() || it == '\n' || it == '\t' }.take(2000)
  }
}
