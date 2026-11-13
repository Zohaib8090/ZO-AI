package org.zohara.zoai.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.isDialog
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import java.util.Locale
import java.util.TimeZone
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ActivityItem
import org.zohara.zoai.core.net.ArtifactItem
import org.zohara.zoai.core.net.ChatSummary
import org.zohara.zoai.core.net.ClientSeen
import org.zohara.zoai.core.net.ScheduledTask
import org.zohara.zoai.core.net.SearchHit
import org.zohara.zoai.core.store.RunOn

/**
 * JVM screenshots of every page in both themes (pages_<name>_dark.png and _light.png in src/test/screenshots), with a fixed clock, locale and
 * zone so they never change. `recordRoborazziDebug` writes them; the Read tool shows them.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h860dp-xhdpi")
class PagesScreenshotTest {
  @get:Rule val rule = createComposeRule()

  private val now = 1_794_509_131_000L
  private val clock = PagesClock(now, Locale.US, TimeZone.getTimeZone("UTC"))
  private val min = 60_000L

  private fun both(name: String, dialog: Boolean = false, content: @Composable () -> Unit) {
    val dark = mutableStateOf(true)
    rule.setContent { ZoAiTheme(dark.value) { Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg)) { content() } } }
    rule.waitForIdle()
    snap("pages_${name}_dark", dialog)
    dark.value = false
    rule.waitForIdle()
    snap("pages_${name}_light", dialog)
  }

  private fun snap(file: String, dialog: Boolean) {
    val n = if (dialog) rule.onNode(isDialog()) else rule.onRoot()
    n.captureRoboImage("src/test/screenshots/$file.png")
  }

  private val chats = listOf(
    ChatSummary("c1", true, "Plan the Zohara release notes", now - 4 * min),
    ChatSummary("c2", true, "Why is my disk full?", now - 3 * 3_600_000),
    ChatSummary("c3", false, "Recipe for lentil soup with a very long title that has to be cut at the end of the line", now - 26 * 3_600_000),
    ChatSummary("c4", false, "", now - 3 * 86_400_000L),
    ChatSummary("c5", false, "Rust lifetimes explained", now - 9 * 86_400_000L),
  )

  @Test fun chatsList() = both("chats") {
    ChatsContent(Loadable(chats), clock, {}, {}) { emptyList() }
  }

  @Test fun chatsEmpty() = both("chats_empty") { ChatsContent(Loadable(emptyList<ChatSummary>()), clock, {}, {}) { emptyList() } }
  @Test fun chatsLoading() = both("chats_loading") { ChatsContent(Loadable<List<ChatSummary>>(loading = true), clock, {}, {}) { emptyList() } }
  @Test fun chatsError() = both("chats_error") { ChatsContent(Loadable<List<ChatSummary>>(error = "Offline. Retrying."), clock, {}, {}) { emptyList() } }

  @Test fun chatsFullPageNote() = both("chats_sixty") {
    ChatsContent(Loadable((1..60).map { ChatSummary("x$it", false, "Chat $it", now - it * min) }), clock, {}, {}) { emptyList() }
  }

  @Test fun sidebar() = both("sidebar") {
    Column(Modifier.padding(horizontal = 10.dp)) {
      SidebarChatListsContent(PagesFormat.sections(chats + (6..14).map { ChatSummary("r$it", false, "Recent chat $it", now - it * 1000L) }), "r8", {}) { emptyList() }
    }
  }

  @Test fun search() = both("search") {
    SearchContent(
      SearchUi("fake", hits = listOf(SearchHit("c1", "Hello from the fake model. It answered in a few words and then a few more words.", "run it", now - 5 * min), SearchHit("c2", "A fake test", "", now - 2 * 86_400_000L))),
      chats, clock, {}, {},
    )
  }
  @Test fun searchRecents() = both("search_recents") { SearchContent(SearchUi(), chats, clock, {}, {}) }
  @Test fun searchNone() = both("search_none") { SearchContent(SearchUi("zzz", hits = emptyList()), chats, clock, {}, {}) }
  @Test fun searchNoChatsAtAll() = both("search_nochats") { SearchContent(SearchUi(), emptyList(), clock, {}, {}) }

  private val projects = listOf(
    PagesFormat.Project("p1", "Zohara OS 1.2", "Ship the stable release", "3 memory notes", ""),
    PagesFormat.Project("p2", "Thesis", "", "0 memory notes", ""),
  )
  @Test fun projects() = both("projects") { ProjectsContent(Loadable(projects), {}, { _, _ -> null }) }
  @Test fun projectsEmpty() = both("projects_empty") { ProjectsContent(Loadable(emptyList()), {}, { _, _ -> null }) }
  @Test fun projectsRefused() = both("projects_refused") { ProjectsContent(Loadable<List<PagesFormat.Project>>(error = "Projects and scheduled tasks are not available to this account."), {}, { _, _ -> null }) }
  @Test fun newProjectSheet() = both("projects_new", dialog = true) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }, startWithSheet = true) }
  @Test fun projectSheet() = both("projects_open", dialog = true) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }, startWithProject = projects[0]) }

  private val artifacts = listOf(
    ArtifactItem("a1", "code", "main.rs", "", "fn main() {\n    println!(\"hello\");\n}\n", "c1", now - 5 * min),
    ArtifactItem("a2", "file", "report.md", "/srv/zo/report.md", null, "c2", now - 3 * 3_600_000),
    ArtifactItem("a3", "screenshot", "Screenshot of the settings page", "/srv/zo/shots/settings.png", null, "", now - 2 * 86_400_000L),
  )
  @Test fun artifacts() = both("artifacts") { ArtifactsContent(Loadable(artifacts), clock, {}, {}, {}) }
  @Test fun artifactsEmpty() = both("artifacts_empty") { ArtifactsContent(Loadable(emptyList()), clock, {}, {}, {}) }
  @Test fun artifactCodeSheet() = both("artifacts_code", dialog = true) { ArtifactsContent(Loadable(artifacts), clock, {}, {}, {}, startWith = artifacts[0]) }
  @Test fun artifactFileSheet() = both("artifacts_file", dialog = true) { ArtifactsContent(Loadable(artifacts), clock, {}, {}, {}, startWith = artifacts[1]) }

  private val tasks = listOf(
    ScheduledTask("t1", "Disk check", "check my disk space and tell me if it is above 90%", 1_794_555_600_000L, "daily", true),
    ScheduledTask("t2", "", "Water the plants reminder", 1_794_600_000_000L, "once", false),
  )
  @Test fun scheduled() = both("scheduled") { ScheduledContent(Loadable(tasks), clock, {}, {}, { null }) }
  @Test fun scheduledEmpty() = both("scheduled_empty") { ScheduledContent(Loadable(emptyList()), clock, {}, {}, { null }) }
  @Test fun scheduledNewSheet() = both("scheduled_new", dialog = true) { ScheduledContent(Loadable(tasks), clock, {}, {}, { null }, startWithSheet = true) }
  @Test fun scheduledConfirm() = both("scheduled_cancel", dialog = true) { ScheduledContent(Loadable(tasks), clock, {}, {}, { null }, startWithConfirm = tasks[0]) }

  private val activity = ActivityUi(
    items = listOf(
      ActivityItem(1, "answer", "Plan the Zohara release notes", "m1", "web", "c1", now - 95_000, true),
      ActivityItem(2, "scheduled", "Disk check", "", "scheduler", null, now - 12_000, false),
      ActivityItem(3, "memory_job", "Remembering a chat", "", "background", "c2", now - 3_720_000, false),
      ActivityItem(4, "answer", "Waiting for your answer", "", "phone", "c3", now - 40_000, true),
      ActivityItem(5, "client", "web", source = "web"),
    ),
    clientsSeen = listOf(ClientSeen(now - 7_000, "web")), serverNow = now, fetchedAt = now, loaded = true, stopSent = setOf(4),
  )
  @Test fun activity() = both("activity") { ActivityContent(activity, now, RunOn.Cloud, ActivityActions({}, {})) }
  @Test fun activityComputer() = both("activity_computer") { ActivityContent(activity.copy(items = emptyList(), clientsSeen = emptyList()), now, RunOn("d1"), ActivityActions({}, {}), computerName = "Zohaib's desktop") }

  @Test fun runningNow() = both("running_now") {
    Column(Modifier.padding(16.dp)) { RunningNowContent(activity, now, ActivityActions({}, {})) }
  }
  @Test fun runningNowEmpty() = both("running_now_empty") {
    Column(Modifier.padding(16.dp)) { RunningNowContent(ActivityUi(loaded = true), now, ActivityActions({}, {})); Text("Settings", color = ZoTheme.colors.text) }
  }

  @Test fun terminal() = both("terminal") {
    TerminalContent(TerminalUi("ZO-AI Terminal\nConnected. Type a shell command below (e.g. ls, pwd, df -h, uname -a).\n\$ df -h\nFilesystem      Size  Used Avail Use% Mounted on\n/dev/vda1        20G  7.1G   12G  38% /\n"), RunOn.Cloud, "uname", {}, {}, {}, {}, {})
  }
  @Test fun terminalComputer() = both("terminal_computer") {
    TerminalContent(TerminalUi(), RunOn("d1"), "", {}, {}, {}, {}, {}, computerName = "Zohaib's desktop")
  }
}
