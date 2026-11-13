package org.zohara.zoai.media

import android.graphics.BitmapFactory
import android.util.Base64
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoSpinner
import org.zohara.zoai.core.design.ZoTheme

/** A thumbnail from a data URI of the saved chat; null unless it is a small base64 png, jpeg or webp (ATT-23, ATT-27). */
fun decodeThumb(dataUri: String?): ImageBitmap? {
  if (!MediaRules.safeThumb(dataUri)) return null
  return try {
    val bytes = Base64.decode(dataUri!!.substringAfter(','), Base64.DEFAULT)
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
  } catch (_: Exception) { null }
}

private fun decodeBytes(b: ByteArray): ImageBitmap? = try { BitmapFactory.decodeByteArray(b, 0, b.size)?.asImageBitmap() } catch (_: Exception) { null }

/** What a preview sheet shows (ATT-12): a picture full size with "W × H · size", or the first 4000 characters of a text file. */
sealed interface PreviewTarget {
  val name: String
  class Picture(override val name: String, val bytes: ByteArray?, val thumb: String?, val w: Int, val h: Int, val size: Long) : PreviewTarget
  class TextFile(override val name: String, val text: String) : PreviewTarget
}

fun previewOf(i: AttItem): PreviewTarget? = when (i) {
  is ImageItem -> PreviewTarget.Picture(i.name, i.bytes, i.thumb, i.w, i.h, i.size.toLong())
  is TextItem -> PreviewTarget.TextFile(i.name, i.text)
  else -> null
}

@Composable
fun PreviewSheet(target: PreviewTarget, onClose: () -> Unit) {
  ZoBottomSheet(target.name, onClose, footer = { SecondaryButton("Close", onClose) }) { PreviewBody(target) }
}

@Composable
fun PreviewBody(target: PreviewTarget) {
  val c = ZoTheme.colors
  when (target) {
    is PreviewTarget.Picture -> {
      val bmp = remember(target) { target.bytes?.let { decodeBytes(it) } ?: decodeThumb(target.thumb) }
      if (bmp != null) Image(bmp, target.name, Modifier.fillMaxWidth().heightIn(max = 460.dp).clip(ZoTheme.shapes.card), contentScale = ContentScale.Fit)
      else Text("(no preview)", style = ZoTheme.type.cardMeta, color = c.dim)
      val meta = listOfNotNull(if (target.w > 0 && target.h > 0) "${target.w} × ${target.h}" else null, MediaRules.kb(target.size)).joinToString(" · ")
      Text(meta, style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 8.dp))
    }
    is PreviewTarget.TextFile -> {
      val shown = if (target.text.length > 4000) target.text.take(4000) + "\n…" else target.text.ifEmpty { "(no preview)" }
      Box(Modifier.fillMaxWidth().heightIn(max = 420.dp).clip(ZoTheme.shapes.code).background(c.code).padding(12.dp)) {
        Text(shown, style = ZoTheme.type.code, color = c.codeFg, modifier = Modifier.verticalScroll(rememberScrollState()))
      }
    }
  }
}

// ---------------------------------------------------------------- chips

@Composable
private fun RemoveX(label: String, onRemove: () -> Unit, modifier: Modifier) {
  val c = ZoTheme.colors
  Box(
    modifier.size(20.dp).clip(CircleShape).background(c.popover).border(1.dp, c.line12, CircleShape).clickable(onClick = onRemove).semantics { contentDescription = "Remove $label" },
    contentAlignment = Alignment.Center,
  ) { ZoIcon(ZoIcons.Close, tint = c.text2, size = 12.dp) }
}

/** One chip of the strip above the composer (ATT-11): picture thumbnail with an x, file chip, busy chip, link chip. */
@Composable
fun AttachmentChip(item: AttItem, onRemove: () -> Unit, onOpen: (() -> Unit)? = null, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  val small = t.fieldHint.copy(fontSize = 11.5.sp)
  val body = t.fieldHint.copy(fontSize = 12.5.sp)
  val label = when (item) {
    is ImageItem -> "picture ${item.name}"
    is TextItem -> "file ${item.name}"
    is LinkItem -> "link ${item.title.ifEmpty { item.host }}"
    is BusyItem -> item.name
  }
  val a11y = Modifier.semantics {
    contentDescription = label
    customActions = listOf(CustomAccessibilityAction("Remove $label") { onRemove(); true })
  }
  if (item is ImageItem) {
    Box(modifier.then(a11y).size(58.dp)) {
      val bmp = remember(item.id) { decodeThumb(item.thumb) ?: decodeBytes(item.bytes) }
      Box(Modifier.size(58.dp).clip(ZoTheme.shapes.field).background(c.card2).border(1.dp, c.line12, ZoTheme.shapes.field).then(if (onOpen != null) Modifier.clickable(onClick = onOpen) else Modifier)) {
        if (bmp != null) Image(bmp, item.name, Modifier.size(58.dp), contentScale = ContentScale.Crop)
      }
      RemoveX(label, onRemove, Modifier.align(Alignment.TopEnd).offset(x = 6.dp, y = (-6).dp))
    }
    return
  }
  Row(
    modifier.then(a11y).widthIn(max = 260.dp).heightIn(min = 40.dp).clip(ZoTheme.shapes.field).background(c.card2).border(1.dp, c.line12, ZoTheme.shapes.field)
      .then(if (onOpen != null && item is TextItem) Modifier.clickable(onClick = onOpen) else Modifier)
      .padding(start = 9.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
    verticalAlignment = Alignment.CenterVertically,
    horizontalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    when (item) {
      is BusyItem -> {
        ZoSpinner(size = 13.dp)
        Text(item.name, style = body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        Text(item.note, style = small, color = c.dim, maxLines = 1)
      }
      is LinkItem -> {
        ZoIcon(ZoIcons.Link, tint = c.dim, size = 16.dp)
        Text(item.title.ifEmpty { item.host }, style = body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        Text(if (item.title.isNotEmpty()) item.host else if (item.detected) "in your message" else "will be read", style = small, color = c.dim, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 80.dp))
      }
      is TextItem -> {
        ZoIcon(ZoIcons.File, tint = c.dim, size = 16.dp)
        Text(item.name, style = body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
        Text(item.note.ifEmpty { MediaRules.kb(item.bytes) }, style = small, color = c.dim, maxLines = 1)
      }
      is ImageItem -> Unit
    }
    RemoveX(label, onRemove, Modifier)
  }
}

/** The strip of chips above the text (COMP-17). Pictures, files, links, busy items, and the web addresses found in the typed text (their x removes the address from the text). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AttachmentStrip(media: ComposerMedia, text: String, onTextChange: (String) -> Unit, modifier: Modifier = Modifier) {
  val items by media.attachments.items.collectAsState()
  val titles by media.attachments.titles.collectAsState()
  val detected = remember(text, items, titles) { media.attachments.detected(text) }
  if (items.isEmpty() && detected.isEmpty()) return
  FlowRow(modifier.fillMaxWidth().padding(start = 2.dp, end = 2.dp, bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
    items.forEach { it ->
      AttachmentChip(it, onRemove = { media.attachments.remove(it.id) }, onOpen = previewOf(it)?.let { p -> { media.preview = p } })
    }
    detected.forEach { d -> AttachmentChip(d, onRemove = { onTextChange(media.attachments.removeFromText(text, d.url)) }) }
  }
}

// ---------------------------------------------------------------- bubble

/** What a sent message shows above its text (CHAT-04, ATT-23): a picture as a thumbnail (tap to enlarge), a file as a chip with its size (tap shows the text). */
class BubbleAttachment(val kind: String, val name: String, val size: Long, val thumb: String? = null, val text: String? = null, val image: ByteArray? = null)

fun AttMeta.toBubble() = BubbleAttachment(kind, name, size.toLong(), thumb.ifEmpty { null }, text.ifEmpty { null }, image)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AttachmentBubbles(list: List<BubbleAttachment>, modifier: Modifier = Modifier) {
  if (list.isEmpty()) return
  val c = ZoTheme.colors
  var open by remember { mutableStateOf<PreviewTarget?>(null) }
  FlowRow(modifier.padding(bottom = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
    list.forEach { a ->
      val bmp = if (a.kind == "image") (remember(a.thumb, a.image) { decodeThumb(a.thumb) ?: a.image?.let { decodeBytes(it) } }) else null
      if (bmp != null) {
        Image(
          bmp, "Picture ${a.name}, tap to enlarge",
          Modifier.size(72.dp).clip(ZoTheme.shapes.field).border(1.dp, c.line12, ZoTheme.shapes.field)
            .clickable { open = PreviewTarget.Picture(a.name, a.image, a.thumb, 0, 0, a.size) },
          contentScale = ContentScale.Crop,
        )
      } else {
        Row(
          Modifier.semantics { contentDescription = (if (a.kind == "image") "Picture " else "File ") + a.name }.heightIn(min = 34.dp).clip(ZoTheme.shapes.field).background(c.user).border(1.dp, c.line, ZoTheme.shapes.field)
            .then(if (a.text != null) Modifier.clickable { open = PreviewTarget.TextFile(a.name, a.text) } else Modifier)
            .padding(horizontal = 10.dp, vertical = 6.dp),
          verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
          ZoIcon(if (a.kind == "image") ZoIcons.Image else ZoIcons.File, tint = c.dim, size = 16.dp)
          Text(a.name, style = ZoTheme.type.fieldHint.copy(fontSize = 12.5.sp), color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.widthIn(max = 160.dp))
          Text(MediaRules.kb(a.size), style = ZoTheme.type.fieldHint.copy(fontSize = 11.5.sp), color = c.dim)
        }
      }
    }
  }
  open?.let { PreviewSheet(it) { open = null } }
}

/** A row of the "+" menu and other bottom sheets: icon, title, sub line (`.pop-row`). */
@Composable
fun SheetRow(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, sub: String?, onClick: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Row(modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).clickable(onClick = onClick).padding(horizontal = 4.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
    ZoIcon(icon, tint = c.icon, size = 22.dp)
    Spacer(Modifier.size(14.dp))
    Column(Modifier.weight(1f)) {
      Text(title, style = ZoTheme.type.popTitle, color = c.text)
      if (sub != null) Text(sub, style = ZoTheme.type.popSub, color = c.dim)
    }
  }
}
