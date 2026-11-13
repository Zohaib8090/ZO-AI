package org.zohara.agent.pages

import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.zohara.agent.core.net.ZoApi
import org.zohara.agent.core.net.ZoHttp
import org.zohara.agent.core.net.ZoJson

/** The real client against a mock server with the answers captured live (docs/API.md): the wire shapes the pages depend on. */
class PagesApiTest {
  private lateinit var server: MockWebServer
  private lateinit var api: PagesApi

  @Before fun up() {
    server = MockWebServer().also { it.start() }
    api = ZoPagesApi(ZoApi(ZoHttp.newClient({ _ -> "tok" }), { server.url("/").toString().trimEnd('/') }, retryDelayMs = { 0L }))
  }
  @After fun down() { server.shutdown() }

  private fun ok(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)
  private fun body() = ZoJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject

  @Test fun chatsLive() = runBlocking {
    server.enqueue(ok("""{"ok":true,"chats":[{"id":"c1794509120577","pinned":true,"title":"run it","updated":1794509131852}]}"""))
    val c = api.chats().single()
    assertEquals("run it", c.title); assertTrue(c.pinned); assertEquals(1794509131852, c.updated)
  }

  @Test fun searchLive() = runBlocking {
    server.enqueue(ok("""{"ok":true,"hits":[{"id":"c1","snippet":"Hello from the fake model.","title":"run it","updated":1794509131852}]}"""))
    val h = api.search("fake").single()
    assertEquals("Hello from the fake model.", h.snippet)
    assertEquals("fake", body()["query"]!!.jsonPrimitive.content)
  }

  @Test fun pinAndDeleteBodies() = runBlocking {
    server.enqueue(ok("""{"ok":true}""")); server.enqueue(ok("""{"ok":true}"""))
    api.pinChat("c1", true); api.deleteChat("c1")
    val pin = body(); val del = body()
    assertEquals("pin_chat", pin["op"]!!.jsonPrimitive.content); assertEquals("true", pin["on"]!!.jsonPrimitive.content)
    assertEquals("delete_chat", del["op"]!!.jsonPrimitive.content); assertEquals("c1", del["id"]!!.jsonPrimitive.content)
  }

  @Test fun projectsLiveTextBecomesCards() = runBlocking {
    server.enqueue(ok("""{"ok":true,"text":"p1794509093697 | Demo | g | 0 memory notes"}"""))
    assertEquals("Demo", PagesFormat.parseProjects(api.projects()).single().name)
  }

  @Test fun artifactsAndScheduledLive() = runBlocking {
    server.enqueue(ok("""{"ok":true,"items":[{"id":"a1","kind":"code","title":"x.rs","path":"","text":"fn main() {}","chat":"c1","created":1794509131852}]}"""))
    server.enqueue(ok("""{"ok":true,"tasks":[{"id":"t17945090995370","title":"t","prompt":"t","next_run":1794512699000,"repeat":"once","enabled":true}]}"""))
    val a = api.artifacts().single(); val t = api.scheduled().single()
    assertEquals("fn main() {}", a.text); assertEquals("c1", a.chat)
    assertEquals(1794512699000, t.nextRun); assertTrue(t.enabled)
  }

  @Test fun scheduleAddRewritesInAndSendsRepeat() = runBlocking {
    server.enqueue(ok("""{"ok":true,"message":"Scheduled \"t\" (once)."}"""))
    api.scheduleAdd("check disk", "in 2 hours", "every 90 minutes")
    val b = body()
    assertEquals("now + 2 hours", b["when"]!!.jsonPrimitive.content)
    assertEquals("every 90 minutes", b["repeat"]!!.jsonPrimitive.content)
    assertEquals("check disk", b["task"]!!.jsonPrimitive.content)
  }

  @Test fun activityLive() = runBlocking {
    server.enqueue(ok("""{"ok":true,"now_ms":1794509131887,"clients_seen":[{"last_ms":1794509131884,"source":"web"}],"items":[{"id":3,"kind":"answer","title":"hello","detail":"m1","source":"web","chat_id":"c1","started_ms":1794509131000,"can_stop":true}]}"""))
    val a = api.activity()
    assertEquals(887, a.nowMs - a.items[0].startedMs)
    assertEquals("c1", a.items[0].chatId); assertTrue(a.items[0].canStop)
    assertEquals("web", a.clientsSeen[0].source)
  }

  @Test fun refusedOpIsTheServersSentence() = runBlocking {
    server.enqueue(ok("""{"ok":false,"error":"Scheduled tasks are not available to this account."}"""))
    val h = ScheduledHolder(api, kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Unconfined))
    h.refresh()
    val done = kotlinx.coroutines.withTimeout(5000) { h.state.first { !it.loading } }
    assertEquals("Scheduled tasks are not available to this account.", done.error)
  }
}
