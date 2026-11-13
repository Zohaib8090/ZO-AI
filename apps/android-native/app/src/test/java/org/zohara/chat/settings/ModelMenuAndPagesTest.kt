package org.zohara.chat.settings

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.zohara.chat.core.net.ModelsResp
import org.zohara.chat.core.store.TtlCache

@RunWith(RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [35])
class ModelMenuAndPagesTest {
  private lateinit var fake: FakeServer
  private val toasts = java.util.concurrent.CopyOnWriteArrayList<String>()

  @Before fun up() {
    fake = FakeServer()
    fake.on("status", """{"ok":true,"mode":"Auto","model":"claude-sonnet-5","provider":"anthropic","ready":true,"version":"0.1.0"}""")
    fake.on("profiles", """{"ok":true,"active":"p1","profiles":[
      {"id":"p1","name":"Work","provider":"anthropic","model":"claude-sonnet-5","effort":"high","pinned":["claude-sonnet-5","claude-haiku-5"]},
      {"id":"p2","name":"Free","provider":"openrouter","model":"deepseek-chat","pinned":[]}]}""")
    fake.on("providers", """{"ok":true,"current":"anthropic","providers":[{"id":"anthropic","name":"Anthropic (Claude)","has_key":true},{"id":"openrouter","name":"OpenRouter","has_key":true}]}""")
  }
  @After fun down() { fake.shutdown() }

  private fun menu(cache: TtlCache<String, ModelsResp> = TtlCache()) = ModelMenuController(fake.api, CoroutineScope(Dispatchers.Default), cache, { toasts.add(it) }, { })
  private fun <T> wait(f: kotlinx.coroutines.flow.StateFlow<T>, c: (T) -> Boolean): T = runBlocking { withTimeout(8000) { f.first(c) } }

  @Test fun theMenuGroupsPinnedModelsByProfileAndTicksTheCurrentOne() {
    val m = menu(); m.start()
    val s = wait(m.state) { !it.loading }
    assertEquals(listOf("Work · Anthropic (Claude)", "Free · OpenRouter"), s.groups.map { it.header })
    assertEquals(listOf("claude-sonnet-5", "claude-haiku-5"), s.groups[0].models)
    assertEquals("p1", s.activeProfile); assertEquals("claude-sonnet-5", s.currentModel)
    assertFalse(s.showInUseNow)
    assertTrue(s.hasProvider)
  }

  @Test fun inUseNowShowsWhenTheModelIsNotInAnyGroup() {
    fake.on("status", """{"ok":true,"mode":"Auto","model":"claude-opus-5","provider":"anthropic","ready":true,"version":"0.1.0"}""")
    val m = menu(); m.start()
    assertTrue(wait(m.state) { !it.loading }.showInUseNow)
  }

  @Test fun nothingChosenShowsTheBeginState() {
    fake.on("status", """{"ok":true,"mode":"Auto","model":"","provider":"","ready":false,"version":"0.1.0"}""")
    val m = menu(); m.start()
    val s = wait(m.state) { !it.loading }
    assertFalse(s.hasProvider); assertFalse(s.showInUseNow)
  }

  @Test fun oneTapSwitchesProfileThenModel() {
    fake.on("profile_use", """{"ok":true,"provider":"anthropic","model":"claude-sonnet-5"}"""); fake.on("select", """{"ok":true}""")
    val m = menu(); m.start(); wait(m.state) { !it.loading }
    var done = false
    m.pick("p1", "claude-haiku-5") { done = true }
    runBlocking { withTimeout(5000) { while (!done) kotlinx.coroutines.delay(20) } }
    val ops = fake.ops().filter { it == "profile_use" || it == "select" }
    assertEquals(listOf("profile_use", "select"), ops)
    assertEquals("p1", fake.body("profile_use")["id"]!!.jsonPrimitive.content)
    assertEquals("claude-haiku-5", fake.body("select")["model"]!!.jsonPrimitive.content)
    assertTrue(toasts.contains("Using claude-haiku-5"))
  }

  @Test fun effortRowAppearsWhenTheModelListIsCached() {
    val cache = TtlCache<String, ModelsResp>()
    cache.put(ModelLogic.cacheKey("anthropic", ""), ModelsResp(efforts = listOf("low", "high"), effort = "high"))
    val m = menu(cache); m.start()
    val s = wait(m.state) { !it.loading }
    assertEquals(listOf("low", "high"), s.efforts); assertEquals("high", s.effort)
    fake.on("select", """{"ok":true}""")
    m.setEffort("low")
    wait(m.state) { it.effort == "low" }
    assertEquals("low", fake.body("select")["effort"]!!.jsonPrimitive.content)
  }

  // ---- the Model page
  @Test fun modelPageLoadsEverythingAndAdminsGetTheClaudeCard() = runBlocking {
    fake.on("settings_get", """{"ok":true,"hidden_providers":["groq"],"provider_prefs":{},"updated":1}""")
    fake.on("/api/account/claude_status", """{"ok":true,"present":false,"source":"none","set_at":0,"admin":true}""")
    val store = HiddenStore(fake.api, LocalSettings(RuntimeEnvironment.getApplication().getSharedPreferences("m", 0)))
    val d = loadModelPage(fake.api, store, restricted = false)
    assertEquals(2, d.profiles.profiles.size); assertEquals("anthropic", d.providers.current)
    assertEquals(listOf("groq"), d.hidden); assertTrue(d.hiddenOnServer)
    assertEquals(false, d.claude!!.present)
  }

  @Test fun restrictedAccountsDoNotAskForClaudeStatus() = runBlocking {
    val store = HiddenStore(fake.api, LocalSettings(RuntimeEnvironment.getApplication().getSharedPreferences("m2", 0)))
    val d = loadModelPage(fake.api, store, restricted = true)
    assertNull(d.claude)
    assertFalse("/api/account/claude_status" in fake.ops())
    assertFalse(d.hiddenOnServer) // this fake server has no settings_get
  }

  @Test fun loaderShowsLoadingThenDataThenTheErrorSentence() = runBlocking {
    var fail = false
    val l = Loader(CoroutineScope(Dispatchers.Default)) { if (fail) throw org.zohara.chat.core.net.ApiException(org.zohara.chat.core.net.ErrorKind.Rejected, "That chat is gone.") else 42 }
    assertTrue(l.state.value is Load.Loading)
    l.load(); assertEquals(Load.Ready(42), wait(l.state) { it !is Load.Loading })
    fail = true; l.load()
    assertEquals(Load.Failed("That chat is gone."), wait(l.state) { it is Load.Failed })
  }

  @Test fun doerShowsTheSentenceOfAFailedCall() = runBlocking {
    val said = java.util.concurrent.CopyOnWriteArrayList<String>()
    val d = Doer(CoroutineScope(Dispatchers.Default)) { said.add(it) }
    var done = false
    d.run(onDone = { done = true }) { throw org.zohara.chat.core.net.ApiException(org.zohara.chat.core.net.ErrorKind.Rejected, "That note is too short.") }.join()
    assertEquals(listOf("That note is too short."), said); assertFalse(done)
    d.run(onDone = { done = true }) { }.join()
    assertTrue(done)
  }
}
