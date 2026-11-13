package org.zohara.chat.media

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import java.io.File
import java.io.InputStream
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** What another app shared with Zohara Chat (ATT-22): text for the box, links for chips, files for the picture and file readers. */
class SharedContent(val text: String, val links: List<String>, val files: List<PickedFile>, val skippedSome: Boolean)

/** A shared file copied into the app's cache (a share grant ends with the sharing activity, so the file is copied at once). */
class CachedFile(private val file: File, override val name: String, override val mime: String?) : PickedFile {
  override val size: Long get() = file.length()
  override fun open(): InputStream = file.inputStream()
}

object ShareParser {
  /** at most this many files per share: 4 pictures and 6 files */
  const val MAX_FILES = Lim.IMAGES + Lim.TEXTS
  const val MAX_TEXT = 50_000

  /** Reads a SEND or SEND_MULTIPLE intent. A bare address becomes a link chip and leaves the text empty; any other text goes to the box. Null for other intents. */
  fun parse(context: Context, intent: Intent, copy: (Uri, String?) -> PickedFile? = { u, m -> copyToCache(context, u, m) }): SharedContent? {
    if (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE) return null
    val raw = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString().orEmpty().trim().take(MAX_TEXT)
    val links = ArrayList<String>()
    var text = raw
    if (raw.isNotEmpty() && raw.none { it.isWhitespace() }) MediaRules.normalizeUrl(raw)?.takeIf { Regex("""^https?://""", RegexOption.IGNORE_CASE).containsMatchIn(raw) }?.let { links.add(it); text = "" }
    val uris = LinkedHashSet<Uri>()
    if (intent.action == Intent.ACTION_SEND) streamOf(intent)?.let { uris.add(it) } else streamsOf(intent).forEach { uris.add(it) }
    intent.clipData?.let { c -> for (i in 0 until c.itemCount) c.getItemAt(i).uri?.let { u -> if (u.scheme == "content" || u.scheme == "file") uris.add(u) } }
    val hint = intent.type?.takeIf { !it.contains('*') }
    val files = ArrayList<PickedFile>()
    var skipped = false
    for (u in uris) {
      if (files.size >= MAX_FILES) { skipped = true; break }
      val f = copy(u, hint)
      if (f == null) skipped = true else files.add(f)
    }
    return SharedContent(text, links, files, skipped)
  }

  @Suppress("DEPRECATION")
  private fun streamOf(i: Intent): Uri? = if (Build.VERSION.SDK_INT >= 33) i.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java) else i.getParcelableExtra(Intent.EXTRA_STREAM) as? Uri

  @Suppress("DEPRECATION")
  private fun streamsOf(i: Intent): List<Uri> = (if (Build.VERSION.SDK_INT >= 33) i.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java) else i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)).orEmpty()

  /** copies a shared uri into `cache/shared`, at most 40 MB; null when it can not be read or is bigger */
  fun copyToCache(context: Context, uri: Uri, hintMime: String?): PickedFile? = try {
    val src = UriFiles.resolve(context, uri, hintMime)
    val dir = File(context.cacheDir, "shared").apply { mkdirs() }
    dir.listFiles()?.filter { System.currentTimeMillis() - it.lastModified() > 24 * 3600 * 1000L }?.forEach { it.delete() }
    val f = src.copyTo(dir, Lim.IMG_FILE)
    if (f == null) null else CachedFile(f, src.name, src.mime)
  } catch (_: Exception) { null }
}

/**
 * Where shared things wait for the chat screens (ATT-22): files and links go straight into the attachment strip, the text waits in [text] for the composer of a new message
 * (`rememberComposerMedia(forNewMessage = true)` puts it in the box), and [openRequests] asks the app to open a new message ([ShareNavigation]).
 */
object ShareInbox {
  const val EXTRA_SHARED = "org.zohara.chat.extra.SHARED"
  const val SKIPPED = "Some shared files were too big or too many and were skipped."

  private val _text = MutableStateFlow<String?>(null)
  val text: StateFlow<String?> = _text.asStateFlow()
  private val _open = MutableStateFlow(0)
  val openRequests: StateFlow<Int> = _open.asStateFlow()

  fun deliver(module: MediaModule?, content: SharedContent, toast: (String) -> Unit) {
    if (module != null) {
      if (content.files.isNotEmpty()) module.attachments.clear()
      content.links.forEach { module.attachments.addLink(it) }
      if (content.files.isNotEmpty()) module.attachments.addFiles(content.files)
    }
    if (content.text.isNotEmpty()) _text.value = content.text
    if (content.skippedSome) toast(SKIPPED)
    _open.value = _open.value + 1
  }

  fun consumeText() { _text.value = null }
  fun consumeOpen() { _open.value = 0 }
}

/** Put once in the main shell: a share from another app opens a new message ([onOpen] navigates home). */
@androidx.compose.runtime.Composable
fun ShareNavigation(onOpen: () -> Unit) {
  val n by ShareInbox.openRequests.collectAsState()
  LaunchedEffect(n) { if (n > 0) { onOpen(); ShareInbox.consumeOpen() } }
}
