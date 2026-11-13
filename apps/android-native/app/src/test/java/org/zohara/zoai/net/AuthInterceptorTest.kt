package org.zohara.zoai.core.net

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class AuthInterceptorTest {
  private lateinit var server: MockWebServer
  private val calls = ArrayList<Boolean>()

  @Before fun up() { server = MockWebServer().also { it.start() } }
  @After fun down() { server.shutdown() }

  private fun client(local: Boolean = false, tokens: TokenSource) =
    OkHttpClient.Builder().addInterceptor(AuthInterceptor(tokens) { local }).build()

  private fun get(c: OkHttpClient, path: String) = c.newCall(Request.Builder().url(server.url(path)).build()).execute().use { it.code }

  @Test fun addsBearerToken() {
    server.enqueue(MockResponse().setBody("{}"))
    assertEquals(200, get(client { _ -> "tok1" }, "/api/op"))
    assertEquals("Bearer tok1", server.takeRequest().getHeader("Authorization"))
  }

  @Test fun noTokenMeansNoHeader() {
    server.enqueue(MockResponse().setBody("{}"))
    get(client { _ -> null }, "/api/op")
    assertNull(server.takeRequest().getHeader("Authorization"))
  }

  @Test fun a401RefreshesOnceAndRetries() {
    server.enqueue(MockResponse().setResponseCode(401))
    server.enqueue(MockResponse().setBody("{}"))
    val c = client { force -> calls.add(force); if (force) "fresh" else "old" }
    assertEquals(200, get(c, "/api/op"))
    assertEquals("Bearer old", server.takeRequest().getHeader("Authorization"))
    assertEquals("Bearer fresh", server.takeRequest().getHeader("Authorization"))
    assertEquals(listOf(false, true), calls)
  }

  @Test fun aSecond401IsReturnedAndNotRetriedAgain() {
    server.enqueue(MockResponse().setResponseCode(401))
    server.enqueue(MockResponse().setResponseCode(401))
    assertEquals(401, get(client { f -> if (f) "fresh" else "old" }, "/api/op"))
    assertEquals(2, server.requestCount)
  }

  @Test fun noFreshTokenReturnsThe401() {
    server.enqueue(MockResponse().setResponseCode(401))
    assertEquals(401, get(client { f -> if (f) null else "old" }, "/api/op"))
    assertEquals(1, server.requestCount)
  }

  @Test fun publicPagesGetNoToken() {
    server.enqueue(MockResponse().setBody("ok"))
    get(client { _ -> "tok" }, "/privacy")
    assertNull(server.takeRequest().getHeader("Authorization"))
  }

  @Test fun localTestHeader() {
    server.enqueue(MockResponse().setBody("{}"))
    get(client(true) { _ -> null }, "/api/op")
    assertEquals("1", server.takeRequest().getHeader("X-ZO-Local"))
  }

  @Test fun serverUrlRules() {
    assertEquals(true, ZoHttp.isAcceptableServer("https://zo-ai-server.onrender.com", false))
    assertEquals(false, ZoHttp.isAcceptableServer("http://example.com", true))
    assertEquals(false, ZoHttp.isAcceptableServer("http://localhost:8080", false))
    assertEquals(true, ZoHttp.isAcceptableServer("http://localhost:8080", true))
    assertEquals("https://a.b", ZoHttp.normalizeServer(" https://a.b/ "))
  }
}
