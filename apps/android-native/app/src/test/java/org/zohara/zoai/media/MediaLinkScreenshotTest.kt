package org.zohara.zoai.media

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color as AColor
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Shader
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import java.io.ByteArrayOutputStream
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SheetContent
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.LinkCaps
import org.zohara.zoai.core.net.LinkDevice
import org.zohara.zoai.link.DesktopUi
import org.zohara.zoai.link.LinkModes
import org.zohara.zoai.link.LinkState
import org.zohara.zoai.link.ModeRows
import org.zohara.zoai.link.MyComputersContent
import org.zohara.zoai.link.RunNoteContent
import org.zohara.zoai.link.RunOnChoice
import org.zohara.zoai.link.RunOnPillView
import org.zohara.zoai.link.RunOnRows
import org.zohara.zoai.link.RunningOnLine
import org.zohara.zoai.link.WaitingApprovalRow

/** JVM screenshots of the media and link screens in both themes (`./gradlew recordRoborazziDebug`); the PNGs are in src/test/screenshots/media_* and link_*. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w400dp-h1000dp-xhdpi")
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
class MediaLinkScreenshotTest {
  @get:Rule val rule = createComposeRule()

  private fun thumb(): String {
    val b = Bitmap.createBitmap(160, 120, Bitmap.Config.ARGB_8888)
    val p = Paint().apply { shader = LinearGradient(0f, 0f, 160f, 120f, AColor.rgb(95, 212, 212), AColor.rgb(242, 184, 75), Shader.TileMode.CLAMP) }
    Canvas(b).drawRect(0f, 0f, 160f, 120f, p)
    val o = ByteArrayOutputStream(); b.compress(Bitmap.CompressFormat.JPEG, 70, o)
    return "data:image/jpeg;base64," + java.util.Base64.getEncoder().encodeToString(o.toByteArray())
  }

  private fun shot(name: String, dark: Boolean, content: @Composable () -> Unit) {
    rule.setContent { ZoAiTheme(dark) { Column(Modifier.background(ZoTheme.colors.bg).padding(16.dp).width(380.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) { content() } } }
    rule.onRoot().captureRoboImage("src/test/screenshots/${name}_${if (dark) "dark" else "light"}.png")
  }

  private fun sheet(name: String, dark: Boolean, title: String, footer: (@Composable androidx.compose.foundation.layout.RowScope.() -> Unit)? = null, body: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    rule.setContent { ZoAiTheme(dark) { Column(Modifier.background(ZoTheme.colors.sheet).fillMaxWidth()) { SheetContent(title, onClose = {}, footer = footer, body = body) } } }
    rule.onRoot().captureRoboImage("src/test/screenshots/${name}_${if (dark) "dark" else "light"}.png")
  }

  private fun dev(id: String, name: String, online: Boolean, seen: Long = 0, cc: Boolean = false, web: Boolean = false) = LinkDevice(id, name, online, seen, LinkCaps(cc, web, "0.9.2", "linux"))

  // ---------------------------------------------------------------- media

  @Composable
  private fun Strip() {
    val c = ZoTheme.colors
    val img = ImageItem(1, "holiday.jpg", "image/jpeg", ByteArray(48_000), thumb(), 1600, 1067)
    @Composable fun chip(i: AttItem) = AttachmentChip(i, onRemove = {}, onOpen = {})
    androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
      chip(img)
      chip(ImageItem(2, "second.png", "image/png", ByteArray(10), thumb(), 10, 10))
      chip(TextItem(3, "main.rs", "Attached file: main.rs\nfn main() {}", 5_300, ""))
      chip(TextItem(4, "report.pdf", "x", 120_000, "12 pages"))
      chip(TextItem(5, "project.zip", "x", 30_000, "12 files", zip = true))
      chip(BusyItem(6, "scan.pdf", BusyAs.Text, "Page 3 of 12"))
      chip(LinkItem(7, "https://example.com/post", "example.com", "A very long title of the page that is read"))
      chip(LinkItem(8, "https://news.example.org/a", "news.example.org"))
      chip(LinkItem(-1, "https://typed.example.net/a", "typed.example.net", "", detected = true))
    }
    Row(Modifier.fillMaxWidth().background(c.composer).padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
      ZoIconButton(ZoIcons.Plus, "Add pictures, files or a link", {})
      MicButtonView(listening = false, enabled = true, available = true, onClick = {})
      MicButtonView(listening = true, enabled = true, available = true, onClick = {})
      MicButtonView(listening = false, enabled = false, available = true, onClick = {})
      Text("Listening… hello how are", style = ZoTheme.type.cardMeta, color = c.dim)
    }
  }

  @Test fun stripDark() = shot("media_strip", true) { Strip() }
  @Test fun stripLight() = shot("media_strip", false) { Strip() }

  @Composable
  private fun Bubbles() {
    val t = thumb()
    AttachmentBubbles(listOf(
      BubbleAttachment("image", "holiday.jpg", 48_000, t),
      BubbleAttachment("image", "no-thumb.png", 4_000, null),
      BubbleAttachment("text", "notes.md", 1_200, null, "Attached file: notes.md\n# Notes"),
    ))
    Text("What is in these?", style = ZoTheme.type.message, color = ZoTheme.colors.text)
  }
  @Test fun bubblesDark() = shot("media_bubbles", true) { Bubbles() }
  @Test fun bubblesLight() = shot("media_bubbles", false) { Bubbles() }

  @Test fun menuDark() = sheet("media_menu", true, "Add to your message") { PlusMenuBody(true, true, {}, {}, {}, {}) }
  @Test fun menuLight() = sheet("media_menu", false, "Add to your message") { PlusMenuBody(true, false, {}, {}, {}, {}) }

  @Test fun micRationaleDark() = sheet("media_mic", true, "Use the microphone?", { SecondaryButton("Not now", {}); PrimaryButton("Allow", {}) }) { MicRationaleBody() }
  @Test fun micBlockedLight() = sheet("media_micblocked", false, "The microphone is blocked", { SecondaryButton("Close", {}); PrimaryButton("Open settings", {}) }) { MicBlockedBody() }

  @Test fun linkSheetDark() = sheet("media_addlink", true, "Add a link", { SecondaryButton("Cancel", {}); PrimaryButton("Add", {}) }) { AddLinkBody("example.com/page") {} }
  @Test fun linkSheetLight() = sheet("media_addlink", false, "Add a link", { SecondaryButton("Cancel", {}); PrimaryButton("Add", {}) }) { AddLinkBody("") {} }

  @Test fun previewDark() = sheet("media_preview", true, "holiday.jpg", { SecondaryButton("Close", {}) }) { PreviewBody(PreviewTarget.Picture("holiday.jpg", null, thumb(), 1600, 1067, 48_000)) }
  @Test fun previewLight() = sheet("media_preview", false, "main.rs", { SecondaryButton("Close", {}) }) { PreviewBody(PreviewTarget.TextFile("main.rs", "Attached file: main.rs\nfn main() {\n    println!(\"hello\");\n}")) }

  // ---------------------------------------------------------------- link

  private val now = 1_700_000_100_000L
  private val devices = listOf(dev("a", "Work laptop", true, cc = true, web = true), dev("b", "Home desktop", false, seen = (now - 3 * 3_600_000L) / 1000))

  @Composable
  private fun RunOn(choice: RunOnChoice) {
    RunOnRows(LinkState(devices, ok = true, at = now), choice, {}, {}, now)
  }

  @Test fun runOnDark() = sheet("link_runon", true, "Run on") { RunOn(RunOnChoice("a", "Work laptop")) }
  @Test fun runOnLight() = sheet("link_runon", false, "Run on") { RunOn(RunOnChoice.Cloud) }

  @Composable
  private fun Pills() {
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
      RunOnPillView(false, false, "") {}
      RunOnPillView(true, true, "Work laptop") {}
      RunOnPillView(true, false, "Home desktop") {}
    }
    RunNoteContent(RunOnChoice("b", "Home desktop"), LinkState(devices, ok = true, at = now), {})
    RunNoteContent(RunOnChoice("b", "Home desktop"), LinkState(devices, ok = true, at = now, busy = true), {})
    WaitingApprovalRow("Run: ls -la ~/Documents")
    RunningOnLine("Work laptop")
  }
  @Test fun pillsDark() = shot("link_pills", true) { Pills() }
  @Test fun pillsLight() = shot("link_pills", false) { Pills() }

  @Test fun modeDark() = sheet("link_mode", true, "What ZO-AI may do") { ModeRows(LinkModes.available(true, false), "Bypass", true) {} }
  @Test fun modeLight() = sheet("link_mode", false, "What ZO-AI may do") { ModeRows(LinkModes.available(false, false), "Auto", false) {} }

  @Test fun computersDark() = rule.run {
    setContent { ZoAiTheme(true) { androidx.compose.foundation.layout.Box(Modifier.background(ZoTheme.colors.bg).width(400.dp).height(900.dp)) { MyComputersContent(LinkState(devices, ok = true, at = now), RunOnChoice("a", "Work laptop"), {}, {}, now) } } }
    onRoot().captureRoboImage("src/test/screenshots/link_computers_dark.png")
  }
  @Test fun computersLight() = rule.run {
    setContent { ZoAiTheme(false) { androidx.compose.foundation.layout.Box(Modifier.background(ZoTheme.colors.bg).width(400.dp).height(900.dp)) { MyComputersContent(LinkState(devices, ok = true, at = now), RunOnChoice.Cloud, {}, {}, now) } } }
    onRoot().captureRoboImage("src/test/screenshots/link_computers_light.png")
  }
  @Test fun computersEmptyDark() = rule.run {
    setContent { ZoAiTheme(true) { androidx.compose.foundation.layout.Box(Modifier.background(ZoTheme.colors.bg).width(400.dp).height(700.dp)) { MyComputersContent(LinkState(emptyList(), ok = true, at = now), RunOnChoice.Cloud, {}, {}, now) } } }
    onRoot().captureRoboImage("src/test/screenshots/link_computers_empty_dark.png")
  }
  @Test fun computersErrorLight() = rule.run {
    setContent { ZoAiTheme(false) { androidx.compose.foundation.layout.Box(Modifier.background(ZoTheme.colors.bg).width(400.dp).height(600.dp)) { MyComputersContent(LinkState(emptyList(), ok = false, at = 0), RunOnChoice.Cloud, {}, {}, now) } } }
    onRoot().captureRoboImage("src/test/screenshots/link_computers_error_light.png")
  }
}
