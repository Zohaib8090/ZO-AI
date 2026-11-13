package org.zohara.zoai.settings

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
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
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.store.TtlCache

@RunWith(RobolectricTestRunner::class)
@org.robolectric.annotation.Config(sdk = [35])
class ModelControllerTest {
  private lateinit var fake: FakeServer
  private val toasts = java.util.concurrent.CopyOnWriteArrayList<String>()
  private var invalidated = 0
  private lateinit var local: LocalSettings

  private fun str(o: JsonObject, k: String) = o[k]?.jsonPrimitive?.content

  private val providersJson = """{"ok":true,"current":"anthropic","hidden_providers":["groq"],"providers":[
    {"id":"anthropic","name":"Anthropic (Claude)","has_key":true,"default_url":"https://api.anthropic.com/v1","base_url":"https://api.anthropic.com/v1"},
    {"id":"openai","name":"OpenAI","has_key":false,"default_url":"https://api.openai.com/v1","base_url":"https://api.openai.com/v1"},
    {"id":"groq","name":"Groq","has_key":false},
    {"id":"ollama","name":"Ollama","local":true,"needs_url":true,"default_url":"http://127.0.0.1:11434/v1"},
    {"id":"custom","name":"Custom endpoint","needs_url":true}]}"""
  private val profilesJson = """{"ok":true,"active":"p1","profiles":[{"id":"p1","name":"Work","provider":"anthropic","model":"claude-sonnet-5","pinned":["claude-sonnet-5"]}]}"""

  @Before fun up() {
    fake = FakeServer()
    local = LocalSettings(RuntimeEnvironment.getApplication().getSharedPreferences("t", 0))
    fake.on("providers", providersJson)
    fake.on("profiles", profilesJson)
    fake.on("status", """{"ok":true,"mode":"Auto","model":"claude-sonnet-5","provider":"anthropic","ready":true,"version":"0.1.0"}""")
    fake.on("settings_get", """{"ok":true,"hidden_providers":["openrouter"],"provider_prefs":{},"updated":1}""")
    fake.on("models", """{"ok":true,"provider":"anthropic","models":["claude-sonnet-5","claude-haiku-5","claude-opus-5"],"current":"claude-sonnet-5","efforts":["low","high"],"effort":"high"}""")
  }
  @After fun down() { fake.shutdown() }

  private fun controller(restricted: Boolean = false, admin: Boolean = true, cache: TtlCache<String, ModelsResp> = TtlCache()) =
    ModelSheetController(
      fake.api, CoroutineScope(Dispatchers.Default), restricted, admin, cache, HiddenStore(fake.api, local), { toasts.add(it) }, { invalidated++ },
    )

  private fun waitFor(c: ModelSheetController, cond: (ModelSheetState) -> Boolean): ModelSheetState = runBlocking { withTimeout(8000) { c.state.first(cond) } }

  @Test fun startLoadsEverythingAndFetchesTheModelsOfTheCurrentProvider() {
    fake.on("/api/account/claude_status", """{"ok":true,"present":true,"source":"own","set_at":1794509000,"admin":true}""")
    val c = controller()
    c.start()
    val s = waitFor(c) { !it.loading && !it.fetching && it.models.isNotEmpty() }
    assertEquals("anthropic", s.providerId)
    assertEquals("claude-sonnet-5", s.chosen)
    assertEquals(setOf("claude-sonnet-5"), s.pins)
    assertEquals(listOf("low", "high"), s.efforts); assertEquals("high", s.effort)
    // hidden: the server's settings and the providers answer, merged; the one in use stays
    assertFalse(s.providers.any { it.id == "groq" })
    assertTrue(s.hiddenHint!!.startsWith("2 providers are hidden") || s.hiddenHint!!.startsWith("1 provider is hidden"))
    assertTrue(s.claudeLine!!.startsWith("Claude Code (your subscription) is ready on the server."))
    assertEquals("", s.address) // the normal address is not shown as a typed one
  }

  @Test fun restrictedAccountsNeverAskForClaudeAndGetNoLocalProviders() {
    val c = controller(restricted = true, admin = false)
    c.start()
    val s = waitFor(c) { !it.loading && !it.fetching }
    assertFalse("/api/account/claude_status" in fake.ops())
    assertFalse(s.providers.any { it.id == "ollama" })
    assertNull(s.claudeLine)
  }

  @Test fun aFailedLoadShowsTheSentence() {
    fake.answers.remove("providers")
    val c = controller()
    c.start()
    val s = waitFor(c) { !it.loading }
    assertTrue(s.loadError!!.isNotBlank())
  }

  @Test fun changingProviderListsItsModelsAndResetsTheChoice() {
    fake.on("models") { """{"ok":true,"provider":"${str(it, "provider")}","models":["gpt-4o","o3"],"current":""}""" }
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.selectProvider("openai")
    val s = waitFor(c) { it.providerId == "openai" && !it.fetching && it.models.isNotEmpty() }
    assertEquals(listOf("gpt-4o", "o3"), s.models)
    assertEquals("", s.chosen)
    assertEquals("openai", str(fake.body("models"), "provider"))
  }

  @Test fun aCustomEndpointWaitsForTheAddressBeforeFetching() {
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    val before = fake.ops().count { it == "models" }
    c.selectProvider("custom")
    Thread.sleep(300)
    assertEquals(before, fake.ops().count { it == "models" })
    assertEquals("", c.state.value.fetchError)
    // a bad address is reported at once and nothing is asked
    c.setAddress("my host"); c.addressDone()
    assertTrue(c.state.value.addressError.startsWith("That does not look like a web address"))
    c.setAddress("https://my-host.example/v1"); c.addressDone()
    waitFor(c) { !it.fetching && it.models.isNotEmpty() }
    assertEquals("https://my-host.example/v1", str(fake.body("models"), "base_url"))
  }

  @Test fun aFailedFetchKeepsTheFormAndOffersRetry() {
    fake.on("models", """{"ok":true,"provider":"anthropic","models":[],"error":"The provider did not accept the key (401). Check it and try again."}""")
    val c = controller(); c.start()
    val s = waitFor(c) { !it.loading && !it.fetching && it.fetchError.isNotEmpty() }
    assertEquals("The provider did not accept the key (401). Check it and try again.", s.fetchError)
    assertTrue(s.models.isEmpty())
    // Retry asks again and a good answer clears the error
    fake.on("models", """{"ok":true,"provider":"anthropic","models":["m1"],"current":""}""")
    c.fetchModels(force = true)
    val s2 = waitFor(c) { !it.fetching && it.models.isNotEmpty() }
    assertEquals("", s2.fetchError)
  }

  @Test fun noModelsAndNoErrorSaysTheProviderReturnedNone() {
    fake.on("models", """{"ok":true,"provider":"anthropic","models":[]}""")
    val c = controller(); c.start()
    assertEquals("The provider returned no models. You can still type a model name above.", waitFor(c) { !it.loading && !it.fetching }.fetchError)
  }

  @Test fun modelListsAreCachedAndForcedOnRetry() {
    val cache = TtlCache<String, ModelsResp>()
    val c = controller(cache = cache); c.start(); waitFor(c) { !it.loading && !it.fetching && it.models.isNotEmpty() }
    assertEquals(1, fake.ops().count { it == "models" })
    c.fetchModels(); waitFor(c) { !it.fetching }; Thread.sleep(100)
    assertEquals(1, fake.ops().count { it == "models" })
    c.fetchModels(force = true); waitFor(c) { !it.fetching }; Thread.sleep(100)
    assertEquals(2, fake.ops().count { it == "models" })
  }

  @Test fun useThisModelSendsSelectWithEffortAndAddress() {
    fake.on("select", """{"ok":true,"provider":"anthropic","model":"claude-opus-5"}""")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching && it.models.isNotEmpty() }
    c.choose("claude-opus-5"); c.setEffort("low")
    var used = false
    c.use { used = true }
    waitFor(c) { !it.using }; Thread.sleep(100)
    assertTrue(used)
    val b = fake.body("select")
    assertEquals("anthropic", str(b, "provider")); assertEquals("claude-opus-5", str(b, "model")); assertEquals("low", str(b, "effort")); assertEquals("", str(b, "base_url"))
    assertTrue(toasts.contains("Using claude-opus-5")); assertTrue(invalidated > 0)
  }

  @Test fun useIsRefusedWithABadAddressForARestrictedAccount() {
    fake.on("select", """{"ok":true}""")
    val c = controller(restricted = true, admin = false); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.selectProvider("custom"); c.setAddress("http://192.168.1.20:8000/v1"); c.choose("m")
    c.use { }
    Thread.sleep(200)
    assertFalse("select" in fake.ops())
    assertEquals("Only https addresses work here.", c.state.value.addressError)
  }

  @Test fun serverRefusalOfSelectIsShownAsAToast() {
    fake.on("select", """{"ok":false,"error":"A restricted account can only use a model server with a public https address."}""")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching && it.models.isNotEmpty() }
    c.use { }
    waitFor(c) { !it.using }; Thread.sleep(100)
    assertTrue(toasts.contains("A restricted account can only use a model server with a public https address."))
  }

  @Test fun savingAKeySendsItOnceAndMarksTheProvider() {
    fake.on("set_key", """{"ok":true}""")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.selectProvider("openai"); waitFor(c) { it.providerId == "openai" && !it.fetching }
    c.saveKey("  sk-secret  ")
    waitFor(c) { it.provider?.hasKey == true && !it.savingKey }
    assertEquals("sk-secret", str(fake.body("set_key"), "key")); assertEquals("openai", str(fake.body("set_key"), "provider"))
    assertTrue(toasts.contains("Key saved"))
    assertTrue(c.state.value.toString().contains("sk-secret").not()) // the key is never kept in the state
  }

  @Test fun anEmptyKeyIsNotSent() {
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.saveKey("   ")
    assertEquals("Paste the key first.", toasts.last())
    assertFalse("set_key" in fake.ops())
  }

  @Test fun testConnectionShowsTheServersMessage() {
    fake.on("model_test", """{"ok":true,"works":true,"message":"Connected. 3 models available."}""")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.testConnection()
    val s = waitFor(c) { !it.testing && it.testOk != null }
    assertEquals(true, s.testOk); assertEquals("Connected. 3 models available.", s.testText)
    assertEquals("claude-sonnet-5", str(fake.body("model_test"), "model"))
    fake.on("model_test", """{"ok":true,"works":false,"message":"The provider did not accept the key (401)."}""")
    c.testConnection()
    assertEquals(false, waitFor(c) { !it.testing && it.testOk == false }.testOk)
  }

  @Test fun testConnectionOnAnOldServerSaysSo() {
    fake.answers.remove("model_test")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.testConnection()
    assertEquals("This server cannot test connections yet.", waitFor(c) { !it.testing && it.testOk == false }.testText)
  }

  @Test fun pinningToAnotherProviderCreatesAProfileFirst() {
    fake.on("select", """{"ok":true}""")
    fake.on("profile_create", """{"ok":true,"id":"p9"}""")
    fake.on("profile_pin", """{"ok":true}""")
    fake.on("models") { """{"ok":true,"provider":"openai","models":["gpt-4o"],"current":""}""" }
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching }
    c.selectProvider("openai"); waitFor(c) { it.providerId == "openai" && !it.fetching && it.models.isNotEmpty() }
    c.togglePin("gpt-4o")
    Thread.sleep(500)
    val ops = fake.ops()
    val i = ops.indexOf("select")
    assertTrue(i >= 0)
    assertEquals(listOf("select", "profile_create", "profile_pin"), ops.drop(i).take(3))
    assertEquals("OpenAI", str(fake.body("profile_create"), "name"))
    assertEquals("p9", str(fake.body("profile_pin"), "id")); assertEquals(true, fake.body("profile_pin")["on"]?.jsonPrimitive?.booleanOrNull)
    assertTrue(toasts.contains("Pinned to a new profile, OpenAI"))
  }

  @Test fun pinningOnTheActiveProfileJustPinsAndUnpinning() {
    fake.on("profile_pin", """{"ok":true}""")
    val c = controller(); c.start(); waitFor(c) { !it.loading && !it.fetching && it.models.isNotEmpty() }
    c.togglePin("claude-haiku-5"); Thread.sleep(400)
    assertFalse("profile_create" in fake.ops())
    assertEquals("p1", str(fake.body("profile_pin"), "id"))
    assertTrue(toasts.contains("Pinned"))
    c.togglePin("claude-sonnet-5"); Thread.sleep(400) // already pinned: this is an unpin
    assertEquals(false, fake.body("profile_pin")["on"]?.jsonPrimitive?.booleanOrNull)
    assertTrue(toasts.contains("Unpinned"))
  }

  // ---- hidden providers (MODEL-29)
  @Test fun hiddenProvidersGoToTheServer() = runBlocking {
    fake.on("settings_set", """{"ok":true,"hidden_providers":["groq"],"provider_prefs":{},"updated":2}""")
    val store = HiddenStore(fake.api, local)
    assertEquals(listOf("openrouter"), store.get())
    store.set(listOf("groq"))
    assertEquals(listOf("groq"), fake.body("settings_set")["hidden_providers"]!!.let { (it as kotlinx.serialization.json.JsonArray).map { e -> e.jsonPrimitive.content } })
    assertTrue(store.onServer); assertEquals(emptyList<String>(), local.hiddenProviders())
  }

  @Test fun anOldServerKeepsTheChoiceOnThePhone() = runBlocking {
    fake.answers.remove("settings_get")
    val store = HiddenStore(fake.api, local)
    assertEquals(emptyList<String>(), store.get())
    assertFalse(store.onServer)
    store.set(listOf("groq", "openai"))
    assertEquals(listOf("groq", "openai"), local.hiddenProviders())
    assertEquals(listOf("groq", "openai"), store.get())
    assertFalse("settings_set" in fake.ops())
  }

  // ---- add a custom endpoint (MODEL-25)
  @Test fun addingAnEndpointSelectsKeysAndCreatesTheProfileInThatOrder() {
    fake.on("select", """{"ok":true}"""); fake.on("set_key", """{"ok":true}"""); fake.on("profile_create", """{"ok":true,"id":"p7"}""")
    val c = AddEndpointController(fake.api, CoroutineScope(Dispatchers.Default), false, { toasts.add(it) }, { invalidated++ })
    var done = false
    c.add("Home GPU", "http://192.168.1.20:8000/v1", "sk-x", "llama3", { done = true })
    runBlocking { withTimeout(5000) { while (!done) kotlinx.coroutines.delay(20) } }
    assertEquals(listOf("select", "set_key", "profile_create"), fake.ops())
    assertEquals("custom", str(fake.body("select"), "provider")); assertEquals("http://192.168.1.20:8000/v1", str(fake.body("select"), "base_url")); assertEquals("llama3", str(fake.body("select"), "model"))
    assertEquals("Home GPU", str(fake.body("profile_create"), "name"))
    assertTrue(toasts.contains("Added Home GPU"))
  }

  @Test fun addingAnEndpointValidatesFirst() {
    val c = AddEndpointController(fake.api, CoroutineScope(Dispatchers.Default), true, { }, { })
    c.add("x", "", "", "", { })
    assertEquals("Type the address first.", c.state.value.error)
    c.add("x", "192.168.1.1/v1", "", "", { })
    assertEquals("The address should start with http:// or https://", c.state.value.error)
    c.add("", "https://api.example.com/v1", "", "", { })
    assertEquals("Give it a name first.", c.state.value.error)
    assertTrue(fake.ops().isEmpty())
  }

  @Test fun testingAnEndpointShowsTheMessage() {
    fake.on("select", """{"ok":true}"""); fake.on("model_test", """{"ok":true,"works":true,"message":"Connected. 2 models available."}""")
    val c = AddEndpointController(fake.api, CoroutineScope(Dispatchers.Default), false, { }, { })
    c.test("https://my-host.example/v1", "", "")
    runBlocking { withTimeout(5000) { c.state.first { it.testOk != null } } }
    assertEquals("Connected. 2 models available.", c.state.value.testText)
    assertFalse("set_key" in fake.ops()) // no key typed: nothing saved
  }
}
