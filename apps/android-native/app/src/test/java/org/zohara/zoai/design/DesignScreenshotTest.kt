package org.zohara.zoai.core.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.markdown.MarkdownText

/**
 * JVM screenshot tests of the design components in both themes. `./gradlew recordRoborazziDebug` writes the PNGs to
 * app/src/test/screenshots, `verifyRoborazziDebug` compares against them. The reference palette: dark bg #191919 text #ECECEC accent #5FD4D4,
 * light bg #FAF9F5 text #1F1E1D accent #1FA5A5.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h1100dp-xhdpi")
class DesignScreenshotTest {
  @get:Rule val rule = createComposeRule()

  @Composable
  private fun Gallery() {
    val c = ZoTheme.colors
    Column(Modifier.background(c.bg).padding(16.dp).width(380.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
      Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = androidx.compose.ui.Alignment.CenterVertically) {
        ZoLogo(size = 40.dp)
        Text("ZO-AI", style = ZoTheme.type.wordmark, color = c.text)
        Avatar("Z", badge = 2)
        ZoChip("beta")
        ZoSpinner()
      }
      Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        PrimaryButton("Send", {})
        SecondaryButton("Cancel", {})
        DangerButton("Delete", {}, outlined = true)
        PillButton("Pill", {}, accent = true)
      }
      SectionHeader("General")
      SettingsCard {
        SettingsRow("Theme", subtitle = "Follow the system, dark or light", control = { PillButton("System", {}) })
        SettingsRow("Notifications", subtitle = "When ZO-AI answers", control = { ZoSwitch(true, {}) })
        SettingsRow("Details", divider = false, control = { ZoSwitch(false, {}) })
      }
      ZoTextField("you@example.com", {}, label = "E-mail", hint = "We never share it.")
      ZoTextField("", {}, placeholder = "Search chats")
      Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        listOf(ZoIcons.Plus, ZoIcons.Chat, ZoIcons.Search, ZoIcons.Gear, ZoIcons.Mic, ZoIcons.Send, ZoIcons.Trash, ZoIcons.Pin).forEach { ZoIcon(it) }
      }
      ZoCard { Text("A card with a heading", style = ZoTheme.type.cardName, color = c.text, modifier = Modifier.padding(14.dp)) }
      EmptyBox("Nothing here yet", subtitle = "Start a chat and it shows up.")
      ErrorLine("Signed out or not allowed.")
      MarkdownText("## Heading\nSome **bold**, *italic* and `code` with a [link](https://zohara.org).\n- one\n- two\n\n```rust\nfn main() {}\n```")
    }
  }

  private fun shot(name: String, dark: Boolean) {
    rule.setContent { ZoAiTheme(dark) { Gallery() } }
    rule.onRoot().captureRoboImage("src/test/screenshots/$name.png")
  }

  @Test fun galleryDark() = shot("components_dark", true)
  @Test fun galleryLight() = shot("components_light", false)

  private fun sheet(dark: Boolean) {
    rule.setContent {
      ZoAiTheme(dark) {
        Column(Modifier.background(ZoTheme.colors.sheet).fillMaxWidth()) {
          SheetContent("Add a model", onClose = {}, footer = { SecondaryButton("Cancel", {}); PrimaryButton("Save", {}) }) {
            ZoTextField("sk-...", {}, label = "API key", password = true)
          }
        }
      }
    }
    rule.onRoot().captureRoboImage("src/test/screenshots/sheet_${if (dark) "dark" else "light"}.png")
  }

  @Test fun sheetDark() = sheet(true)
  @Test fun sheetLight() = sheet(false)
}
