package org.zohara.zoai.app

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
import org.zohara.zoai.chat.ChatHooks
import org.zohara.zoai.media.ChatPlug
import org.zohara.zoai.nav.ZoApp

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
    rule.waitUntil(8000) { has("Message to ZO-AI") && hasText("Not now") }
    rule.onNodeWithText("Not now").performClick()
    rule.waitUntil(8000) { !hasText("Not now") }
  }

  @Test @Config(qualifiers = "w1000dp-h700dp-night-xhdpi")
  fun at1000dpTheSidebarIsAlwaysOpenBesideTheChat() { // NAV-07
    launch()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    assertFalse("no button to open it: it is open", has("Open the sidebar"))
    assertTrue(hasText("Search") && hasText("Activity") && has("Settings"))
    assertTrue(has("Message to ZO-AI"))
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
    assertTrue(has("Message to ZO-AI") && has("Send"))
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/app_home_rtl_dark.png")
    rule.onNodeWithContentDescription("Open the sidebar").performClick()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    val panel = rule.onAllNodesWithText("Search")[0].fetchSemanticsNode().boundsInRoot
    assertTrue("the sidebar slides in from the right (x=${panel.left})", panel.left > width / 4)
    rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/app_drawer_rtl_dark.png")
  }
}
