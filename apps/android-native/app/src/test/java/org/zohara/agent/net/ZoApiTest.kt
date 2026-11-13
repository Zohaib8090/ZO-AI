package org.zohara.agent.core.net

import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

class ZoApiTest {
  private lateinit var server: MockWebServer
  private lateinit var api: ZoApi

  @Before fun up() {
    server = MockWebServer().also { it.start() }
    val http = ZoHttp.newClient({ _ -> "tok" })
    api = ZoApi(http, { server.url("/").toString().trimEnd('/') }, retryDelayMs = { 0L })
  }
  @After fun down() { server.shutdown() }

  private fun ok(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)

  @Test fun opPostsTheOpAndDecodes() = runBlocking {
    server.enqueue(ok("""{"ok":true,"chats":[{"id":"a","title":"One","pinned":true,"updated":5}]}"""))
    val chats = api.chats()
    assertEquals("a", chats[0].id)
    val req = server.takeRequest()
    assertEquals("/api/op", req.path)
    assertEquals("Bearer tok", req.getHeader("Authorization"))
    assertEquals("chats", ZoJson.parseToJsonElement(req.body.readUtf8()).jsonObject["op"]!!.jsonPrimitive.content)
  }

  @Test fun okFalseBecomesAnException() = runBlocking {
    server.enqueue(ok("""{"ok":false,"error":"unknown request: model_test"}"""))
    try { api.modelTest(); fail() } catch (e: ApiException) { assertEquals(ErrorKind.UnknownOp, e.kind) }
  }

  @Test fun rateLimitThrowsWithRetryAfter() = runBlocking {
    server.enqueue(MockResponse().setResponseCode(429).setHeader("Retry-After", "3").setBody("{}"))
    try { api.deleteChat("x"); fail() } catch (e: ApiException) {
      assertEquals(ErrorKind.RateLimited, e.kind)
      assertEquals(3, e.retryAfterSec)
    }
  }

  @Test fun readOnlyCallRetriesOnceOn503() = runBlocking {
    server.enqueue(MockResponse().setResponseCode(503).setBody("{}"))
    server.enqueue(ok("""{"ok":true,"chats":[]}"""))
    assertEquals(0, api.chats().size)
    assertEquals(2, server.requestCount)
  }

  @Test fun listenerSeesErrors() = runBlocking {
    var seen: ErrorKind? = null
    api.errorListener = { seen = it.kind }
    server.enqueue(MockResponse().setResponseCode(403).setBody("""{"error":"t","terms_required":true,"version":"v9"}"""))
    try { api.chats(); fail() } catch (_: ApiException) {}
    assertEquals(ErrorKind.TermsRequired, seen)
  }

  @Test fun sendStreamsEventsAndEndsAfterDone() = runBlocking {
    val body = listOf(
      """{"event":"stream","id":1}""", """{"event":"chat","id":"c","title":"T"}""", """{"event":"delta","text":"He"}""",
      """{"event":"delta","text":"llo"}""", """{"event":"usage","input":1,"output":2}""", """{"event":"done"}""", """{"event":"delta","text":"after done"}""",
    ).joinToString("\n", postfix = "\n")
    server.enqueue(MockResponse().setHeader("Content-Type", "application/x-ndjson").setBody(body))
    val events = api.send(SendRequest("hi", chat = "c", keepRunning = true, mode = "auto")).toList()
    assertEquals(listOf("Stream", "Chat", "Delta", "Delta", "Usage", "Done"), events.map { it::class.simpleName })
    val sent = ZoJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject
    assertEquals("hi", sent["text"]!!.jsonPrimitive.content)
    assertEquals("true", sent["keep_running"]!!.jsonPrimitive.content)
    assertEquals("auto", sent["mode"]!!.jsonPrimitive.content)
  }

  @Test fun sendWithoutDoneReportsConnectionLost() = runBlocking {
    server.enqueue(MockResponse().setBody("""{"event":"delta","text":"x"}""" + "\n"))
    val events = api.send(SendRequest("hi")).toList()
    assertTrue(events.last() is SendEvent.ConnectionLost)
  }

  @Test fun sendRefusedThrowsOnCollect() = runBlocking {
    server.enqueue(MockResponse().setResponseCode(429).setHeader("Retry-After", "2").setBody("{}"))
    try { api.send(SendRequest("hi")).toList(); fail() } catch (e: ApiException) { assertEquals(ErrorKind.RateLimited, e.kind) }
  }

  @Test fun replyBodies() = runBlocking {
    server.enqueue(ok("""{"ok":true}"""))
    assertTrue(api.replyApprove(5, 9, true))
    val a = ZoJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject
    assertEquals("/api/reply", "/api/reply")
    assertEquals("approve", a["op"]!!.jsonPrimitive.content)
    assertEquals("5", a["stream"]!!.jsonPrimitive.content)
    server.enqueue(ok("""{"ok":true}"""))
    api.replyAnswer(5, 9, listOf(QuestionAnswer(listOf("A", "B"), "other\u0007 text")))
    val b = ZoJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject
    assertEquals("answer", b["op"]!!.jsonPrimitive.content)
    assertTrue(b.toString().contains("other"))
    assertTrue(!b.toString().contains("\\u0007"))
    // "You decide" is sent as decide:true and nothing else is set
    server.enqueue(ok("""{"ok":true}"""))
    api.replyAnswer(5, 9, listOf(QuestionAnswer(emptyList(), null, decide = true), QuestionAnswer(listOf("A"))))
    val c = ZoJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject["answers"]!!.jsonArray
    assertEquals("true", c[0].jsonObject["decide"]!!.jsonPrimitive.content)
    assertEquals(0, c[0].jsonObject["selected"]!!.jsonArray.size)
    assertTrue(c[1].jsonObject["decide"] == null)
  }
}
