package org.zohara.agent.app

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.flow.MutableStateFlow
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
import org.zohara.agent.chat.ChatHooks
import org.zohara.agent.media.ChatPlug
import org.zohara.agent.nav.ZoApp

/** Wide windows (the sidebar stays open from 900 dp, NAV-07) and right-to-left languages (PHN-12). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], application = android.app.Application::class)
class WideAndRtlTest {
  @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()
  private lateinit var server: AppServer
  private val container by lazy { server.signedInContainer() }
  private val pending = MutableStateFlow<String?>(null)

  @Before fun up() { server = AppServer(); ChatPlug.resetForTests() }
  @After fun down() {
    server.shutdown()
    ChatHooks.composerHost = null; ChatHooks.attachmentChips = null; ChatHooks.attachButton = null; ChatHooks.micButton = null
    ChatHooks.fieldModifier = null; ChatHooks.runOnPill = null; ChatHooks.textEdited = null; ChatHooks.readAloud = null; ChatHooks.readAloudButton = null
  }

  private fun has(desc: String) = rule.onAllNodesWithContentDescription(desc).fetchSemanticsNodes().isNotEmpty()
  private fun hasText(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

  private fun launch() {
    rule.setContent { ZoApp(container, pending) }
    rule.waitUntil(8000) { has("Message to Zohara Agent") && hasText("Not now") }
    rule.onNodeWithText("Not now").performClick()
    rule.waitUntil(8000) { !hasText("Not now") }
  }

  @Test @Config(qualifiers = "w1000dp-h700dp-night-xhdpi")
  fun at1000dpTheSidebarIsAlwaysOpenBesideTheChat() { // NAV-07
    launch()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    assertFalse("no button to open it: it is open", has("Open the sidebar"))
    assertTrue(hasText("Search") && hasText("Activity") && has("Settings"))
    assertTrue(has("Message to Zohara Agent"))
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/app_tablet_1000dp_dark.png")
  }

  @Test @Config(qualifiers = "w880dp-h700dp-night-xhdpi")
  fun at880dpTheSidebarIsADrawer() { // NAV-07
    launch()
    assertTrue(has("Open the sidebar"))
    assertFalse(hasText("Activity"))
  }

  @Test @Config(qualifiers = "ar-ldrtl-w411dp-h860dp-night-xhdpi")
  fun inRightToLeftTheSidebarButtonIsOnTheRightAndEverythingStillDraws() { // PHN-12
    launch()
    assertEquals("the system says right to left", android.view.View.LAYOUT_DIRECTION_RTL, rule.activity.resources.configuration.layoutDirection)
    val width = rule.onAllNodes(isRoot())[0].fetchSemanticsNode().boundsInRoot.width
    val button = rule.onNodeWithContentDescription("Open the sidebar").fetchSemanticsNode().boundsInRoot
    assertTrue("the button is at the start = the right side (x=${button.left} of $width)", button.left > width / 2)
    assertTrue(has("Message to Zohara Agent") && has("Send"))
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/app_home_rtl_dark.png")
    rule.onNodeWithContentDescription("Open the sidebar").performClick()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    val panel = rule.onAllNodesWithText("Search")[0].fetchSemanticsNode().boundsInRoot
    assertTrue("the sidebar slides in from the right (x=${panel.left})", panel.left > width / 4)
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/app_drawer_rtl_dark.png")
  }

  private fun count(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().size

  // NAV-23: the screenshots are taken at about 900 dp, in both themes
  private fun settingsTwoPane(file: String) {
    launch()
    rule.onNodeWithContentDescription("Settings").performClick()
    rule.waitUntil(8000) { count("Model and profiles") >= 2 }
    assertFalse("beside the list there is nothing to go back to", has("Back to Settings"))
    assertTrue("the list stays", hasText("Personality") && hasText("Notifications"))
    rule.waitUntil(8000) { !hasText("Loading…") }
    rule.waitForIdle()
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/$file.png")
    // a page chosen in the list replaces the one on the right, the list stays
    rule.onAllNodesWithText("Personality")[0].performClick()
    rule.waitUntil(8000) { count("Personality") >= 2 && hasText("How Zohara Agent talks to you.") }
    assertTrue(hasText("Notifications"))
    assertFalse(has("Back to Settings"))
  }

  @Test @Config(qualifiers = "w900dp-h700dp-night-xhdpi")
  fun settingsShowsTheListAndThePageSideBySideDark() = settingsTwoPane("app_twopane_settings_dark") // NAV-23

  @Test @Config(qualifiers = "w900dp-h700dp-notnight-xhdpi")
  fun settingsShowsTheListAndThePageSideBySideLight() = settingsTwoPane("app_twopane_settings_light") // NAV-23

  @Test @Config(qualifiers = "w411dp-h860dp-night-xhdpi")
  fun onAPhoneSettingsStaysASingleList() { // NAV-23: below 840 dp nothing changes
    launch()
    rule.onNodeWithContentDescription("Open the sidebar").performClick()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    rule.onNodeWithContentDescription("Settings").performClick()
    rule.waitUntil(8000) { hasText("Personality") }
    assertEquals("the list only, no page beside it", 1, count("Model and profiles"))
  }

  private fun chatsTwoPane(file: String) {
    launch()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    rule.onAllNodesWithText("Chats")[0].performClick()
    rule.waitUntil(8000) { count("Recent idea") >= 2 }
    assertTrue("nothing is open yet", hasText("Pick a chat to read it here"))
    rule.onAllNodesWithText("Recent idea").let { it[it.fetchSemanticsNodes().size - 1] }.performClick()
    rule.waitUntil(8000) { hasText("Plan my week") }
    assertTrue("the list stays beside the chat", count("Recent idea") >= 2)
    assertFalse(hasText("Pick a chat to read it here"))
    rule.waitForIdle()
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/$file.png")
  }

  @Test @Config(qualifiers = "w900dp-h700dp-night-xhdpi")
  fun chatsShowsTheListAndTheOpenedChatSideBySideDark() = chatsTwoPane("app_twopane_chats_dark") // NAV-23

  @Test @Config(qualifiers = "w900dp-h700dp-notnight-xhdpi")
  fun chatsShowsTheListAndTheOpenedChatSideBySideLight() = chatsTwoPane("app_twopane_chats_light") // NAV-23
}
