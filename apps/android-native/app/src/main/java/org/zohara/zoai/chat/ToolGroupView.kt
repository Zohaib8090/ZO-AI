package org.zohara.zoai.chat

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.core.design.MonoFamily
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoSpinner
import org.zohara.zoai.core.design.ZoTheme

/**
 * The collapsed box of tool runs (CHAT-27, 28, 31, 34): a grey head "Ran N commands" (teal with a spinner while it works) that opens a body
 * with one line per call and per result. A line opens the full-screen viewer through [onOpenLine].
 */
@Composable
fun ToolGroupView(item: ToolGroupItem, onOpen: (lineIndex: Int) -> Unit, modifier: Modifier = Modifier, startExpanded: Boolean = false) {
  val c = ZoTheme.colors
  var open by rememberSaveable(item.key) { mutableStateOf(startExpanded) }
  val headColor = if (item.active) c.accent else c.dim
  Column(modifier.fillMaxWidth().padding(top = 2.dp, bottom = 4.dp)) {
    Row(
      Modifier.heightIn(min = 44.dp).clip(ZoTheme.shapes.button)
        .clickable(role = Role.Button) { open = !open }
        .semantics { stateDescription = if (open) "Expanded" else "Collapsed"; contentDescription = item.head }
        .padding(horizontal = 8.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      ZoIcon(ZoIcons.Chevron, Modifier.rotate(if (open) 90f else 0f), tint = headColor, size = 16.dp)
      Spacer(Modifier.width(8.dp))
      Text(item.head, style = ZoTheme.type.body.copy(fontSize = 13.5.sp), color = headColor)
      if (item.active) { Spacer(Modifier.width(8.dp)); ZoSpinner(size = 14.dp, color = c.accent) }
    }
    AnimatedVisibility(open) {
      Row(Modifier.padding(start = 22.dp, top = 2.dp, bottom = 8.dp).height(IntrinsicSize.Min)) {
        Box(Modifier.width(2.dp).fillMaxHeight().background(c.active))
        Column(Modifier.padding(start = 12.dp)) {
          item.lines.forEachIndexed { i, line ->
            val (prefix, color, style) = when (line.kind) {
              LineKind.Start -> Triple("▸ ", c.dim, ZoTheme.type.fieldHint.copy(fontSize = 12.5.sp))
              LineKind.Ok -> Triple("✓ ", c.dim, mono())
              LineKind.Fail -> Triple("✗ ", c.danger, mono())
            }
            Text(
              prefix + line.text, style = style, color = color, maxLines = if (line.kind == LineKind.Start) 2 else 4, overflow = TextOverflow.Ellipsis,
              modifier = Modifier.fillMaxWidth().heightIn(min = 32.dp).clickable { onOpen(i) }.padding(vertical = 5.dp, horizontal = 2.dp),
            )
          }
        }
      }
    }
  }
}

@Composable
private fun mono(): TextStyle = TextStyle(fontFamily = MonoFamily, fontSize = 12.sp, lineHeight = 17.sp)
