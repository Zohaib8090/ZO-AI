package org.zohara.agent.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.store.RunOn

/**
 * Terminal command box (admin, cloud only; D1). Parity rows: TERM-01..04, COMP-12 (the pill and the menu entry that open it are in the chat
 * module and call `LocalNav.current.terminal()`), LINK-12. Runs `terminal_run` on the cloud server; on My computer nothing is sent.
 */
@Composable
fun TerminalScreen() {
  val hub = rememberPagesHub()
  val holder = rememberHolder("pages-terminal") { TerminalHolder(hub.api, it) }
  val ui by holder.ui.collectAsState()
  val c = LocalAppContainer.current
  val runOn by c.prefs.runOn.collectAsState(initial = RunOn.Cloud)
  var text by remember { mutableStateOf("") }
  val computerName = rememberComputerName()
  TerminalContent(
    ui, runOn, text, { text = it },
    onRun = { holder.run(text, !runOn.isCloud); text = "" },
    onClear = holder::clear,
    onUp = { holder.historyUp()?.let { text = it } },
    onDown = { holder.historyDown()?.let { text = it } },
    computerName = computerName,
  )
}

@Composable
fun TerminalContent(
  ui: TerminalUi,
  runOn: RunOn,
  text: String,
  onText: (String) -> Unit,
  onRun: () -> Unit,
  onClear: () -> Unit,
  onUp: () -> Unit,
  onDown: () -> Unit,
  computerName: String? = null,
) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  val scroll = rememberScrollState()
  LaunchedEffect(ui.log) { scroll.animateScrollTo(scroll.maxValue) }
  Column(Modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    Row(verticalAlignment = Alignment.CenterVertically) {
      Text("Zohara Agent Terminal", style = t.sheetTitle, color = c.text, modifier = Modifier.weight(1f))
      SecondaryButton("Clear", onClear)
    }
    // TERM-01: the target line, a blue dot for the cloud session and a teal one for a computer
    Row(verticalAlignment = Alignment.CenterVertically) {
      Box(Modifier.size(8.dp).clip(CircleShape).background(if (runOn.isCloud) c.blue else c.accent))
      Text(if (runOn.isCloud) "Cloud session" else computerName ?: PagesFormat.COMPUTER_FALLBACK, style = t.cardMeta, color = c.dim, modifier = Modifier.padding(start = 8.dp))
    }
    Box(Modifier.weight(1f).fillMaxWidth().clip(ZoTheme.shapes.code).background(c.input).border(1.dp, c.border, ZoTheme.shapes.code)) {
      // no wrapping: columns of command output stay aligned and the log scrolls sideways
      Text(ui.log, style = t.code, color = c.codeFg, softWrap = false, modifier = Modifier.fillMaxSize().verticalScroll(scroll).horizontalScroll(rememberScrollState()).padding(12.dp))
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
      Text("$", style = t.code, color = c.accent)
      Box(
        Modifier.weight(1f).clip(ZoTheme.shapes.field).background(c.input).border(1.dp, c.border, ZoTheme.shapes.field).padding(horizontal = 12.dp, vertical = 11.dp)
          .onPreviewKeyEvent { e ->
            if (e.type == KeyEventType.KeyDown && e.key == Key.DirectionUp) { onUp(); true }
            else if (e.type == KeyEventType.KeyDown && e.key == Key.DirectionDown) { onDown(); true }
            else false
          },
      ) {
        BasicTextField(
          text, onText, singleLine = true, textStyle = t.code.copy(color = c.text), cursorBrush = SolidColor(c.accent),
          keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send, autoCorrectEnabled = false),
          keyboardActions = KeyboardActions(onSend = { onRun() }),
          modifier = Modifier.fillMaxWidth(),
        )
        if (text.isEmpty()) Text(PagesFormat.TERMINAL_PLACEHOLDER, style = t.code, color = c.dimmer)
      }
      ZoIconButton(ZoIcons.Up, "Older command", onUp, size = 36.dp, iconSize = 18.dp)
      ZoIconButton(ZoIcons.Down, "Newer command", onDown, size = 36.dp, iconSize = 18.dp)
      PrimaryButton("Run", onRun, enabled = text.isNotBlank() && !ui.running, loading = ui.running)
    }
  }
}
