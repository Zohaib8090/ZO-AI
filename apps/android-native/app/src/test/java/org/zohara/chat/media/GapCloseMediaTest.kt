package org.zohara.chat.media

import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityManager
import androidx.activity.ComponentActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.test.core.app.ApplicationProvider
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.chat.AppContainer
import org.zohara.chat.chat.AppChatEnv
import org.zohara.chat.core.design.ZoChatTheme
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.nav.screenReaderAnnouncer

/** The gaps the parity check of 2026-11-13 found in MEDIA, VOICE and NOTIF: ATT-16, ATT-18, VOICE-09, VOICE-11, VOICE-15. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h860dp-xhdpi", application = android.app.Application::class)
class GapCloseMediaTest {
  @get:Rule val rule = createAndroidComposeRule<ComponentActivity>()

  private val container by lazy { AppContainer(ApplicationProvider.getApplicationContext()) }

  private class FakeEngine(override val available: Boolean = true) : SpeechEngine {
    var started = 0; var cancelled = 0
    override fun start(language: String, listener: SpeechEngine.Listener) { started++ }
    override fun stop() {}
    override fun cancel() { cancelled++ }
    override fun destroy() {}
  }

  private fun composerMedia(engine: SpeechEngine = FakeEngine()): ComposerMedia {
    val module = MediaModule.of(container)
    val voice = VoiceController(engine, { "" }, {}, { "en-US" }, { false }, {}, {})
    return ComposerMedia(module, module.attachments, voice, container.app, { "" }, {})
  }

  // ---------------------------------------------------------------- ATT-18

  @Test fun enterOnTheKeyboardAddsTheLink() {
    var done = 0
    rule.setContent { ZoChatTheme(true) { Box(Modifier.fillMaxSize()) { AddLinkBody("https://example.com/a", onDone = { done++ }) {} } } }
    rule.onNode(hasSetTextAction()).performImeAction()
    assertEquals(1, done)
  }

  @Test fun theLinkSheetAddsOnDoneAndClosesAndTheChipExists() {
    val m = composerMedia()
    MediaModule.of(container).attachments.clear()
    m.linkSheetOpen = true
    rule.setContent { ZoChatTheme(true) { Box(Modifier.fillMaxSize()) { ComposerMediaSheets(m, overlay = false) } } }
    rule.waitForIdle()
    rule.onNode(hasSetTextAction()).performTextInput("example.com/page")
    rule.onNode(hasSetTextAction()).performImeAction()
    rule.waitForIdle()
    assertFalse("the sheet closed", m.linkSheetOpen)
    assertEquals(listOf("https://example.com/page"), MediaModule.of(container).attachments.items.value.filterIsInstance<LinkItem>().map { it.url })
  }

  // ---------------------------------------------------------------- ATT-16

  @Test fun aDropOfAFileOrAnAddressAttaches() {
    MediaModule.of(container).attachments.clear()
    val m = composerMedia()
    val file = android.content.ClipData("drop", arrayOf("image/png"), android.content.ClipData.Item(android.net.Uri.parse("content://zo.test/dropped.png")))
    assertTrue(m.attachClip(file))
    val list = android.content.ClipData("drop", arrayOf("text/uri-list"), android.content.ClipData.Item(android.net.Uri.parse("https://dropped.example.org/page")))
    assertTrue("an address from a drag becomes a link chip", m.attachClip(list))
    assertTrue(MediaModule.of(container).attachments.items.value.any { it is LinkItem && it.url == "https://dropped.example.org/page" })
  }

  @Test fun theDropOverlayShowsOnlyWhileSomethingIsDragged() {
    val m = composerMedia()
    rule.setContent { ZoChatTheme(true) { Box(Modifier.fillMaxSize()) { DropOverlay(m) } } }
    rule.onNodeWithText("Drop to attach").assertDoesNotExist()
    rule.runOnUiThread { m.dropping = true }
    rule.waitForIdle()
    rule.onNodeWithText("Drop to attach").assertExists()
    rule.onNodeWithText("Pictures, PDF, ZIP, text and code files").assertExists()
    rule.runOnUiThread { m.dropping = false }
    rule.waitForIdle()
    rule.onNodeWithText("Drop to attach").assertDoesNotExist()
  }

  private fun dropShot(dark: Boolean) {
    rule.setContent { ZoChatTheme(dark) { Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg), contentAlignment = androidx.compose.ui.Alignment.Center) { DropCard() } } }
    rule.onRoot().captureRoboImage("src/test/screenshots/media_dropcard_${if (dark) "dark" else "light"}.png")
  }
  @Test fun dropCardDark() = dropShot(true)
  @Test fun dropCardLight() = dropShot(false)

  // ---------------------------------------------------------------- VOICE-15

  @OptIn(ExperimentalTestApi::class)
  @Composable private fun Keys(m: ComposerMedia) {
    val f = androidx.compose.runtime.remember { FocusRequester() }
    Box(Modifier.fillMaxSize().testTag("keys").mediaKeys(m).focusRequester(f).focusable())
    LaunchedEffect(Unit) { f.requestFocus() }
  }

  @OptIn(ExperimentalTestApi::class)
  @Test fun ctrlShiftMStartsTheMicrophone() {
    shadowOf(container.app).grantPermissions(android.Manifest.permission.RECORD_AUDIO)
    val engine = FakeEngine()
    val m = composerMedia(engine)
    rule.setContent { Keys(m) }
    rule.waitForIdle()
    rule.onNodeWithTag("keys").performKeyInput { keyDown(Key.CtrlLeft); keyDown(Key.ShiftLeft); keyDown(Key.M); keyUp(Key.M); keyUp(Key.ShiftLeft); keyUp(Key.CtrlLeft) }
    rule.waitForIdle()
    assertTrue("listening", m.voice.listening.value)
    assertEquals(1, engine.started)
  }

  @OptIn(ExperimentalTestApi::class)
  @Test fun escapeCancelsListeningAndPutsTheTextBack() {
    val engine = FakeEngine()
    val m = composerMedia(engine)
    rule.setContent { Keys(m) }
    rule.runOnUiThread { m.voice.start() }
    assertTrue(m.voice.listening.value)
    rule.onNodeWithTag("keys").performKeyInput { keyDown(Key.Escape); keyUp(Key.Escape) }
    rule.waitForIdle()
    assertFalse(m.voice.listening.value)
    assertTrue("the recogniser was cancelled", engine.cancelled >= 1)
  }

  @OptIn(ExperimentalTestApi::class)
  @Test fun escapeStopsReadingAloud() {
    val m = composerMedia(FakeEngine(available = false))
    rule.setContent { Keys(m) }
    rule.runOnUiThread { m.module.readAloud.speak("answer-1", "Hello there, this is an answer.") }
    assertEquals("answer-1", m.module.readAloud.speaking.value)
    rule.onNodeWithTag("keys").performKeyInput { keyDown(Key.Escape); keyUp(Key.Escape) }
    rule.waitForIdle()
    assertNull(m.module.readAloud.speaking.value)
  }

  // ---------------------------------------------------------------- VOICE-09

  @Test fun theScreenReaderIsToldListeningAndStopped() {
    val am = shadowOf(container.app.getSystemService(AccessibilityManager::class.java))
    am.setEnabled(true)
    val say = mutableListOf<String>()
    rule.setContent { val v = LocalView.current; LaunchedEffect(Unit) { screenReaderAnnouncer(v)("Listening"); screenReaderAnnouncer(v)("Stopped"); say += "done" } }
    rule.waitForIdle()
    val texts = am.sentAccessibilityEvents.filter { it.eventType == AccessibilityEvent.TYPE_ANNOUNCEMENT }.flatMap { e -> e.text.map { it.toString() } }
    assertTrue("Listening" in texts && "Stopped" in texts)
  }

  // ---------------------------------------------------------------- VOICE-11

  @Test fun readAloudAutoReadsOnlyAVisibleChatsNonBlankAnswerWhenTheSettingIsOn() {
    assertTrue(AppChatEnv.readAloudNow(foreground = true, auto = true, chatShown = true, answer = "Hello"))
    assertFalse("the setting is off", AppChatEnv.readAloudNow(true, false, true, "Hello"))
    assertFalse("the app is in the background", AppChatEnv.readAloudNow(false, true, true, "Hello"))
    assertFalse("another chat is on screen", AppChatEnv.readAloudNow(true, true, false, "Hello"))
    assertFalse("an answer without text", AppChatEnv.readAloudNow(true, true, true, "  \n"))
  }
}
