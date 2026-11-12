package org.zohara.zoai.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.zohara.zoai.model.ChatSummary
import org.zohara.zoai.model.QuestionAnswerData
import org.zohara.zoai.model.QuestionData
import org.zohara.zoai.model.QuestionOptionData
import org.zohara.zoai.model.StreamEvent
import org.zohara.zoai.model.Whoami
import java.io.BufferedReader
import java.io.InputStreamReader
import java.util.concurrent.TimeUnit

class ApiClient(
    private var baseUrl: String = "https://zo-ai-server.onrender.com",
    private var tokenProvider: suspend () -> String? = { null }
) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(120, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .build()

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }

    fun setBaseUrl(url: String) {
        baseUrl = url.trimEnd('/')
    }

    fun setTokenProvider(provider: suspend () -> String?) {
        tokenProvider = provider
    }

    private suspend fun buildRequest(path: String, bodyJson: String): Request {
        val token = tokenProvider()
        val builder = Request.Builder()
            .url("$baseUrl$path")
            .post(bodyJson.toRequestBody("application/json; charset=utf-8".toMediaType()))

        if (!token.isNullOrEmpty()) {
            builder.addHeader("Authorization", "Bearer $token")
        }
        return builder.build()
    }

    suspend fun whoami(): Result<Whoami> = withContext(Dispatchers.IO) {
        try {
            val req = buildRequest("/api/account/whoami", "{}")
            val resp = client.newCall(req).execute()
            val text = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) {
                return@withContext Result.failure(Exception("HTTP ${resp.code}: $text"))
            }
            val obj = json.parseToJsonElement(text).jsonObject
            val email = obj["email"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val admin = obj["admin"]?.jsonPrimitive?.booleanOrNull ?: false
            val restricted = obj["restricted"]?.jsonPrimitive?.booleanOrNull ?: true
            Result.success(Whoami(email = email, admin = admin, restricted = restricted))
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    suspend fun getChats(): Result<List<ChatSummary>> = withContext(Dispatchers.IO) {
        try {
            val req = buildRequest("/api/op", """{"op":"chats"}""")
            val resp = client.newCall(req).execute()
            val text = resp.body?.string().orEmpty()
            if (!resp.isSuccessful) {
                return@withContext Result.failure(Exception("HTTP ${resp.code}: $text"))
            }
            val obj = json.parseToJsonElement(text).jsonObject
            val arr = obj["chats"]?.jsonArray ?: return@withContext Result.success(emptyList())
            val list = arr.mapNotNull { item ->
                try {
                    val o = item.jsonObject
                    val id = o["id"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
                    val title = o["title"]?.jsonPrimitive?.contentOrNull ?: "Untitled"
                    val updated = o["updated"]?.jsonPrimitive?.longOrNull ?: 0L
                    val pinned = o["pinned"]?.jsonPrimitive?.booleanOrNull ?: false
                    ChatSummary(id = id, title = title, updated = updated, pinned = pinned)
                } catch (e: Exception) {
                    null
                }
            }
            Result.success(list)
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    fun send(
        chatId: String,
        text: String,
        model: String,
        provider: String,
        mode: String = "Auto"
    ): Flow<StreamEvent> = callbackFlow {
        val payload = buildString {
            append("""{"chat":"$chatId","text":""")
            append(json.encodeToString(kotlinx.serialization.serializer(), text))
            append(""","model":"$model","provider":"$provider","mode":"$mode"}""")
        }

        val req = buildRequest("/api/send", payload)
        val call = client.newCall(req)

        val readerThread = Thread {
            try {
                val resp = call.execute()
                if (!resp.isSuccessful) {
                    trySend(StreamEvent.Error("HTTP ${resp.code}: ${resp.message}"))
                    close()
                    return@Thread
                }
                val stream = resp.body?.byteStream() ?: run {
                    close()
                    return@Thread
                }
                val reader = BufferedReader(InputStreamReader(stream, Charsets.UTF_8))
                var line: String?

                while (reader.readLine().also { line = it } != null) {
                    val l = line?.trim().orEmpty()
                    if (l.isEmpty()) continue
                    try {
                        val obj = json.parseToJsonElement(l).jsonObject
                        val ev = obj["event"]?.jsonPrimitive?.contentOrNull
                        when (ev) {
                            "stream" -> {
                                val id = obj["id"]?.jsonPrimitive?.longOrNull ?: 0L
                                val dev = obj["device"]?.jsonPrimitive?.contentOrNull
                                trySend(StreamEvent.Stream(id, dev))
                            }
                            "chat" -> {
                                val id = obj["id"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                val title = obj["title"]?.jsonPrimitive?.contentOrNull ?: "Chat"
                                trySend(StreamEvent.Chat(id, title))
                            }
                            "delta" -> {
                                val t = obj["text"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                trySend(StreamEvent.Delta(t))
                            }
                            "text" -> {
                                val t = obj["text"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                trySend(StreamEvent.Text(t))
                            }
                            "tool_start" -> {
                                val name = obj["name"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                val sum = obj["summary"]?.jsonPrimitive?.contentOrNull ?: name
                                trySend(StreamEvent.ToolStart(name, sum))
                            }
                            "tool_done" -> {
                                val name = obj["name"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                val ok = obj["ok"]?.jsonPrimitive?.booleanOrNull ?: true
                                val out = obj["output"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                trySend(StreamEvent.ToolDone(name, ok, out))
                            }
                            "approval" -> {
                                val id = obj["id"]?.jsonPrimitive?.longOrNull ?: 0L
                                val tool = obj["tool"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                val sum = obj["summary"]?.jsonPrimitive?.contentOrNull ?: tool
                                trySend(StreamEvent.Approval(id, tool, sum))
                            }
                            "question" -> {
                                val id = obj["id"]?.jsonPrimitive?.longOrNull ?: 0L
                                val qs = obj["questions"]?.jsonArray?.mapNotNull { qElem ->
                                    val qObj = qElem.jsonObject
                                    val qText = qObj["question"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
                                    val header = qObj["header"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                    val multi = qObj["multi_select"]?.jsonPrimitive?.booleanOrNull ?: false
                                    val opts = qObj["options"]?.jsonArray?.mapNotNull { oElem ->
                                        val oObj = oElem.jsonObject
                                        val label = oObj["label"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
                                        val desc = oObj["description"]?.jsonPrimitive?.contentOrNull.orEmpty()
                                        QuestionOptionData(label, desc)
                                    }.orEmpty()
                                    QuestionData(qText, header, multi, opts)
                                }.orEmpty()
                                trySend(StreamEvent.Question(id, qs))
                            }
                            "done" -> trySend(StreamEvent.Done)
                            "stopped" -> trySend(StreamEvent.Stopped)
                            "error" -> {
                                val err = obj["error"]?.jsonPrimitive?.contentOrNull ?: "Unknown error"
                                val msg = obj["message"]?.jsonPrimitive?.contentOrNull
                                trySend(StreamEvent.Error(err, msg))
                            }
                        }
                    } catch (e: Exception) {
                        // ignore malformed line
                    }
                }
                close()
            } catch (e: Exception) {
                trySend(StreamEvent.Error(e.message ?: "Connection error"))
                close()
            }
        }
        readerThread.start()

        awaitClose {
            call.cancel()
            readerThread.interrupt()
        }
    }

    suspend fun replyApproval(streamId: Long, approvalId: Long, allow: Boolean): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val body = """{"stream":$streamId,"op":"approve","id":$approvalId,"yes":$allow}"""
            val req = buildRequest("/api/reply", body)
            val resp = client.newCall(req).execute()
            if (resp.isSuccessful) Result.success(Unit) else Result.failure(Exception("HTTP ${resp.code}"))
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    suspend fun replyAnswer(
        streamId: Long,
        questionId: Long,
        answers: List<QuestionAnswerData>?,
        cancelled: Boolean = false
    ): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val body = if (cancelled) {
                """{"stream":$streamId,"op":"answer","id":$questionId,"cancelled":true}"""
            } else {
                val answersJson = json.encodeToString(
                    kotlinx.serialization.builtins.ListSerializer(QuestionAnswerData.serializer()),
                    answers ?: emptyList()
                )
                """{"stream":$streamId,"op":"answer","id":$questionId,"answers":$answersJson}"""
            }
            val req = buildRequest("/api/reply", body)
            val resp = client.newCall(req).execute()
            if (resp.isSuccessful) Result.success(Unit) else Result.failure(Exception("HTTP ${resp.code}"))
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    suspend fun stopStream(streamId: Long): Result<Unit> = withContext(Dispatchers.IO) {
        try {
            val body = """{"stream":$streamId,"op":"stop"}"""
            val req = buildRequest("/api/reply", body)
            val resp = client.newCall(req).execute()
            if (resp.isSuccessful) Result.success(Unit) else Result.failure(Exception("HTTP ${resp.code}"))
        } catch (e: Exception) {
            Result.failure(e)
        }
    }
}
