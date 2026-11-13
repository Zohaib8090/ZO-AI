package org.zohara.chat.link

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.ErrorKind
import org.zohara.chat.core.net.LinkCaps
import org.zohara.chat.core.net.LinkDevice
import org.zohara.chat.core.net.SendEvent
import org.zohara.chat.core.net.SendRequest
import org.zohara.chat.core.net.ZoApi
import org.zohara.chat.core.net.ZoHttp
import org.zohara.chat.core.net.ZoJson

/** LINK-01 to LINK-14, COMP-10, COMP-15, COMP-20, CHAT-37, PERM-03, INTER-04, INTER-17, MODEL-32: the link module against fakes and a mock server. */
@OptIn(ExperimentalCoroutinesApi::class)
class LinkTest {
  private lateinit var server: MockWebServer
  private lateinit var api: ZoApi

  @Before fun up() {
    server = MockWebServer().also { it.start() }
    api = ZoApi(ZoHttp.newClient({ _ -> "tok" }), { server.url("/").toString().trimEnd('/') }, retryDelayMs = { 0L })
  }
  @After fun down() { server.shutdown() }
  private fun ok(body: String) = MockResponse().setHeader("Content-Type", "application/json").setBody(body)

  private fun dev(id: String, name: String = "Laptop", online: Boolean = true, seen: Long = 0, cc: Boolean = false, web: Boolean = false) =
    LinkDevice(id, name, online, seen, LinkCaps(claudeCode = cc, approvalsFromWeb = web, version = "1.2", os = "linux"))

  // ---------------------------------------------------------------- format

  @Test fun timestampsMayBeSecondsOrMilliseconds() {
    assertEquals(1_700_000_000_000L, LinkFormat.millis(1_700_000_000))
    assertEquals(1_700_000_000_000L, LinkFormat.millis(1_700_000_000_000))
    assertEquals(0L, LinkFormat.millis(0))
  }

  @Test fun agoLikeTheWeb() {
    val now = 10_000_000_000L
    assertEquals("never", LinkFormat.ago(0, now))
    assertEquals("just now", LinkFormat.ago(now - 20_000, now))
    assertEquals("5 min ago", LinkFormat.ago(now - 5 * 60_000, now))
    assertEquals("3 h ago", LinkFormat.ago(now - 3 * 3_600_000, now))
    assertTrue(LinkFormat.ago(now - 5 * 86_400_000L, now).any { it.isDigit() } && !LinkFormat.ago(now - 5 * 86_400_000L, now).contains("ago"))
  }

  @Test fun rowsAndChips() {
    val now = 1_700_000_100_000L
    assertEquals("Online", LinkFormat.statusLine(dev("1"), now))
    assertEquals("Online · Claude Code installed", LinkFormat.statusLine(dev("1", cc = true), now))
    assertEquals("Not reachable · last seen 2 min ago", LinkFormat.statusLine(dev("1", online = false, seen = 1_700_000_000 + 0).copy(lastSeen = (now - 120_000) / 1000), now))
    assertEquals("Not reachable", LinkFormat.statusLine(dev("1", online = false), now))
    assertEquals(listOf("Claude Code not found", "Zohara Chat 1.2", "linux", "Approvals are answered on the computer"), LinkFormat.chips(dev("1")))
    assertEquals(listOf("Claude Code installed", "Zohara Chat 1.2", "linux", "Approvals can be answered here"), LinkFormat.chips(dev("1", cc = true, web = true)))
    assertEquals("My computer (Laptop)", LinkFormat.runLabel("Laptop"))
    assertEquals("My computer", LinkFormat.runLabel(""))
    assertEquals("Computer", LinkFormat.name(dev("1", name = "")))
    assertEquals(60, LinkFormat.name(dev("1", name = "n".repeat(90))).length)
  }

  @Test fun modesOnAComputer() {
    assertEquals(listOf("Manual", "Auto"), LinkModes.available(onComputer = true, restricted = false))
    assertEquals(listOf("Manual", "Auto"), LinkModes.available(onComputer = false, restricted = true))
    assertEquals(listOf("Manual", "Auto", "Bypass"), LinkModes.available(onComputer = false, restricted = false))
    assertEquals("Auto", LinkModes.shown("Bypass", true))
    assertEquals("Bypass", LinkModes.shown("Bypass", false))
    assertEquals("Manual", LinkModes.shown("", true))
    assertEquals("Your computer may limit this: web sessions never run on Bypass unless you allowed it there.", LinkText.MODE_NOTE)
  }

  // ---------------------------------------------------------------- status

  @Test fun linkStatusIsReadWithNumericIdsAndCapsAndLimitedTo30() = runBlocking {
    val many = (1..40).joinToString(",") { """{"id":$it,"name":"PC $it","online":${it % 2 == 0},"last_seen":1700000000,"caps":{"claude_code":true,"approvals_from_web":true,"version":"0.9","os":"macos"}}""" }
    server.enqueue(ok("""{"ok":true,"devices":[$many]}"""))
    val repo = LinkRepository({ api.linkStatus() })
    assertTrue(repo.refresh())
    val s = repo.state.value
    assertEquals(30, s.devices.size)
    assertEquals("1", s.devices[0].id) // a number became text
    assertFalse(s.devices[0].online); assertTrue(s.devices[1].online)
    assertTrue(s.devices[0].caps.claudeCode && s.devices[0].caps.approvalsFromWeb)
    assertEquals("0.9", s.devices[0].caps.version)
    assertTrue(s.ok && s.at > 0 && !s.busy)
    assertEquals("/api/link/status", server.takeRequest().path)
  }

  @Test fun aFailedCheckKeepsTheOldListAndSaysNotOk() = runBlocking {
    server.enqueue(ok("""{"ok":true,"devices":[{"id":"a","name":"A","online":true}]}"""))
    server.enqueue(MockResponse().setResponseCode(500).setBody("{}"))
    val repo = LinkRepository({ api.linkStatus() }, { 5L })
    repo.refresh()
    assertFalse(repo.refresh())
    assertEquals(1, repo.state.value.devices.size)
    assertFalse(repo.state.value.ok)
    assertEquals(5L, repo.state.value.at)
  }

  @Test fun onlyOneCheckRunsAtATime() = runTest {
    var calls = 0
    val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
    val repo = LinkRepository({ calls++; gate.await(); emptyList() })
    val a = launch { repo.refresh() }
    runCurrent()
    assertTrue(repo.state.value.busy)
    repo.refresh() // returns at once
    gate.complete(Unit)
    a.join()
    assertEquals(1, calls)
  }

  @Test fun theStatusIsPolledEvery20SecondsUntilCancelled() = runTest {
    var n = 0
    val job = launch { pollEvery(LINK_POLL_MS) { n++ } }
    runCurrent(); assertEquals(1, n)
    advanceTimeBy(19_999); runCurrent(); assertEquals(1, n)
    advanceTimeBy(2); runCurrent(); assertEquals(2, n)
    advanceTimeBy(40_000); runCurrent(); assertEquals(4, n)
    job.cancel()
    advanceTimeBy(60_000); runCurrent(); assertEquals(4, n)
    assertEquals(20_000L, LINK_POLL_MS)
  }

  // ---------------------------------------------------------------- run on

  private class Store(var c: RunOnChoice = RunOnChoice.Cloud) : RunOnStore {
    override suspend fun load() = c
    override suspend fun save(choice: RunOnChoice) { c = choice }
  }

  private fun TestScopeRun(devices: () -> List<LinkDevice>, store: Store, toasts: MutableList<String>, scope: kotlinx.coroutines.CoroutineScope, fail: () -> Boolean = { false }): RunOnController {
    val repo = LinkRepository({ if (fail()) throw ApiException(ErrorKind.Offline, "Offline. Retrying.") else devices() })
    return RunOnController(repo, store, scope) { toasts.add(it) }
  }

  @Test fun theChoiceIsRestoredAtStart() = runTest(StandardTestDispatcher()) {
    val c = TestScopeRun({ emptyList() }, Store(RunOnChoice("d1", "Laptop")), ArrayList(), kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()))
    runCurrent()
    assertEquals(RunOnChoice("d1", "Laptop"), c.choice.value)
  }

  @Test fun choosingAnOfflineComputerWarns() = runTest {
    val toasts = ArrayList<String>(); val store = Store()
    val c = TestScopeRun({ emptyList() }, store, toasts, kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()))
    c.choose(dev("d2", "Desk", online = false))
    advanceUntilIdle()
    assertEquals(listOf("That computer is not reachable right now."), toasts)
    assertEquals(RunOnChoice("d2", "Desk"), store.c)
    c.choose(null)
    advanceUntilIdle()
    assertTrue(c.choice.value.isCloud); assertTrue(store.c.isCloud)
  }

  @Test fun aComputerThatIsGoneSendsTheChoiceBackToTheCloud() = runTest {
    val toasts = ArrayList<String>(); val store = Store(RunOnChoice("gone", "Old"))
    val c = TestScopeRun({ listOf(dev("other")) }, store, toasts, kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()))
    advanceUntilIdle()
    assertTrue(c.refresh())
    advanceUntilIdle()
    assertTrue(c.choice.value.isCloud)
    assertEquals(listOf("That computer is no longer linked. Back to the cloud."), toasts)
    assertTrue(store.c.isCloud)
  }

  @Test fun aFailedCheckDoesNotLoseTheChoice() = runTest {
    val toasts = ArrayList<String>()
    val c = TestScopeRun({ emptyList() }, Store(RunOnChoice("d1", "L")), toasts, kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()), fail = { true })
    advanceUntilIdle()
    assertFalse(c.refresh())
    assertEquals("d1", c.choice.value.device)
    assertTrue(toasts.isEmpty())
  }

  @Test fun theNameIsKeptUpToDate() = runTest {
    val store = Store(RunOnChoice("d1", "Old name"))
    val c = TestScopeRun({ listOf(dev("d1", "New name")) }, store, ArrayList(), kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()))
    advanceUntilIdle(); c.refresh(); advanceUntilIdle()
    assertEquals("New name", c.choice.value.name); assertEquals("New name", store.c.name)
  }

  @Test fun beforeSendingToAComputerItIsChecked() = runTest {
    val toasts = ArrayList<String>()
    var devices = listOf(dev("d1", online = true))
    var failing = false
    val c = TestScopeRun({ devices }, Store(RunOnChoice("d1", "Laptop")), toasts, kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()), fail = { failing })
    advanceUntilIdle()
    assertEquals(SendCheck.Ok, c.ensureReachable())
    devices = listOf(dev("d1", online = false))
    assertEquals(SendCheck.Stop("Your computer is not reachable."), c.ensureReachable())
    failing = true
    assertEquals(SendCheck.Stop("Could not check your computer."), c.ensureReachable())
    failing = false; devices = emptyList()
    assertEquals(SendCheck.Stop("Back to the cloud: send again."), c.ensureReachable())
    assertEquals(SendCheck.Ok, c.ensureReachable()) // now in the cloud: no look needed
  }

  @Test fun theCloudNeedsNoLookBeforeSending() = runTest {
    var calls = 0
    val c = TestScopeRun({ calls++; emptyList() }, Store(), ArrayList(), kotlinx.coroutines.CoroutineScope(StandardTestDispatcher(testScheduler) + kotlinx.coroutines.SupervisorJob()))
    advanceUntilIdle()
    assertEquals(SendCheck.Ok, c.ensureReachable())
    assertEquals(0, calls)
  }


  // ---------------------------------------------------------------- desktop ops

  @Test fun onlyTheWhitelistedOpsGoToTheComputer() = runBlocking {
    server.enqueue(ok("""{"ok":true,"result":{"provider":"ollama","model":"llama3","mode":"Manual","ready":true}}"""))
    val ops = DesktopOps({ d, inner -> api.desktopOp(d, inner) }, { "dev1" })
    val st = ops.status()
    assertEquals("llama3", st.model); assertEquals("Manual", st.mode); assertTrue(st.ready)
    val req = server.takeRequest()
    val body = ZoJson.parseToJsonElement(req.body.readUtf8()).jsonObject
    assertEquals("desktop_op", body["op"]!!.jsonPrimitive.content)
    assertEquals("dev1", body["device"]!!.jsonPrimitive.content)
    assertEquals("status", body["inner"]!!.jsonObject["op"]!!.jsonPrimitive.content)
    assertEquals(setOf("status", "providers", "profiles", "profile_use", "models", "select", "mode"), DesktopOps.ALLOWED)
  }

  @Test fun theModelAndProfileCallsHaveTheRightShapes() = runBlocking {
    val seen = ArrayList<JsonObject>()
    val ops = DesktopOps({ _, inner -> seen.add(inner); when (inner["op"]!!.jsonPrimitive.content) {
      "providers" -> ZoJson.parseToJsonElement("""{"current":"ollama","providers":[{"id":"ollama","name":"Ollama","local":true,"has_key":false}]}""").jsonObject
      "models" -> ZoJson.parseToJsonElement("""{"provider":"ollama","models":["a","b"],"current":"a"}""").jsonObject
      "profiles" -> ZoJson.parseToJsonElement("""{"active":"p1","profiles":[{"id":"p1","name":"Work","provider":"ollama","model":"a"}]}""").jsonObject
      else -> ZoJson.parseToJsonElement("""{"provider":"ollama","model":"b"}""").jsonObject
    } }, { "dev1" })
    assertEquals("ollama", ops.providers().current)
    assertEquals(listOf("a", "b"), ops.models("ollama").models)
    assertEquals("Work", ops.profiles().profiles[0].name)
    ops.select("ollama", "b"); ops.profileUse("p1")
    assertEquals("ollama", seen[1]["provider"]!!.jsonPrimitive.content)
    assertEquals(listOf("providers", "models", "profiles", "select", "profile_use"), seen.map { it["op"]!!.jsonPrimitive.content })
    assertEquals("b", seen[3]["model"]!!.jsonPrimitive.content)
    assertEquals("p1", seen[4]["id"]!!.jsonPrimitive.content)
  }

  @Test fun bypassIsRefusedOnAComputer() = runBlocking {
    var called = false
    val ops = DesktopOps({ _, _ -> called = true; buildJsonObject { put("mode", "Auto") } }, { "d" })
    try { ops.mode("Bypass"); fail() } catch (e: DesktopOpException) { assertEquals(LinkText.MODE_NOTE, e.message) }
    assertFalse(called)
    assertEquals("Auto", ops.mode("Auto"))
  }

  @Test fun anOfflineComputerGivesTheOfflineMessageAndAsksForARefresh() = runBlocking {
    server.enqueue(MockResponse().setResponseCode(503).setBody("""{"error":"desktop_offline"}"""))
    var refreshed = 0
    val ops = DesktopOps({ d, inner -> api.desktopOp(d, inner) }, { "dev1" }, { refreshed++ })
    try { ops.status(); fail() } catch (e: DesktopOpException) {
      assertEquals(LinkText.OFFLINE, e.message); assertTrue(e.offline)
    }
    assertEquals(1, refreshed)
  }

  @Test fun otherComputerErrorsHaveSentences() {
    fun msg(text: String, status: Int) = DesktopOps.explain(ApiException(ErrorKind.Server, text, status)).message
    assertEquals("Your computer took too long to answer.", msg("desktop_timeout", 503))
    assertEquals("The connection to your computer was lost.", msg("desktop_lost", 503))
    assertEquals("That is not allowed from the web.", msg("not_allowed", 400))
    assertEquals("Your computer could not do that.", msg("desktop_error", 503))
    assertEquals("something else", msg("something else", 400))
  }

  @Test fun withoutAComputerChosenThereIsNothingToAsk() = runBlocking {
    val ops = DesktopOps({ _, _ -> JsonObject(emptyMap()) }, { null })
    try { ops.status(); fail() } catch (e: DesktopOpException) { assertEquals("Run on My computer first.", e.message) }
  }


  // ---------------------------------------------------------------- claude status

  @Test fun theClaudeCardTexts() {
    assertEquals(LinkText.NO_CLAUDE, claudeLine(null))
    assertEquals(LinkText.NO_CLAUDE, claudeLine(org.zohara.chat.core.net.ClaudeStatus(present = false)))
    val line = claudeLine(org.zohara.chat.core.net.ClaudeStatus(present = true, source = "server", setAt = 1_700_000_000, admin = true), java.util.Locale.US)
    assertTrue(line, line.startsWith("Claude Code (your subscription) is ready on the server. Sign-in sent "))
    assertTrue(line, line.endsWith("It lasts about a year; send a new one from the desktop app before then."))
    assertTrue(line, line.contains("2023"))
    assertTrue(claudeLine(org.zohara.chat.core.net.ClaudeStatus(present = true), java.util.Locale.US).contains("Sign-in sent earlier."))
  }

  @Test fun claudeStatusComesFromTheAccountCall() = runBlocking {
    server.enqueue(ok("""{"ok":true,"present":true,"source":"server","set_at":1700000000,"admin":true}"""))
    val s = api.claudeStatus()
    assertTrue(s.present && s.admin); assertEquals("server", s.source)
    assertEquals("/api/account/claude_status", server.takeRequest().path)
  }
}
