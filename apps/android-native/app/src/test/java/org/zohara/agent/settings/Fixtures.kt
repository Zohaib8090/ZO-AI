package org.zohara.agent.settings

import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.zohara.agent.core.net.ProviderInfo
import org.zohara.agent.core.net.Profile
import org.zohara.agent.core.net.ProfilesResp
import org.zohara.agent.core.net.ZoApi
import org.zohara.agent.core.net.ZoHttp
import org.zohara.agent.core.net.ZoJson

/** A fake Zohara Agent server: an answer per op (or per account path), every request recorded in order. */
class FakeServer {
  val server = MockWebServer()
  val requests = CopyOnWriteArrayList<Pair<String, JsonObject>>()
  /** op name or path -> the JSON body of the answer; a function makes the answer depend on the request */
  val answers = HashMap<String, (JsonObject) -> String>()

  init {
    server.dispatcher = object : Dispatcher() {
      override fun dispatch(request: RecordedRequest): MockResponse {
        val body = try { ZoJson.parseToJsonElement(request.body.readUtf8()).jsonObject } catch (_: Exception) { JsonObject(emptyMap()) }
        val key = if (request.path == "/api/op") body["op"]?.jsonPrimitive?.content.orEmpty() else request.path.orEmpty()
        requests.add(key to body)
        val f = answers[key] ?: return MockResponse().setHeader("Content-Type", "application/json").setBody("""{"ok":false,"error":"unknown request: $key"}""")
        return MockResponse().setHeader("Content-Type", "application/json").setBody(f(body))
      }
    }
    server.start()
  }

  fun on(key: String, json: String) { answers[key] = { json } }
  fun on(key: String, f: (JsonObject) -> String) { answers[key] = f }

  val api: ZoApi by lazy { ZoApi(ZoHttp.newClient({ _ -> "tok" }), { server.url("/").toString().trimEnd('/') }, retryDelayMs = { 0L }) }

  fun ops(): List<String> = requests.map { it.first }
  fun body(op: String): JsonObject = requests.last { it.first == op }.second
  fun shutdown() = server.shutdown()
}

fun provider(id: String, name: String = id, local: Boolean = false, hasKey: Boolean = false, needsUrl: Boolean = false, defaultUrl: String = "", baseUrl: String = defaultUrl) =
  ProviderInfo(id, name, local, hasKey, needsUrl, defaultUrl, baseUrl)

val SAMPLE_PROVIDERS = listOf(
  provider("anthropic", "Anthropic (Claude)", hasKey = true, defaultUrl = "https://api.anthropic.com/v1"),
  provider("openai", "OpenAI", hasKey = false, defaultUrl = "https://api.openai.com/v1"),
  provider("gemini", "Google Gemini", hasKey = true),
  provider("groq", "Groq", hasKey = false),
  provider("openrouter", "OpenRouter", hasKey = true),
  provider("ollama", "Ollama (this computer)", local = true, needsUrl = true, defaultUrl = "http://127.0.0.1:11434/v1"),
  provider("claude-code", "Claude Code (your subscription)", local = true),
  provider("custom", "Custom endpoint", needsUrl = true),
)

val SAMPLE_PROFILES = ProfilesResp(
  active = "p1",
  profiles = listOf(
    Profile("p1", "Work", "anthropic", "claude-sonnet-5", effort = "high", pinned = listOf("claude-sonnet-5", "claude-haiku-5"), vision = true),
    Profile("p2", "Free models", "openrouter", "deepseek/deepseek-chat", pinned = listOf("deepseek/deepseek-chat")),
    Profile("p3", "Home GPU", "custom", "", baseUrl = "http://192.168.1.20:8000/v1"),
  ),
)
