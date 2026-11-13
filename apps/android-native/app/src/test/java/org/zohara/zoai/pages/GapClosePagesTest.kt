package org.zohara.zoai.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.onAllNodesWithText
import com.github.takahirom.roborazzi.captureRoboImage
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
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ArtifactItem
import org.zohara.zoai.core.net.ScheduledTask

/** Rows PROJ-03 (search projects) and LIST-15 (the search box takes the focus), found by the parity check of 2026-11-13. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h860dp-xhdpi")
class GapClosePagesTest {
  @get:Rule val rule = createComposeRule()
  private val clock = PagesClock(1_794_509_131_000L, Locale.US, TimeZone.getTimeZone("UTC"))

  private fun themed(dark: Boolean = true, content: @Composable () -> Unit) = rule.setContent { ZoAiTheme(dark) { Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg)) { content() } } }

  private val projects = listOf(
    PagesFormat.Project("p1", "Zohara OS 1.2", "Ship the stable release", "3 memory notes", ""),
    PagesFormat.Project("p2", "Thesis", "Write chapter four", "0 memory notes", ""),
    PagesFormat.Project("p3", "Garden", "", "", ""),
  )

  // ---- PROJ-03
  @Test fun theFilterMatchesNameAndGoalIgnoringCase() {
    assertEquals(projects, PagesFormat.filterProjects(projects, "  "))
    assertEquals(listOf("p1"), PagesFormat.filterProjects(projects, "zohara").map { it.id })
    assertEquals(listOf("p2"), PagesFormat.filterProjects(projects, "CHAPTER").map { it.id })
    assertEquals(emptyList<String>(), PagesFormat.filterProjects(projects, "nothing like it").map { it.id })
  }

  @Test fun theProjectsPageHasASearchFieldThatFilters() {
    themed { ProjectsContent(Loadable(projects), {}, { _, _ -> null }) }
    rule.onNodeWithText("Search projects").assertExists()
    rule.onNodeWithText("Thesis").assertExists()
    rule.onAllNodes(hasSetTextAction())[0].performTextInput("zoh")
    rule.onNodeWithText("Zohara OS 1.2").assertExists()
    assertEquals("the others are filtered out", 0, rule.onAllNodesWithText("Thesis").fetchSemanticsNodes().size)
    rule.onAllNodes(hasSetTextAction())[0].performTextInput("zzzz")
    rule.onNodeWithText("Nothing matches that.").assertExists()
  }

  @Test fun noSearchFieldWhenThereAreNoProjects() {
    themed { ProjectsContent(Loadable(emptyList()), {}, { _, _ -> null }) }
    assertEquals(0, rule.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().size)
  }

  @Test fun projectsSearchScreenshotDark() {
    themed(true) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }) }
    rule.onAllNodes(hasSetTextAction())[0].performTextInput("zoh")
    rule.waitForIdle()
    rule.onRoot().captureRoboImage("src/test/screenshots/pages_projects_search_dark.png")
  }

  @Test fun projectsSearchNoMatchScreenshotLight() {
    themed(false) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }) }
    rule.onAllNodes(hasSetTextAction())[0].performTextInput("zzzz")
    rule.waitForIdle()
    rule.onRoot().captureRoboImage("src/test/screenshots/pages_projects_search_none_light.png")
  }

  // ---- LIST-15
  @Test fun theSearchBoxHasTheFocusWhenThePageOpens() {
    themed { SearchContent(SearchUi(), emptyList(), clock, {}, {}) }
    rule.waitForIdle()
    rule.onAllNodes(hasSetTextAction())[0].assertIsFocused()
  }
}

/** NAV-23: from 840 dp a list shows the opened item beside it (Projects, Artifacts, Scheduled; Chats and Settings are in the whole-app test). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w900dp-h700dp-xhdpi")
class TwoPaneListsTest {
  @get:Rule val rule = createComposeRule()
  private val now = 1_794_509_131_000L
  private val clock = PagesClock(now, Locale.US, TimeZone.getTimeZone("UTC"))
  private val projects = listOf(
    PagesFormat.Project("p1", "Zohara OS 1.2", "Ship the stable release", "3 memory notes", ""),
    PagesFormat.Project("p2", "Thesis", "Write chapter four", "0 memory notes", ""),
  )
  private val artifacts = listOf(
    ArtifactItem("a1", "code", "main.rs", "", "fn main() {\n    println!(\"hello\");\n}\n", "c1", now - 300_000),
    ArtifactItem("a2", "file", "report.md", "/srv/zo/report.md", null, "c2", now - 3 * 3_600_000),
  )
  private val tasks = listOf(ScheduledTask("t1", "Disk check", "check my disk space and tell me if it is above 90%", 1_794_555_600_000L, "daily", true))

  private fun shot(name: String, dark: Boolean, content: @Composable () -> Unit) {
    val d = mutableStateOf(dark)
    rule.setContent { ZoAiTheme(d.value) { Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg)) { content() } } }
    rule.waitForIdle()
    rule.onRoot().captureRoboImage("src/test/screenshots/pages_twopane_${name}_${if (dark) "dark" else "light"}.png")
  }

  @Test fun projectsOpenBesideTheList() {
    rule.setContent { ZoAiTheme(true) { Box(Modifier.fillMaxSize()) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }) } } }
    rule.onNodeWithText("Pick a project to see it here").assertExists()
    rule.onNodeWithText("Thesis").performClick()
    assertTrue("the goal is also shown on the right", rule.onAllNodesWithText("Write chapter four", substring = true).fetchSemanticsNodes().size >= 2)
    assertTrue("still the list", rule.onAllNodesWithText("Zohara OS 1.2").fetchSemanticsNodes().isNotEmpty())
    assertEquals("no sheet: no Close button", 0, rule.onAllNodesWithText("Close").fetchSemanticsNodes().size)
  }

  @Test fun projectsDark() = shot("projects", true) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }, startWithProject = projects[0]) }
  @Test fun projectsLight() = shot("projects", false) { ProjectsContent(Loadable(projects), {}, { _, _ -> null }, startWithProject = projects[0]) }
  @Test fun artifactsDark() = shot("artifacts", true) { ArtifactsContent(Loadable(artifacts), clock, {}, {}, {}, startWith = artifacts[0]) }
  @Test fun artifactsLight() = shot("artifacts", false) { ArtifactsContent(Loadable(artifacts), clock, {}, {}, {}, startWith = artifacts[0]) }

  @Test fun artifactsOpenBesideTheListWithCopy() {
    var copied: String? = null
    rule.setContent { ZoAiTheme(true) { Box(Modifier.fillMaxSize()) { ArtifactsContent(Loadable(artifacts), clock, {}, { copied = it }, {}) } } }
    rule.onNodeWithText("Pick an item to see it here").assertExists()
    rule.onNodeWithText("main.rs").performClick()
    rule.onNodeWithText("Copy").performClick()
    assertEquals("fn main() {\n    println!(\"hello\");\n}\n", copied)
  }

  @Test fun scheduledTaskIsDescribedBesideTheList() {
    rule.setContent { ZoAiTheme(true) { Box(Modifier.fillMaxSize()) { ScheduledContent(Loadable(tasks), clock, {}, {}, { null }) } } }
    rule.onNodeWithText("Pick a task to see it here").assertExists()
    rule.onNodeWithText("Disk check").performClick()
    assertTrue(rule.onAllNodesWithText("check my disk space and tell me if it is above 90%", substring = true).fetchSemanticsNodes().size >= 2)
    rule.onNodeWithText("Cancel this task").assertExists()
  }
}
