package org.zohara.zoai.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SheetContent
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import java.text.NumberFormat
import java.util.Locale

@Composable
private fun PopHead(text: String) {
  Text(text, style = ZoTheme.type.popSub, color = ZoTheme.colors.dim, modifier = Modifier.padding(start = 4.dp, top = 12.dp, bottom = 4.dp))
}

/** A row of the model and mode lists: a title, an optional second line, a tick when it is the current one. */
@Composable
fun ChoiceRowSheet(title: String, sub: String?, current: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Row(
    modifier.fillMaxWidth().heightIn(min = 52.dp).clip(ZoTheme.shapes.navItem)
      .clickable(role = Role.RadioButton, onClick = onClick).semantics { selected = current }
      .padding(horizontal = 10.dp, vertical = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Column(Modifier.weight(1f)) {
      Text(title, style = ZoTheme.type.popTitle, color = c.text)
      if (!sub.isNullOrBlank()) Text(sub, style = ZoTheme.type.popSub, color = c.dim)
    }
    if (current) ZoIcon(ZoIcons.Check, tint = c.check, size = 20.dp, contentDescription = "Current")
  }
}

// ---------------------------------------------------------------- model

/** The model menu (COMP-08): pinned models by profile, a profile row for those without pins, and the way to the model settings. */
@Composable
fun ModelSheetContent(ui: PillsUi, onPickModel: (profileId: String?, model: String) -> Unit, onPickProfile: (String) -> Unit, onMore: () -> Unit) {
  Column(Modifier.verticalScroll(rememberScrollState())) {
    val pinned = ui.profiles.filter { it.pinned.isNotEmpty() }
    for (p in pinned) {
      PopHead("${p.name} · ${p.provider}")
      for (m in p.pinned) ChoiceRowSheet(Transcript.niceName(m), null, current = ui.activeProfile == p.id && ui.model == m, onClick = { onPickModel(p.id, m) })
    }
    val inPins = pinned.any { it.id == ui.activeProfile && ui.model in it.pinned }
    if (!inPins && ui.ready && ui.model.isNotBlank()) {
      PopHead(if (pinned.isEmpty()) "In use now" else "In use now")
      ChoiceRowSheet(Transcript.niceName(ui.model), ui.provider.ifBlank { null }, current = true, onClick = {})
    }
    val plain = ui.profiles.filter { it.pinned.isEmpty() && it.id != ui.activeProfile }
    if (plain.isNotEmpty()) {
      PopHead("Profiles")
      for (p in plain) ChoiceRowSheet(p.name, listOf(p.provider, p.model).filter { it.isNotBlank() }.joinToString(" · "), current = false, onClick = { onPickProfile(p.id) })
    }
    if (!ui.ready && pinned.isEmpty() && plain.isEmpty()) PopHead("Choose a provider to begin")
    Box(Modifier.fillMaxWidth().padding(vertical = 6.dp).height(1.dp).background(ZoTheme.colors.line8))
    ChoiceRowSheet("Model and API key…", null, current = false, onClick = onMore)
  }
}

@Composable
fun ModelSheet(ui: PillsUi, onDismiss: () -> Unit, onPickModel: (String?, String) -> Unit, onPickProfile: (String) -> Unit, onMore: () -> Unit) {
  ZoBottomSheet("Model", onDismiss) { ModelSheetContent(ui, onPickModel, onPickProfile, onMore) }
}

// ---------------------------------------------------------------- mode

@Composable
fun ModeSheetContent(current: PermMode, allowBypass: Boolean, onPick: (PermMode) -> Unit) {
  Column {
    for (m in PermMode.entries) {
      if (m == PermMode.Bypass && !allowBypass) continue
      ChoiceRowSheet(m.label, m.text, current == m, { onPick(m) })
    }
    if (!allowBypass) Text("Bypass is not available for this account or on your computer.", style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(start = 10.dp, top = 6.dp))
  }
}

@Composable
fun ModeSheet(current: PermMode, allowBypass: Boolean, onDismiss: () -> Unit, onPick: (PermMode) -> Unit) {
  ZoBottomSheet("What ZO-AI may do", onDismiss) { ModeSheetContent(current, allowBypass, onPick) }
}

// ---------------------------------------------------------------- context

data class ContextView(
  val info: Transcript.ContextInfo,
  val usage: Usage?,
  /** "In Claude Code mode ..." or "Choose a provider first." instead of the numbers */
  val message: String? = null,
  val compacting: Boolean = false,
  val result: String? = null,
)

private fun n(v: Int): String = NumberFormat.getIntegerInstance(Locale.US).format(v)

@Composable
fun ContextSheetContent(v: ContextView, onRefresh: () -> Unit, onCompact: () -> Unit, onPin: (() -> Unit)? = null) {
  val c = ZoTheme.colors
  Column(Modifier.verticalScroll(rememberScrollState())) {
    if (v.message != null) {
      Text(v.message, style = ZoTheme.type.body, color = c.text2, modifier = Modifier.padding(vertical = 8.dp))
      return@Column
    }
    val i = v.info
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
      Text("${i.percent}% used", style = ZoTheme.type.sheetTitle.copy(fontSize = 22.sp), color = c.text, modifier = Modifier.weight(1f))
      Text("≈ ${n(i.used)} / ${n(i.window)} tokens", style = ZoTheme.type.fieldHint, color = c.dim)
    }
    val segs = listOf(
      Triple(i.system, c.accent, "System instructions"), Triple(i.history, c.blue, "Chat history"), Triple(i.tools, c.gold, "Tools & MCP"), Triple(i.buffer, c.dimmer.copy(alpha = 0.45f), "Auto-compact buffer"), Triple(i.free, c.line12, "Free capacity"),
    )
    Row(Modifier.fillMaxWidth().padding(vertical = 12.dp).height(10.dp).clip(CircleShape).background(c.line8).semantics { contentDescription = "${i.percent} percent of the context window is used" }) {
      for ((tokens, color, _) in segs.take(3)) {
        val w = (tokens.toFloat() / i.window).coerceAtLeast(0.01f)
        Box(Modifier.weight(w).height(10.dp).background(color))
      }
      // the free part stays empty (track colour); the buffer sits at the end of the bar, as on the desktop
      Box(Modifier.weight(((i.free.toFloat() / i.window)).coerceAtLeast(0.0001f)).height(10.dp))
      Box(Modifier.weight((i.buffer.toFloat() / i.window).coerceAtLeast(0.0001f)).height(10.dp).background(c.dimmer.copy(alpha = 0.45f)))
    }
    Column(Modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(c.card2).padding(horizontal = 14.dp, vertical = 6.dp)) {
      for ((tokens, color, label) in segs) {
        Row(Modifier.fillMaxWidth().heightIn(min = 36.dp), verticalAlignment = Alignment.CenterVertically) {
          Box(Modifier.size(10.dp).clip(CircleShape).background(color))
          Spacer(Modifier.width(10.dp))
          Text(label, style = ZoTheme.type.body, color = c.text, modifier = Modifier.weight(1f))
          val pct = tokens.toDouble() / i.window * 100
          Text("≈ ${n(tokens)} tokens · ${String.format(Locale.US, "%.1f", pct)}%", style = ZoTheme.type.fieldHint, color = c.dim)
        }
      }
    }
    Text("${n(Transcript.untilCompact(i))} tokens until auto-compact", style = ZoTheme.type.fieldHint, color = c.dim, modifier = Modifier.padding(top = 10.dp))
    Text("Sizes are estimated from the length of the text; the provider's count is exact.", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 4.dp))
    v.usage?.let {
      Text("Last request, as the provider counted it: ${n(it.input.toInt())} in, ${n(it.output.toInt())} out.", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 4.dp))
    }
    v.result?.let { Text(it, style = ZoTheme.type.cardMeta, color = c.text2, modifier = Modifier.padding(top = 10.dp)) }
    onPin?.let { Text("Keep this panel open next to the chat.", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 10.dp)) }
    Row(Modifier.fillMaxWidth().padding(top = 14.dp), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
      onPin?.let { SecondaryButton("Pin to the side", it, Modifier.heightIn(min = 48.dp)) }
      SecondaryButton("Refresh", onRefresh, Modifier.heightIn(min = 48.dp))
      PrimaryButton(if (v.compacting) "Summarizing…" else "Compact session", onCompact, Modifier.heightIn(min = 48.dp), enabled = !v.compacting, loading = v.compacting)
    }
  }
}

@Composable
fun ContextSheet(v: ContextView, onDismiss: () -> Unit, onRefresh: () -> Unit, onCompact: () -> Unit, onPin: (() -> Unit)? = null) {
  ZoBottomSheet("Context", onDismiss) { ContextSheetContent(v, onRefresh, onCompact, onPin) }
}
