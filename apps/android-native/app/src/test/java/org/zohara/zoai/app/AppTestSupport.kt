package org.zohara.zoai.app

import androidx.test.core.app.ApplicationProvider
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.zohara.zoai.AppContainer
import org.zohara.zoai.core.auth.AuthState
import org.zohara.zoai.core.auth.AuthUser
import org.zohara.zoai.core.net.ZoJson

/** A mock server that answers the calls the whole app makes at start, with the shapes captured live (docs/API.md). */
class AppServer(isAdmin: Boolean = true) {
  val server = MockWebServer()
  /** what `device_lookup` answers: a (status, body) pair */
  @Volatile var deviceLookup: Pair<Int, String> = 200 to """{"ok":true,"name":"My laptop","kind":"cli","requested_at":1794509000,"ip":"203.0.113.4","same_network":false,"email":"zo@example.com"}"""
  val bodies = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()
  val paths = java.util.concurrent.CopyOnWriteArrayList<String>()
  val ops = java.util.concurrent.CopyOnWriteArrayList<String>()
  @Volatile var admin: Boolean = isAdmin
  /** the `efforts` of the models op (a JSON array) */
  @Volatile var effortsJson: String = "[]"
  /** milliseconds `whoami` waits before it answers (the splash screen is on until then) */
  @Volatile var slowWhoami: Long = 0
  @Volatile var chatsJson: String = """[{"id":"c1","pinned":true,"title":"Pinned plan","updated":1794509131852},{"id":"c2","pinned":false,"title":"Recent idea","updated":1794509120577}]"""

  private fun json(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)

  init {
    server.dispatcher = object : Dispatcher() {
      override fun dispatch(request: RecordedRequest): MockResponse {
        val path = request.path.orEmpty()
        paths += path
        if (path.startsWith("/api/account/whoami") && slowWhoami > 0) Thread.sleep(slowWhoami)
        val body = request.body.readUtf8()
        bodies += path to body
        return when {
          path.startsWith("/api/account/whoami") ->
            json("""{"ok":true,"admin":$admin,"restricted":${!admin},"email":"zo@example.com","tenant":"t1","limits":{"requests_per_min":60,"concurrent_sends":2},"data_bytes":2224,"terms":{"version":"2026-11-12","accepted":true}}""")
          path.startsWith("/api/account/devices_list") ->
            json("""{"ok":true,"devices":[{"id":"d1","name":"My laptop","kind":"desktop","scope":"sync","created":1794509000,"last_used":1794509100}]}""")
          path.startsWith("/api/send") -> MockResponse().setHeader("Content-Type", "application/x-ndjson").setBody(
            listOf(
              """{"event":"stream","id":1}""", """{"event":"chat","id":"cnew","title":"Hello there"}""",
              """{"event":"delta","text":"Hi from "}""", """{"event":"delta","text":"the **fake** model."}""",
              """{"event":"usage","input":5,"output":6}""", """{"event":"done"}""",
            ).joinToString("\n", postfix = "\n"),
          )
          path.startsWith("/api/auth/device_lookup") -> deviceLookup.let { (code, b) -> MockResponse().setResponseCode(code).setHeader("Content-Type", "application/json").setBody(b) }
          path.startsWith("/api/auth/device_approve") -> json("""{"ok":true}""")
          path.startsWith("/api/account/claude_status") -> json("""{"ok":true,"present":false,"source":"none","set_at":0,"admin":$admin}""")
          path.startsWith("/api/account/push_config") -> json("""{"ok":true,"vapid_public_key":"","fcm":false}""")
          path.startsWith("/api/link/status") -> json("""{"ok":true,"devices":[]}""")
          path.startsWith("/api/op") -> {
            val op = runCatching { ZoJson.parseToJsonElement(body).jsonObject["op"]!!.jsonPrimitive.content }.getOrDefault("?")
            ops += op
            when (op) {
              "chats" -> json("""{"ok":true,"chats":$chatsJson}""")
              "status" -> json("""{"ok":true,"mode":"Manual","model":"claude-opus-5-5","provider":"anthropic","ready":true,"version":"0.1.0"}""")
              "providers" -> json("""{"ok":true,"current":"anthropic","providers":[{"id":"anthropic","name":"Anthropic (Claude)","local":false,"has_key":true,"current":true}]}""")
              "models" -> json("""{"ok":true,"provider":"anthropic","models":["claude-opus-5-5","claude-sonnet-5-5"],"current":"claude-opus-5-5","efforts":$effortsJson,"effort":null}""")
              "profiles" -> json("""{"ok":true,"active":"","profiles":[]}""")
              "chat" -> if (runCatching { ZoJson.parseToJsonElement(body).jsonObject["id"]!!.jsonPrimitive.content }.getOrNull() == "cnew")
                json("""{"ok":true,"id":"cnew","title":"Hello there","entries":[{"kind":"msg-user","text":"hello there"},{"kind":"msg-ai","text":"Hi from the **fake** model."}],"takes":[]}""")
              else json("""{"ok":true,"id":"c1","title":"Pinned plan","entries":[{"kind":"msg-user","text":"Plan my week"},{"kind":"msg-ai","text":"Here is a plan for **your week**."}],"takes":[]}""")
              "memory" -> json("""{"ok":true,"enabled":true,"every":3,"items":[{"id":"m1","kind":"preference","text":"I prefer short answers","title":"","chat":"","pinned":true,"updated":1794509148940},{"id":"m2","kind":"person","text":"My name is Zo and I live in Lahore","title":"","chat":"","pinned":false,"updated":1794509148000}]}""")
              "personalities" -> json("""{"ok":true,"active":"friend","items":[{"id":"friend","name":"Friendly buddy","prompt":"Sound like a warm friend.","generated":false,"builtin":true}]}""")
              "activity" -> json("""{"ok":true,"items":[]}""")
              else -> json("""{"ok":true}""")
            }
          }
          else -> json("""{"ok":true}""")
        }
      }
    }
    server.start()
  }

  fun url(): String = "http://localhost:${server.port}"
  fun shutdown() = server.shutdown()

  /** A container for the test that is signed in against this server. */
  fun signedInContainer(): AppContainer {
    val c = AppContainer(ApplicationProvider.getApplicationContext())
    c.useServerForTests(url())
    val user = AuthUser("u1", "zo@example.com", "Zo Tester", emailVerified = true, passwordAccount = false)
    c.auth.forceStateForTests(AuthState.SignedIn(user))
    c.session.onAuthState(AuthState.SignedIn(user))
    return c
  }
}
