package org.zohara.agent.chat

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.performTextReplacement
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.click
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.platform.UriHandler
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.unit.height
import androidx.compose.ui.unit.width
import androidx.compose.ui.test.junit4.StateRestorationTester
import androidx.compose.ui.unit.dp
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.agent.core.design.ZoAgentTheme
import org.zohara.agent.core.markdown.MarkdownText
import org.zohara.agent.core.net.QuestionAnswer
import org.zohara.agent.core.net.QuestionOption
import org.zohara.agent.core.net.QuestionSpec
import org.zohara.agent.core.net.TakeInfo

/**
 * What a person can do in the chat, driven through the real composables (Robolectric, no device): composer, bubbles, takes, tool groups, cards,
 * the thread's scrolling rules, menus and sheets. Each test names the matrix rows it covers.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h860dp-xhdpi", application = android.app.Application::class)
@OptIn(androidx.compose.ui.test.ExperimentalTestApi::class)
class ChatInteractionTest {
  @get:Rule val rule = createComposeRule()

  private fun themed(content: @Composable () -> Unit) = rule.setContent { ZoAgentTheme(true) { Box(Modifier.fillMaxSize()) { content() } } }

  /** everything the screen would be told, recorded */
  private class Rec {
    val copied = mutableListOf<String>()
    val shared = mutableListOf<String>()
    val edits = mutableListOf<Pair<Int, String>>()
    var retries = 0
    val takes = mutableListOf<Pair<Int, Int>>()
    val opened = mutableListOf<Pair<Int, Int>>()
    val approvals = mutableListOf<Pair<Long, Boolean>>()
    val answers = mutableListOf<Pair<Long, List<QuestionAnswer>>>()
    val skips = mutableListOf<Long>()
    var earlier = 0
    var chooseModel = 0
    val aloud = mutableListOf<String>()
  }

  private fun cb(r: Rec, readAloud: Boolean = false) = ThreadCallbacks(
    MessageActions(true, { r.copied += it }, { r.shared += it }, { r.edits += it.nth to it.text }, { r.retries++ }, { n, d -> r.takes += n to d }, { r.chooseModel++ }, if (readAloud) { t -> r.aloud += t } else null),
    { r.earlier++ }, { g, l -> r.opened += g to l }, { k, y -> r.approvals += k to y }, { k, a -> r.answers += k to a }, { k -> r.skips += k },
  )

  private fun clipboardText(): String? {
    val cm = ApplicationProvider.getApplicationContext<Context>().getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    return cm.primaryClip?.getItemAt(0)?.text?.toString()
  }

  private fun composerModel(
    busy: Boolean = false, question: Boolean = false, limit: Int? = null, context: Boolean = true,
    onSend: (String) -> Unit = {}, onStop: () -> Unit = {}, onModel: () -> Unit = {}, onMode: () -> Unit = {}, onContext: () -> Unit = {}, onTerminal: (() -> Unit)? = null,
  ) = ComposerModel(busy, question, "Opus 5.5", PermMode.Manual, null, limit != null, limit, context, false, onSend, onStop, onModel, onMode, onContext, onTerminal)

  private val field get() = rule.onNode(hasSetTextAction())

  // ====================================================================== composer

  @Test fun sendIsOffWithoutTextAndSendsTheTypedTextWhenOn() { // COMP-01, COMP-02
    var sent: String? = null
    themed {
      var text by remember { mutableStateOf("") }
      Composer(text, { text = it }, composerModel(onSend = { sent = it }))
    }
    rule.onNodeWithText(PLACEHOLDER).assertExists()
    rule.onNodeWithContentDescription("Send").assertIsNotEnabled()
    field.performTextInput("hello there")
    rule.onNodeWithContentDescription("Send").assertIsEnabled().performClick()
    assertEquals("hello there", sent)
  }

  @Test fun whileAnAnswerIsWrittenTheArrowIsAStopSquareAndTheBoxIsOff() { // COMP-02
    var stopped = 0
    themed { Composer("draft", {}, composerModel(busy = true, onStop = { stopped++ })) }
    rule.onNodeWithContentDescription("Send").assertDoesNotExist()
    rule.onNodeWithTag("message-field").assertIsNotEnabled()
    rule.onNodeWithContentDescription("Stop").assertIsEnabled().performClick()
    assertEquals(1, stopped)
  }

  @Test fun aWaitingQuestionTurnsTheBoxOffAndSaysWhy() { // COMP-16
    themed { Composer("", {}, composerModel(busy = true, question = true)) }
    rule.onNodeWithText("Answer the question above first.").assertExists()
    rule.onNodeWithTag("message-field").assertIsNotEnabled()
  }

  @Test fun enterSendsOnAHardwareKeyboardAndShiftEnterIsANewLine() { // COMP-03
    val sent = mutableListOf<String>()
    themed {
      var text by remember { mutableStateOf("") }
      Composer(text, { text = it }, composerModel(onSend = { sent += it }))
    }
    field.performClick()
    field.performTextInput("one")
    field.performKeyInput { keyDown(Key.ShiftLeft); pressKey(Key.Enter); keyUp(Key.ShiftLeft) }
    assertTrue("shift+enter must not send", sent.isEmpty())
    field.performKeyInput { pressKey(Key.Enter) }
    assertEquals(1, sent.size)
    assertEquals("one", sent.single().trim())
  }

  @Test fun theBoxGrowsWithTheTextAndStopsAtAboutTwoHundredAndTwentyDp() { // COMP-01
    themed {
      var text by remember { mutableStateOf("") }
      Composer(text, { text = it }, composerModel())
    }
    val h1 = field.getBoundsInRoot().height
    field.performTextInput((1..60).joinToString("\n") { "line $it" })
    rule.waitForIdle()
    val h2 = field.getBoundsInRoot().height
    assertTrue("grew: $h1 -> $h2", h2 > h1)
    assertTrue("stops at 220 dp, was $h2", h2 <= 221.dp)
  }

  @Test fun theCounterAppearsFromNinetyPercentOfTheLimit() { // SRV-01
    themed { Composer("x".repeat(100), {}, composerModel(limit = 20_000)) }
    rule.onNodeWithText("100 / 20000").assertDoesNotExist()
  }

  @Test fun theCounterShowsAtNinetyPercent() { // SRV-01
    themed { Composer("x".repeat(18_000), {}, composerModel(limit = 20_000)) }
    rule.onNodeWithText("18000 / 20000").assertExists()
  }

  @Test fun thePillsOpenTheirSheetsAndContextAndTerminalAppearOnlyWhenOffered() { // COMP-07..12, PERM-01
    var model = 0; var mode = 0; var ctx = 0; var term = 0
    themed { Composer("", {}, composerModel(onModel = { model++ }, onMode = { mode++ }, onContext = { ctx++ }, onTerminal = { term++ })) }
    rule.onNodeWithContentDescription("Model: Opus 5.5").performClick()
    rule.onNodeWithContentDescription("Permission mode: Manual").performClick()
    rule.onNodeWithContentDescription("Context window").performClick()
    rule.onNodeWithContentDescription("Terminal").performClick()
    assertEquals(listOf(1, 1, 1, 1), listOf(model, mode, ctx, term))
  }

  @Test fun noContextOrTerminalPillOnAnEmptyHome() {
    themed { Composer("", {}, composerModel(context = false)) }
    rule.onNodeWithContentDescription("Context window").assertDoesNotExist()
    rule.onNodeWithContentDescription("Terminal").assertDoesNotExist()
  }

  @Test fun theSlotsOfTheOtherModulesAreDrawnInTheComposer() { // hooks: attachment chips, "+" button, Run on pill, mic
    ChatHooks.attachmentChips = { androidx.compose.material3.Text("chips here") }
    ChatHooks.attachButton = { androidx.compose.material3.Text("plus here") }
    ChatHooks.runOnPill = { androidx.compose.material3.Text("run on here") }
    ChatHooks.micButton = { _, _ -> androidx.compose.material3.Text("mic here") }
    try {
      themed { Composer("", {}, composerModel()) }
      for (t in listOf("chips here", "plus here", "run on here", "mic here")) rule.onNodeWithText(t).assertExists()
    } finally {
      ChatHooks.attachmentChips = null; ChatHooks.attachButton = null; ChatHooks.runOnPill = null; ChatHooks.micButton = null
    }
  }

  @Test fun anAttachmentAloneIsEnoughToSend() { // ATT: send with no text
    var sent = 0
    themed { Composer("", {}, composerModel(onSend = { sent++ }), hasAttachments = true) }
    rule.onNodeWithContentDescription("Send").assertIsEnabled().performClick()
    assertEquals(1, sent)
  }

  @Test fun theDraftComesBackAfterTheSavedStateIsRestored() { // COMP-18, NAV-19
    val flows = mutableListOf<kotlinx.coroutines.flow.MutableStateFlow<String>>()
    val tester = StateRestorationTester(rule)
    // a new session (a new flow) is made whenever the screen is built again, as after the system ended the process
    tester.setContent { val f = remember { kotlinx.coroutines.flow.MutableStateFlow("").also { flows += it } }; DraftKeeper(f) }
    flows[0].value = "my half written message"
    rule.waitForIdle()
    tester.emulateSavedInstanceStateRestore()
    rule.waitForIdle()
    assertEquals(2, flows.size)
    assertEquals("my half written message", flows.last().value)
  }

  // ====================================================================== user messages

  private fun state(vararg items: ChatItem, busy: Boolean = false, hidden: Int = 0, loading: Boolean = false) =
    ChatUiState(chatId = "c1", title = "T", items = items.toList(), busy = busy, visibleFrom = hidden, loading = loading)

  @Test fun aUserBubbleHasCopyAndEditUnderTheLastOne() { // CHAT-05
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "first", 0), AiItem(2, "a"), UserItem(3, "second", 1), AiItem(4, "b")), cb(r)) }
    rule.onNodeWithContentDescription("Copy your message").performClick()
    assertEquals(listOf("second"), r.copied)
  }

  @Test fun theLongPressMenuOfABubbleHasCopySelectShareAndEdit() { // CHAT-43
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "pick me", 0)), cb(r)) }
    rule.onNodeWithContentDescription("You: pick me").performTouchInput { longClick() }
    rule.onNodeWithText("Select text").assertExists()
    rule.onNodeWithText("Edit").assertExists()
    rule.onNodeWithText("Share").performClick()
    assertEquals(listOf("pick me"), r.shared)
    rule.onNodeWithContentDescription("You: pick me").performTouchInput { longClick() }
    rule.onNodeWithText("Copy").performClick()
    assertEquals(listOf("pick me"), r.copied)
  }

  @Test fun editingReplacesTheBubbleCancelRestoresAndSendAsksAgainFromThatMessage() { // CHAT-06, CHAT-48
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "old words", 0), AiItem(2, "reply")), cb(r)) }
    rule.onNodeWithContentDescription("Edit your message").performClick()
    // the bubble and its buttons are hidden while the editor is open
    rule.onNodeWithContentDescription("You: old words").assertDoesNotExist()
    rule.onNodeWithContentDescription("Copy your message").assertDoesNotExist()
    rule.onNodeWithText("Cancel").assertExists()
    rule.onNodeWithText("Send").assertExists()
    rule.onNodeWithText("Cancel").performClick()
    rule.onNodeWithContentDescription("You: old words").assertExists()
    assertTrue(r.edits.isEmpty())
    // again, with new words
    rule.onNodeWithContentDescription("Edit your message").performClick()
    field.performTextReplacement("old words and more")
    rule.onNodeWithText("Send").performClick()
    assertEquals(listOf(0 to "old words and more"), r.edits)
  }

  @Test fun anEmptiedEditCannotBeSent() { // CHAT-06
    val r = Rec()
    themed { EditBox("x", {}, { r.edits += 0 to it }) }
    rule.onNodeWithText("Send").assertIsEnabled()
    field.performTextReplacement("   ")
    rule.onNodeWithText("Send").assertIsNotEnabled()
  }

  @Test fun noEditWhileAnAnswerIsRunning() { // CHAT-06
    themed { ThreadView(state(UserItem(1, "x", 0), busy = true), cb(Rec())) }
    rule.onNodeWithContentDescription("Edit your message").assertDoesNotExist()
    rule.onNodeWithContentDescription("You: x").performTouchInput { longClick() }
    rule.onNodeWithText("Edit").assertDoesNotExist()
  }

  @Test fun takesArrowsSwitchAndStopAtTheEnds() { // CHAT-07, CHAT-46
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0, take = TakeInfo(count = 3, current = 1, nth = 0)), AiItem(2, "a")), cb(r)) }
    rule.onNodeWithText("2 / 3").assertExists()
    rule.onNodeWithContentDescription("Previous take").assertIsEnabled().performClick()
    rule.onNodeWithContentDescription("Next take").assertIsEnabled().performClick()
    assertEquals(listOf(0 to -1, 0 to 1), r.takes)
  }

  @Test fun theFirstTakeHasNoPreviousAndTheArrowsHideWhileBusy() {
    themed { ThreadView(state(UserItem(1, "q", 0, take = TakeInfo(count = 2, current = 0, nth = 0))), cb(Rec())) }
    rule.onNodeWithContentDescription("Previous take").assertIsNotEnabled()
    rule.onNodeWithContentDescription("Next take").assertIsEnabled()
  }

  @Test fun noTakeArrowsWhileBusy() {
    themed { ThreadView(state(UserItem(1, "q", 0, take = TakeInfo(count = 2, current = 0, nth = 0)), busy = true), cb(Rec())) }
    rule.onNodeWithContentDescription("Next take").assertDoesNotExist()
  }

  // ====================================================================== AI messages

  @Test fun theNewestAnswerHasCopyTryAgainAndReadAloud() { // CHAT-08, CHAT-09
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), AiItem(2, "first answer"), UserItem(3, "q2", 1), AiItem(4, "second answer")), cb(r, readAloud = true)) }
    rule.onAllNodesWithText("Try again").assertCountEquals(1)
    rule.onNodeWithContentDescription("Copy the reply").performClick()
    rule.onNodeWithText("Try again").performClick()
    rule.onNodeWithContentDescription("Read aloud").performClick()
    assertEquals(listOf("second answer"), r.copied)
    assertEquals(1, r.retries)
    assertEquals(listOf("second answer"), r.aloud)
  }

  @Test fun noActionRowWhileTheAnswerIsStreaming() { // CHAT-11
    themed { ThreadView(state(UserItem(1, "q", 0), AiItem(2, "partial", live = true), busy = true), cb(Rec())) }
    rule.onNodeWithContentDescription("Copy the reply").assertDoesNotExist()
    rule.onNodeWithText("Try again").assertDoesNotExist()
  }

  @Test fun theAnswerMenuOffersCopySelectShareTryAgainAndEachLink() { // CHAT-43, CHAT-22, CHAT-25
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), AiItem(2, "Read [the docs](https://zo.test/docs) and [more](https://zo.test/more).")), cb(r)) }
    rule.onNodeWithContentDescription("Zohara Agent").performTouchInput { longClick() }
    rule.onNodeWithText("Select text").assertExists()
    rule.onNodeWithText("Share").assertExists()
    rule.onNodeWithText("Copy link: zo.test/more").assertExists()
    rule.onNodeWithText("Copy link: zo.test/docs").performClick()
    assertEquals(listOf("https://zo.test/docs"), r.copied)
  }

  @Test fun selectTextTurnsOnSelectionAndKeepsTheAnswer() { // CHAT-25
    themed { ThreadView(state(UserItem(1, "q", 0), AiItem(2, "select this sentence")), cb(Rec())) }
    rule.onNodeWithContentDescription("Zohara Agent").performTouchInput { longClick() }
    rule.onNodeWithText("Select text").performClick()
    rule.onNodeWithText("select this sentence").assertIsDisplayed()
  }

  @Test fun aLinkInAnAnswerGoesToTheUriHandlerWhichOpensACustomTab() { // CHAT-22
    val opened = mutableListOf<String>()
    val handler = object : UriHandler { override fun openUri(uri: String) { opened += uri } }
    themed { CompositionLocalProvider(LocalUriHandler provides handler) { ThreadView(state(AiItem(1, "[the docs](https://zo.test/docs)")), cb(Rec())) } }
    // the text is short and sits at the left of a wide row: tap on the words, not in the middle of the row
    rule.onNodeWithText("the docs").performTouchInput { click(androidx.compose.ui.geometry.Offset(30f, 20f)) }
    assertEquals(listOf("https://zo.test/docs"), opened)
  }

  @Test fun onlyWebAddressesAreOpenedByTheLinkHandler() { // CHAT-22
    val ctx = org.robolectric.Robolectric.buildActivity(android.app.Activity::class.java).create().get()
    val shadow = org.robolectric.Shadows.shadowOf(ctx)
    ChatUriHandler(ctx).openUri("file:///etc/passwd")
    ChatUriHandler(ctx).openUri("javascript:alert(1)")
    assertNull(shadow.nextStartedActivity)
    ChatUriHandler(ctx).openUri("https://zo.test/x")
    val started = shadow.nextStartedActivity
    assertEquals("https://zo.test/x", started.dataString)
  }

  @Test fun theLinksOfAnAnswerAreListedOnceAndAtMostThree() { // CHAT-22
    val text = "[a](https://x.test/1) [b](https://x.test/2) **[c](https://x.test/3)** [d](https://x.test/4) [again](https://x.test/1)"
    assertEquals(listOf("https://x.test/1", "https://x.test/2", "https://x.test/3"), AnswerLinks.of(text))
    assertTrue(AnswerLinks.of("a bare address https://bare.test is plain text").isEmpty())
  }

  @Test fun aCodeBlockHasItsLanguageACopyButtonAndTheWholeCode() { // CHAT-23
    val code = "fn main() {\n    println!(\"hi\");\n}"
    themed { ThreadView(state(AiItem(1, "Try this:\n\n```rust\n$code\n```\n")), cb(Rec())) }
    rule.onNodeWithText("rust").assertExists()
    rule.onNodeWithContentDescription("Copy the code").performClick()
    assertEquals(code, clipboardText())
  }

  @Test fun aBlockWithoutALanguageIsCalledCode() { // CHAT-23
    themed { ThreadView(state(AiItem(1, "```\nls -la\n```")), cb(Rec())) }
    rule.onNodeWithText("code").assertExists()
  }

  @Test fun tablesImagesAndHtmlAreShownAsTextNotRendered() { // CHAT-24
    themed { MarkdownText("| name | age |\n|---|---|\n| Ali | 7 |\n\n<b>bold html</b>\n\n<img src=\"x.png\">") }
    rule.onNodeWithText("| name | age |").assertExists()
    rule.onNodeWithText("| Ali | 7 |").assertExists()
    rule.onNodeWithText("<b>bold html</b>").assertExists()
    rule.onNodeWithText("<img src=\"x.png\">").assertExists()
  }

  // ====================================================================== tool groups and errors

  private val twoCommands = ToolGroupItem(
    2, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: ls"), ToolLine(LineKind.Ok, "a.txt"), ToolLine(LineKind.Start, "read_file  path: a.txt"), ToolLine(LineKind.Fail, "no such file")), active = false,
  )

  @Test fun aFinishedToolGroupIsCollapsedSaysRanNCommandsAndOpensOnTap() { // CHAT-27, 28, 31
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), twoCommands, AiItem(3, "done")), cb(r)) }
    rule.onNodeWithText("Ran 2 commands").assertExists()
    rule.onNodeWithText("▸ zohara_shell  cmd: ls").assertDoesNotExist()
    rule.onNodeWithText("Ran 2 commands").performClick()
    rule.onNodeWithText("▸ zohara_shell  cmd: ls").assertExists()
    rule.onNodeWithText("✗ no such file").assertExists()
    rule.onNode(hasText("Ran 2 commands") and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Expanded")).assertExists()
    rule.onNodeWithText("Ran 2 commands").performClick()
    rule.onNodeWithText("▸ zohara_shell  cmd: ls").assertDoesNotExist()
  }

  @Test fun aTappedToolLineOpensTheFullOutputOfThatGroup() { // CHAT-34
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), twoCommands), cb(r)) }
    rule.onNodeWithText("Ran 2 commands").performClick()
    rule.onNodeWithText("✓ a.txt").performClick()
    assertEquals(listOf(0 to 1), r.opened)
  }

  @Test fun anActiveGroupSaysWhatItDoes() { // CHAT-28
    themed { ThreadView(state(ToolGroupItem(1, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: cargo build")), active = true), busy = true), cb(Rec())) }
    rule.onNodeWithText("Running a task in the terminal…").assertExists()
  }

  @Test fun theToolOutputViewerShowsEverythingTheShortenedNoteAndTheScreenshotLabel() { // CHAT-34, 35, 36
    val g = ToolGroupItem(1, listOf(
      ToolLine(LineKind.Start, "zohara_shell  cmd: ls"), ToolLine(LineKind.Ok, "one\ntwo\nthree…"),
      ToolLine(LineKind.Start, "screenshot  of the desktop"), ToolLine(LineKind.Ok, "/tmp/shot-1.png"),
    ), active = false)
    themed { ToolOutputBody(g, "Laptop") }
    rule.onNodeWithText("▸ zohara_shell  cmd: ls").assertExists()
    rule.onNodeWithText("✓ one\ntwo\nthree…").assertExists()
    rule.onNodeWithText("…(shortened by the server)").assertExists()
    rule.onNodeWithText("screenshot (kept on Laptop)").assertExists()
    rule.onNodeWithText("✓ /tmp/shot-1.png").assertExists() // the path as text only
    assertEquals("▸ zohara_shell  cmd: ls\n✓ one\ntwo\nthree…\n▸ screenshot  of the desktop\n✓ /tmp/shot-1.png", toolOutputText(g))
  }

  @Test fun anOutputCutAt1500CharactersIsMarkedAsShortened() { // CHAT-35
    assertTrue(looksShortened(ToolLine(LineKind.Ok, "x".repeat(1500))))
    assertFalse(looksShortened(ToolLine(LineKind.Ok, "complete output")))
    assertFalse(looksShortened(ToolLine(LineKind.Start, "…")))
  }

  @Test fun errorRowsOfferTheRightButton() { // CHAT-19, CHAT-20, NET-07
    val r = Rec()
    themed {
      ThreadView(state(
        UserItem(1, "q", 0),
        ErrorItem(2, "You are going a little fast.", "RateLimited", retry = true, chooseModel = false),
        ErrorItem(3, "No model is chosen on the computer yet.", chooseModel = true),
      ), cb(r))
    }
    rule.onNodeWithText("Choose a model").performClick()
    assertEquals(1, r.chooseModel)
    // the Try again belongs to the last row only: the older error has none
    rule.onAllNodesWithText("Try again").assertCountEquals(0)
  }

  @Test fun theLastErrorOffersTryAgainUnlessTheConnectionWasLost() {
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), ErrorItem(2, "Offline. Retrying.", "Offline")), cb(r)) }
    rule.onNodeWithText("Offline. Retrying.").assertExists()
    rule.onNodeWithText("Try again").performClick()
    assertEquals(1, r.retries)
  }

  @Test fun aLostConnectionHasNoTryAgainBecauseTheChatIsReloaded() {
    themed { ThreadView(state(UserItem(1, "q", 0), ErrorItem(2, ChatSession.CONNECTION_LOST_TEXT(false), "lost", retry = false)), cb(Rec())) }
    rule.onNodeWithText("Try again").assertDoesNotExist()
  }

  @Test fun stoppedAndWaitingRows() { // CHAT-18, CHAT-37
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), StoppedItem(2)), cb(r)) }
    rule.onNodeWithText("Stopped.").assertExists()
    rule.onNodeWithText("Try again").performClick()
    assertEquals(1, r.retries)
  }

  @Test fun theWaitingForYourComputerStateSaysSo() { // CHAT-37
    themed { ThreadView(state(UserItem(1, "q", 0), WaitingItem(2, "zohara_shell cmd: rm x"), busy = true), cb(Rec())) }
    rule.onNodeWithText("Waiting for your approval on your computer: zohara_shell cmd: rm x").assertExists()
  }

  // ====================================================================== cards

  private val approval = ApprovalItem(5, 1, "zohara_shell", "zohara_shell\ncmd: rm -rf build/")

  @Test fun theApprovalCardHasAllowAndDenyOfAtLeast48dp() { // INTER-01, INTER-02
    val r = Rec()
    themed { ThreadView(state(UserItem(1, "q", 0), approval, busy = true), cb(r)) }
    rule.onNodeWithText("Zohara Agent wants to:").assertExists()
    rule.onNodeWithText("Allow").assertHeightIsAtLeast(48.dp).performClick()
    rule.onNodeWithText("Deny").assertHeightIsAtLeast(48.dp).performClick()
    assertEquals(listOf(5L to true, 5L to false), r.approvals)
  }

  @Test fun theApprovalCardOnAComputerSaysSoAndIsAnnouncedAsAPoliteLiveRegion() { // INTER-10, INTER-04
    themed { ThreadView(state(approval.copy(onComputer = true), busy = true), cb(Rec())) }
    rule.onNodeWithText("Your computer: Zohara Agent wants to:").assertExists()
    rule.onNode(hasContentDescription("Zohara Agent wants your approval") and SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)).assertExists()
  }

  @Test fun whileTheAnswerIsSentTheCardCannotBeAnsweredTwice() {
    themed { ThreadView(state(approval.copy(sending = true), busy = true), cb(Rec())) }
    rule.onNodeWithText("Deny").assertIsNotEnabled()
  }

  @Test fun answeredApprovalsCollapseToOneLine() { // INTER-08
    themed {
      ThreadView(state(
        approval.copy(state = ApprovalState.Allowed), approval.copy(key = 6, state = ApprovalState.Denied), approval.copy(key = 7, state = ApprovalState.NoAnswer),
      ), cb(Rec()))
    }
    rule.onNodeWithText("Allowed: zohara_shell cmd: rm -rf build/").assertExists()
    rule.onNodeWithText("Denied: zohara_shell cmd: rm -rf build/").assertExists()
    rule.onNodeWithText("No answer: denied").assertExists()
    rule.onNodeWithText("Allow").assertDoesNotExist()
  }

  private val single = QuestionSpec("Which database?", "Database", false, listOf(QuestionOption("SQLite", "One file."), QuestionOption("PostgreSQL", "A server."), QuestionOption("Keep both", "")))
  private val multi = QuestionSpec("Which platforms?", "Platforms", true, listOf(QuestionOption("Android", ""), QuestionOption("Linux", ""), QuestionOption("Web", "")))

  private fun themedCard(q: QuestionItem, r: Rec) = themed { ThreadView(state(UserItem(1, "q", 0), q, busy = true), cb(r)) }

  @Test fun aSingleChoiceQuestionNeedsAChoiceThenSendsIt() { // INTER-06
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(single)), r)
    rule.onNodeWithText("Which database?").assertExists()
    rule.onNodeWithText("Database").assertExists() // the header chip
    rule.onNodeWithText("One file.").assertExists() // the description under the option
    rule.onNodeWithText("Send").assertIsNotEnabled()
    rule.onNodeWithText("SQLite").performClick()
    rule.onNodeWithText("PostgreSQL").performClick() // single choice: the second replaces the first
    rule.onNodeWithText("SQLite").assertIsNotSelected()
    rule.onNodeWithText("PostgreSQL").assertIsSelected()
    rule.onNodeWithText("Send").assertIsEnabled().performClick()
    assertEquals(listOf(2L to listOf(QuestionAnswer(listOf("PostgreSQL"), null))), r.answers)
  }

  @Test fun aMultiChoiceQuestionTakesSeveralInTheOrderOffered() { // INTER-06
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(multi)), r)
    rule.onNodeWithText("Web").performClick()
    rule.onNodeWithText("Android").performClick()
    rule.onNodeWithText("Send").performClick()
    assertEquals(listOf(QuestionAnswer(listOf("Android", "Web"), null)), r.answers.single().second)
  }

  @Test fun otherHasATextBoxAndTypingTicksIt() { // INTER-07
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(single)), r)
    field.performTextInput("MariaDB please")
    rule.onNodeWithText("Other").assertIsSelected()
    rule.onNodeWithText("Send").assertIsEnabled().performClick()
    assertEquals(listOf(QuestionAnswer(emptyList(), "MariaDB please")), r.answers.single().second)
  }

  @Test fun skipSendsNothingButTheSkip() { // INTER-05
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(single)), r)
    rule.onNodeWithText("Skip").assertHeightIsAtLeast(48.dp).performClick()
    assertEquals(listOf(2L), r.skips)
    assertTrue(r.answers.isEmpty())
  }

  @Test fun severalQuestionsGoPageByPageWithBackAndNextAndSendOnTheLast() { // INTER-06
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(single, multi)), r)
    rule.onNodeWithText("1 of 2").assertExists()
    rule.onNodeWithText("Back").assertDoesNotExist()
    rule.onNodeWithText("Next").assertIsNotEnabled()
    rule.onNodeWithText("SQLite").performClick()
    rule.onNodeWithText("Next").assertIsEnabled().performClick()
    rule.onNodeWithText("2 of 2").assertExists()
    rule.onNodeWithText("Which platforms?").assertExists()
    rule.onNodeWithText("Send").assertIsNotEnabled()
    rule.onNodeWithText("Back").performClick()
    rule.onNodeWithText("SQLite").assertIsSelected() // the first answer is kept
    rule.onNodeWithText("Next").performClick()
    rule.onNodeWithText("Linux").performClick()
    rule.onNodeWithText("Send").performClick()
    assertEquals(listOf(QuestionAnswer(listOf("SQLite"), null), QuestionAnswer(listOf("Linux"), null)), r.answers.single().second)
  }

  @Test fun everyOptionRowAndButtonOfAQuestionIsAtLeast48dpHigh() { // INTER-10, keyboard-less touch targets
    themedCard(QuestionItem(2, 9, listOf(single, multi)), Rec())
    for (t in listOf("SQLite", "PostgreSQL", "Keep both", "Other")) rule.onNodeWithText(t).assertHeightIsAtLeast(48.dp)
    for (t in listOf("Skip", "Next")) rule.onNodeWithText(t).assertHeightIsAtLeast(48.dp)
  }

  @Test fun aQuestionCardIsAnnouncedAndLabelled() { // INTER-10
    themedCard(QuestionItem(2, 9, listOf(single)), Rec())
    rule.onNode(hasContentDescription("Zohara Agent is asking you a question") and SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)).assertExists()
  }

  @Test fun numberKeysPickEscSkipsAndEnterSends() { // INTER-09
    val r = Rec()
    themedCard(QuestionItem(2, 9, listOf(single)), r)
    val card = rule.onNodeWithContentDescription("Zohara Agent is asking you a question")
    card.performKeyInput { pressKey(Key.Two) }
    rule.onNodeWithText("PostgreSQL").assertIsSelected()
    card.performKeyInput { pressKey(Key.Nine) } // there is no ninth option: nothing happens
    card.performKeyInput { pressKey(Key.Enter) }
    assertEquals(listOf(QuestionAnswer(listOf("PostgreSQL"), null)), r.answers.single().second)
    card.performKeyInput { pressKey(Key.Escape) }
    assertEquals(listOf(2L), r.skips)
  }

  @Test fun anHalfFilledCardSurvivesARotation() { // INTER-06
    val tester = StateRestorationTester(rule)
    val q = QuestionItem(2, 9, listOf(single, multi))
    tester.setContent { ZoAgentTheme(true) { Box(Modifier.fillMaxSize()) { ThreadView(state(q, busy = true), cb(Rec())) } } }
    rule.onNodeWithText("SQLite").performClick()
    rule.onNodeWithText("Next").performClick()
    rule.onNodeWithText("Linux").performClick()
    tester.emulateSavedInstanceStateRestore()
    rule.onNodeWithText("2 of 2").assertExists()
    rule.onNodeWithText("Linux").assertIsSelected()
    rule.onNodeWithText("Back").performClick()
    rule.onNodeWithText("SQLite").assertIsSelected()
  }

  @Test fun answeredQuestionsCollapseToTheirResultLine() { // INTER-08
    themed {
      ThreadView(state(
        QuestionItem(2, 1, listOf(single), QuestionState.Answered("SQLite")), QuestionItem(3, 2, listOf(single), QuestionState.Skipped), QuestionItem(4, 3, listOf(single), QuestionState.Expired),
      ), cb(Rec()))
    }
    rule.onNodeWithText("You answered: SQLite").assertExists()
    rule.onNodeWithText("Skipped").assertExists()
    rule.onNodeWithText("No answer was given").assertExists()
  }

  // ====================================================================== the thread

  private fun longThread(n: Int) = (0 until n).flatMap { listOf(UserItem(it * 2L + 1, "question $it", it), AiItem(it * 2L + 2, "answer $it")) }

  @Test fun aLongChatDrawsOnlyWhatIsOnScreenAndShowEarlierAddsTheRest() { // CHAT-47
    val r = Rec()
    val items = longThread(400)
    themed { ThreadView(ChatUiState(chatId = "c", items = items, visibleFrom = items.size - 60), cb(r)) }
    rule.onNodeWithTag("thread").performScrollToIndex(0)
    rule.onNodeWithText("Show earlier (740)").assertExists()
    rule.onNodeWithTag("thread").performScrollToIndex(60)
    val drawn = rule.onAllNodesWithText("answer ", substring = true).fetchSemanticsNodes().size
    assertTrue("lazy list drew $drawn answers", drawn in 1..40)
    rule.onNodeWithText("answer 399").assertExists()
    rule.onNodeWithText("answer 100").assertDoesNotExist()
    rule.onNodeWithTag("thread").performScrollToIndex(0)
    rule.onNodeWithText("Show earlier (740)").performClick()
    assertEquals(1, r.earlier)
  }

  @Test fun theThreadFollowsNewContentUntilThePersonScrollsUpThenAButtonJumpsBack() { // CHAT-01
    var st by mutableStateOf(ChatUiState(chatId = "c", items = longThread(30)))
    themed { ThreadView(st, cb(Rec())) }
    rule.onNodeWithText("answer 29").assertIsDisplayed()
    rule.runOnIdle { st = st.copy(items = st.items + UserItem(1001, "question new", 30) + AiItem(1002, "answer new")) }
    rule.waitForIdle()
    rule.onNodeWithText("answer new").assertIsDisplayed()
    rule.onNodeWithContentDescription("Jump to the newest").assertDoesNotExist()
    // the person scrolls up
    rule.onNodeWithTag("thread").performTouchInput { swipeDown(); swipeDown() }
    rule.waitForIdle()
    rule.runOnIdle { st = st.copy(items = st.items + AiItem(1003, "answer later")) }
    rule.waitForIdle()
    rule.onNodeWithText("answer later").assertDoesNotExist() // not dragged down
    rule.onNodeWithContentDescription("Jump to the newest").performClick()
    rule.waitForIdle()
    rule.onNodeWithText("answer later").assertIsDisplayed()
  }

  @Config(qualifiers = "w1200dp-h800dp-xhdpi")
  @Test fun theColumnIsCentredAndNeverWiderThan760dpOnABigScreen() { // CHAT-01
    themed { ThreadView(state(UserItem(1, "q", 0), AiItem(2, "a")), cb(Rec())) }
    val w = rule.onNodeWithTag("thread").getBoundsInRoot().width
    assertTrue("thread was $w wide", w <= 760.dp)
  }

  @Test fun aLoadingChatShowsPlaceholderRowsAndAGoneChatAnErrorRow() { // CHAT-45
    themed { ThreadView(ChatUiState(chatId = "c", loading = true), cb(Rec())) }
    rule.onNodeWithContentDescription("Loading the chat").assertExists()
  }

  @Test fun theGoneChatIsAnErrorRowWithoutTryAgain() { // CHAT-45
    themed { ThreadView(ChatUiState(chatId = "c", items = listOf(ErrorItem(1, "That chat is gone.", retry = false))), cb(Rec())) }
    rule.onNodeWithText("That chat is gone.").assertExists()
    rule.onNodeWithText("Try again").assertDoesNotExist()
  }

  @Test fun rolesAndStatesAreAnnouncedForTalkBack() { // CHAT-44
    themed { ThreadView(state(UserItem(1, "hello", 0), twoCommands, AiItem(3, "world", live = true), busy = true), cb(Rec())) }
    rule.onNodeWithContentDescription("You: hello").assertExists()
    rule.onNode(hasContentDescription("Ran 2 commands") and SemanticsMatcher.expectValue(SemanticsProperties.StateDescription, "Collapsed")).assertExists()
    rule.onNode(hasContentDescription("Zohara Agent") and SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite)).assertExists()
  }

  // ====================================================================== menus and sheets

  @Test fun theChatMenuHasContextCopyPinTerminalAndDelete() { // NAV-17, CHAT-26, CTX-01
    val calls = mutableListOf<String>()
    val a = ChatMenuActions({ calls += "context" }, { calls += "copy" }, { calls += "pin" }, { calls += "terminal" }, { calls += "delete" })
    var open by mutableStateOf(true)
    themed { ChatMenu(open, { open = false }, pinned = false, a = a) }
    for (t in listOf("Context", "Copy conversation", "Pin chat", "Terminal", "Delete chat")) rule.onNodeWithText(t).assertExists()
    rule.onNodeWithText("Copy conversation").performClick()
    assertEquals(listOf("copy"), calls)
  }

  @Test fun theMenuHasNoTerminalForPeopleWhoHaveNone() { // NAV-17
    themed { ChatMenu(true, {}, pinned = true, a = ChatMenuActions({}, {}, {}, null, {})) }
    rule.onNodeWithText("Terminal").assertDoesNotExist()
    rule.onNodeWithText("Unpin chat").assertExists()
  }

  @Test fun theContextSheetRefreshesCompactsAndOffersPinOnlyOnWideScreens() { // CTX-04..08
    val calls = mutableListOf<String>()
    val view = ContextView(Transcript.context(listOf(UserItem(1, "q", 0), AiItem(2, "a")), "claude-opus-5-5"), Usage(100, 20))
    themed { ContextSheetContent(view, { calls += "refresh" }, { calls += "compact" }, null) }
    rule.onNodeWithText("Pin to the side").assertDoesNotExist()
    rule.onNodeWithText("Refresh").assertHeightIsAtLeast(48.dp).performClick()
    rule.onNodeWithText("Compact session").performClick()
    assertEquals(listOf("refresh", "compact"), calls)
  }

  @Test fun theContextSheetCanBePinnedAndThePanelUnpinned() { // CTX-08, CTX-11
    val calls = mutableListOf<String>()
    val view = ContextView(Transcript.context(listOf(UserItem(1, "q", 0)), "claude-opus-5-5"), null)
    themed { ContextSheetContent(view, {}, {}, { calls += "pin" }) }
    rule.onNodeWithText("Keep this panel open next to the chat.").assertExists()
    rule.onNodeWithText("Pin to the side").performClick()
    assertEquals(listOf("pin"), calls)
  }

  @Test fun thePinnedPanelHasUnpinRefreshAndCompact() { // CTX-11, CTX-07
    val calls = mutableListOf<String>()
    val view = ContextView(Transcript.context(listOf(UserItem(1, "q", 0)), "claude-opus-5-5"), null)
    themed { ContextPanel(view, { calls += "unpin" }, { calls += "refresh" }, { calls += "compact" }) }
    rule.onNodeWithText("Unpin").performClick()
    rule.onNodeWithText("Refresh").performClick()
    assertEquals(listOf("unpin", "refresh"), calls)
  }

  @Test fun theModeSheetListsTheThreeModesAndMarksTheCurrentOne() { // PERM-01..03
    val picked = mutableListOf<PermMode>()
    themed { ModeSheetContent(PermMode.Auto, true) { picked += it } }
    for (m in PermMode.entries) rule.onNodeWithText(m.label).assertExists()
    rule.onNodeWithText(PermMode.Auto.label).assertIsSelected()
    rule.onNodeWithText(PermMode.Manual.label).performClick()
    assertEquals(listOf(PermMode.Manual), picked)
  }

  @Test fun bypassIsNotOfferedWhenNotAllowed() { // PERM-03
    themed { ModeSheetContent(PermMode.Manual, false) {} }
    rule.onNodeWithText(PermMode.Bypass.label).assertDoesNotExist()
  }
}
