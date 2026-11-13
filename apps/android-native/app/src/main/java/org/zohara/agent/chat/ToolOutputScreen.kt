package org.zohara.agent.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.EmptyBox
import org.zohara.agent.core.design.MonoFamily
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoSpinner
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.nav.SetTopBar

/** What the viewer shows for a group of tool runs: the text of every line, whole. */
fun toolOutputText(g: ToolGroupItem): String = g.lines.joinToString("\n") {
  when (it.kind) { LineKind.Start -> "▸ ${it.text}"; LineKind.Ok -> "✓ ${it.text}"; LineKind.Fail -> "✗ ${it.text}" }
}

/** The server cuts a tool's output at 1,500 characters (CHAT-35). */
fun looksShortened(line: ToolLine): Boolean = line.kind != LineKind.Start && (line.text.length >= 1500 || line.text.endsWith("…"))

/** Full-screen monospace viewer of a tool run (D1). Parity rows: CHAT-34..36. */
@Composable
fun ToolOutputScreen(chatId: String, index: Int) {
  val container = LocalAppContainer.current
  val inst = remember { ChatHub.of(container) }
  val session = remember(chatId) { inst.hub.forId(chatId) }
  val state by session.state.collectAsState()
  val clipboard = LocalClipboardManager.current
  val group = state.items.filterIsInstance<ToolGroupItem>().getOrNull(index)
  SetTopBar("Output") {
    if (group != null) ZoIconButton(ZoIcons.Copy, "Copy the output", { clipboard.setText(AnnotatedString(toolOutputText(group))); inst.env.toast("Copied") })
  }
  val c = ZoTheme.colors
  when {
    group == null && state.loading -> androidx.compose.foundation.layout.Box(Modifier.fillMaxSize(), contentAlignment = androidx.compose.ui.Alignment.Center) { ZoSpinner() }
    group == null -> EmptyBox("Nothing to show", subtitle = "That run is not in this chat any more.")
    else -> ToolOutputBody(group, state.computerName)
  }
}

/** The selectable, scrollable text of a tool run, monospace (CHAT-34), with the notes of CHAT-35 and CHAT-36. */
@Composable
fun ToolOutputBody(group: ToolGroupItem, computerName: String?, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Column(modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp)) {
    SelectionContainer {
      Column(Modifier.fillMaxWidth().background(c.code, ZoTheme.shapes.code).padding(12.dp)) {
        for (l in group.lines) {
          Text(
            when (l.kind) { LineKind.Start -> "▸ "; LineKind.Ok -> "✓ "; LineKind.Fail -> "✗ " } + l.text,
            style = TextStyle(fontFamily = MonoFamily, fontSize = 13.sp, lineHeight = 19.sp),
            color = if (l.kind == LineKind.Fail) c.danger else c.codeFg,
            modifier = Modifier.padding(bottom = 6.dp),
          )
          if (looksShortened(l)) Text("…(shortened by the server)", style = ZoTheme.type.fieldHint, color = c.dim, modifier = Modifier.padding(bottom = 6.dp))
          if (l.kind == LineKind.Start && l.text.contains("screenshot", ignoreCase = true)) {
            Text("screenshot (kept on ${computerName ?: "the computer"})", style = ZoTheme.type.fieldHint, color = c.dim, modifier = Modifier.padding(bottom = 6.dp))
          }
        }
      }
    }
  }
}
