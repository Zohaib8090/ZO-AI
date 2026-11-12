package org.zohara.zoai.chat

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoSpinner
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.markdown.MarkdownText
import org.zohara.zoai.core.net.AttachInfo
import org.zohara.zoai.core.net.TakeInfo

/** What a row can do; the thread gives every row the same set. */
class MessageActions(
  val canRetry: Boolean,
  val onCopy: (String) -> Unit,
  val onShare: (String) -> Unit,
  val onEdit: (UserItem) -> Unit,
  val onRetry: () -> Unit,
  val onSwitchTake: (nth: Int, delta: Int) -> Unit,
  val onChooseModel: () -> Unit,
  val onReadAloud: ((String) -> Unit)?,
  val onPicture: (AttachInfo) -> Unit = {},
)

private fun decodeThumb(data: String?): androidx.compose.ui.graphics.ImageBitmap? {
  val b64 = data?.substringAfter("base64,", "")?.takeIf { it.isNotEmpty() } ?: return null
  return try {
    val bytes = java.util.Base64.getMimeDecoder().decode(b64)
    android.graphics.BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
  } catch (_: Exception) { null }
}

/** CHAT-03: right aligned bubble, 85 percent wide at most, with its attachments above the text (CHAT-04) and the takes arrows below (CHAT-07). */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun UserBubble(item: UserItem, actions: MessageActions, busy: Boolean, editing: Boolean, onStartEdit: () -> Unit, onEditDone: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  var menu by remember { mutableStateOf(false) }
  var selecting by remember { mutableStateOf(false) }
  Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.End) {
    if (editing) {
      EditBox(item.text, onCancel = onEditDone, onSend = { actions.onEdit(item.copy(text = it)); onEditDone() })
    } else {
      BoxWithConstraints(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
        Box {
          Column(
            Modifier.widthIn(max = this@BoxWithConstraints.maxWidth * 0.85f).clip(ZoTheme.shapes.bubble).background(c.user)
              .combinedClickable(onClick = {}, onLongClick = { menu = true })
              .padding(horizontal = 14.dp, vertical = 9.dp)
              .semantics { contentDescription = "You: ${item.text}" },
          ) {
            for (a in item.attach) AttachmentRow(a, actions.onPicture)
            if (item.text.isNotEmpty()) {
              val t = @Composable { Text(item.text, style = ZoTheme.type.message, color = c.text) }
              if (selecting) SelectionContainer { t() } else t()
            }
          }
          DropdownMenu(menu, { menu = false }, containerColor = c.popover) {
            DropdownMenuItem(text = { Text("Copy", color = c.text) }, onClick = { actions.onCopy(item.text); menu = false })
            DropdownMenuItem(text = { Text("Select text", color = c.text) }, onClick = { selecting = !selecting; menu = false })
            DropdownMenuItem(text = { Text("Share", color = c.text) }, onClick = { actions.onShare(item.text); menu = false })
            if (!busy && item.text.isNotEmpty()) DropdownMenuItem(text = { Text("Edit", color = c.text) }, onClick = { onStartEdit(); menu = false })
          }
        }
      }
      val take = item.take
      if (take != null && take.count > 1 && !busy) TakeNav(take, actions)
    }
  }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun AttachmentRow(a: AttachInfo, onPicture: (AttachInfo) -> Unit) {
  val c = ZoTheme.colors
  val bmp = remember(a.thumb) { if (a.kind == "image") decodeThumb(a.thumb) else null }
  if (bmp != null) {
    Image(
      bmp, contentDescription = "Picture ${a.name}", contentScale = ContentScale.Crop,
      modifier = Modifier.padding(bottom = 6.dp).size(width = 160.dp, height = 110.dp).clip(ZoTheme.shapes.code).combinedClickable(onClick = { onPicture(a) }),
    )
  } else {
    Row(
      Modifier.padding(bottom = 6.dp).clip(ZoTheme.shapes.field).background(c.line8).padding(horizontal = 10.dp, vertical = 6.dp).semantics { contentDescription = "Attachment ${a.name}" },
      verticalAlignment = Alignment.CenterVertically,
    ) {
      ZoIcon(if (a.kind == "image") ZoIcons.Image else ZoIcons.File, size = 16.dp)
      Spacer(Modifier.size(6.dp))
      Text(a.name + if (a.size > 0) "  " + formatSize(a.size) else "", style = ZoTheme.type.fieldHint, color = c.text2, maxLines = 1)
    }
  }
}

fun formatSize(b: Long): String = when {
  b >= 1024 * 1024 -> String.format(java.util.Locale.US, "%.1f MB", b / 1048576.0)
  b >= 1024 -> "${b / 1024} KB"
  else -> "$b B"
}

/** "‹ 2 / 3 ›" under a message with more than one take (CHAT-07, CHAT-46). */
@Composable
fun TakeNav(take: TakeInfo, actions: MessageActions) {
  val c = ZoTheme.colors
  Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
    ZoIconButton(ZoIcons.Back, "Previous take", { actions.onSwitchTake(take.nth, -1) }, enabled = take.current > 0, size = 40.dp, iconSize = 16.dp)
    Text("${take.current + 1} / ${take.count}", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.semantics { contentDescription = "Take ${take.current + 1} of ${take.count}" })
    ZoIconButton(ZoIcons.Chevron, "Next take", { actions.onSwitchTake(take.nth, 1) }, enabled = take.current < take.count - 1, size = 40.dp, iconSize = 16.dp)
  }
}

/** The inline editor that replaces a bubble (CHAT-48). */
@Composable
fun EditBox(initial: String, onCancel: () -> Unit, onSend: (String) -> Unit) {
  val c = ZoTheme.colors
  var text by remember { mutableStateOf(initial) }
  Column(Modifier.fillMaxWidth().clip(ZoTheme.shapes.bubble).background(c.user).padding(12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    androidx.compose.foundation.text.BasicTextField(
      text, { text = it }, textStyle = ZoTheme.type.message.copy(color = c.text),
      cursorBrush = androidx.compose.ui.graphics.SolidColor(c.accent),
      modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp).semantics { contentDescription = "Edit your message" },
    )
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End)) {
      SecondaryButton("Cancel", onCancel)
      org.zohara.zoai.core.design.PrimaryButton("Send", { onSend(text) }, enabled = text.isNotBlank())
    }
  }
}

/** The AI's answer: markdown, no bubble (CHAT-09, CHAT-11). The menu has Copy, Select text, Share, Try again. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AiMessage(item: AiItem, actions: MessageActions, modifier: Modifier = Modifier) {
  var menu by remember { mutableStateOf(false) }
  var selecting by remember { mutableStateOf(false) }
  Box(modifier.fillMaxWidth()) {
    Column(
      Modifier.fillMaxWidth().combinedClickable(onClick = {}, onLongClick = { menu = true }).padding(horizontal = 2.dp, vertical = 4.dp)
        .semantics { contentDescription = "ZO-AI"; if (item.live) liveRegion = LiveRegionMode.Polite },
    ) {
      val body = @Composable { MarkdownText(item.text) }
      if (selecting) SelectionContainer { body() } else body()
    }
    val c = ZoTheme.colors
    DropdownMenu(menu, { menu = false }, containerColor = c.popover) {
      DropdownMenuItem(text = { Text("Copy", color = c.text) }, onClick = { actions.onCopy(item.text); menu = false })
      DropdownMenuItem(text = { Text("Select text", color = c.text) }, onClick = { selecting = !selecting; menu = false })
      DropdownMenuItem(text = { Text("Share", color = c.text) }, onClick = { actions.onShare(item.text); menu = false })
      if (actions.canRetry) DropdownMenuItem(text = { Text("Try again", color = c.text) }, onClick = { actions.onRetry(); menu = false })
    }
  }
}

/** Copy / Try again / Read aloud under the newest answer (CHAT-08, CHAT-09). */
@Composable
fun AiActionRow(text: String, actions: MessageActions, showRetry: Boolean) {
  Row(Modifier.padding(top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
    ZoIconButton(ZoIcons.Copy, "Copy the reply", { actions.onCopy(text) }, size = 44.dp, iconSize = 17.dp)
    if (showRetry) Text(
      "Try again", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim,
      modifier = Modifier.heightIn(min = 44.dp).clip(ZoTheme.shapes.button).clickable(role = androidx.compose.ui.semantics.Role.Button, onClick = actions.onRetry).padding(horizontal = 10.dp, vertical = 12.dp),
    )
    actions.onReadAloud?.let { ZoIconButton(ZoIcons.Speaker, "Read aloud", { it(text) }, size = 44.dp, iconSize = 17.dp) }
  }
}

/** Under the newest user message: Copy and Edit. */
@Composable
fun UserActionRow(item: UserItem, actions: MessageActions, onEdit: () -> Unit) {
  Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
    ZoIconButton(ZoIcons.Copy, "Copy your message", { actions.onCopy(item.text) }, size = 44.dp, iconSize = 17.dp)
    ZoIconButton(ZoIcons.Edit, "Edit your message", onEdit, size = 44.dp, iconSize = 17.dp)
  }
}

@Composable
fun ErrorRow(item: ErrorItem, canRetry: Boolean, actions: MessageActions, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Column(modifier.fillMaxWidth().padding(vertical = 2.dp).semantics { liveRegion = LiveRegionMode.Polite }) {
    SelectionContainer { Text(item.text, style = ZoTheme.type.cardMeta.copy(fontSize = 13.sp), color = c.danger) }
    Row(Modifier.padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
      if (item.chooseModel) PillButton("Choose a model", actions.onChooseModel, accent = true)
      if (canRetry && item.retry && !item.chooseModel) PillButton("Try again", actions.onRetry)
    }
  }
}

@Composable
fun StoppedRow(canRetry: Boolean, actions: MessageActions, modifier: Modifier = Modifier) {
  Column(modifier.fillMaxWidth().padding(vertical = 2.dp)) {
    Text("Stopped.", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
    if (canRetry) Row(Modifier.padding(top = 6.dp)) { PillButton("Try again", actions.onRetry) }
  }
}

@Composable
fun NoticeRow(text: String, modifier: Modifier = Modifier) {
  Text(text, style = ZoTheme.type.fieldHint.copy(fontSize = 12.5.sp), color = ZoTheme.colors.dim, modifier = modifier.fillMaxWidth().padding(horizontal = 2.dp, vertical = 2.dp))
}

/** CHAT-10 */
@Composable
fun ThinkingRow(modifier: Modifier = Modifier) {
  Row(modifier.padding(horizontal = 2.dp, vertical = 4.dp).semantics { contentDescription = "ZO-AI is thinking" }, verticalAlignment = Alignment.CenterVertically) {
    ZoSpinner(size = 14.dp, color = ZoTheme.colors.dim)
    Spacer(Modifier.size(8.dp))
    Text("Thinking…", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
  }
}

/** CHAT-37 */
@Composable
fun WaitingRow(summary: String, modifier: Modifier = Modifier) {
  Row(modifier.fillMaxWidth().padding(horizontal = 2.dp, vertical = 4.dp).semantics { liveRegion = LiveRegionMode.Polite }, verticalAlignment = Alignment.CenterVertically) {
    ZoSpinner(size = 14.dp, color = ZoTheme.colors.accent)
    Spacer(Modifier.size(8.dp))
    Text("Waiting for your approval on your computer: $summary", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, maxLines = 3)
  }
}
