package org.zohara.chat.media

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.launch
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.nav.LocalNav

/**
 * Take a picture to attach (ATT-17): opens the camera app, puts the result in the attachment strip (through the same picture pipeline as the picker), and goes back.
 * No camera permission is needed: the camera app writes into a file of this app's cache.
 */
@Composable
fun CameraScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val nav = LocalNav.current
  val module = remember(container) { MediaModule.of(container) }
  val scope = rememberCoroutineScope()
  var path by rememberSaveable { mutableStateOf<String?>(null) }
  var started by rememberSaveable { mutableStateOf(false) }
  val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
    val p = path
    path = null
    if (ok && p != null) {
      val f = java.io.File(p)
      if (f.exists() && f.length() > 0) module.attachments.addFiles(listOf(CachedFile(f, "photo-${System.currentTimeMillis() / 1000}.jpg", "image/jpeg")))
    }
    nav.back()
  }
  LaunchedEffect(Unit) {
    if (started) return@LaunchedEffect
    started = true
    if (!CameraCapture.available(context)) {
      scope.launch { container.toast.show("This phone has no camera app.") }
      nav.back()
      return@LaunchedEffect
    }
    val (f, uri) = CameraCapture.newTarget(context)
    path = f.absolutePath
    camera.launch(uri)
  }
  Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
    Text("Opening the camera…", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
  }
}
