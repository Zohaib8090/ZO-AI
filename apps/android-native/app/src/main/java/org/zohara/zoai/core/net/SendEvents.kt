package org.zohara.zoai.core.net

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull

data class QuestionOption(val label: String, val description: String = "")
data class QuestionSpec(val question: String, val header: String = "", val multiSelect: Boolean = false, val options: List<QuestionOption> = emptyList())

/** The events of `POST /api/send` (API.md section 3.1), plus [ConnectionLost] which the client makes when the stream stops without `done`. */
sealed interface SendEvent {
  /** first line: the handle for `/api/reply`. [target] and [device] are set for a computer relay. */
  data class Stream(val id: Long, val target: String? = null, val device: String? = null) : SendEvent
  data class Chat(val id: String, val title: String) : SendEvent
  data class Delta(val text: String) : SendEvent
  /** the complete text of the step; arrives after its deltas: use it only when there were none */
  data class Text(val text: String) : SendEvent
  data class ToolStart(val name: String, val summary: String) : SendEvent
  data class ToolDone(val name: String, val ok: Boolean, val output: String) : SendEvent
  data class Approval(val id: Long, val tool: String, val summary: String, val local: Boolean = false) : SendEvent
  data class Question(val id: Long, val questions: List<QuestionSpec>) : SendEvent
  data class Usage(val input: Long, val output: Long) : SendEvent
  /** [code] is the raw `error` field (for example `desktop_offline`), [message] what to show */
  data class Error(val code: String, val message: String) : SendEvent
  data object Stopped : SendEvent
  data object Ping : SendEvent
  data object Done : SendEvent
  /** an event name this app does not know (ignored by screens) */
  data class Unknown(val name: String) : SendEvent
  /** made by the client: the connection ended or went silent before `done` */
  data class ConnectionLost(val reason: String) : SendEvent
}

object SendEventParser {
  /** One NDJSON line to an event; null for an empty line, a line that is not JSON, or JSON without `event` (the web does `catch { continue }`). */
  fun parse(line: String): SendEvent? {
    val t = line.trim()
    if (t.isEmpty()) return null
    val obj: JsonObject = try {
      ZoJson.parseToJsonElement(t) as? JsonObject ?: return null
    } catch (_: Exception) {
      return null
    }
    val name = obj.str("event") ?: return null
    return when (name) {
      "stream" -> SendEvent.Stream(obj.long("id") ?: 0, obj.str("target"), obj.str("device"))
      "chat" -> SendEvent.Chat(obj.str("id").orEmpty(), obj.str("title").orEmpty())
      "delta" -> SendEvent.Delta(obj.str("text").orEmpty())
      "text" -> SendEvent.Text(obj.str("text").orEmpty())
      "tool_start" -> SendEvent.ToolStart(obj.str("name").orEmpty(), obj.str("summary").orEmpty())
      "tool_done" -> SendEvent.ToolDone(obj.str("name").orEmpty(), obj.bool("ok") ?: false, obj.str("output").orEmpty())
      "approval", "approval_wait", "waiting_approval" ->
        SendEvent.Approval(obj.long("id") ?: 0, obj.str("tool").orEmpty(), obj.str("summary").orEmpty(), obj.bool("local") ?: false)
      "question" -> SendEvent.Question(obj.long("id") ?: 0, parseQuestions(obj["questions"]))
      "usage" -> SendEvent.Usage(obj.long("input") ?: 0, obj.long("output") ?: 0)
      "error" -> {
        val code = obj.str("error").orEmpty()
        val msg = obj.str("message")?.takeIf { it.isNotBlank() } ?: friendlyCode(code)
        SendEvent.Error(code, msg)
      }
      "stopped" -> SendEvent.Stopped
      "ping" -> SendEvent.Ping
      "done" -> SendEvent.Done
      else -> SendEvent.Unknown(name)
    }
  }

  /** Relay error codes (API.md 3.1) when the server did not send a `message`. */
  fun friendlyCode(code: String): String = when (code) {
    "desktop_offline" -> "Your computer is not reachable. Start ZO-AI on your computer and try again."
    "desktop_busy" -> "Your computer is busy with another answer."
    "limit" -> "You have reached a limit. Wait a moment and try again."
    "desktop_timeout" -> "Your computer took too long to answer."
    "desktop_lost" -> "The connection to your computer was lost."
    "job_timeout" -> "That took too long and was stopped."
    "too_much_output" -> "That produced too much output and was stopped."
    "desktop_revoked" -> "This computer is no longer linked. Sign in on it again."
    "account_deleted" -> ApiErrors.DELETED
    "" -> ApiErrors.GENERIC
    else -> code
  }

  private fun parseQuestions(el: JsonElement?): List<QuestionSpec> {
    val arr = el as? JsonArray ?: return emptyList()
    return arr.mapNotNull { q ->
      val o = q as? JsonObject ?: return@mapNotNull null
      QuestionSpec(
        question = o.str("question").orEmpty(),
        header = o.str("header").orEmpty(),
        multiSelect = o.bool("multi_select") ?: false,
        options = (o["options"] as? JsonArray)?.mapNotNull { opt ->
          val oo = opt as? JsonObject ?: return@mapNotNull null
          QuestionOption(oo.str("label").orEmpty(), oo.str("description").orEmpty())
        }.orEmpty(),
      )
    }
  }

  private fun JsonObject.str(k: String): String? = (this[k] as? kotlinx.serialization.json.JsonPrimitive)?.takeIf { it.isString }?.contentOrNull
  private fun JsonObject.long(k: String): Long? = (this[k] as? kotlinx.serialization.json.JsonPrimitive)?.let { it.longOrNull ?: it.doubleOrNull?.toLong() }
  private fun JsonObject.bool(k: String): Boolean? = (this[k] as? kotlinx.serialization.json.JsonPrimitive)?.booleanOrNull
}

/**
 * Splits a byte stream into lines. Bytes are buffered until a `\n` (0x0A), and only whole lines are decoded as UTF-8, so a multi-byte
 * character cut in two by a network chunk is never damaged. A line may be any size (until [maxLine], default 16 MB).
 */
class NdjsonLineBuffer(private val maxLine: Int = 16 * 1024 * 1024) {
  private var buf = ByteArray(8192)
  private var len = 0

  /** Feeds [count] bytes of [chunk]; returns the complete, non-empty lines found (trimmed). */
  fun feed(chunk: ByteArray, count: Int = chunk.size): List<String> {
    val out = ArrayList<String>(2)
    var start = 0
    for (i in 0 until count) {
      if (chunk[i] == NL) {
        append(chunk, start, i - start)
        flushLine(out)
        start = i + 1
      }
    }
    append(chunk, start, count - start)
    return out
  }

  /** At the end of the stream: a last line without a newline. */
  fun finish(): List<String> {
    val out = ArrayList<String>(1)
    flushLine(out)
    return out
  }

  private fun append(src: ByteArray, off: Int, n: Int) {
    if (n <= 0) return
    if (len + n > maxLine) { len = 0; return } // a runaway line is dropped
    if (len + n > buf.size) buf = buf.copyOf(maxOf(buf.size * 2, len + n))
    System.arraycopy(src, off, buf, len, n)
    len += n
  }

  private fun flushLine(out: MutableList<String>) {
    if (len > 0) {
      val s = String(buf, 0, len, Charsets.UTF_8).trim()
      if (s.isNotEmpty()) out.add(s)
    }
    len = 0
  }

  private companion object { const val NL: Byte = 10 }
}
