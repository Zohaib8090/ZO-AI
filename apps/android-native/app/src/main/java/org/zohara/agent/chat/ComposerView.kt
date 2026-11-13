package org.zohara.agent.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.agent.core.design.ZoIcon
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme

/** Everything the composer shows and does; the screen fills it from the session and the pills. */
class ComposerModel(
  val busy: Boolean,
  val questionOpen: Boolean,
  val modelText: String,
  val mode: PermMode,
  val runsOnComputer: String?,
  val restricted: Boolean,
  val limit: Int?,
  val showContext: Boolean,
  /** the sentence about where it runs (home screen, or when a computer is chosen) */
  val showRunNote: Boolean = false,
  val onSend: (String) -> Unit,
  val onStop: () -> Unit,
  val onModel: () -> Unit,
  val onMode: () -> Unit,
  val onContext: () -> Unit,
  /** the Terminal pill (COMP-12): admin accounts on the cloud only */
  val onTerminal: (() -> Unit)? = null,
)

const val PLACEHOLDER = "How can I help you today?"
const val FOOT_NOTE = "Zohara Agent can make mistakes. Check what it did."

fun whereItRuns(computer: String?): String? = if (computer == null)
  "Running in the cloud: chat, web search and memory. Choose My computer to use the tools on your own computer."
else "Running on your computer ($computer): it can use the files and tools there, and asks on the computer before it changes anything."

/** True from 90 percent of the limit (SRV-01): the counter shows. */
fun showCounter(length: Int, limit: Int?): Boolean = limit != null && length >= limit * 9 / 10

/** The composer card with its pills and the foot note (COMP-01..14). [text] is owned by the caller so it survives navigation and rotation. */
@Composable
fun Composer(text: String, onText: (String) -> Unit, m: ComposerModel, modifier: Modifier = Modifier, hasAttachments: Boolean = false) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  val enabled = !m.busy && !m.questionOpen
  val canSend = enabled && (text.isNotBlank() || hasAttachments)
  val send = { if (canSend) m.onSend(text) }
  val body: @Composable () -> Unit = {
  Column(modifier.fillMaxWidth().widthIn(max = 760.dp), horizontalAlignment = Alignment.CenterHorizontally) {
    Column(
      Modifier.fillMaxWidth().shadow(3.dp, ZoTheme.shapes.composer, ambientColor = c.shadow, spotColor = c.shadow)
        .clip(ZoTheme.shapes.composer).background(c.composer).border(1.dp, c.line, ZoTheme.shapes.composer)
        .padding(start = 14.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
    ) {
      ChatHooks.attachmentChips?.invoke()
      Box(Modifier.fillMaxWidth().heightIn(min = 36.dp, max = 220.dp), contentAlignment = Alignment.CenterStart) {
        val fieldExtra: Modifier = ChatHooks.fieldModifier?.invoke() ?: Modifier
        BasicTextField(
          value = text, onValueChange = { v -> onText(ChatHooks.textEdited?.invoke(text, v) ?: v) }, enabled = enabled,
          textStyle = t.message.copy(color = c.text),
          cursorBrush = SolidColor(c.accent),
          keyboardOptions = KeyboardOptions(imeAction = ImeAction.Default),
          modifier = Modifier.fillMaxWidth().then(fieldExtra).testTag("message-field").semantics { contentDescription = "Message to Zohara Agent" }.onPreviewKeyEvent {
            // COMP-03: Enter sends and Shift+Enter is a new line on a hardware keyboard; the on-screen Enter is always a new line
            if (it.type == KeyEventType.KeyDown && it.key == Key.Enter && !it.isShiftPressed) { send(); true } else false
          },
        )
        if (text.isEmpty()) Text(if (m.questionOpen) "Answer the question above first." else PLACEHOLDER, style = t.message, color = c.dim)
      }
      Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        ChatHooks.attachButton?.invoke()
        Spacer(Modifier.weight(1f))
        if (showCounter(text.length, m.limit)) {
          Text("${text.length} / ${m.limit}", style = t.fieldHint, color = if (text.length > (m.limit ?: Int.MAX_VALUE)) c.danger else c.dim, modifier = Modifier.padding(end = 8.dp))
        }
        ChatHooks.micButton?.invoke({ extra -> onText((text + (if (text.isEmpty() || text.endsWith(" ")) "" else " ") + extra)) }, send)
        SendButton(busy = m.busy, canSend = canSend, onSend = send, onStop = m.onStop)
      }
    }
    // pills row (COMP-07): scrolls sideways on a narrow phone
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
      ChatHooks.runOnPill?.invoke()
      Pill(m.modelText, "Model: ${m.modelText}", m.onModel, chevron = true)
      Pill(m.mode.label, "Permission mode: ${m.mode.label}", m.onMode, chevron = true)
      if (m.showContext) Pill("Context", "Context window", m.onContext, icon = ZoIcons.Context)
      m.onTerminal?.let { Pill("Terminal", "Terminal", it, icon = ZoIcons.Terminal) }
    }
    Text(
      FOOT_NOTE, style = t.fieldHint.copy(fontSize = 12.sp), color = c.dimmer, textAlign = TextAlign.Center,
      modifier = Modifier.fillMaxWidth().padding(top = 4.dp, bottom = 2.dp),
    )
    (if (m.showRunNote || m.runsOnComputer != null) whereItRuns(m.runsOnComputer) else null)?.let {
      Text(it, style = t.fieldHint.copy(fontSize = 11.5.sp), color = c.dimmer, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth().padding(start = 8.dp, end = 8.dp, bottom = 4.dp))
    }
  }
  }
  // the media module wraps the card once and shares its controller with the slots inside (attachments, "+", microphone, paste and drop)
  val host = ChatHooks.composerHost
  if (host != null) host(text, onText, send, m.busy, body) else body()
}

@Composable
fun Pill(text: String, description: String, onClick: () -> Unit, modifier: Modifier = Modifier, chevron: Boolean = false, icon: androidx.compose.ui.graphics.vector.ImageVector? = null) {
  val c = ZoTheme.colors
  Row(
    modifier.heightIn(min = 40.dp).clip(ZoTheme.shapes.pill).border(1.dp, c.line, ZoTheme.shapes.pill)
      .clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = description }
      .padding(horizontal = 12.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    if (icon != null) { ZoIcon(icon, size = 15.dp, tint = c.dim); Spacer(Modifier.size(6.dp)) }
    Text(text, style = ZoTheme.type.cardMeta.copy(fontSize = 13.sp), color = c.text3, maxLines = 1)
    if (chevron) { Spacer(Modifier.size(4.dp)); ZoIcon(ZoIcons.Down, size = 14.dp, tint = c.dim) }
  }
}

/** COMP-02: the arrow (teal when there is something to send) turns into a stop square while an answer is written. */
@Composable
fun SendButton(busy: Boolean, canSend: Boolean, onSend: () -> Unit, onStop: () -> Unit) {
  val c = ZoTheme.colors
  val active = busy || canSend
  Box(
    Modifier.size(46.dp).padding(3.dp).clip(CircleShape).background(if (active) c.accent else c.raise)
      .clickable(enabled = active, role = Role.Button) { if (busy) onStop() else onSend() }
      .semantics { contentDescription = if (busy) "Stop" else "Send" },
    contentAlignment = Alignment.Center,
  ) {
    ZoIcon(if (busy) ZoIcons.Stop else ZoIcons.Send, tint = if (active) c.onAccent else c.dimmer, size = 20.dp)
  }
}
