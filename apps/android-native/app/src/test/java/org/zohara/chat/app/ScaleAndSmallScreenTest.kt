package org.zohara.chat.app

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.printToString
import androidx.compose.ui.test.performScrollTo
import com.github.takahirom.roborazzi.captureRoboImage
import kotlinx.coroutines.flow.MutableStateFlow
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.chat.chat.ChatHooks
import org.zohara.chat.media.ChatPlug
import org.zohara.chat.nav.ZoApp

/**
 * A 320 dp wide phone with the largest system font (PHN-12, A11Y-02): the main screens must not push anything out of the window sideways,
 * and the controls must still be reachable.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w320dp-h568dp-night-xhdpi", application = android.app.Application::class)
class ScaleAndSmallScreenTest {
  @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()
  private lateinit var server: AppServer
  private val container by lazy { server.signedInContainer() }
  private val pending = MutableStateFlow<String?>(null)

  @Before fun up() { server = AppServer(); ChatPlug.resetForTests() }
  @After fun down() {
    server.shutdown(); RuntimeEnvironment.setFontScale(1f)
    ChatHooks.composerHost = null; ChatHooks.attachmentChips = null; ChatHooks.attachButton = null; ChatHooks.micButton = null
    ChatHooks.fieldModifier = null; ChatHooks.runOnPill = null; ChatHooks.textEdited = null; ChatHooks.readAloud = null; ChatHooks.readAloudButton = null
  }

  private fun has(desc: String) = rule.onAllNodesWithContentDescription(desc).fetchSemanticsNodes().isNotEmpty()
  private fun hasText(text: String) = rule.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()
  private fun shot(name: String) { rule.onAllNodes(isRoot())[0].captureRoboImage("src/test/screenshots/$name.png") }

  private fun launch(answerTheQuestion: Boolean = true) {
    rule.setContent { ZoApp(container, pending) }
    try { rule.waitUntil(8000) { has("Open the sidebar") && has("Message to Zohara Chat") && hasText("Not now") } } catch (e: Throwable) {
      throw AssertionError("sidebar=${has("Open the sidebar")} field=${has("Message to Zohara Chat")} notNow=${hasText("Not now")}\n" + rule.onAllNodes(isRoot()).fetchSemanticsNodes().indices.joinToString("\n--- root\n") { rule.onAllNodes(isRoot())[it].printToString(40) }, e)
    }
    if (!answerTheQuestion) return
    rule.onNodeWithText("Not now").performClick()
    rule.waitUntil(8000) { !hasText("Not now") }
  }

  private fun inHorizontalScroller(n: SemanticsNode): Boolean {
    var p: SemanticsNode? = n.parent
    while (p != null) { if (p.config.getOrNull(SemanticsProperties.HorizontalScrollAxisRange) != null) return true; p = p.parent }
    return false
  }

  /** Text or controls that stick out of the window on the left or right (outside a sideways scroller, where that is the point). */
  private fun outside(): List<String> {
    val root = rule.onAllNodes(isRoot())[0].fetchSemanticsNode().boundsInRoot
    val out = mutableListOf<String>()
    for (n in rule.onAllNodes(androidx.compose.ui.test.hasAnyAncestor(isRoot()) or isRoot()).fetchSemanticsNodes()) {
      val b = n.boundsInRoot
      if (b.width <= 0f || b.height <= 0f || inHorizontalScroller(n)) continue
      if (b.left < root.left - 1 || b.right > root.right + 1) {
        val label = n.config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text } ?: n.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString() ?: "node ${n.id}"
        out += "$label $b"
      }
    }
    return out
  }

  private fun scaled(scale: Float) { RuntimeEnvironment.setFontScale(scale) }

  @Test fun firstRunQuestionAt320dpWithFont20() { // PHN-12: the buttons of a sheet stay reachable
    scaled(2.0f); launch(answerTheQuestion = false)
    rule.onAllNodes(isRoot())[1].captureRoboImage("src/test/screenshots/app_first_run_320dp_font20_dark.png")
    rule.onNodeWithText("Not now").assertIsDisplayed()
    rule.onNodeWithText("Turn on").assertIsDisplayed()
  }

  @Test fun homeAt320dpWithFont13() { // PHN-12
    scaled(1.3f); launch()
    rule.onNodeWithContentDescription("Message to Zohara Chat").assertIsDisplayed()
    rule.onNodeWithContentDescription("Send").assertIsDisplayed()
    assertEquals(emptyList<String>(), outside())
    shot("app_home_320dp_font13_dark")
  }

  @Test fun homeAt320dpWithFont20() { // PHN-12, THEME-10
    scaled(2.0f); launch()
    rule.onNodeWithContentDescription("Message to Zohara Chat").assertIsDisplayed()
    assertEquals(emptyList<String>(), outside())
    shot("app_home_320dp_font20_dark")
  }

  @Test fun drawerAndSettingsAt320dpWithFont13() { // PHN-12, NAV-05
    scaled(1.3f); launch()
    rule.onNodeWithContentDescription("Open the sidebar").performClick()
    rule.waitUntil(8000) { hasText("Pinned plan") }
    assertEquals("drawer", emptyList<String>(), outside())
    shot("app_drawer_320dp_font13_dark")
    rule.onNodeWithContentDescription("Settings").performClick()
    rule.waitUntil(8000) { hasText("Model and profiles") }
    assertEquals("settings list", emptyList<String>(), outside())
    for (page in listOf("Notifications", "Appearance and permissions", "Account")) {
      rule.onAllNodesWithText(page)[0].performScrollTo().performClick()
      rule.waitUntil(8000) { has("Back to Settings") }
      rule.waitForIdle()
      assertEquals("page $page", emptyList<String>(), outside())
      if (page == "Notifications") shot("app_settings_notifications_320dp_font13_dark")
      rule.onNodeWithContentDescription("Back to Settings").performClick()
      rule.waitUntil(8000) { !has("Back to Settings") }
    }
  }

  @Test fun chatAt320dpWithFont13() { // PHN-12, CHAT
    scaled(1.3f); launch()
    pending.value = "c1"
    rule.waitUntil(8000) { hasText("Plan my week") }
    rule.waitForIdle()
    assertEquals(emptyList<String>(), outside())
    shot("app_chat_320dp_font13_dark")
  }
}
