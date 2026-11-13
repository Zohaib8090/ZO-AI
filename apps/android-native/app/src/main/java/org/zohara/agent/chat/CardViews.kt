package org.zohara.agent.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.foundation.focusable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.agent.core.design.MonoFamily
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoChip
import org.zohara.agent.core.design.ZoIcon
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTextField
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.QuestionAnswer

private val CardShape = RoundedCornerShape(12.dp)

@Composable
private fun CardFrame(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
  val c = ZoTheme.colors
  Column(modifier.fillMaxWidth().padding(vertical = 4.dp).clip(CardShape).background(c.card).border(1.dp, c.line12, CardShape).padding(14.dp)) { content() }
}

/** The collapsed result line of a card: a tick and the sentence. */
@Composable
private fun ResultLine(text: String, modifier: Modifier = Modifier, ok: Boolean = true) {
  val c = ZoTheme.colors
  Row(modifier.fillMaxWidth().padding(vertical = 4.dp, horizontal = 2.dp), verticalAlignment = Alignment.CenterVertically) {
    ZoIcon(if (ok) ZoIcons.Check else ZoIcons.Close, tint = if (ok) c.accent else c.dim, size = 16.dp)
    Spacer(Modifier.width(8.dp))
    Text(text, style = ZoTheme.type.cardMeta, color = c.text3, maxLines = 3)
  }
}

/** INTER-01, 02, 19: "Zohara Agent wants to: ..." with the whole summary (it scrolls when long) and Allow / Deny; then one collapsed line. */
@Composable
fun ApprovalCard(item: ApprovalItem, onAnswer: (Boolean) -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  if (item.state != ApprovalState.Pending) {
    ResultLine(item.result, modifier, ok = item.state == ApprovalState.Allowed)
    return
  }
  CardFrame(modifier.semantics { contentDescription = "Zohara Agent wants your approval"; liveRegion = LiveRegionMode.Polite }) {
    Text(if (item.onComputer) "Your computer: Zohara Agent wants to:" else "Zohara Agent wants to:", style = ZoTheme.type.popTitle, color = c.text)
    Spacer(Modifier.size(8.dp))
    Box(Modifier.fillMaxWidth().heightIn(max = 160.dp).clip(ZoTheme.shapes.field).background(c.input).padding(10.dp)) {
      SelectionContainer {
        Text(
          item.summary.ifBlank { item.tool },
          style = TextStyle(fontFamily = MonoFamily, fontSize = 12.5.sp, lineHeight = 18.sp), color = c.codeFg,
          modifier = Modifier.verticalScroll(rememberScrollState()),
        )
      }
    }
    Text("Waiting for you", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 8.dp))
    Row(Modifier.fillMaxWidth().padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
      SecondaryButton("Deny", { onAnswer(false) }, Modifier.heightIn(min = 48.dp), enabled = !item.sending)
      PrimaryButton("Allow", { onAnswer(true) }, Modifier.heightIn(min = 48.dp), loading = item.sending)
    }
  }
}

/** INTER-06..10: header chip, question, options (radio or checkboxes) with their descriptions, "Other" with a text box, Skip / Back / Next / Send. */
@Composable
fun QuestionCard(
  item: QuestionItem,
  onSend: (List<QuestionAnswer>) -> Unit,
  onSkip: () -> Unit,
  modifier: Modifier = Modifier,
  initial: QuestionDraft = QuestionDraft(item.questions),
) {
  val c = ZoTheme.colors
  if (item.state != QuestionState.Pending) {
    ResultLine(item.result, modifier, ok = item.state is QuestionState.Answered)
    return
  }
  var draft by rememberSaveable(item.key, stateSaver = QuestionDraft.saver(item.questions)) { mutableStateOf(initial) }
  val spec = item.questions.getOrNull(draft.page) ?: return
  val p = draft.page
  val focus = remember { FocusRequester() }
  LaunchedEffect(item.key) { try { focus.requestFocus() } catch (_: Exception) { } }
  // INTER-09: hardware keyboard. Arrows and Enter work through the focus of the option rows; the number keys pick an option, Esc skips,
  // and an Enter that no row took goes to the next question or sends. A text box takes its own keys first (onKeyEvent runs after the child).
  val keys = Modifier.onKeyEvent { e ->
    if (e.type != KeyEventType.KeyDown) return@onKeyEvent false
    val digit = QuestionLogic.digitOf(e.key)
    when {
      e.key == Key.Escape -> { if (!item.sending) onSkip(); true }
      digit != null && digit - 1 < spec.options.size -> { draft = draft.toggle(p, spec.options[digit - 1].label); true }
      e.key == Key.Enter || e.key == Key.NumPadEnter -> {
        if (!draft.last) { if (draft.currentAnswered) draft = draft.next() }
        else if (draft.allAnswered && !item.sending) onSend(draft.answers())
        true
      }
      else -> false
    }
  }
  CardFrame(modifier.then(keys).focusRequester(focus).focusable().semantics { contentDescription = "Zohara Agent is asking you a question"; liveRegion = LiveRegionMode.Polite }) {
    Row(verticalAlignment = Alignment.CenterVertically) {
      if (spec.header.isNotBlank()) ZoChip(spec.header.take(24), color = c.accent)
      Spacer(Modifier.weight(1f))
      if (item.questions.size > 1) Text("${p + 1} of ${item.questions.size}", style = ZoTheme.type.fieldHint, color = c.dim)
    }
    Text(spec.question, style = ZoTheme.type.message.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.Medium), color = c.text, modifier = Modifier.padding(top = 8.dp, bottom = 6.dp))
    for (o in spec.options) {
      val on = o.label in draft.selected[p]
      ChoiceRow(o.label, o.description, on, spec.multiSelect) { draft = draft.toggle(p, o.label) }
    }
    ChoiceRow("Other", "", draft.otherOn[p], spec.multiSelect) { draft = draft.toggleOther(p) }
    ZoTextField(
      draft.other[p], { draft = draft.setOther(p, it) }, Modifier.padding(top = 6.dp, bottom = 4.dp),
      placeholder = "Type your own answer", singleLine = false, minLines = 1,
    )
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
      SecondaryButton("Skip", onSkip, Modifier.heightIn(min = 48.dp), enabled = !item.sending)
      Spacer(Modifier.weight(1f))
      if (!draft.first) SecondaryButton("Back", { draft = draft.back() }, Modifier.heightIn(min = 48.dp))
      if (!draft.last) PrimaryButton("Next", { draft = draft.next() }, Modifier.heightIn(min = 48.dp), enabled = draft.currentAnswered)
      else PrimaryButton("Send", { onSend(draft.answers()) }, Modifier.heightIn(min = 48.dp), enabled = draft.allAnswered, loading = item.sending)
    }
  }
}

/** One option: a radio or a checkbox, the label and the description under it; the whole row is the touch target (48 dp at least). */
@Composable
private fun ChoiceRow(label: String, description: String, selected: Boolean, multi: Boolean, onClick: () -> Unit) {
  val c = ZoTheme.colors
  Row(
    Modifier.fillMaxWidth().heightIn(min = 48.dp).clip(ZoTheme.shapes.field)
      .background(if (selected) c.accent.copy(alpha = 0.10f) else androidx.compose.ui.graphics.Color.Transparent)
      .selectable(selected, role = if (multi) Role.Checkbox else Role.RadioButton, onClick = onClick)
      .padding(horizontal = 8.dp, vertical = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Mark(selected, multi)
    Spacer(Modifier.width(12.dp))
    Column(Modifier.weight(1f)) {
      Text(label, style = ZoTheme.type.body.copy(fontSize = 14.5.sp), color = c.text)
      if (description.isNotBlank()) Text(description, style = ZoTheme.type.fieldHint, color = c.dim, modifier = Modifier.padding(top = 1.dp))
    }
  }
}

@Composable
private fun Mark(selected: Boolean, multi: Boolean) {
  val c = ZoTheme.colors
  val shape = if (multi) RoundedCornerShape(5.dp) else CircleShape
  Box(
    Modifier.size(20.dp).clip(shape).background(if (selected) c.accent else androidx.compose.ui.graphics.Color.Transparent).border(2.dp, if (selected) c.accent else c.line12.copy(alpha = 0.35f), shape),
    contentAlignment = Alignment.Center,
  ) {
    if (selected) {
      if (multi) ZoIcon(ZoIcons.Check, tint = c.onAccent, size = 14.dp) else Box(Modifier.size(8.dp).clip(CircleShape).background(c.onAccent))
    }
  }
}
