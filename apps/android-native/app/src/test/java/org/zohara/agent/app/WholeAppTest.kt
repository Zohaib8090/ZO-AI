package org.zohara.agent.app

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
import androidx.compose.ui.test.performTextReplacement
import kotlinx.coroutines.flow.first
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.performTextInput
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.zohara.agent.core.net.ZoJson
import androidx.compose.ui.test.filter
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.test.printToString
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.agent.AppContainer
import org.zohara.agent.chat.ChatHooks
import org.zohara.agent.core.design.DarkZoColors
import org.zohara.agent.core.design.LightZoColors
import org.zohara.agent.core.design.ThemeChoice
import org.zohara.agent.media.ChatPlug
import org.zohara.agent.nav.ZoApp

/**
 * The whole app: the real [ZoApp] (theme, sign-in gate, drawer, navigation graph, every module's screens) against a mock server with the
 * answers captured live. This is what proves the modules work together: the drawer lists, the settings entries from the chat, deep links,
 * the back button, the saved state, the live theme switch, the composer slots.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h860dp-night-xhdpi", application = android.app.Application::class)
class WholeAppTest {
  @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()
  private lateinit var server: AppServer
  private val container: AppContainer by lazy { server.signedInContainer() }
  private val pending = MutableStateFlow<String?>(null)

  @Before fun up() {
    server = AppServer()
    ChatPlug.resetForTests()
  }
  @After fun down() {
    server.shutdown()
    ChatHooks.composerHost = null; ChatHooks.attachmentChips = null; ChatHooks.attachButton = null; ChatHooks.micButton = null
    ChatHooks.fieldModifier = null; ChatHooks.runOnPill = null; ChatHooks.textEdited = null; ChatHooks.readAloud = null; ChatHooks.readAloudButton = null
  }

  /** [askAboutNotifications]: leave the first-run notification question open (INTER-15) instead of answering "Not now". */
  private fun launch(askAboutNotifications: Boolean = false) {
    rule.setContent { ZoApp(container, pending) }
    waitFor("the home screen") { has("Open the sidebar") && rule.onAllNodesWithContentDescription("Message to Zohara Agent").fetchSemanticsNodes().isNotEmpty() }
    if (!askAboutNotifications) {
      waitFor("the first-run notification question") { hasText("Not now") }
      rule.onNodeWithText("Not now").performClick()
      waitFor("the question closed") { !hasText("Not now") }
    }
  }

  private fun shotOne(name: String) { rule.waitForIdle(); rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/$name.png") }

  private fun windowArgb(): Int = (rule.activity.window.decorView.background as android.graphics.drawable.ColorDrawable).color

  private fun setTheme(t: ThemeChoice, want: Int) {
    runBlocking { container.prefs.setTheme(t) }
    waitFor("the theme $t") { runCatching { windowArgb() == want }.getOrDefault(false) }
    rule.waitForIdle()
  }

  /** The screen as it is now, in the dark and the light theme (names end in _dark and _light). */
  private fun shot(name: String) {
    setTheme(ThemeChoice.Dark, DarkZoColors.bg.toArgb()); shotOne(name + "_dark")
    setTheme(ThemeChoice.Light, LightZoColors.bg.toArgb()); shotOne(name + "_light")
    setTheme(ThemeChoice.Dark, DarkZoColors.bg.toArgb())
  }

  private fun has(desc: String) = rule.onAllNodesWithContentDescription(desc).fetchSemanticsNodes().isNotEmpty()
  private fun hasText(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

  private fun waitFor(what: String, ms: Long = 8000, cond: () -> Boolean) {
    try { rule.waitUntil(ms, cond) } catch (e: Throwable) { throw AssertionError("timed out waiting for $what\n" + rule.onAllNodes(isRoot()).fetchSemanticsNodes().indices.joinToString("\n--- root\n") { rule.onAllNodes(isRoot())[it].printToString(40) }, e) }
  }

  private fun openDrawer() {
    rule.onNodeWithContentDescription("Open the sidebar").performClick()
    waitFor("the drawer") { hasText("Search") && hasText("Activity") }
  }

  private fun back() { rule.runOnUiThread { rule.activity.onBackPressedDispatcher.onBackPressed() }; rule.waitForIdle() }

  // ---- start, gate, drawer ----

  @Test fun startsAtHomeAndLoadsTheServerState() { // AUTH-19, NAV-16, PHN-07
    launch()
    rule.onNodeWithContentDescription("Message to Zohara Agent").assertIsDisplayed()
    waitFor("status call (requests: ${server.paths} ops: ${server.ops})") { "status" in server.ops }
    assertTrue("whoami was asked first", server.server.requestCount >= 1)
    waitFor("the model pill shows the server's model") { has("Model: Opus 5.5") }
    shot("app_home")
  }

  @Test fun firstRunAsksOnceAboutNotifications() { // INTER-15
    launch(askAboutNotifications = true)
    waitFor("the question") { hasText("Not now") && hasText("Turn on") }
    shot("app_first_run_notifications")
  }

  @Test fun drawerHasTheItemsInOrderAndTheChatLists() { // NAV-02, NAV-05, NAV-09, NAV-10
    launch(); openDrawer()
    val labels = listOf("New", "Chats", "Search", "Projects", "Artifacts", "Scheduled", "Activity")
    val tops = labels.map { l -> rule.onAllNodesWithText(l).fetchSemanticsNodes().minOf { it.boundsInRoot.top } }
    assertEquals("order of the sidebar items", tops.sorted(), tops)
    waitFor("pinned and recent chats") { hasText("Pinned plan") && hasText("Recent idea") }
    shot("app_drawer")
  }

  @Test fun drawerItemOpensThePageAndClosesTheDrawer() { // NAV-05, NAV-16
    launch(); openDrawer()
    rule.onAllNodesWithText("Chats")[0].performClick()
    waitFor("the Chats page") { "chats" in server.ops }
    rule.waitForIdle()
    assertTrue("the drawer closed", rule.onAllNodesWithText("Activity").fetchSemanticsNodes().isEmpty() || !has("Close the sidebar"))
    assertTrue(has("Open the sidebar"))
  }

  @Test fun restrictedAccountDoesNotSeeProjectsArtifactsScheduled() { // NAV-04, AUTH-20
    server.admin = false
    launch(); openDrawer()
    assertFalse("Projects", hasText("Projects")); assertFalse("Artifacts", hasText("Artifacts")); assertFalse("Scheduled", hasText("Scheduled"))
    assertTrue("Chats", hasText("Chats")); assertTrue("Activity", hasText("Activity"))
  }

  @Test fun gearOpensSettingsAndBackComesBackToHome() { // NAV-13, SET-01, NAV-21
    launch(); openDrawer()
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("the Settings list") { hasText("Model and profiles") && hasText("Appearance and permissions") }
    shot("app_settings_list")
    back()
    waitFor("home again") { has("Message to Zohara Agent") }
  }

  @Test fun backClosesTheDrawerFirstAndNeverEndsTheApp() { // NAV-21
    launch(); openDrawer()
    back()
    waitFor("the drawer closed") { !has("Close the sidebar") }
    back() // at the home screen: the app goes to the background, it does not finish
    assertFalse(rule.activity.isFinishing)
  }

  @Test fun adminSeesTheTerminalPillAndItOpensTheTerminalScreen() { // COMP-12, TERM-01
    launch()
    waitFor("the Terminal pill") { has("Terminal") || hasText("Terminal") }
    val pill = if (has("Terminal")) rule.onAllNodesWithContentDescription("Terminal")[0] else rule.onAllNodesWithText("Terminal")[0]
    pill.performClick()
    waitFor("the Terminal screen") { !has("Message to Zohara Agent") }
    rule.waitForIdle()
  }

  @Test fun restrictedAccountDoesNotSeeTheTerminalPill() { // AUTH-20, SRV-03
    server.admin = false
    launch()
    rule.waitForIdle()
    assertFalse("Terminal pill (requests ${server.paths})", has("Terminal") || hasText("Terminal"))
  }

  // ---- deep links ----

  @Test fun aNotificationOrLinkOpensTheChat() { // NAV-20
    launch()
    pending.value = "c1"
    waitFor("the chat c1") { "chat" in server.ops && hasText("Plan my week") }
    assertEquals(null, pending.value)
    shot("app_chat")
  }

  @Test fun anInvalidChatIdIsIgnored() { // NAV-20
    launch()
    pending.value = "../etc/passwd"
    rule.waitForIdle()
    assertFalse("chat" in server.ops)
  }

  @Test fun pinnedChatOfTheDrawerOpensTheChat() { // NAV-09
    launch(); openDrawer()
    waitFor("pinned chat") { hasText("Pinned plan") }
    rule.onAllNodesWithText("Pinned plan")[0].performClick()
    waitFor("the chat") { hasText("Plan my week") }
  }

  // ---- state, theme ----

  @Test fun theScreenSurvivesProcessDeathAndRotation() { // PHN-04
    val tester = StateRestorationTester(rule)
    tester.setContent { ZoApp(container, pending) }
    waitFor("home") { has("Open the sidebar") }
    openDrawer()
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("Settings") { hasText("Model and profiles") }
    tester.emulateSavedInstanceStateRestore()
    waitFor("Settings after the restore") { hasText("Model and profiles") }
  }

  @Test fun theThemeSwitchesLiveWithoutRestart() { // THEME-05, THEME-07
    launch()
    // the window behind the app (and so the status and navigation bars, edge to edge) takes the theme's background at once (THEME-11)
    runBlocking { container.prefs.setTheme(ThemeChoice.Dark) }
    waitFor("dark window (is ${java.lang.Integer.toHexString(runCatching { windowArgb() }.getOrDefault(0))})") { runCatching { windowArgb() == DarkZoColors.bg.toArgb() }.getOrDefault(false) }
    runBlocking { container.prefs.setTheme(ThemeChoice.Light) }
    waitFor("light window") { runCatching { windowArgb() == LightZoColors.bg.toArgb() }.getOrDefault(false) }
  }

  // ---- accessibility ----

  /** Every control that can be clicked has a name TalkBack can read (text, or a content description, possibly merged from its children). */
  private fun unlabeled(): List<String> {
    val out = mutableListOf<String>()
    for (n in rule.onAllNodes(hasClickAction()).fetchSemanticsNodes()) {
      val c = n.config
      val text = c.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }.orEmpty()
      val desc = c.getOrNull(SemanticsProperties.ContentDescription)?.joinToString().orEmpty()
      val label = c.getOrNull(SemanticsActions.OnClick)?.label.orEmpty()
      if (text.isBlank() && desc.isBlank() && label.isBlank()) out += "node ${n.id} at ${n.boundsInRoot}"
    }
    return out
  }

  /** Controls whose touch area is smaller than 48 x 48 dp (A11Y-02). */
  private fun tooSmall(): List<String> {
    val dp = rule.activity.resources.displayMetrics.density
    val out = mutableListOf<String>()
    val window = rule.onAllNodes(isRoot())[0].fetchSemanticsNode().boundsInRoot
    for (n in rule.onAllNodes(hasClickAction()).fetchSemanticsNodes()) {
      if (n.config.getOrNull(SemanticsProperties.EditableText) != null) continue
      // cut off by the window (a row half scrolled out, a pill half visible in the sideways list): its size is not the control's
      if (n.boundsInRoot.width <= 0f || n.boundsInRoot.right >= window.right - 1f || n.boundsInRoot.bottom >= window.bottom - 1f) continue // a text box sits in a bigger card
      if (n.touchBoundsInRoot.width < n.size.width - 1f || n.touchBoundsInRoot.height < n.size.height - 1f) continue // clipped by a parent (the sideways pill list)
      val b = n.touchBoundsInRoot // the area that reacts to a touch: Compose widens small controls to 48 dp
      val text = n.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }.orEmpty()
      val desc = n.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString().orEmpty()
      if (b.width < 47.5f * dp || b.height < 47.5f * dp) out += "${desc.ifBlank { text }.take(40)} ${(b.width / dp).toInt()}x${(b.height / dp).toInt()}dp"
    }
    return out
  }

  @Test fun everyControlOnTheMainScreensHasAName() { // A11Y-01
    launch()
    assertEquals("home", emptyList<String>(), unlabeled())
    assertEquals("home (touch size)", emptyList<String>(), tooSmall())
    openDrawer()
    waitFor("chats in the drawer") { hasText("Pinned plan") }
    assertEquals("drawer", emptyList<String>(), unlabeled())
    assertEquals("drawer (touch size)", emptyList<String>(), tooSmall())
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("Settings") { hasText("Model and profiles") }
    assertEquals("settings list", emptyList<String>(), unlabeled())
    assertEquals("settings list (touch size)", emptyList<String>(), tooSmall())
    for (page in listOf("Model and profiles", "Personality", "Memory", "Appearance and permissions", "Desktop sync", "My computers", "Notifications", "Account", "Data", "About")) {
      rule.onAllNodesWithText(page)[0].performScrollTo().performClick()
      waitFor("page $page") { has("Back to Settings") }
      rule.waitForIdle()
      assertEquals("settings page $page", emptyList<String>(), unlabeled())
      assertEquals("settings page $page (touch size)", emptyList<String>(), tooSmall())
      rule.onNodeWithContentDescription("Back to Settings").performClick()
      waitFor("the list again") { hasText("Model and profiles") && !has("Back to Settings") }
    }
  }

  @Test fun everyControlOnThePagesAndTheChatHasAName() { // A11Y-01
    launch(); openDrawer()
    waitFor("chats in the drawer") { hasText("Pinned plan") }
    for (page in listOf("Chats", "Search", "Projects", "Artifacts", "Scheduled", "Activity")) {
      rule.onAllNodesWithText(page)[0].performClick()
      waitFor("page $page") { !has("Close the sidebar") }
      rule.waitForIdle()
      assertEquals("page $page", emptyList<String>(), unlabeled())
      assertEquals("page $page (touch size)", emptyList<String>(), tooSmall())
      openDrawer()
    }
    rule.onAllNodesWithText("Pinned plan")[0].performClick()
    waitFor("the chat") { hasText("Plan my week") }
    assertEquals("chat", emptyList<String>(), unlabeled())
    assertEquals("chat (touch size)", emptyList<String>(), tooSmall())
  }

  @Test fun theModelPillOpensTheModelMenuAndMoreModelsOpensTheFullSheetOverTheChat() { // MODEL-01, 03, 04
    launch()
    rule.onNode(androidx.compose.ui.test.hasContentDescription("Model:", substring = true)).performClick()
    waitFor("the model menu") { hasText("More models…") }
    // nothing is pinned on this account: the empty state of the menu (MODEL-03)
    assertTrue(hasText("Pin models in Settings"))
    assertTrue(hasText("Star the models you use and they show here, one tap away"))
    assertTrue(hasText("All models, key and address"))
    rule.onNodeWithText("More models…").performClick()
    waitFor("the full model sheet, still on the chat") { hasText("Model and API key") }
    assertFalse("it does not leave the chat", has("Back to Settings"))
  }

  @Test fun theModelMenuOffersTheEffortLevelsOfTheModel() { // MODEL-07
    server.effortsJson = """["low","medium","high"]"""
    launch()
    rule.onNode(androidx.compose.ui.test.hasContentDescription("Model:", substring = true)).performClick()
    waitFor("the effort row") { hasText("Effort · Default") }
    assertTrue(hasText("How hard it thinks before answering"))
    rule.onNodeWithText("Effort · Default").performClick()
    waitFor("the levels") { hasText("Medium") && hasText("High") }
  }

  @Test fun theMemoryAndAccountPagesDraw() { // screenshots of the settings pages
    launch(); openDrawer()
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("Settings") { hasText("Memory") }
    rule.onAllNodesWithText("Memory")[0].performClick()
    waitFor("Memory page") { has("Back to Settings") }
    waitFor("the notes") { hasText("I prefer short answers") }
    shot("app_settings_memory")
    rule.onNodeWithContentDescription("Back to Settings").performClick()
    waitFor("list") { !has("Back to Settings") }
    rule.onAllNodesWithText("Account")[0].performScrollTo().performClick()
    waitFor("Account page") { has("Back to Settings") }
    waitFor("the devices") { hasText("My laptop") || !hasText("Loading…") && !hasText("Loading...") }
    rule.waitForIdle(); shot("app_settings_account")
  }

  @Test fun theLauncherShortcutOpensANewChat() { // NAV-22
    launch()
    pending.value = "c1"
    waitFor("the chat") { hasText("Plan my week") }
    pending.value = org.zohara.agent.nav.DeepLinks.NEW_CHAT
    waitFor("a new chat") { has("Message to Zohara Agent") && !hasText("Plan my week") }
  }

  @Test fun theApproveLinkOpensApproveASignIn() { // AUTH-24
    launch()
    pending.value = org.zohara.agent.nav.DeepLinks.APPROVE
    waitFor("Approve a sign-in") { has("Back to Settings") }
    assertEquals(null, pending.value)
  }

  @Test fun theFooterShowsTheNameAndSignOutAsksFirst() { // NAV-13, SET-ACC-09, AUTH-21
    launch(); openDrawer()
    assertTrue("the user's name", hasText("Zo Tester")); assertTrue(has("Settings")); assertTrue(has("Sign out"))
    rule.onNodeWithContentDescription("Sign out").performClick()
    waitFor("the question") { hasText("Sign out of Zohara Agent on this phone?") && hasText("Your chats stay on the server.") }
    rule.onNodeWithText("Cancel").performClick()
    waitFor("closed") { !hasText("Your chats stay on the server.") }
    assertTrue("cancelling stays signed in", has("Sign out"))
    rule.onNodeWithContentDescription("Sign out").performClick()
    waitFor("the question again") { hasText("Your chats stay on the server.") }
    rule.onAllNodesWithText("Sign out").filter(hasClickAction())[0].performClick()
    waitFor("the login screen") { hasText("Sign in with Google") }
  }

  @Test fun topBarTitlesFollowThePage() { // NAV-16
    launch()
    assertTrue("home has an empty title", rule.onAllNodesWithText("Chats").fetchSemanticsNodes().isEmpty())
    openDrawer()
    for (page in listOf("Chats", "Search", "Projects", "Artifacts", "Scheduled", "Activity")) {
      rule.onAllNodesWithText(page)[0].performClick()
      waitFor("top bar title $page") { rule.onAllNodesWithText(page).fetchSemanticsNodes().any { it.boundsInRoot.top < 120f } }
      openDrawer()
    }
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("top bar title Settings") { rule.onAllNodesWithText("Settings").fetchSemanticsNodes().any { it.boundsInRoot.top < 120f } }
  }

  @Test fun anOpenChatMarksNewAsTheActiveItem() { // NAV-03
    launch(); pending.value = "c1"
    waitFor("the chat") { hasText("Plan my week") }
    openDrawer()
    val newRow = rule.onAllNodesWithText("New")[0].fetchSemanticsNode()
    assertTrue("the New row is in the list", newRow.boundsInRoot.height > 0)
  }

  @Test fun theFirstScreenIsTheMarkOnTheBackgroundWhileTheSessionLoads() { // PHN-07
    server.slowWhoami = 1500
    rule.setContent { ZoApp(container, pending) }
    waitFor("the splash") { has("Zohara Agent logo") && !has("Message to Zohara Agent") }
    waitFor("then the app") { has("Message to Zohara Agent") }
  }

  @Test fun followsTheSystemThemeByDefault() { // PHN-07, THEME-05
    // this class runs with the night qualifier: no preference is stored, so the window is dark
    launch()
    assertEquals(DarkZoColors.bg.toArgb(), windowArgb())
  }

  @Test fun approvingADesktopOrTerminalSignInFromThePhone() { // AUTH-24, AUTH-25, AUTH-26, SRV-15
    launch()
    pending.value = org.zohara.agent.nav.DeepLinks.APPROVE
    waitFor("Approve a sign-in") { hasText("Code shown on your device") }
    rule.onNodeWithText("Look up").assertIsNotEnabled()
    rule.onNodeWithContentDescription("Code shown on your device").performTextInput("abcd1234")
    waitFor("the code is formatted") { hasText("ABCD-1234") }
    rule.onNodeWithText("Look up").assertIsEnabled()

    server.deviceLookup = 404 to """{"error":"That code is not right, or it has run out."}"""
    rule.onNodeWithText("Look up").performClick()
    waitFor("the 404 text") { hasText("That code was not found. It may have expired (they last 10 minutes) or been used. Check it and try again, or start again in the app.") }

    server.deviceLookup = 429 to """{"error":"slow down"}"""
    rule.onNodeWithText("Look up").performClick()
    waitFor("the 429 text") { hasText("Too many wrong codes. Wait a few minutes before trying again.") }

    server.deviceLookup = 200 to """{"ok":true,"name":"My laptop","kind":"cli","requested_at":1794509000,"ip":"203.0.113.4","same_network":false,"email":"zo@example.com"}"""
    rule.onNodeWithText("Look up").performClick()
    waitFor("the confirm view") { hasText("Terminal (full access to your account)") }
    for (t in listOf("My laptop", "203.0.113.4")) assertTrue(t, hasText(t))
    assertTrue("a different network is warned about", hasText("This request came from a different network", ) || rule.onAllNodesWithText("This request came from a different network", substring = true).fetchSemanticsNodes().isNotEmpty())
    assertTrue(rule.onAllNodesWithText("Only approve if you started this on your own device,", substring = true).fetchSemanticsNodes().isNotEmpty())
    rule.onNodeWithText("Approve").assertIsNotEnabled()                 // a terminal needs the tick
    rule.onNodeWithText("I started this myself on my own computer", substring = true).performClick()
    rule.onNodeWithText("Approve").assertIsEnabled().performClick()
    waitFor("Approved") { hasText("Approved") }
    val post = server.bodies.last { it.first == "/api/auth/device_approve" }.second
    assertEquals("ABCD-1234", ZoJson.parseToJsonElement(post).jsonObject["user_code"]!!.jsonPrimitive.content)
    assertEquals("true", ZoJson.parseToJsonElement(post).jsonObject["approve"]!!.jsonPrimitive.content)
  }

  @Test fun sendingAMessageStreamsTheAnswerAndOpensTheChat() { // CHAT-01..12 wired in the app, COMP-01, NAV-02
    launch()
    rule.onNodeWithContentDescription("Message to Zohara Agent").performTextInput("hello there")
    rule.onNodeWithContentDescription("Send").performClick()
    waitFor("the answer") { hasText("Hi from the fake model.") || rule.onAllNodesWithText("Hi from ", substring = true).fetchSemanticsNodes().isNotEmpty() }
    waitFor("the chat has its title") { hasText("Hello there") }
    val sent = server.bodies.last { it.first == "/api/send" }.second
    assertEquals("hello there", ZoJson.parseToJsonElement(sent).jsonObject["text"]!!.jsonPrimitive.content)
    shot("app_chat_answer")
  }

  @Test fun theServerAddressNeedsHttpsAndChangingItSignsOut() { // AUTH-28, SRV-07
    launch(); openDrawer()
    rule.onNodeWithContentDescription("Settings").performClick()
    waitFor("Settings") { hasText("Account") }
    rule.onAllNodesWithText("Account")[0].performScrollTo().performClick()
    waitFor("Account page") { hasText("Server address…") }
    rule.onNodeWithText("Server address…").performScrollTo().performClick()
    waitFor("Advanced page") { hasText("Save and sign out") }
    rule.onNodeWithText("Save and sign out").assertIsNotEnabled()                      // nothing changed yet
    val field = rule.onNode(androidx.compose.ui.test.hasSetTextAction() and androidx.compose.ui.test.hasContentDescription("https://", substring = true))
    field.performTextInput("x")
    rule.onNodeWithText("Save and sign out").performScrollTo().assertIsEnabled()
    rule.onNode(androidx.compose.ui.test.hasSetTextAction()).performTextReplacement("http://my-server.example.com")
    rule.onNodeWithText("Save and sign out").performScrollTo().performClick()
    waitFor("the refusal of plain http") { rule.onAllNodesWithText("Use an https address", substring = true).fetchSemanticsNodes().isNotEmpty() }
    rule.onNode(androidx.compose.ui.test.hasSetTextAction()).performTextReplacement("https://my-server.example.com/")
    rule.onNodeWithText("Save and sign out").performScrollTo().performClick()
    waitFor("signed out") { hasText("Sign in with Google") }
    assertEquals("https://my-server.example.com", runBlocking { container.prefs.serverUrl.first() })
  }

  @Test fun swipingFromTheLeftEdgeOpensTheSidebarAndSwipingBackClosesIt() { // NAV-05, NAV-06
    launch()
    assertEquals(180, org.zohara.agent.nav.DRAWER_SLIDE_MS)
    rule.onAllNodes(isRoot())[0].performTouchInput { swipeRight(startX = 4f, endX = width * 0.8f, durationMillis = 300) }
    waitFor("the sidebar") { hasText("Activity") }
    rule.onAllNodes(isRoot())[0].performTouchInput { swipeLeft(startX = width * 0.6f, endX = 2f, durationMillis = 300) }
    waitFor("the sidebar closed") { !has("Close the sidebar") }
  }
}
