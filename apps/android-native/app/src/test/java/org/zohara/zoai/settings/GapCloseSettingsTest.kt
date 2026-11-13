package org.zohara.zoai.settings

import android.provider.Settings
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SheetContent
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.net.PushSubscription
import org.zohara.zoai.core.store.TtlCache
import org.zohara.zoai.core.util.NotificationChannels

/** The gaps of the parity check of 2026-11-13 in Settings: MODEL-07, MODEL-08, MODEL-26, SYS-06, NOTIF-12, SET-APP-04/05/10. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h1600dp-hdpi")
class GapCloseSettingsTest {
  @get:Rule val rule = createComposeRule()
  private val fake = FakeServer()
  @After fun down() = fake.shutdown()

  private fun themed(dark: Boolean = true, sheet: Boolean = false, content: @Composable () -> Unit) = rule.setContent {
    ZoAiTheme(dark) { Column(Modifier.background(if (sheet) ZoTheme.colors.sheet else ZoTheme.colors.bg).width(400.dp).padding(horizontal = if (sheet) 0.dp else 16.dp)) { content() } }
  }

  // ---------------------------------------------------------------- NOTIF-12
  private val notif = NotifState(
    permission = true, denied = false, answers = true, needsYou = true, details = false, showTitle = false, sound = true, closed = false, admin = true,
    devices = listOf(PushSubscription("i1", "fcm", "Android: Pixel 8", 1_794_509_000)), thisDevice = "i1",
  )

  @Test fun theNotificationsPageOpensTheSystemSettingsAndEachChannel() {
    var system = 0; val channels = mutableListOf<String>()
    themed { NotificationsContent(notif, NotifActions(onOpenSystem = { system++ }, onOpenChannel = { channels += it })) }
    rule.onNodeWithText("Open system notification settings").assertExists()
    rule.onAllNodesWithText("Open")[0].performClick()
    assertEquals(1, system)
    rule.onAllNodesWithText("Open")[1].performClick()
    rule.onAllNodesWithText("Open")[2].performClick()
    assertEquals(listOf(NotificationChannels.ANSWERS, NotificationChannels.NEEDS_YOU), channels)
  }

  @Test fun theChannelIntentsPointAtTheRightPages() {
    val i = NotifLogic.channelSettings("org.zohara.zoai", NotificationChannels.NEEDS_YOU)
    assertEquals(Settings.ACTION_CHANNEL_NOTIFICATION_SETTINGS, i.action)
    assertEquals("org.zohara.zoai", i.getStringExtra(Settings.EXTRA_APP_PACKAGE))
    assertEquals("needs_you", i.getStringExtra(Settings.EXTRA_CHANNEL_ID))
  }

  private fun notifShot(dark: Boolean) {
    themed(dark) { NotificationsContent(notif, NotifActions()) }
    rule.onRoot().captureRoboImage("src/test/screenshots/settings_notifications_system_${if (dark) "dark" else "light"}.png")
  }
  @Test fun notificationsWithSystemRowsDark() = notifShot(true)
  @Test fun notificationsWithSystemRowsLight() = notifShot(false)

  // ---------------------------------------------------------------- SYS-06 / ATT-18 on the generic one-line sheet
  @Test fun enterConfirmsAOneLineSheet() {
    val got = mutableListOf<String>()
    var closed = false
    themed(sheet = true) { InputSheet("Rename", "Save", { closed = true }, label = "Name", initial = "Work") { got += it } }
    rule.waitForIdle()
    rule.onNode(hasSetTextAction()).performTextReplacement("Work 2")
    rule.onNode(hasSetTextAction()).performImeAction()
    rule.waitForIdle()
    assertEquals(listOf("Work 2"), got)
    assertTrue(closed)
  }

  // ---------------------------------------------------------------- MODEL-26
  @Test fun theNewProfileHintIsTheDesktopsSentence() {
    assertEquals("Keeps the provider, model, address, effort and the API key that is in use now.", ModelLogic.NEW_PROFILE_HINT)
  }

  // ---------------------------------------------------------------- MODEL-08
  private val claudeModels = listOf(
    "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "claude-opus-5-1", "claude-sonnet-5-1", "claude-opus-4-8", "claude-haiku-4-5",
  )

  @Test fun theOlderClaudeModelsAreSplitOffByFamily() {
    val (main, more) = ModelLogic.splitMain(claudeModels)
    assertEquals(listOf("claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"), main)
    assertEquals(listOf("claude-opus-5-1", "claude-sonnet-5-1", "claude-opus-4-8", "claude-haiku-4-5"), more)
    assertEquals("opus", ModelLogic.family("claude-opus-5-5"))
    assertEquals("llama3", ModelLogic.family("llama3"))
  }

  private val claudeState = ModelSheetState(
    loading = false, providers = SAMPLE_PROVIDERS, providerId = "claude-code", models = claudeModels, chosen = "claude-opus-5-5", pins = setOf("claude-sonnet-5-5"),
    efforts = listOf("low", "medium", "high"), effort = "",
  )
  @Composable private fun ClaudeSheet(state: ModelSheetState) = Column(Modifier.background(ZoTheme.colors.sheet)) {
    SheetContent("Model and API key", {}, footer = { SecondaryButton("Close", {}); PrimaryButton("Use this model", {}) }) { ModelSheetBody(state, false, ModelSheetActions()) }
  }

  @Test fun claudeCodeShowsMainModelsAndAMoreModelsGroupThatOpens() {
    themed(sheet = true) { ClaudeSheet(claudeState) }
    rule.onNodeWithText("claude-opus-5-5").assertExists()
    rule.onNodeWithText("More models").assertExists()
    assertEquals("older ones are hidden", 0, rule.onAllNodesWithText("claude-opus-4-8").fetchSemanticsNodes().size)
    rule.onNodeWithText("More models").performClick()
    rule.onNodeWithText("claude-opus-4-8").assertExists()
  }

  @Test fun aSearchShowsEveryModelAndOtherProvidersKeepAFlatList() {
    themed(sheet = true) { ClaudeSheet(claudeState.copy(filter = "opus")) }
    rule.onNodeWithText("claude-opus-4-8").assertExists()
    rule.onAllNodesWithText("More models").let { assertEquals(0, it.fetchSemanticsNodes().size) }
  }

  @Test fun claudeSheetDark() { themed(true, sheet = true) { ClaudeSheet(claudeState) }; rule.onNodeWithText("More models").performClick(); rule.onRoot().captureRoboImage("src/test/screenshots/settings_model_sheet_claude_dark.png") }
  @Test fun claudeSheetLight() { themed(false, sheet = true) { ClaudeSheet(claudeState) }; rule.onRoot().captureRoboImage("src/test/screenshots/settings_model_sheet_claude_light.png") }

  // ---------------------------------------------------------------- MODEL-07 (the effort levels come from the models list when the cache is empty)
  @Test fun theMenuAsksForTheEffortLevelsWhenTheCacheDoesNotHaveThem() {
    fake.on("status", """{"ok":true,"mode":"Auto","model":"claude-sonnet-5","provider":"claude-code","ready":true,"version":"0.1.0"}""")
    fake.on("profiles", """{"ok":true,"active":"","profiles":[]}""")
    fake.on("providers", """{"ok":true,"current":"claude-code","providers":[{"id":"claude-code","name":"Claude Code","has_key":true}]}""")
    fake.on("models", """{"ok":true,"provider":"claude-code","models":["claude-sonnet-5"],"current":"claude-sonnet-5","efforts":["low","medium","high"],"effort":"medium"}""")
    val cache = TtlCache<String, ModelsResp>()
    val m = ModelMenuController(fake.api, CoroutineScope(Dispatchers.Default), cache, {}, {})
    m.start()
    val s = runBlocking { withTimeout(8000) { m.state.first { !it.loading } } }
    assertEquals(listOf("low", "medium", "high"), s.efforts)
    assertEquals("medium", s.effort)
    assertTrue("the answer is cached for the sheet", cache.get(ModelLogic.cacheKey("claude-code", "")) != null)
  }

  // ---------------------------------------------------------------- SET-APP-04/05/10
  @Test fun theMcpSheetsSayWhatTheyAreDoing() {
    assertEquals("Trying 2 server(s)…", McpLogic.trying("""{"mcpServers":{"a":{"url":"https://a/mcp"},"b":{"url":"https://b/mcp"}}}"""))
    assertEquals("Trying 1 server(s)…", McpLogic.trying("not json"))
    assertEquals("Trying 3 server(s)…", McpLogic.trying("""{"a":{},"b":{},"c":{}}"""))
    assertEquals("Connecting…", McpLogic.CONNECTING)
    assertEquals("If it needs a sign-in, add it in the desktop app first.", McpLogic.SIGN_IN_HINT)
  }
}
