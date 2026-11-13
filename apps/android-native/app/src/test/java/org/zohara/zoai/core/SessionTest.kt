package org.zohara.zoai.core

import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.test.core.app.ApplicationProvider
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.mockwebserver.Dispatcher
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import okhttp3.mockwebserver.RecordedRequest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.zohara.zoai.core.auth.AuthRepository
import org.zohara.zoai.core.auth.AuthState
import org.zohara.zoai.core.auth.AuthUser
import org.zohara.zoai.core.net.ZoApi
import org.zohara.zoai.core.net.ZoHttp
import org.zohara.zoai.core.net.ZoJson
import org.zohara.zoai.core.session.Gate
import org.zohara.zoai.core.session.NetBanner
import org.zohara.zoai.core.session.Session
import org.zohara.zoai.core.store.RunOn
import org.zohara.zoai.core.store.TtlCache
import org.zohara.zoai.core.store.ZoPrefs

/** The start sequence and the global error handling (AUTH-14, 17..22, NET-03, NET-05, NET-06) against a mock server with the shapes of docs/API.md. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class SessionTest {
  @get:Rule val tmp = TemporaryFolder()
  private lateinit var server: MockWebServer
  private lateinit var session: Session
  private lateinit var auth: AuthRepository
  private lateinit var prefs: ZoPrefs
  private lateinit var api: ZoApi
  private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
  @Volatile private var base = ""
  private val cache = TtlCache<String, String>()
  private val requests = java.util.concurrent.CopyOnWriteArrayList<Pair<String, String>>()
  @Volatile private var whoami: () -> MockResponse = { ok(WHOAMI_ADMIN) }
  @Volatile private var other: () -> MockResponse = { ok("""{"ok":true}""") }

  private fun ok(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)
  private fun fail(code: Int, body: String) = MockResponse().setResponseCode(code).setHeader("Content-Type", "application/json").setBody(body)

  @Before fun up() {
    server = MockWebServer().also {
      it.dispatcher = object : Dispatcher() {
        override fun dispatch(request: RecordedRequest): MockResponse {
          requests += request.path.orEmpty() to request.body.readUtf8()
          return if (request.path == "/api/account/whoami") whoami() else other()
        }
      }
      it.start()
    }
    val ds = PreferenceDataStoreFactory.create(scope = CoroutineScope(Dispatchers.IO)) { File(tmp.root, "p${System.nanoTime()}.preferences_pb") }
    prefs = ZoPrefs(ds, "https://default.example")
    auth = AuthRepository(ApplicationProvider.getApplicationContext(), false, "")
    base = server.url("/").toString().trimEnd('/')
    api = ZoApi(ZoHttp.newClient({ _ -> "tok" }), { base }, retryDelayMs = { 0L })
    session = Session(auth, api, prefs, scope, listOf(cache))
  }
  @After fun down() { runCatching { server.shutdown() } }

  private val user = AuthUser("u1", "zo@example.com", "Zo", emailVerified = true, passwordAccount = false)
  private fun signIn() = session.onAuthState(AuthState.SignedIn(user))

  private fun <T> waitFor(what: String, get: () -> T, ok: (T) -> Boolean): T {
    val end = System.currentTimeMillis() + 8000
    while (System.currentTimeMillis() < end) { val v = get(); if (ok(v)) return v; Thread.sleep(10) }
    throw AssertionError("timed out waiting for $what, last value ${get()}")
  }

  @Test fun startsWithWhoamiThenOpensTheApp() { // AUTH-19, AUTH-20
    assertEquals(Gate.Loading, session.gate.value)
    assertTrue("until known the app behaves as restricted", session.restricted)
    signIn()
    val g = waitFor("ready", { session.gate.value }) { it is Gate.Ready } as Gate.Ready
    assertEquals(true, g.account?.admin)
    assertFalse(session.restricted); assertTrue(session.admin)
    assertEquals("/api/account/whoami", requests.first().first)
  }

  @Test fun aRestrictedAccountStaysRestricted() { // AUTH-20
    whoami = { ok(WHOAMI_RESTRICTED) }
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    assertTrue(session.restricted); assertFalse(session.admin)
    assertEquals("zo@example.com", session.account?.email)
  }

  @Test fun whenWhoamiFailsTheAppStillOpensAsRestricted() { // AUTH-19
    whoami = { fail(500, """{"error":"boom"}""") }
    signIn()
    val g = waitFor("ready", { session.gate.value }) { it is Gate.Ready } as Gate.Ready
    assertNull(g.account); assertTrue(session.restricted)
  }

  @Test fun termsNotAcceptedShowConsentAndAcceptingContinues() { // AUTH-16, AUTH-17
    var accepted = false
    whoami = { ok(if (accepted) WHOAMI_ADMIN else WHOAMI_ADMIN.replace(""""accepted":true""", """"accepted":false""")) }
    other = { accepted = true; ok("""{"ok":true,"version":"2026-11-12"}""") }
    signIn()
    val c = waitFor("consent", { session.gate.value }) { it is Gate.Consent } as Gate.Consent
    assertEquals("2026-11-12", c.version)
    session.acceptTerms(c.version)
    waitFor("ready after accepting", { session.gate.value }) { it is Gate.Ready }
    val post = requests.first { it.first == "/api/account/accept_terms" }
    assertEquals("2026-11-12", ZoJson.parseToJsonElement(post.second).jsonObject["version"]!!.jsonPrimitive.content)
    runBlocking { assertEquals("nothing is stored on the phone", "", prefs.consentVersion.first()) }
  }

  @Test fun failingToAcceptGivesTheMessageAndStaysOnConsent() { // AUTH-17
    whoami = { ok(WHOAMI_ADMIN.replace(""""accepted":true""", """"accepted":false""")) }
    other = { fail(400, """{"error":"That is not the current version of the notice and the terms. Reload the page."}""") }
    signIn(); val c = waitFor("consent", { session.gate.value }) { it is Gate.Consent } as Gate.Consent
    var msg: String? = null
    session.acceptTerms(c.version) { msg = it }
    waitFor("message", { msg }) { it != null }
    assertTrue(msg!!.startsWith("That is not the current version"))
    assertTrue(session.gate.value is Gate.Consent)
  }

  @Test fun aTermsRequired403OnAnyCallReopensConsentWithThatVersion() { // AUTH-18
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    other = { fail(403, """{"terms_required":true,"error":"Please accept the terms.","version":"2027-01-01"}""") }
    runCatching { runBlocking { api.chats() } }
    val c = waitFor("consent", { session.gate.value }) { it is Gate.Consent } as Gate.Consent
    assertEquals("2027-01-01", c.version)
  }

  @Test fun a401ShowsTheSignedOutBannerAndAGoodCallDoesNotInventOne() { // NET-03
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    assertNull(session.banner.value)
    other = { fail(401, """{"error":"Sign in first."}""") }
    runCatching { runBlocking { api.chats() } }
    assertEquals(NetBanner.SignedOut, session.banner.value)
    session.dismissBanner(); assertNull(session.banner.value)
  }

  @Test fun offlineShowsTheBannerAndTheNextGoodCallHidesIt() { // NET-03
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    val good = base
    base = "http://localhost:1" // nothing listens there: the phone has no connection
    val e = runCatching { runBlocking { api.chats() } }.exceptionOrNull()
    assertEquals("the call failed as " + e, NetBanner.Offline, session.banner.value)
    base = good
    other = { ok("""{"ok":true,"chats":[]}""") }
    runBlocking { api.chats() }
    assertNull(session.banner.value)
  }

  @Test fun aBlockedAccountIsSignedOutWithTheMessage() { // AUTH-14, NET-06
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    other = { fail(403, """{"error":"This account is blocked."}""") }
    runCatching { runBlocking { api.chats() } }
    val m = waitFor("login message", { session.loginMessage.value }) { it != null }
    assertEquals("This account is blocked.", m)
    waitFor("signed out", { auth.state.value }) { it == AuthState.NotConfigured }
  }

  @Test fun aDeletedAccountIsSignedOutWithTheMessage() { // NET-06
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    other = { fail(410, """{"error":"This account was deleted."}""") }
    runCatching { runBlocking { api.chats() } }
    assertEquals("This account was deleted.", waitFor("login message", { session.loginMessage.value }) { it != null })
  }

  @Test fun busyServerMessageIsShownAsTextWithRetryAfter() { // NET-05
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    other = { fail(503, """{"error":"ZO-AI has as many people as it can take right now. Try again later."}""").setHeader("Retry-After", "7") }
    val before = requests.size
    val e = runCatching { runBlocking { api.send0() } }.exceptionOrNull()
    assertTrue("failed as $e", e is org.zohara.zoai.core.net.ApiException)
    assertTrue((e as org.zohara.zoai.core.net.ApiException).text.contains("as many people as it can take"))
    assertEquals("an op that is not read-only is never repeated (NET-09)", before + 1, requests.size)
  }

  @Test fun signOutRunsTheHooksFirstThenForgetsTheAccountAndCaches() { // AUTH-21, AUTH-22
    signIn(); waitFor("ready", { session.gate.value }) { it is Gate.Ready }
    cache.put("k", "v")
    runBlocking { prefs.setRunOn(RunOn("dev1")) }
    val order = java.util.concurrent.CopyOnWriteArrayList<String>()
    session.beforeSignOut += { order += "hook"; runBlocking { assertEquals("dev1", prefs.runOn.first().device) } }
    session.signOut()
    waitFor("the hook", { order.size }) { it == 1 }
    waitFor("the preferences cleared", { runBlocking { prefs.runOn.first().isCloud } }) { it }
    assertEquals(listOf("hook"), order)
    runBlocking { assertTrue("the Run on choice is forgotten", prefs.runOn.first().isCloud) }
    session.onAuthState(AuthState.NotConfigured)
    assertEquals(Gate.Loading, session.gate.value)
    assertNull("caches are cleared", cache.get("k"))
  }

  private suspend fun ZoApi.send0() = op("chats")

  companion object {
    const val WHOAMI_ADMIN = """{"ok":true,"admin":true,"restricted":false,"email":"zo@example.com","tenant":"t1","limits":{"requests_per_min":60,"concurrent_sends":2},"data_bytes":2224,"terms":{"version":"2026-11-12","accepted":true}}"""
    const val WHOAMI_RESTRICTED = """{"ok":true,"admin":false,"restricted":true,"email":"zo@example.com","tenant":"t1","limits":{"requests_per_min":30,"concurrent_sends":1,"message_chars":20000},"data_bytes":10,"terms":{"version":"2026-11-12","accepted":true}}"""
  }
}
