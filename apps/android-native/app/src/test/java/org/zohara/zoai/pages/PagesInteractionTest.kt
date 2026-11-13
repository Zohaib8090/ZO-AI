package org.zohara.zoai.pages

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasScrollAction
import androidx.compose.ui.test.performScrollToNode
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.longClick
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.onAllNodesWithText
import java.util.Locale
import java.util.TimeZone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.net.ActivityItem
import org.zohara.zoai.core.net.ChatSummary
import org.zohara.zoai.core.net.ScheduledTask
import org.zohara.zoai.core.store.RunOn

/** Taps and typing on the stateless page contents: what the person can do on each page (Robolectric, no device). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h860dp-xhdpi")
class PagesInteractionTest {
  @get:Rule val rule = createComposeRule()
  private val clock = PagesClock(1_794_509_131_000L, Locale.US, TimeZone.getTimeZone("UTC"))

  /** the i-th text field on screen (the placeholder text is a separate node, so it can not take the input) */
  private fun field(i: Int) = rule.onAllNodes(hasSetTextAction())[i]

  private fun themed(content: @Composable () -> Unit) = rule.setContent { ZoAiTheme(true) { Box(Modifier.fillMaxSize()) { content() } } }

  // ------------------------------------------------------------ chats
  @Test fun tapOpensTheChat() {
    var opened: String? = null
    themed { ChatsContent(Loadable(listOf(ChatSummary("c1", false, "Alpha", 1))), clock, {}, { opened = it.id }) { emptyList() } }
    rule.onNodeWithText("Alpha").performClick()
    assertEquals("c1", opened)
  }

  @Test fun longPressShowsTheRowMenu() {
    var deleted = false
    val actions = listOf(MenuAction("Pin this chat", org.zohara.zoai.core.design.ZoIcons.Pin) {}, MenuAction("Delete this chat", org.zohara.zoai.core.design.ZoIcons.Trash, danger = true) { deleted = true })
    themed { ChatsContent(Loadable(listOf(ChatSummary("c1", false, "Alpha", 1))), clock, {}, {}) { actions } }
    rule.onNodeWithText("Alpha").performTouchInput { longClick() }
    rule.onNodeWithText("Pin this chat").assertExists()
    rule.onNodeWithText("Delete this chat").performClick()
    assertTrue(deleted)
  }

  @Test fun moreButtonShowsTheSameMenu() {
    themed { ChatsContent(Loadable(listOf(ChatSummary("c1", false, "Alpha", 1))), clock, {}, {}) { listOf(MenuAction("Share conversation", org.zohara.zoai.core.design.ZoIcons.Send) {}) } }
    rule.onNodeWithContentDescription("More for this chat").performClick()
    rule.onNodeWithText("Share conversation").assertExists()
  }

  @Test fun emptyAndErrorStates() {
    themed { ChatsContent(Loadable(emptyList<ChatSummary>()), clock, {}, {}) { emptyList() } }
    rule.onNodeWithText("No chats yet").assertExists()
    rule.onNodeWithText("Start one with New.").assertExists()
  }

  @Test fun errorRowOffersTryAgain() {
    var retried = false
    themed { ChatsContent(Loadable<List<ChatSummary>>(error = "Offline. Retrying."), clock, { retried = true }, {}) { emptyList() } }
    rule.onNodeWithText("Offline. Retrying.").assertExists()
    rule.onNodeWithText("Try again").performClick()
    assertTrue(retried)
  }

  @Test fun theSixtyChatNoteShowsOnlyAtSixty() {
    themed { ChatsContent(Loadable((1..60).map { ChatSummary("x$it", false, "C$it", it.toLong()) }), clock, {}, {}) { emptyList() } }
    rule.onNode(hasScrollAction()).performScrollToNode(hasText("Older chats can be found with Search"))
    rule.onNodeWithText("Older chats can be found with Search").assertExists()
  }

  // ------------------------------------------------------------ sidebar
  @Test fun sidebarShowsLabelsOnlyWhenThereAreRows() {
    themed { SidebarChatListsContent(PagesFormat.sections(listOf(ChatSummary("a", false, "Only recent", 1))), null, {}) { emptyList() } }
    rule.onNodeWithText("Recents").assertExists()
    rule.onAllNodesWithText("Pinned").let { assertEquals(0, it.fetchSemanticsNodes().size) }
  }

  @Test fun sidebarRowOpens() {
    var opened: String? = null
    themed { SidebarChatListsContent(PagesFormat.sections(listOf(ChatSummary("a", true, "Pinned one", 1))), "a", { opened = it.id }) { emptyList() } }
    rule.onNodeWithText("Pinned").assertExists()
    rule.onNodeWithText("Pinned one").performClick()
    assertEquals("a", opened)
  }

  // ------------------------------------------------------------ search
  @Test fun typingSendsTheText() {
    var typed = ""
    themed { SearchContent(SearchUi(), null, clock, { typed = it }, {}) }
    field(0).performTextInput("fake")
    assertEquals("fake", typed)
  }

  @Test fun emptyBoxWithNoChatsSaysSo() {
    themed { SearchContent(SearchUi(), emptyList(), clock, {}, {}) }
    rule.onNodeWithText("No chats yet").assertExists()
    rule.onNodeWithText("Start one and it will show up here.").assertExists()
  }

  @Test fun noHitsSaysNothingFound() {
    themed { SearchContent(SearchUi("zzz", hits = emptyList()), emptyList(), clock, {}, {}) }
    rule.onNodeWithText("Nothing found").assertExists()
    rule.onNodeWithText("Try other words.").assertExists()
  }

  // ------------------------------------------------------------ projects
  @Test fun createProjectIsDisabledUntilThereIsAName() {
    var got: Pair<String, String>? = null
    themed { ProjectsContent(Loadable(emptyList()), {}, { n, g -> got = n to g; null }, startWithSheet = true) }
    rule.onNodeWithText("Create project").assertIsNotEnabled()
    field(0).performTextInput("Demo")
    field(1).performTextInput("g")
    rule.onNodeWithText("Create project").assertIsEnabled().performClick()
    rule.waitForIdle()
    assertEquals("Demo" to "g", got)
  }

  @Test fun createProjectKeepsTheSheetAndShowsTheServersError() {
    themed { ProjectsContent(Loadable(emptyList()), {}, { _, _ -> "That project already exists." }, startWithSheet = true) }
    field(0).performTextInput("Demo")
    rule.onNodeWithText("Create project").performClick()
    rule.waitForIdle()
    rule.onNodeWithText("That project already exists.").assertExists()
    // the sheet is still open: its button is still there
    rule.onNodeWithText("Create project").assertExists()
  }

  @Test fun aProjectCardOpensItsSheet() {
    val p = PagesFormat.Project("p1", "Thesis", "Finish chapter 3", "2 memory notes", "")
    themed { ProjectsContent(Loadable(listOf(p)), {}, { _, _ -> null }) }
    rule.onNodeWithText("Thesis").performClick()
    assertEquals(2, rule.onAllNodesWithText("Finish chapter 3", substring = true).fetchSemanticsNodes().size)
    rule.onNodeWithText("Instructions, context files, project memory and chats inside a project are kept in the desktop app.").assertExists()
  }

  @Test fun emptyProjects() {
    themed { ProjectsContent(Loadable(emptyList()), {}, { _, _ -> null }) }
    rule.onNodeWithText("No projects yet").assertExists()
    rule.onNodeWithText(PagesFormat.PROJECTS_EMPTY_TEXT).assertExists()
  }

  // ------------------------------------------------------------ artifacts
  @Test fun codeOpensInTheViewerAndCopies() {
    var copied: String? = null
    val a = org.zohara.zoai.core.net.ArtifactItem("a", "code", "main.rs", "", "fn main() {}", "c9", 1)
    var chat: String? = null
    themed { ArtifactsContent(Loadable(listOf(a)), clock, {}, { copied = it }, { chat = it }) }
    rule.onNodeWithText("main.rs").performClick()
    rule.onNodeWithText("fn main() {}").assertExists()
    rule.onNodeWithText("Copy").performClick()
    assertEquals("fn main() {}", copied)
    rule.onNodeWithText("Open the chat it came from").performClick()
    rule.waitForIdle()
    assertEquals("c9", chat)
  }

  @Test fun aFileShowsItsPathAndWhereItLives() {
    val a = org.zohara.zoai.core.net.ArtifactItem("a", "file", "report.md", "/srv/zo/report.md", null, "", 1)
    themed { ArtifactsContent(Loadable(listOf(a)), clock, {}, {}, {}, startWith = a) }
    rule.onAllNodesWithText("/srv/zo/report.md").let { assertTrue(it.fetchSemanticsNodes().isNotEmpty()) }
    rule.onNodeWithText("This file is stored on the server, not on this phone.").assertExists()
  }

  // ------------------------------------------------------------ scheduled
  @Test fun cancelAsksFirst() {
    var cancelled: String? = null
    val t = ScheduledTask("t1", "Disk", "check disk", 1, "daily", true)
    themed { ScheduledContent(Loadable(listOf(t)), clock, {}, { cancelled = it }, { null }) }
    rule.onNodeWithContentDescription("Cancel this task").performClick()
    rule.onNodeWithText("Cancel this task?").assertExists()
    rule.onNodeWithText("It will not run again.").assertExists()
    assertEquals(null, cancelled)
    rule.onNodeWithText("Cancel task").performClick()
    rule.waitForIdle()
    assertEquals("t1", cancelled)
  }

  @Test fun keepingTheTaskCancelsNothing() {
    var cancelled: String? = null
    val t = ScheduledTask("t1", "Disk", "check disk", 1, "daily", true)
    themed { ScheduledContent(Loadable(listOf(t)), clock, {}, { cancelled = it }, { null }, startWithConfirm = t) }
    rule.onNodeWithText("Keep it").performClick()
    rule.waitForIdle()
    assertEquals(null, cancelled)
  }

  @Test fun newTaskSendsTheFormWithTheOwnIntervalWinning() {
    var form: TaskForm? = null
    themed { ScheduledContent(Loadable(emptyList()), clock, {}, {}, { form = it; null }, startWithSheet = true) }
    rule.onNodeWithText("Create task").assertIsNotEnabled()
    field(0).performTextInput("check disk")
    field(1).performTextInput("in 2 hours")
    rule.onNodeWithText("Every day").performClick()
    field(2).performTextInput("every 90 minutes")
    rule.onNodeWithText("Create task").assertIsEnabled().performClick()
    rule.waitForIdle()
    assertEquals(TaskForm("check disk", "in 2 hours", "daily", "every 90 minutes"), form)
    assertEquals("every 90 minutes", PagesFormat.repeatValue(form!!.repeatChoice, form!!.ownInterval))
  }

  @Test fun emptyScheduled() {
    themed { ScheduledContent(Loadable(emptyList()), clock, {}, {}, { null }) }
    rule.onNodeWithText("Nothing is scheduled").assertExists()
  }

  // ------------------------------------------------------------ activity
  private val ui = ActivityUi(
    items = listOf(ActivityItem(1, "answer", "Hello", "m1", "web", "c1", 100_000, true), ActivityItem(2, "scheduled", "Disk", "", "scheduler", null, 90_000, false), ActivityItem(3, "client", "c")),
    serverNow = 112_000, fetchedAt = 5, loaded = true,
  )

  @Test fun stopAndOpenChat() {
    var stopped: Long? = null
    var chat: String? = null
    themed { ActivityContent(ui, 5, RunOn.Cloud, ActivityActions({ stopped = it.id }, { chat = it })) }
    rule.onNodeWithText("12 s").assertExists()
    rule.onNodeWithText("22 s").assertExists()
    rule.onNodeWithText("Stop").performClick()
    rule.onNodeWithText("Open chat").performClick()
    assertEquals(1L, stopped)
    assertEquals("c1", chat)
    rule.onNodeWithText("Cannot be stopped").assertExists()
    rule.onNodeWithText("1 connection open").assertExists()
  }

  @Test fun theClockTicksWithTheLocalClock() {
    themed { ActivityContent(ui, 5 + 3_000, RunOn.Cloud, ActivityActions({}, {})) }
    rule.onNodeWithText("15 s").assertExists()
    rule.onNodeWithText("25 s").assertExists()
  }

  @Test fun emptyActivityKeepsTheOtherSections() {
    themed { ActivityContent(ActivityUi(loaded = true), 0, RunOn.Cloud, ActivityActions({}, {})) }
    rule.onNodeWithText("Nothing is running").assertExists()
    rule.onNodeWithText("Connected").assertExists()
    rule.onNodeWithText("Not connected yet").assertExists()
    rule.onNodeWithText("Background services").assertExists()
    rule.onNodeWithText("Standby — link a computer in Settings > My computers").assertExists()
  }

  @Test fun onAComputerTheLinkRowsSayWhichOne() {
    themed { ActivityContent(ActivityUi(loaded = true), 0, RunOn("dev-1"), ActivityActions({}, {}), computerName = "Desk") }
    rule.onNodeWithText("Desk (Online and ready)").assertExists()
    rule.onNodeWithText("Connected to Desk").assertExists()
  }

  @Test fun aComputerWithoutAKnownNameIsCalledYourComputerNotItsId() { // LINK-15
    themed { ActivityContent(ActivityUi(loaded = true), 0, RunOn("dev-9f3a"), ActivityActions({}, {})) }
    rule.onNodeWithText("Your computer (Online and ready)").assertExists()
    rule.onNodeWithText("Connected to Your computer").assertExists()
    assertEquals(0, rule.onAllNodesWithText("dev-9f3a", substring = true).fetchSemanticsNodes().size)
  }

  @Test fun theTerminalTargetLineNeverShowsTheDeviceId() { // LINK-15
    themed { TerminalContent(TerminalUi(), RunOn("dev-9f3a"), "", {}, {}, {}, {}, {}) }
    rule.onNodeWithText("Your computer").assertExists()
    assertEquals(0, rule.onAllNodesWithText("dev-9f3a", substring = true).fetchSemanticsNodes().size)
  }

  @Test fun runningNowSheetPartShowsNothingRunning() {
    themed { RunningNowContent(ActivityUi(loaded = true), 0, ActivityActions({}, {})) }
    rule.onNodeWithText("Running now").assertExists()
    rule.onNodeWithText("Nothing is running.").assertExists()
  }

  // ------------------------------------------------------------ terminal
  @Test fun terminalRunsWhatIsTyped() {
    themed { TerminalContent(TerminalUi(), RunOn.Cloud, "", {}, {}, {}, {}, {}) }
    rule.onNodeWithText("Run").assertIsNotEnabled()
    rule.onNodeWithText("Cloud session").assertExists()
    assertEquals(2, rule.onAllNodesWithText("ZO-AI Terminal", substring = true).fetchSemanticsNodes().size)
  }

  @Test fun terminalOnAComputerNamesIt() {
    themed { TerminalContent(TerminalUi(), RunOn("dev-1"), "ls", {}, {}, {}, {}, {}, computerName = "Desk") }
    rule.onNodeWithText("Desk").assertExists()
    rule.onNodeWithText("Run").assertIsEnabled()
  }
}

/** The refresh rules of the lists (LIST-04): on entry, every period while visible, and by pulling down. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h860dp-xhdpi")
class PagesRefreshTest {
  @get:Rule val rule = createComposeRule()

  @Test fun refreshesOnEntryAndEveryPeriodWhileVisible() {
    var n = 0
    rule.mainClock.autoAdvance = false
    rule.setContent { RefreshWhileVisible(60_000) { n++ } }
    rule.mainClock.advanceTimeByFrame()
    assertEquals(1, n)
    rule.mainClock.advanceTimeBy(60_001)
    assertEquals(2, n)
    rule.mainClock.advanceTimeBy(60_000)
    assertEquals(3, n)
  }

  @Test fun withoutAPeriodItRefreshesOnlyOnEntry() {
    var n = 0
    rule.mainClock.autoAdvance = false
    rule.setContent { RefreshWhileVisible(null) { n++ } }
    rule.mainClock.advanceTimeBy(300_000)
    assertEquals(1, n)
  }

  @Test fun pullingDownRefreshes() {
    var pulled = 0
    rule.setContent {
      ZoAiTheme(true) {
        ChatsContent(Loadable((1..30).map { ChatSummary("c$it", false, "Chat $it", it.toLong()) }), PagesClock(1_000_000), { pulled++ }, {}) { emptyList() }
      }
    }
    rule.onNode(hasScrollAction()).performTouchInput { swipeDown(startY = top + 40f, endY = bottom - 40f, durationMillis = 400) }
    rule.waitForIdle()
    assertTrue("pulled $pulled", pulled >= 1)
  }
}
