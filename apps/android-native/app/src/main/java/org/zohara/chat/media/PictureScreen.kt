package org.zohara.chat.media

import androidx.compose.foundation.Image
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.unit.dp
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.EmptyBox
import org.zohara.chat.core.design.SecondaryButton
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.nav.LocalNav

/** Full-screen view of the [index]th attached picture (ATT-12): pinch or double tap to zoom, "W × H · size", Remove. */
@Composable
fun PictureScreen(index: Int) {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val module = remember(container) { MediaModule.of(container) }
  val items by module.attachments.items.collectAsState()
  val pictures = items.filterIsInstance<ImageItem>()
  val pic = pictures.getOrNull(index)
  val c = ZoTheme.colors
  if (pic == null) {
    EmptyBox("That picture is not attached any more.", action = { SecondaryButton("Back", { nav.back() }) })
    return
  }
  val bmp = remember(pic.id) { BitmapFactoryDecode.bytes(pic.bytes) }
  var scale by remember { mutableFloatStateOf(1f) }
  var dx by remember { mutableFloatStateOf(0f) }
  var dy by remember { mutableFloatStateOf(0f) }
  Column(Modifier.fillMaxSize()) {
    Box(
      Modifier.weight(1f).fillMaxWidth().pointerInput(Unit) {
        detectTransformGestures { _, pan, zoom, _ -> scale = (scale * zoom).coerceIn(1f, 6f); if (scale > 1f) { dx += pan.x; dy += pan.y } else { dx = 0f; dy = 0f } }
      }.pointerInput(Unit) {
        detectTapGestures(onDoubleTap = { if (scale > 1f) { scale = 1f; dx = 0f; dy = 0f } else scale = 2.5f })
      },
      contentAlignment = Alignment.Center,
    ) {
      if (bmp != null) Image(bmp, pic.name, Modifier.fillMaxSize().graphicsLayer { scaleX = scale; scaleY = scale; translationX = dx; translationY = dy }, contentScale = ContentScale.Fit)
    }
    Row(Modifier.fillMaxWidth().padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
      Column(Modifier.weight(1f)) {
        Text(pic.name, style = ZoTheme.type.cardName, color = c.text, maxLines = 1)
        Text("${pic.w} × ${pic.h} · ${MediaRules.kb(pic.size)}", style = ZoTheme.type.cardMeta, color = c.dim)
      }
      SecondaryButton("Remove", { module.attachments.remove(pic.id); nav.back() })
      SecondaryButton("Back", { nav.back() })
    }
  }
}

/** a tiny wrapper so the screen file has no direct BitmapFactory import */
internal object BitmapFactoryDecode {
  fun bytes(b: ByteArray) = try { android.graphics.BitmapFactory.decodeByteArray(b, 0, b.size)?.let { it.asImageBitmap() } } catch (_: Exception) { null }
}
