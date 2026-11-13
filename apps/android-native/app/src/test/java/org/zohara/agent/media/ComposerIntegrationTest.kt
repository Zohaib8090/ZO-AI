package org.zohara.agent.media

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTextInput
import com.github.takahirom.roborazzi.captureRoboImage
import androidx.test.core.app.ApplicationProvider
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.agent.AppContainer
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.chat.ChatHooks
import org.zohara.agent.chat.Composer
import org.zohara.agent.chat.ComposerModel
import org.zohara.agent.chat.PermMode
import org.zohara.agent.core.design.ZoAgentTheme

/**
 * The real chat composer with the media and link controls plugged into its slots (COMP-05, COMP-06, COMP-17, ATT-01, ATT-20, VOICE-05):
 * one controller shared by the strip, the "+" button, the microphone and the text box.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h860dp-xhdpi", application = android.app.Application::class)
class ComposerIntegrationTest {
  @get:Rule val rule = createComposeRule()

  private val container by lazy { AppContainer(ApplicationProvider.getApplicationContext()) }

  @After fun clear() {
    ChatHooks.composerHost = null; ChatHooks.attachmentChips = null; ChatHooks.attachButton = null; ChatHooks.micButton = null
    ChatHooks.fieldModifier = null; ChatHooks.runOnPill = null; ChatHooks.textEdited = null; ChatHooks.readAloud = null; ChatHooks.readAloudButton = null
  }

  private fun model(busy: Boolean = false) = ComposerModel(busy, false, "Opus 5.5", PermMode.Manual, null, false, null, false, false, {}, {}, {}, {}, {}, null)

  private fun show(busy: Boolean = false, onText: (String) -> Unit = {}, start: String = "", dark: Boolean = true) {
    // the plug installs once per process: the test installs the slots it needs against its own container
    rule.runOnUiThread { ChatPlug.resetForTests(); ChatPlug.install(container) }
    rule.setContent {
      CompositionLocalProvider(LocalAppContainer provides container) {
        ZoAgentTheme(dark) {
          Box(Modifier.fillMaxSize().background(org.zohara.agent.core.design.ZoTheme.colors.bg)) {
            var text by remember { mutableStateOf(start) }
            Composer(text, { text = it; onText(it) }, model(busy), hasAttachments = false)
          }
        }
      }
    }
  }

  @Test fun thePlusAndTheMicrophoneAreInTheComposer() { // COMP-05, COMP-06
    show()
    rule.onNodeWithContentDescription("Add pictures, files or a link").assertIsDisplayed()
    rule.onNodeWithContentDescription("Message to Zohara Agent").assertIsDisplayed()
  }

  @Test fun theMessageBoxStillTakesText() {
    var last = ""
    show(onText = { last = it })
    rule.onNode(hasSetTextAction()).performTextInput("hello there")
    rule.waitForIdle()
    assertEquals("hello there", last)
  }

  @Test fun aLinkChipFromTheStripShowsAboveTheBoxAndItsXRemovesIt() { // COMP-17, ATT-11, ATT-13
    show()
    rule.runOnUiThread { MediaModule.of(container).attachments.addLink("https://example.com/a") }
    rule.waitForIdle()
    rule.onNodeWithText("example.com").assertIsDisplayed()
    rule.onNodeWithContentDescription("Remove link example.com").performClick()
    rule.waitForIdle()
    assertTrue(MediaModule.of(container).attachments.items.value.isEmpty())
  }

  @Test fun anAddressTypedInTheTextBecomesAChipAndStaysInTheText() { // ATT-20
    var last = ""
    show(onText = { last = it }, start = "read https://typed.example.net/a please")
    rule.waitForIdle()
    rule.onNodeWithText("typed.example.net").assertIsDisplayed()
    assertEquals("read https://typed.example.net/a please", last.ifEmpty { "read https://typed.example.net/a please" })
    rule.onNodeWithContentDescription("Remove link typed.example.net").performClick()
    rule.waitForIdle()
    rule.onNodeWithText("typed.example.net").assertDoesNotExist()
  }

  private fun shot(dark: Boolean) {
    show(start = "Summarise this page for me https://typed.example.net/a", dark = dark)
    rule.runOnUiThread { MediaModule.of(container).attachments.addLink("https://example.com/post") }
    rule.waitForIdle()
    rule.onRoot().captureRoboImage("src/test/screenshots/media_composer_${if (dark) "dark" else "light"}.png")
  }

  @Test fun composerShotDark() = shot(true)
  @Test fun composerShotLight() = shot(false)

  // ---------------------------------------------------------------- paste (ATT-15)

  private fun composerMedia(): ComposerMedia {
    val module = MediaModule.of(container)
    val engine = object : SpeechEngine {
      override val available = false
      override fun start(language: String, listener: SpeechEngine.Listener) {}
      override fun stop() {}
      override fun cancel() {}
      override fun destroy() {}
    }
    val voice = VoiceController(engine, { "" }, {}, { "en-US" }, { false }, {}, {})
    return ComposerMedia(module, module.attachments, voice, container.app, { "" }, {})
  }

  private fun clip(text: String) {
    val cm = container.app.getSystemService(android.content.ClipboardManager::class.java)
    cm.setPrimaryClip(android.content.ClipData.newPlainText("t", text))
  }

  @Test fun aPastedAddressBecomesAChipAndIsNotKeptInTheText() {
    MediaModule.of(container).attachments.clear()
    clip("https://pasted.example.com/page")
    val m = composerMedia()
    val shown = m.onTextEdited("read ", "read https://pasted.example.com/page")
    assertEquals("read ", shown)
    assertEquals(listOf("https://pasted.example.com/page"), MediaModule.of(container).attachments.items.value.filterIsInstance<LinkItem>().map { it.url })
  }

  @Test fun anAddressTypedByHandStaysInTheText() {
    MediaModule.of(container).attachments.clear()
    clip("something else")
    val m = composerMedia()
    assertEquals("read https://typed.example.com/x", m.onTextEdited("read ", "read https://typed.example.com/x"))
    assertTrue(MediaModule.of(container).attachments.items.value.isEmpty())
  }

  @Test fun aPictureOrFileInTheClipboardIsTakenAndAnAddressBecomesAChip() {
    MediaModule.of(container).attachments.clear()
    val m = composerMedia()
    val img = android.content.ClipData("pic", arrayOf("image/png"), android.content.ClipData.Item(android.net.Uri.parse("content://zo.test/pic.png")))
    assertTrue(m.attachClip(img))
    val link = android.content.ClipData.newPlainText("t", "https://clip.example.org/a")
    assertTrue(m.attachClip(link))
    assertTrue(MediaModule.of(container).attachments.items.value.any { it is LinkItem && it.url == "https://clip.example.org/a" })
    assertTrue(!m.attachClip(android.content.ClipData.newPlainText("t", "just some words")))
  }

  @Test fun theCameraRowIsOnlyOfferedWithACamera() { // ATT-28: this test phone has none
    assertTrue(!CameraCapture.available(container.app))
    assertTrue(!composerMedia().hasCamera)
  }
}
