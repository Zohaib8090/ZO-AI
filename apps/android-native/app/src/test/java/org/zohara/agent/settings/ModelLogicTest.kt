package org.zohara.agent.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.agent.core.net.ModelsResp
import org.zohara.agent.core.net.Profile
import org.zohara.agent.core.net.ProfilesResp

class ModelLogicTest {
  private val all = SAMPLE_PROVIDERS

  // ---- MODEL-10
  @Test fun restrictedAccountsGetNoLocalProvidersAndNoClaudeCode() {
    val ids = ModelLogic.shownProviders(all, emptyList(), null, restricted = true).map { it.id }
    assertEquals(listOf("anthropic", "openai", "gemini", "groq", "openrouter", "custom"), ids)
  }

  @Test fun hiddenProvidersAreLeftOutExceptTheOneInUse() {
    val ids = ModelLogic.shownProviders(all, listOf("groq", "openai"), "openai", restricted = false).map { it.id }
    assertTrue("openai" in ids); assertFalse("groq" in ids)
  }

  @Test fun theSheetDropsClaudeCodeUnlessTheServerHasTheSignIn() {
    assertFalse(ModelLogic.sheetProviders(all, emptyList(), null, false, claudeReady = false).any { it.id == "claude-code" })
    assertTrue(ModelLogic.sheetProviders(all, emptyList(), null, false, claudeReady = true).any { it.id == "claude-code" })
  }

  @Test fun hiddenHintCountsOnlyWhatIsHiddenAndNotInUse() {
    assertEquals(null, ModelLogic.hiddenHint(all, emptyList(), null, false))
    assertEquals("1 provider is hidden. Settings > Model and profiles has a switch for each.", ModelLogic.hiddenHint(all, listOf("groq"), null, false))
    assertEquals("2 providers are hidden. Settings > Model and profiles has a switch for each.", ModelLogic.hiddenHint(all, listOf("groq", "openai", "ollama"), null, true))
    assertEquals(null, ModelLogic.hiddenHint(all, listOf("groq"), "groq", false))
  }

  // ---- MODEL-09
  @Test fun providerGroupsAndTicks() {
    val g = ModelLogic.groups(all)
    assertEquals(listOf("Popular Providers", "Cloud & Open-Weight", "Local Models (This Computer)", "Custom Endpoint"), g.map { it.label })
    assertEquals(listOf("anthropic", "openai", "gemini"), g[0].items.map { it.id })
    assertEquals(listOf("groq", "openrouter"), g[1].items.map { it.id })
    assertEquals(listOf("ollama", "claude-code"), g[2].items.map { it.id })
    assertEquals("Anthropic (Claude) ✓", ModelLogic.providerLabel(all[0]))
    assertEquals("OpenAI", ModelLogic.providerLabel(all[1]))
    assertEquals("Ollama (this computer) ✓", ModelLogic.providerLabel(all[5]))
  }

  // ---- MODEL-12
  @Test fun savedAddressIsOnlyShownWhenNotTheNormalOne() {
    val p = provider("openai", defaultUrl = "https://api.openai.com/v1", baseUrl = "https://proxy.example/v1")
    assertEquals("https://proxy.example/v1", ModelLogic.savedAddress(p, "openai"))
    assertEquals("", ModelLogic.savedAddress(p, "gemini"))
    assertEquals("", ModelLogic.savedAddress(p.copy(baseUrl = p.defaultUrl), "openai"))
  }

  @Test fun addressHintsPerKindOfProvider() {
    assertTrue(ModelLogic.addressHint(all.first { it.id == "custom" }, false).startsWith("The address of any OpenAI-compatible server, for example http://192.168.1.20:8000/v1"))
    assertTrue(ModelLogic.addressHint(all.first { it.id == "custom" }, true).endsWith("Only public https addresses work here."))
    assertEquals("Where the server runs. The default is right unless you changed its port.", ModelLogic.addressHint(all.first { it.id == "ollama" }, false))
    assertTrue(ModelLogic.addressHint(all.first { it.id == "openai" }, false).startsWith("Only change this for a proxy or a regional endpoint."))
  }

  @Test fun addressValidationForAdministrators() {
    assertEquals("", ModelLogic.validateAddress("", false, false))
    assertTrue(ModelLogic.validateAddress("", true, false).startsWith("Enter the address of the endpoint"))
    assertEquals("", ModelLogic.validateAddress("http://192.168.1.20:8000/v1", true, false))
    assertEquals("", ModelLogic.validateAddress("https://my-host.example/v1", true, false))
    assertEquals("The address must start with http:// or https://", ModelLogic.validateAddress("ftp://x.example/v1", true, false))
    assertTrue(ModelLogic.validateAddress("my host", true, false).startsWith("That does not look like a web address"))
  }

  @Test fun restrictedAccountsOnlyGetPublicHttpsAddresses() {
    assertEquals("", ModelLogic.validateAddress("https://api.example.com/v1", true, true))
    assertEquals("Only https addresses work here.", ModelLogic.validateAddress("http://api.example.com/v1", true, true))
    val priv = "Use a public address. Local and private addresses are not allowed on this server."
    for (h in listOf("https://localhost/v1", "https://127.0.0.1/v1", "https://10.0.0.5/v1", "https://192.168.1.20/v1", "https://172.16.0.1/v1", "https://172.31.9.9/v1", "https://169.254.1.1/v1", "https://box.local/v1", "https://nas.internal/v1", "https://router.lan/v1", "https://intranet/v1", "https://[::1]/v1")) {
      assertEquals(h, priv, ModelLogic.validateAddress(h, true, true))
    }
    assertEquals("", ModelLogic.validateAddress("https://172.32.0.1/v1", true, true)) // outside 172.16/12
  }

  @Test fun newEndpointValidationUsesTheMatrixSentences() {
    assertEquals("Type the address first.", ModelLogic.validateNewEndpoint("  ", false))
    assertEquals("The address should start with http:// or https://", ModelLogic.validateNewEndpoint("my-host.example/v1", false))
    assertEquals("", ModelLogic.validateNewEndpoint("http://192.168.1.20:8000/v1", false))
    assertEquals("Only https addresses work here.", ModelLogic.validateNewEndpoint("http://api.example.com/v1", true))
  }

  // ---- MODEL-14
  @Test fun modelListPinnedFirstAndFiltered() {
    val models = listOf("gpt-4o", "gpt-4o-mini", "o3", "gpt-5")
    assertEquals(listOf("o3", "gpt-4o", "gpt-4o-mini", "gpt-5"), ModelLogic.orderModels(models, setOf("o3"), ""))
    assertEquals(listOf("gpt-4o-mini", "gpt-4o"), ModelLogic.orderModels(models, setOf("gpt-4o-mini"), "GPT-4"))
    assertEquals(emptyList<String>(), ModelLogic.orderModels(models, emptySet(), "zzz"))
  }

  @Test fun typedTextThatIsNotInTheListIsOffered() {
    val models = listOf("gpt-4o", "o3")
    assertEquals("my-model", ModelLogic.customOffer(models, " my-model "))
    assertEquals("", ModelLogic.customOffer(models, "GPT-4O"))
    assertEquals("", ModelLogic.customOffer(models, ""))
    assertEquals("gpt", ModelLogic.customOffer(models, "gpt"))
  }

  @Test fun listMessages() {
    assertEquals("Nothing is listed at that address (404). It usually ends in /v1.", ModelLogic.listMessage(ModelsResp(error = "Nothing is listed at that address (404). It usually ends in /v1.")))
    assertEquals("The provider returned no models. You can still type a model name above.", ModelLogic.listMessage(ModelsResp()))
    assertEquals("", ModelLogic.listMessage(ModelsResp(models = listOf("a"))))
  }

  @Test fun effortLabels() {
    assertEquals(listOf("Default", "Low", "Medium", "High", "Extra high", "Max"), listOf(null, "low", "medium", "high", "xhigh", "max").map { ModelLogic.effortLabel(it) })
    assertEquals("Default", ModelLogic.effortLabel(""))
  }

  // ---- profiles and pins
  @Test fun profileLinesAndVision() {
    val p = SAMPLE_PROFILES.profiles
    assertEquals("Anthropic (Claude) · claude-sonnet-5 · 2 pinned", ModelLogic.profileLine(p[0], "Anthropic (Claude)"))
    assertEquals("Custom endpoint · no model yet", ModelLogic.profileLine(p[2], "Custom endpoint"))
    assertEquals("Pictures: this model can see pictures (set on the desktop)", ModelLogic.visionLine(p[0]))
    assertEquals(null, ModelLogic.visionLine(p[1]))
    assertTrue(ModelLogic.visionLine(p[1].copy(vision = false))!!.contains("cannot see pictures"))
  }

  @Test fun pinRowsListEveryPinWithItsProfile() {
    val rows = ModelLogic.pinRows(SAMPLE_PROFILES) { it.uppercase() }
    assertEquals(3, rows.size)
    assertEquals(ModelLogic.PinRow("p1", "Work", "ANTHROPIC", "claude-haiku-5"), rows[1])
    assertEquals(listOf("claude-sonnet-5", "claude-haiku-5"), ModelLogic.activePins(SAMPLE_PROFILES))
    assertEquals(emptyList<String>(), ModelLogic.activePins(ProfilesResp("zz", SAMPLE_PROFILES.profiles)))
  }

  @Test fun providerSwitchSubLines() {
    assertEquals("Key saved · 2 pinned", ModelLogic.providerSub(all[0], 2, false))
    assertEquals("No key yet", ModelLogic.providerSub(all[1], 0, false))
    assertEquals("Runs on this computer, no key", ModelLogic.providerSub(all[5], 0, false))
    assertEquals("Uses your Claude Code sign-in", ModelLogic.providerSub(all[6], 0, false))
    assertEquals("Key saved · in use, always shown", ModelLogic.providerSub(all[0], 0, true))
  }

  // ---- MODEL-01, 02, 06
  @Test fun theMenuHasOneGroupPerProfileWithPinsOrItsOneModel() {
    val g = ModelLogic.menuGroups(SAMPLE_PROFILES) { if (it == "anthropic") "Anthropic (Claude)" else it }
    assertEquals(listOf("p1", "p2"), g.map { it.profileId })  // p3 has no model and no pins
    assertEquals("Work · Anthropic (Claude)", g[0].header)
    assertEquals(listOf("claude-sonnet-5", "claude-haiku-5"), g[0].models)
    assertEquals(listOf("deepseek/deepseek-chat"), g[1].models)
    val noPins = ModelLogic.menuGroups(ProfilesResp("a", listOf(Profile("a", "Solo", "openai", "gpt-4o")))) { it }
    assertEquals(listOf("gpt-4o"), noPins[0].models)
  }

  @Test fun inUseNowWhenTheCurrentModelIsNotInTheMenu() {
    val g = ModelLogic.menuGroups(SAMPLE_PROFILES) { it }
    assertFalse(ModelLogic.needsInUseNow(g, "p1", "claude-sonnet-5"))
    assertTrue(ModelLogic.needsInUseNow(g, "p1", "something-else"))
    assertTrue(ModelLogic.needsInUseNow(g, "p2", "claude-sonnet-5")) // same name, other profile
    assertFalse(ModelLogic.needsInUseNow(g, "p1", ""))
  }
}
