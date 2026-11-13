package org.zohara.zoai.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
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
import org.zohara.zoai.core.net.AccountDevice
import org.zohara.zoai.core.net.ClaudeStatus
import org.zohara.zoai.core.net.DeviceLookup
import org.zohara.zoai.core.net.Limits
import org.zohara.zoai.core.net.Machine
import org.zohara.zoai.core.net.McpServer
import org.zohara.zoai.core.net.MemoryItem
import org.zohara.zoai.core.net.MemoryResp
import org.zohara.zoai.core.net.PersonalitiesResp
import org.zohara.zoai.core.net.Personality
import org.zohara.zoai.core.net.ProvidersResp
import org.zohara.zoai.core.net.PushSubscription
import org.zohara.zoai.core.net.StatusInfo
import org.zohara.zoai.core.net.TermsState
import org.zohara.zoai.core.net.Whoami

/**
 * JVM screenshot tests of every Settings page and sheet, in both themes (`./gradlew recordRoborazziDebug` writes app/src/test/screenshots/settings_*.png).
 * The stateless `...Content` composables are drawn with sample data; the screens around them only add loading and the server calls.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h2400dp-hdpi")
class SettingsScreenshotTest {
  @get:Rule val rule = createComposeRule()

  private fun shot(name: String, dark: Boolean, sheet: Boolean = false, content: @Composable () -> Unit) {
    rule.setContent {
      ZoAiTheme(dark) {
        val c = ZoTheme.colors
        Column(Modifier.background(if (sheet) c.sheet else c.bg).width(400.dp).padding(horizontal = if (sheet) 0.dp else 16.dp)) { content() }
      }
    }
    rule.onRoot().captureRoboImage("src/test/screenshots/settings_${name}_${if (dark) "dark" else "light"}.png")
  }

  private fun both(name: String, content: @Composable () -> Unit): Pair<() -> Unit, () -> Unit> = Pair({ shot(name, true, content = content) }, { shot(name, false, content = content) })

  // ---------------------------------------------------------------- the home list
  @Composable private fun Home() = SettingsHomeContent(SettingsPages.visible(false)) {}
  @Test fun homeDark() = shot("home", true) { Home() }
  @Test fun homeLight() = shot("home", false) { Home() }

  // ---------------------------------------------------------------- model page
  private val providers = ProvidersResp(current = "anthropic", providers = SAMPLE_PROVIDERS)
  private val pageData = ModelPageData(
    StatusInfo("Auto", "claude-sonnet-5", "anthropic", true, "0.1.0"), SAMPLE_PROFILES, providers, hidden = listOf("groq"), hiddenOnServer = true,
    claude = ClaudeStatus(true, "own", 1_794_509_000, true),
  )
  @Composable private fun ModelPage() = ModelPageContent(pageData, false, setOf("groq"), ModelPageActions())
  @Test fun modelPageDark() = shot("model_page", true) { ModelPage() }
  @Test fun modelPageLight() = shot("model_page", false) { ModelPage() }

  // ---------------------------------------------------------------- model sheet
  private val sheetState = ModelSheetState(
    loading = false, providers = SAMPLE_PROVIDERS, hiddenHint = "1 provider is hidden. Settings > Model and profiles has a switch for each.",
    providerId = "openai", models = listOf("gpt-4o", "gpt-4o-mini", "o3", "gpt-5", "gpt-5-mini", "text-embedding-3-large"), chosen = "gpt-4o", pins = setOf("o3"),
    filter = "gpt", efforts = listOf("low", "medium", "high"), effort = "high",
    claudeLine = null,
  )
  @Composable private fun SheetModels() = Column(Modifier.background(ZoTheme.colors.sheet)) {
    SheetContent("Model and API key", {}, footer = { SecondaryButton("Close", {}); PrimaryButton("Use this model", {}) }) { ModelSheetBody(sheetState, false, ModelSheetActions()) }
  }
  @Test fun modelSheetDark() = shot("model_sheet", true, sheet = true) { SheetModels() }
  @Test fun modelSheetLight() = shot("model_sheet", false, sheet = true) { SheetModels() }

  private val customState = sheetState.copy(
    providerId = "custom", address = "http://192.168.1.20:8000/v1", models = emptyList(), chosen = "", filter = "", efforts = emptyList(), effort = "",
    fetchError = "Nothing is listed at that address (404). It usually ends in /v1.", testOk = false, testText = "The provider did not accept the key (401).",
  )
  @Composable private fun SheetCustom() = Column(Modifier.background(ZoTheme.colors.sheet)) {
    SheetContent("Model and API key", {}, footer = { SecondaryButton("Close", {}); PrimaryButton("Use this model", {}, enabled = false) }) { ModelSheetBody(customState, false, ModelSheetActions()) }
  }
  @Test fun modelSheetCustomDark() = shot("model_sheet_custom", true, sheet = true) { SheetCustom() }
  @Test fun modelSheetCustomLight() = shot("model_sheet_custom", false, sheet = true) { SheetCustom() }

  @Composable private fun Endpoint() = Column(Modifier.background(ZoTheme.colors.sheet)) {
    SheetContent("Add a custom endpoint", {}, footer = { SecondaryButton("Cancel", {}); PrimaryButton("Add and use", {}) }) {
      EndpointForm("Home GPU", {}, "http://192.168.1.20:8000/v1", {}, "", {}, "llama3", {}, "", "Connected. 2 models available.", true, false, {})
    }
  }
  @Test fun endpointDark() = shot("endpoint", true, sheet = true) { Endpoint() }
  @Test fun endpointLight() = shot("endpoint", false, sheet = true) { Endpoint() }

  // ---------------------------------------------------------------- model menu
  private val menuState = ModelMenuState(
    loading = false, groups = ModelLogic.menuGroups(SAMPLE_PROFILES) { if (it == "anthropic") "Anthropic (Claude)" else if (it == "openrouter") "OpenRouter" else it },
    activeProfile = "p1", currentModel = "claude-opus-5", currentProviderName = "Anthropic (Claude)", hasProvider = true, efforts = listOf("low", "high"), effort = "high",
  )
  @Composable private fun Menu() = Column(Modifier.background(ZoTheme.colors.sheet)) {
    SheetContent("Pinned models", {}) { ModelMenuBody(menuState, { _, _ -> }, {}, {}, {}) }
  }
  @Test fun modelMenuDark() = shot("model_menu", true, sheet = true) { Menu() }
  @Test fun modelMenuLight() = shot("model_menu", false, sheet = true) { Menu() }

  // ---------------------------------------------------------------- personality, memory
  private val pers = PersonalitiesResp(
    "friend",
    listOf(
      Personality("friend", "Friendly buddy", "Sound like a warm, easygoing friend. Keep it short and relaxed.", builtin = true),
      Personality("engineer", "Straight engineer", "Be precise and brief. Show the reasoning only when it matters.", builtin = true),
      Personality("p17", "Patient teacher", "Explain with small everyday examples. Never rush the person.", generated = true),
    ),
  )
  @Composable private fun Personality() = PersonalityContent(personalityRows(pers), {}, {}, {}, {})
  @Test fun personalityDark() = shot("personality", true) { Personality() }
  @Test fun personalityLight() = shot("personality", false) { Personality() }

  private val memory = MemoryResp(
    true, 3,
    listOf(
      MemoryItem("m1", "person", "My name is Zohaib and I live in Karachi.", pinned = true, updated = 9),
      MemoryItem("m2", "preference", "I prefer short answers.", updated = 8),
      MemoryItem("m3", "preference", "Reply in English, even when I write Hindi.", updated = 7),
      MemoryItem("m4", "project", "Building Zohara OS, an Arch-based distribution.", updated = 6),
      MemoryItem("m5", "episode", "We fixed the lock screen and tested the VM.", title = "Lock screen fix", chat = "c1", updated = 5),
    ),
  )
  @Composable private fun Memory() = MemoryContent(memory, true, {}, {}, {}, {}, {}, {}, {})
  @Test fun memoryDark() = shot("memory", true) { Memory() }
  @Test fun memoryLight() = shot("memory", false) { Memory() }

  // ---------------------------------------------------------------- general, apps, machines
  @Composable private fun General() = GeneralContent("system", {}, false, "Auto", {}, "hi-IN", {}, true, {}, false, {}, VoiceSupport())
  @Test fun generalDark() = shot("general", true) { General() }
  @Test fun generalLight() = shot("general", false) { General() }

  @Composable private fun Apps() = AppsContent(listOf(McpServer("exa", "https://mcp.exa.ai/mcp", true), McpServer("notes", "https://notes.example.com/mcp/with/a/very/long/path/that/must/wrap", false)), { _, _ -> }, {}, {}, {})
  @Test fun appsDark() = shot("apps", true) { Apps() }
  @Test fun appsLight() = shot("apps", false) { Apps() }

  @Composable private fun Machines() = MachinesContent(listOf(Machine("home-server", "192.168.1.20", "zohaib", 22, "the NAS", true), Machine("vm1", "203.0.113.7", "ubuntu", 2222, "", false)), "vm1", {}, {}, {})
  @Test fun machinesDark() = shot("machines", true) { Machines() }
  @Test fun machinesLight() = shot("machines", false) { Machines() }

  // ---------------------------------------------------------------- sync, account
  @Composable private fun Sync() = SyncContent(SyncInfo(12, 7, 3, System.currentTimeMillis() - 5 * 60_000, OutwardSync("https://sync.example.com", 0, "The last try failed: offline")), {})
  @Test fun syncDark() = shot("sync", true) { Sync() }
  @Test fun syncLight() = shot("sync", false) { Sync() }

  private val account = AccountData(
    Whoami(true, false, "zohaib@example.com", "t1", Limits(60, 2, 20000, 26_214_400, 8, 120), 2_224_000, TermsState("2026-11-12", true)),
    listOf(AccountDevice("d1", "ZOHARA laptop", "desktop", "sync", 1_794_509_000, 1_794_509_100), AccountDevice("d2", "", "cli", "full", 1_794_000_000, 0)),
    ClaudeStatus(true, "own", 1_794_509_000, true),
  )
  @Composable private fun Account() = AccountContent(account, false, {}, {}, {}, {}, {}, {}, {})
  @Test fun accountDark() = shot("account", true) { Account() }
  @Test fun accountLight() = shot("account", false) { Account() }

  // ---------------------------------------------------------------- notifications
  private val notif = NotifState(
    permission = true, denied = false, answers = true, needsYou = true, details = false, showTitle = false, sound = true, closed = true, admin = true,
    devices = listOf(PushSubscription("i1", "fcm", "Android: Pixel 8", 1_794_509_000), PushSubscription("i2", "webpush", "", 1_794_000_000)), thisDevice = "i1",
  )
  @Composable private fun Notifications() = NotificationsContent(notif, NotifActions())
  @Test fun notificationsDark() = shot("notifications", true) { Notifications() }
  @Test fun notificationsLight() = shot("notifications", false) { Notifications() }

  @Composable private fun NotificationsAsk() = NotificationsContent(notif.copy(permission = false, denied = true), NotifActions())
  @Test fun notificationsBlockedDark() = shot("notifications_blocked", true) { NotificationsAsk() }
  @Test fun notificationsBlockedLight() = shot("notifications_blocked", false) { NotificationsAsk() }

  // ---------------------------------------------------------------- approve a sign-in
  @Composable private fun Approve() = ConfirmView(DeviceLookup("My laptop", "cli", 1_794_509_000, "203.0.113.4", false, "zohaib@example.com"), "zohaib@example.com", false, {}, false, "", {}, {})
  @Test fun approveDark() = shot("approve", true) { Approve() }
  @Test fun approveLight() = shot("approve", false) { Approve() }
}
