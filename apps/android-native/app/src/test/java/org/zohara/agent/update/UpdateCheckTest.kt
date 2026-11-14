package org.zohara.agent.update

import kotlinx.coroutines.runBlocking
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

/** The server call and what a check comes to (docs/UPDATES.md): available, up to date, unknown, required, and every way the call itself fails. */
class UpdateCheckTest {
  private val server = MockWebServer()
  private lateinit var base: String

  @Before fun start() { server.start(); base = server.url("/").toString().trimEnd('/') }
  @After fun stop() { server.shutdown() }

  private fun checker(current: String) = UpdateChecker(UpdateApi(serverUrl = { base }), current)

  private val latestJson = """{"ok":true,"server_version":"0.1.0","repo":"o/r","checked_at":1794600000,"stale":false,
    "latest":{"version":"0.2.0","date":"2026-11-20","notes":"Update checks.\nBug fixes.","min_supported":"0.1.0",
      "assets":{"apk":"zohara-agent-0.2.0.apk","sha256s":{"zohara-agent-0.2.0.apk":"${"a".repeat(64)}"}},
      "release_url":"https://github.com/o/r/releases/tag/v0.2.0"}}"""

  private fun answer(body: String, code: Int = 200) = server.enqueue(MockResponse().setResponseCode(code).setHeader("Content-Type", "application/json").setBody(body))

  @Test fun aNewerReleaseIsAvailable() = runBlocking {
    answer(latestJson)
    val o = checker("0.1.0").check() as UpdateOutcome.Available
    assertEquals("0.2.0", o.latest.version)
    assertEquals("Update checks.\nBug fixes.", o.latest.notes)
    assertEquals("zohara-agent-0.2.0.apk", o.latest.assets.apk)
    assertEquals("0.1.0", o.serverVersion)
    assertFalse(o.required)
    assertEquals("Version 0.2.0 is available (you have 0.1.0).", UpdateOutcomes.summary(o))
    assertEquals("https://github.com/o/r/releases/tag/v0.2.0", UpdateOutcomes.releasePage(o.latest))
  }

  @Test fun theCallIsPublicAndCarriesNothingAboutThePerson() = runBlocking {
    answer(latestJson)
    checker("0.1.0").check()
    val r = server.takeRequest()
    assertEquals("GET", r.method)
    assertEquals("/api/update/latest", r.path)
    assertNull(r.getHeader("Authorization"))
    assertNull(r.getHeader("Cookie"))
    assertNull(r.getHeader("X-ZO-Local"))
  }

  @Test fun theSameOrANewerVersionIsUpToDate() = runBlocking {
    answer(latestJson)
    assertTrue(checker("0.2.0").check() is UpdateOutcome.UpToDate)
    answer(latestJson)
    val o = checker("0.3.0").check()
    assertTrue(o is UpdateOutcome.UpToDate)
    assertEquals("You are up to date (0.3.0).", UpdateOutcomes.summary(o))
  }

  @Test fun anOldVersionBelowMinSupportedIsRequired() = runBlocking {
    answer(latestJson.replace("\"min_supported\":\"0.1.0\"", "\"min_supported\":\"0.1.5\""))
    assertTrue((checker("0.1.0").check() as UpdateOutcome.Available).required)
    answer(latestJson.replace("\"min_supported\":\"0.1.0\"", "\"min_supported\":\"0.1.5\""))
    assertFalse((checker("0.1.5").check() as UpdateOutcome.Available).required)
  }

  @Test fun noReleaseYetIsUnknownNotAnError() = runBlocking {
    answer("""{"ok":true,"server_version":"0.1.0","latest":null,"error":"no release has been published yet","stale":false}""")
    val o = checker("0.1.0").check() as UpdateOutcome.Unknown
    assertEquals("no release has been published yet", o.reason)
    assertEquals("Could not check for updates: no release has been published yet.", UpdateOutcomes.summary(o))
  }

  @Test fun aReleaseWithAnUnreadableVersionIsUnknown() = runBlocking {
    answer("""{"ok":true,"latest":{"version":"soon"}}""")
    assertTrue(checker("0.1.0").check() is UpdateOutcome.Unknown)
  }

  @Test fun onlyGithubAddressesAreOpened() {
    assertEquals(UpdateOutcomes.DEFAULT_RELEASES, UpdateOutcomes.releasePage(null))
    for (bad in listOf("http://github.com/o/r", "https://evil.example/https://github.com/", "javascript:alert(1)", "", "https://github.com/a b")) {
      assertEquals(bad, UpdateOutcomes.DEFAULT_RELEASES, UpdateOutcomes.releasePage(LatestRelease("1.0.0", releaseUrl = bad)))
    }
  }

  private fun assertFails(message: String, block: suspend () -> Unit) {
    try { runBlocking { block() }; fail("expected a failure") } catch (e: UpdateCheckException) { assertEquals(message, e.message) }
  }

  @Test fun everyFailureOfTheCallIsOneShortSentence() {
    answer("{}", 404)
    assertFails("the server answered 404") { checker("0.1.0").check() }
    answer("<html>", 200)
    assertFails("the server's answer is not understood") { checker("0.1.0").check() }
    answer("""{"ok":false}""")
    assertFails("the server does not support update checks") { checker("0.1.0").check() }
    answer("x".repeat(300_000))
    assertFails("the server's answer is too big") { checker("0.1.0").check() }
    server.shutdown()
    assertFails("the server could not be reached") { checker("0.1.0").check() }
  }
}
