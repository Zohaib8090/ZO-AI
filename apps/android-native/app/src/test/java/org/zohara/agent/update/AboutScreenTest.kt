package org.zohara.agent.update

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.agent.core.design.ZoAgentTheme
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.settings.AboutActions
import org.zohara.agent.settings.AboutContent
import org.zohara.agent.settings.AboutUi

/** Settings > About: the version line, "Check for updates", the result, Download, and the automatic switch; screenshots in both themes (`recordRoborazziDebug` writes settings_about_*.png). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h1200dp-hdpi", application = android.app.Application::class)
class AboutScreenTest {
  @get:Rule val rule = createComposeRule()

  private val latest = LatestRelease("0.2.0", "2026-11-20", "Update checks on every client.\nBug fixes.", "0.1.0", releaseUrl = "https://github.com/o/r/releases/tag/v0.2.0")
  private val available = CheckState.Done(UpdateOutcome.Available("0.1.0", latest, false, "0.1.0"))
  private fun ui(check: CheckState = CheckState.Idle, auto: Boolean = true, play: Boolean = false, server: String? = "0.1.0") = AboutUi("0.1.0", 1, server, check, auto, play)

  private fun show(ui: AboutUi, a: AboutActions = AboutActions(), dark: Boolean = true) = rule.setContent {
    ZoAgentTheme(dark) { Column(Modifier.background(ZoTheme.colors.bg).width(400.dp).padding(horizontal = 16.dp)) { AboutContent(ui, a) } }
  }

  private fun checkButton() = rule.onAllNodes(androidx.compose.ui.test.hasText("Check for updates").and(androidx.compose.ui.test.hasClickAction()))[0]

  @Test fun theVersionLineAndTheCheckButton() {
    var checks = 0
    show(ui(), AboutActions(onCheck = { checks++ }))
    rule.onNodeWithText("App version 0.1.0 (build 1) · server version 0.1.0 · the AI built into Zohara OS").assertExists()
    rule.onNodeWithText("Looks for a newer version of Zohara Agent.").assertExists()
    // the button (a pill) is the one with the click action; the row title has the same words
    checkButton().performClick()
    assertEquals(1, checks)
  }

  @Test fun whileCheckingTheButtonIsOffAndSaysSo() {
    show(ui(CheckState.Checking))
    rule.onNodeWithText("Checking...").assertExists()
    checkButton().assertIsNotEnabled()
  }

  @Test fun aNewVersionShowsItsNotesAndADownloadButton() {
    var downloads = 0
    show(ui(available), AboutActions(onDownload = { downloads++ }))
    rule.onNodeWithText("Version 0.2.0 is available (you have 0.1.0).").assertExists()
    rule.onNodeWithText("Update checks on every client.\nBug fixes.").assertExists()
    rule.onNodeWithText("Download").performClick()
    assertEquals(1, downloads)
  }

  @Test fun aPlayInstallSaysOpenGooglePlayInsteadOfDownload() {
    show(ui(available, play = true))
    rule.onNodeWithText("Open Google Play").assertExists()
    assertTrue(rule.onAllNodesWithText("Download").fetchSemanticsNodes().isEmpty())
  }

  @Test fun anUnsupportedVersionIsToldToUpdate() {
    show(ui(CheckState.Done(UpdateOutcome.Available("0.1.0", latest, true, null))))
    rule.onNodeWithText("This version is no longer supported. Please update.").assertExists()
  }

  @Test fun upToDateHasNoDownloadButton() {
    show(ui(CheckState.Done(UpdateOutcome.UpToDate("0.2.0", latest, null))))
    rule.onNodeWithText("You are up to date (0.2.0).").assertExists()
    assertTrue(rule.onAllNodesWithText("Download").fetchSemanticsNodes().isEmpty())
  }

  @Test fun aFailureAndAnUnknownAnswerAreShownAsSentences() {
    show(ui(CheckState.Failed("Could not check for updates: the server could not be reached.")))
    rule.onNodeWithText("Could not check for updates: the server could not be reached.").assertExists()
  }

  @Test fun theAutomaticSwitchReportsTheNewValue() {
    var last: Boolean? = null
    show(ui(auto = true), AboutActions(onAuto = { last = it }))
    rule.onNodeWithText("Check for updates automatically").assertExists()
    rule.onNodeWithText("Looks about every 12 hours when the phone has a network and the battery is not low, and tells you once per new version. Nothing is downloaded or installed without you.").assertExists()
    rule.onNode(androidx.compose.ui.test.hasContentDescription("Check for updates automatically").or(androidx.compose.ui.test.hasText("Check for updates automatically").and(androidx.compose.ui.test.hasClickAction()))).performClick()
    assertEquals(false, last)
  }

  // ---------------------------------------------------------------- screenshots
  private fun shot(name: String, dark: Boolean, ui: AboutUi) {
    show(ui, dark = dark)
    rule.onRoot().captureRoboImage("src/test/screenshots/settings_about_${name}_${if (dark) "dark" else "light"}.png")
  }

  @Test fun aboutAvailableDark() = shot("available", true, ui(available))
  @Test fun aboutAvailableLight() = shot("available", false, ui(available))
  @Test fun aboutIdleDark() = shot("idle", true, ui())
  @Test fun aboutIdleLight() = shot("idle", false, ui())
  @Test fun aboutPlayDark() = shot("play", true, ui(available, auto = false, play = true))
}
