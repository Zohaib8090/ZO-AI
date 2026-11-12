package org.zohara.zoai.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import org.zohara.zoai.core.design.EmptyBox
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ArtifactItem
import org.zohara.zoai.nav.LocalNav

/**
 * Artifacts page. Parity rows: ART-01, 02, 03, 05, 08. The service lists up to 100 items (code it showed, files and screenshots it made on the
 * server). Text opens in a viewer sheet (monospace, scrolls both ways, Copy); a file or screenshot has no reachable bytes on the phone, so the
 * sheet shows what is known (path, time) and the chat it came from can be opened.
 */
@Composable
fun ArtifactsScreen() {
  val hub = rememberPagesHub()
  val holder = rememberHolder("pages-artifacts") { ArtifactsHolder(hub.api, it) }
  val state by holder.state.collectAsState()
  val nav = LocalNav.current
  val toast = rememberToastLambda()
  val clipboard = LocalClipboardManager.current
  RefreshWhileVisible(null) { holder.refresh() }
  ArtifactsContent(
    state, remember(state) { PagesClock(System.currentTimeMillis()) }, onRefresh = holder::refresh,
    onCopy = { clipboard.setText(AnnotatedString(it)); toast("Copied") }, onOpenChat = { nav.openChat(it) },
  )
}

fun artifactIcon(kind: String) = when (kind) {
  "code" -> ZoIcons.Terminal
  "screenshot" -> ZoIcons.Image
  else -> ZoIcons.File
}

@Composable
fun ArtifactsContent(
  state: Loadable<List<ArtifactItem>>,
  clock: PagesClock,
  onRefresh: () -> Unit,
  onCopy: (String) -> Unit,
  onOpenChat: (String) -> Unit,
  startWith: ArtifactItem? = null,
) {
  var shown by remember { mutableStateOf(startWith) }
  ListPage(
    state = state,
    title = "Artifacts",
    onRefresh = onRefresh,
    empty = { EmptyBox(PagesFormat.ARTIFACTS_EMPTY_TITLE, subtitle = PagesFormat.ARTIFACTS_EMPTY_TEXT, icon = ZoIcons.Folder) },
  ) { a ->
    PageCard(artifactIcon(a.kind), a.title.ifBlank { a.path.ifBlank { "Untitled" } }, meta = PagesFormat.artifactSubline(a, clock.now, clock.zone), onClick = { shown = a })
  }
  shown?.let { ArtifactSheet(it, onCopy, onOpenChat) { shown = null } }
}

/** ART-03: "Show the code" for text; the path (or a note) for the rest. */
@Composable
fun ArtifactSheet(a: ArtifactItem, onCopy: (String) -> Unit, onOpenChat: (String) -> Unit, onDismiss: () -> Unit) {
  val c = ZoTheme.colors
  val body = PagesFormat.artifactBody(a)
  ZoBottomSheet(
    title = a.title.ifBlank { "Untitled" },
    onDismiss = onDismiss,
    footer = {
      SecondaryButton("Close", onDismiss)
      if (body.isNotEmpty()) PrimaryButton("Copy", { onCopy(body) })
    },
  ) {
    if (body.isNotEmpty()) {
      val v = rememberScrollState()
      val h = rememberScrollState()
      Text(
        body, style = ZoTheme.type.code, color = c.codeFg, softWrap = false,
        modifier = Modifier.fillMaxWidth().heightIn(max = 380.dp).clip(ZoTheme.shapes.code).background(c.input).verticalScroll(v).horizontalScroll(h).padding(12.dp),
      )
    }
    if (a.text.isNullOrEmpty()) {
      Text(
        if (a.kind == "screenshot") "This picture is stored on the server, not on this phone." else "This file is stored on the server, not on this phone.",
        style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 10.dp),
      )
    }
    if (a.chat.isNotBlank()) {
      Column(Modifier.padding(top = 14.dp)) { PillButton("Open the chat it came from", { onDismiss(); onOpenChat(a.chat) }) }
    }
  }
}
