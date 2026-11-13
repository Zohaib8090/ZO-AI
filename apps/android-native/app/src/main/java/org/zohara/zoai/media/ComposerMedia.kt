package org.zohara.zoai.media

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.content.MediaType
import androidx.compose.foundation.content.ReceiveContentListener
import androidx.compose.foundation.content.TransferableContent
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.content.hasMediaType
import androidx.compose.foundation.draganddrop.dragAndDropTarget
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draganddrop.DragAndDropEvent
import androidx.compose.ui.draganddrop.DragAndDropTarget
import androidx.compose.ui.draganddrop.toAndroidDragEvent
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme

/**
 * What the chat composer plugs in (docs/MEDIA-LINK.md): attachments, dictation, the "+" menu and its sheets. Made by [rememberComposerMedia].
 * The composer shows [AttachmentStrip], a [PlusButton] and a [MicButton], calls [onTextEdited] on every edit, [ComposerMediaSheets] once in a Box, and at send time
 * [AttachmentsController.take] ([attachments]) and `isBusy` (COMP-19: "Still reading your attachments. One moment.").
 */
@Stable
class ComposerMedia internal constructor(
  val module: MediaModule,
  val attachments: AttachmentsController,
  val voice: VoiceController,
  internal val context: Context,
  private val readText: () -> String,
  private val toast: (String) -> Unit,
) {
  var plusOpen by mutableStateOf(false)
  var linkSheetOpen by mutableStateOf(false)
  var preview by mutableStateOf<PreviewTarget?>(null)
  var micRationale by mutableStateOf(false)
  var micBlocked by mutableStateOf(false)
  /** something is being dragged over the composer (the overlay "Drop to attach", ATT-16) */
  var dropping by mutableStateOf(false)

  internal var launchFiles: () -> Unit = {}
  internal var launchCamera: () -> Unit = {}
  internal var requestMic: () -> Unit = {}

  /** the "Take a photo" row is only offered when there is a camera (ATT-28) */
  val hasCamera: Boolean = CameraCapture.available(context)

  fun openPlus() { plusOpen = true }

  fun micGranted(): Boolean = ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

  /** The mic button was pressed (VOICE-01, VOICE-08). */
  fun micPressed() {
    if (voice.listening.value) { voice.stop(); return }
    if (!voice.available) { toast(VoiceError.ServiceNotAllowed.message); return }
    if (micGranted()) voice.start() else micRationale = true
  }

  /** A new answer started: the mic is disabled and listening stops (VOICE-06). */
  fun onBusy(busy: Boolean) { if (busy) { voice.cancel(); module.readAloud.stop() } }

  /**
   * Call on every edit of the text box with the text before and after. Typing stops listening (VOICE-05); a bare web address that was pasted becomes a link chip
   * and is not kept in the text (ATT-15); typed addresses get a preview 900 ms after typing stops (ATT-20). Returns the text to show.
   */
  fun onTextEdited(old: String, new: String): String {
    voice.onTyped()
    val out = takePastedUrl(old, new) ?: new
    attachments.onTextChanged(out)
    return out
  }

  private fun takePastedUrl(old: String, new: String): String? {
    if (new.length - old.length < 8) return null
    var p = 0
    while (p < old.length && p < new.length && old[p] == new[p]) p++
    var s = 0
    while (s < old.length - p && s < new.length - p && old[old.length - 1 - s] == new[new.length - 1 - s]) s++
    val inserted = new.substring(p, new.length - s)
    val t = inserted.trim()
    if (t.isEmpty() || t.any { it.isWhitespace() } || !Regex("""^https?://""", RegexOption.IGNORE_CASE).containsMatchIn(t)) return null
    if (MediaRules.normalizeUrl(t) == null) return null
    // only what really came from the clipboard counts as a paste
    val clip = clipText()
    if (clip == null || clip.trim() != t) return null
    attachments.addLink(t)
    return new.removeRange(p, new.length - s)
  }

  private fun clipText(): String? = try {
    val cm = context.getSystemService(ClipboardManager::class.java)
    cm?.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(context)?.toString()
  } catch (_: Exception) { null }

  /** Does the clipboard hold a picture or file? (the "+" menu then offers "Paste from clipboard") */
  fun clipboardHasFile(): Boolean = try {
    val d = context.getSystemService(ClipboardManager::class.java)?.primaryClipDescription
    d != null && (0 until d.mimeTypeCount).any { val m = d.getMimeType(it); m.startsWith("image/") || m == "application/pdf" || m == "application/zip" }
  } catch (_: Exception) { false }

  /** Paste: pictures and files in the clipboard become attachments, a bare address becomes a link chip (ATT-15). True when it did something. */
  fun pasteFromClipboard(): Boolean {
    val clip = try { context.getSystemService(ClipboardManager::class.java)?.primaryClip } catch (_: Exception) { null } ?: return false
    return attachClip(clip)
  }

  internal fun attachClip(clip: ClipData): Boolean {
    val files = ArrayList<PickedFile>()
    var link = false
    for (i in 0 until clip.itemCount) {
      val it = clip.getItemAt(i)
      val uri = it.uri
      if (uri != null) {
        val mime = clip.description.getMimeType(0).takeIf { m -> m != "text/uri-list" && m != "text/plain" && m != "text/html" }
        if (uri.scheme == "content" || uri.scheme == "file") files.add(UriFiles.resolve(context, uri, mime, fallbackName = null)) else if (attachments.addLink(uri.toString())) link = true
      } else {
        val t = it.text?.toString()?.trim().orEmpty()
        if (t.isNotEmpty() && t.none { c -> c.isWhitespace() } && MediaRules.normalizeUrl(t) != null && Regex("""^https?://""", RegexOption.IGNORE_CASE).containsMatchIn(t)) { if (attachments.addLink(t)) link = true }
      }
    }
    if (files.isNotEmpty()) attachments.addFiles(files)
    return files.isNotEmpty() || link
  }
}

/**
 * Makes the [ComposerMedia] of one composer. [text] reads the box, [setText] writes it (dictation and shared text use it), [onAutoSend] sends the message when auto-send is on.
 * [forNewMessage] is true for the home composer: it takes the text shared from another app (ATT-22). [owner] false for the extra controls of the same composer, so toasts are told once.
 */
@Composable
fun rememberComposerMedia(text: () -> String, setText: (String) -> Unit, onAutoSend: () -> Unit, forNewMessage: Boolean = false, owner: Boolean = true): ComposerMedia {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val activity = LocalActivity.current
  val module = remember(container) { MediaModule.of(container) }
  val scope = androidx.compose.runtime.rememberCoroutineScope()
  val toast: (String) -> Unit = remember(container) { { s -> scope.launch { container.toast.show(s) } } }
  val textNow by rememberUpdatedState(text)
  val setTextNow by rememberUpdatedState(setText)
  val sendNow by rememberUpdatedState(onAutoSend)
  val lang by module.voiceLanguage.collectAsState()
  val auto by module.voiceAutoSend.collectAsState()
  val langNow by rememberUpdatedState(lang)
  val autoNow by rememberUpdatedState(auto)
  val handler = remember { android.os.Handler(android.os.Looper.getMainLooper()) }

  val voice = remember {
    VoiceController(
      engine = AndroidSpeechEngine(context),
      getText = { textNow() }, setText = { setTextNow(it) },
      language = { VoiceLanguages.effective(langNow) }, autoSend = { autoNow }, send = { sendNow() }, toast = toast,
      later = { ms, block -> handler.postDelayed({ block() }, ms) },
    )
  }
  val media = remember(module, voice) { ComposerMedia(module, module.attachments, voice, context, { textNow() }, toast) }

  // pickers and the camera (ActivityResult APIs: no storage or camera permission, ATT-25)
  val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
    if (uris.isNotEmpty()) module.attachments.addFiles(uris.map { UriFiles.resolve(context, it) })
  }
  var cameraPath by rememberSaveable { mutableStateOf<String?>(null) }
  val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
    val path = cameraPath
    cameraPath = null
    if (ok && path != null) {
      val f = java.io.File(path)
      if (f.exists() && f.length() > 0) module.attachments.addFiles(listOf(MemoryFileFromDisk(f, "photo-${System.currentTimeMillis() / 1000}.jpg")))
    }
  }
  val mic = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
    if (granted) voice.start()
    else if (activity != null && !androidx.core.app.ActivityCompat.shouldShowRequestPermissionRationale(activity, Manifest.permission.RECORD_AUDIO)) media.micBlocked = true
    else toast(VoiceError.NotAllowed.message)
  }
  media.launchFiles = { files.launch(arrayOf("*/*")) }
  media.launchCamera = {
    val (f, uri) = CameraCapture.newTarget(context)
    cameraPath = f.absolutePath
    camera.launch(uri)
  }
  media.requestMic = { mic.launch(Manifest.permission.RECORD_AUDIO) }

  // toasts of the controller and of dictation announcements
  // [owner]: one of the controls of a composer tells the messages (the chat's slots make several ComposerMedia, only one may collect them)
  if (owner) LaunchedEffect(module) { module.attachments.messages.collect { toast(it) } }
  val live = LocalAnnounce.current
  LaunchedEffect(voice) { voice.announcements.collect { live(it) } }

  // text shared from another app (ATT-22): into the box of the new message
  if (forNewMessage) {
    val shared by ShareInbox.text.collectAsState()
    LaunchedEffect(shared) {
      val s = shared
      if (s != null) { setTextNow(s); ShareInbox.consumeText() }
    }
  }

  // listening stops on pause, on leaving the view, and with Back (VOICE-05, VOICE-06, VOICE-16)
  val lifeOwner = LocalLifecycleOwner.current
  DisposableEffect(lifeOwner, voice) {
    val obs = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_PAUSE) { voice.cancel(); module.readAloud.stop() } }
    lifeOwner.lifecycle.addObserver(obs)
    onDispose { lifeOwner.lifecycle.removeObserver(obs); voice.release() }
  }
  val listening by voice.listening.collectAsState()
  BackHandler(enabled = listening) { voice.cancelAndRestore() }
  return media
}

/** A file written by the camera, read from disk, deleted after reading. */
private class MemoryFileFromDisk(private val f: java.io.File, override val name: String) : PickedFile {
  override val mime: String = "image/jpeg"
  override val size: Long get() = f.length()
  override fun open(): java.io.InputStream = f.inputStream()
}

/** Screen-reader announcements ("Listening", "Stopped"): the screen provides one (default: nothing). */
val LocalAnnounce = androidx.compose.runtime.staticCompositionLocalOf<(String) -> Unit> { { } }

// ---------------------------------------------------------------- buttons

/** The "+" button (COMP-05): opens the menu "Add pictures, files or a link". */
@Composable
fun PlusButton(media: ComposerMedia, modifier: Modifier = Modifier, enabled: Boolean = true) {
  ZoIconButton(ZoIcons.Plus, "Add pictures, files or a link", { media.openPlus() }, modifier, enabled = enabled)
}

/** The mic button (COMP-06, COMP-25): red with a pulsing dot while listening; disabled while an answer is being written. */
@Composable
fun MicButton(media: ComposerMedia, modifier: Modifier = Modifier, busy: Boolean = false) {
  val on by media.voice.listening.collectAsState()
  val ok = media.voice.available
  MicButtonView(on, enabled = ok && (!busy || on), available = ok, onClick = { media.micPressed() }, modifier = modifier)
}

/** The mic button without the controller (also drawn by the screenshot tests). */
@Composable
fun MicButtonView(listening: Boolean, enabled: Boolean, available: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  val pulse by rememberInfiniteTransition(label = "mic").animateFloat(1f, 0.25f, infiniteRepeatable(tween(1100), RepeatMode.Reverse), label = "dot")
  val desc = when { !available -> "Voice input is not available on this phone"; listening -> "Stop listening"; else -> "Speak instead of typing" }
  Box(
    modifier.size(40.dp).clip(ZoTheme.shapes.button).background(if (listening) c.mic else Color.Transparent)
      .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
      .semantics { contentDescription = desc; stateDescription = if (listening) "Listening" else "Stopped" }
      .alpha(if (enabled) 1f else 0.4f),
    contentAlignment = Alignment.Center,
  ) {
    ZoIcon(ZoIcons.Mic, tint = if (listening) Color.White else c.icon, size = 20.dp)
    if (listening) Box(Modifier.align(Alignment.TopEnd).padding(top = 5.dp, end = 5.dp).size(7.dp).alpha(pulse).clip(androidx.compose.foundation.shape.CircleShape).background(Color.White))
  }
}

/** The speaker button under an answer (VOICE-10): "Read aloud" / "Stop reading". [id] tells which answer is being read, [markdown] is its text. */
@Composable
fun ReadAloudButton(id: String, markdown: String, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 34.dp, iconSize: androidx.compose.ui.unit.Dp = 18.dp) {
  val module = MediaModule.of(LocalAppContainer.current)
  val speaking by module.readAloud.speaking.collectAsState()
  val on = speaking == id
  ZoIconButton(
    if (on) ZoIcons.Stop else ZoIcons.Speaker, if (on) "Stop reading" else "Read aloud",
    { module.readAloud.toggle(id, markdown) }, modifier, tint = if (on) ZoTheme.colors.accent else ZoTheme.colors.icon, size = size, iconSize = iconSize,
  )
}

/** Put once in a chat screen: reading stops when the screen is left or the app is paused (VOICE-12). */
@Composable
fun ReadAloudLifecycle() {
  val module = MediaModule.of(LocalAppContainer.current)
  val owner = LocalLifecycleOwner.current
  DisposableEffect(owner) {
    val obs = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_PAUSE) module.readAloud.stop() }
    owner.lifecycle.addObserver(obs)
    onDispose { owner.lifecycle.removeObserver(obs); module.readAloud.stop() }
  }
}

// ---------------------------------------------------------------- keys, paste, drop

/** Esc cancels listening (or stops reading), Ctrl+Shift+M toggles the mic (VOICE-05, VOICE-12, VOICE-15). For hardware keyboards. */
fun Modifier.mediaKeys(media: ComposerMedia): Modifier = onPreviewKeyEvent { e ->
  if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
  when {
    e.key == Key.Escape && media.voice.listening.value -> { media.voice.cancelAndRestore(); true }
    e.key == Key.Escape && media.module.readAloud.speaking.value != null -> { media.module.readAloud.stop(); true }
    e.key == Key.M && e.isCtrlPressed && e.isShiftPressed -> { if (media.voice.available) media.micPressed(); true }
    else -> false
  }
}

/** A picture or file inserted by the keyboard (Gboard stickers, GIFs, pictures) or pasted with long-press becomes an attachment (ATT-15). Use on a `BasicTextField(TextFieldState)`. */
@OptIn(ExperimentalFoundationApi::class)
fun Modifier.attachmentPaste(media: ComposerMedia): Modifier = contentReceiver(object : ReceiveContentListener {
  override fun onReceive(transferableContent: TransferableContent): TransferableContent? {
    if (!transferableContent.hasMediaType(MediaType.Image) && !transferableContent.hasMediaType(MediaType.All)) return transferableContent
    val clip = transferableContent.clipEntry.clipData
    return if (media.attachClip(clip)) null else transferableContent
  }
})

/** Files dragged onto the composer from another window attach (ATT-16); a dragged address becomes a link chip. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun Modifier.attachmentDrop(media: ComposerMedia): Modifier {
  val activity = LocalActivity.current
  val target = remember(media) {
    object : DragAndDropTarget {
      override fun onEntered(event: DragAndDropEvent) { media.dropping = true }
      override fun onExited(event: DragAndDropEvent) { media.dropping = false }
      override fun onEnded(event: DragAndDropEvent) { media.dropping = false }
      override fun onDrop(event: DragAndDropEvent): Boolean {
        media.dropping = false
        val de = event.toAndroidDragEvent()
        val clip = de.clipData ?: return false
        runCatching { activity?.requestDragAndDropPermissions(de) }
        return media.attachClip(clip)
      }
    }
  }
  return dragAndDropTarget(shouldStartDragAndDrop = { true }, target = target)
}

// ---------------------------------------------------------------- sheets

/** The "Drop to attach" overlay (ATT-16). */
@Composable
fun DropOverlay(media: ComposerMedia) {
  if (!media.dropping) return
  Box(Modifier.fillMaxSize().background(ZoTheme.colors.scrim), contentAlignment = Alignment.Center) { DropCard() }
}

/** The card of the overlay: "Drop to attach / Pictures, PDF, ZIP, text and code files". */
@Composable
fun DropCard() {
  val c = ZoTheme.colors
  Column(
    Modifier.clip(ZoTheme.shapes.card).background(c.popover).border(1.dp, c.line, ZoTheme.shapes.card).padding(28.dp).semantics { contentDescription = "Drop to attach. Pictures, PDF, ZIP, text and code files" },
    horizontalAlignment = Alignment.CenterHorizontally,
  ) {
    ZoIcon(ZoIcons.File, tint = c.accent, size = 32.dp)
    Text("Drop to attach", style = ZoTheme.type.popTitle, color = c.text, modifier = Modifier.padding(top = 8.dp))
    Text("Pictures, PDF, ZIP, text and code files", style = ZoTheme.type.popSub, color = c.dim)
  }
}

/** All the sheets of the composer: the "+" menu, "Add a link", the preview, the microphone explanations, and the drop overlay. Put it once in a Box over the screen. */
@Composable
fun ComposerMediaSheets(media: ComposerMedia, overlay: Boolean = true) {
  if (media.plusOpen) PlusMenuSheet(media)
  if (media.linkSheetOpen) AddLinkSheet(media)
  media.preview?.let { PreviewSheet(it) { media.preview = null } }
  if (media.micRationale) MicRationaleSheet(media)
  if (media.micBlocked) MicBlockedSheet(media)
  if (overlay) DropOverlay(media)
}

/** ATT-01: "Upload images or files", "Take a photo" (when there is a camera), "Add a link"; and "Paste from clipboard" when the clipboard holds a picture or file. */
@Composable
fun PlusMenuSheet(media: ComposerMedia) {
  val close = { media.plusOpen = false }
  ZoBottomSheet("Add to your message", onDismiss = close) {
    PlusMenuBody(
      hasCamera = media.hasCamera, hasClipboardFile = media.clipboardHasFile(),
      onFiles = { close(); media.launchFiles() }, onCamera = { close(); media.launchCamera() },
      onLink = { close(); media.linkSheetOpen = true }, onPaste = { close(); media.pasteFromClipboard() },
    )
  }
}

/** The rows of the "+" menu (also drawn by the screenshot tests). */
@Composable
fun PlusMenuBody(hasCamera: Boolean, hasClipboardFile: Boolean, onFiles: () -> Unit, onCamera: () -> Unit, onLink: () -> Unit, onPaste: () -> Unit) {
  SheetRow(ZoIcons.Attach, "Upload images or files", "Pictures, PDF, ZIP, text and code", onFiles)
  if (hasCamera) SheetRow(ZoIcons.Camera, "Take a photo", "Use the camera", onCamera)
  SheetRow(ZoIcons.Link, "Add a link", "ZO-AI reads the page for you", onLink)
  if (hasClipboardFile) SheetRow(ZoIcons.Copy, "Paste from clipboard", "The picture or file you copied", onPaste)
}

/** ATT-18: the "Add a link" sheet. */
@Composable
fun AddLinkSheet(media: ComposerMedia) {
  var value by remember { mutableStateOf("") }
  val close = { media.linkSheetOpen = false }
  val go = { if (media.attachments.addLink(value)) close() }
  ZoBottomSheet("Add a link", onDismiss = close, footer = {
    SecondaryButton("Cancel", close)
    PrimaryButton("Add", go)
  }) { AddLinkBody(value, onDone = { go() }) { value = it } }
}

@Composable
fun AddLinkBody(value: String, onDone: () -> Unit = {}, onChange: (String) -> Unit) {
  ZoTextField(
    value, onChange, label = "Web address", placeholder = "https://example.com/page", keyboardType = KeyboardType.Uri,
    hint = "ZO-AI reads the page with its web tool after you send the message.",
    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = androidx.compose.ui.text.input.ImeAction.Done),
    keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { onDone() }),
  )
}

/** VOICE-08: the explanation before the first permission request. */
@Composable
fun MicRationaleSheet(media: ComposerMedia) {
  val close = { media.micRationale = false }
  ZoBottomSheet("Use the microphone?", onDismiss = close, footer = {
    SecondaryButton("Not now", close)
    PrimaryButton("Allow", { close(); media.requestMic() })
  }) { MicRationaleBody() }
}

@Composable
fun MicRationaleBody() {
  Text("ZO-AI uses the microphone only while you dictate a message, to turn your voice into text. Allow it?", style = ZoTheme.type.body, color = ZoTheme.colors.text2)
}

/** VOICE-08: permanently denied: a shortcut to the app's settings. */
@Composable
fun MicBlockedSheet(media: ComposerMedia) {
  val close = { media.micBlocked = false }
  ZoBottomSheet("The microphone is blocked", onDismiss = close, footer = {
    SecondaryButton("Close", close)
    PrimaryButton("Open settings", {
      close()
      media.context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", media.context.packageName, null)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    })
  }) { MicBlockedBody() }
}

@Composable
fun MicBlockedBody() {
  Text(VoiceError.NotAllowed.message, style = ZoTheme.type.body, color = ZoTheme.colors.text2, textAlign = TextAlign.Start)
}
