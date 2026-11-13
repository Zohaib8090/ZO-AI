package org.zohara.chat.media

import java.io.File
import java.util.Base64
import java.util.Locale
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.zohara.chat.core.net.Attachment

/** What a send takes from the strip (`aTake`): the payload, the bubble facts, and the items to put back when nothing reached the model (ATT-21). */
class TakenAttachments(val items: List<AttItem>, val links: List<String>, val attachments: List<Attachment>, val meta: List<AttMeta>) {
  /** the message text with the links chips added after two newlines (COMP-21) */
  fun messageText(typed: String): String = listOf(typed.trim(), MediaRules.linkText(links)).filter { it.isNotEmpty() }.joinToString("\n\n")
}

/**
 * The list of things attached to the message being written (the web's `ATT`). Files are read one after another by one worker so the limits apply in order (ATT-14).
 * Everything the person must be told comes out of [messages] (shown as toasts).
 */
class AttachmentsController(
  private val scope: CoroutineScope,
  private val io: CoroutineDispatcher,
  private val tempDir: File,
  private val pdfOpener: (File) -> PdfTextSource,
  /** the page title for a link chip: nice to have, never needed to send (ATT-19) */
  private val linkTitle: suspend (String) -> String = { "" },
  private val imageProcessor: (name: String, size: Long, bytes: ByteArray) -> ImageItem = ImageProcessor::process,
) {
  private val _items = MutableStateFlow<List<AttItem>>(emptyList())
  val items: StateFlow<List<AttItem>> = _items.asStateFlow()

  private val _titles = MutableStateFlow<Map<String, String>>(emptyMap())
  /** link address to its page title ("" when the preview failed) */
  val titles: StateFlow<Map<String, String>> = _titles.asStateFlow()

  private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 32)
  val messages: SharedFlow<String> = _messages.asSharedFlow()

  private var seq = 0L
  private var pasted = 0
  private val requested = HashSet<String>()
  private var debounce: Job? = null
  private val queue = Channel<PickedFile>(Channel.UNLIMITED)

  init {
    scope.launch { for (f in queue) {  try { addOne(f) } catch (e: CancellationException) { throw e } catch (_: Exception) { } } }
  }

  val isBusy: Boolean get() = _items.value.any { it is BusyItem }
  val hasAny: Boolean get() = _items.value.isNotEmpty()
  private fun toast(s: String) { _messages.tryEmit(s) }

  // ---------------------------------------------------------------- files

  /** Adds files, one after another (ATT-14). */
  fun addFiles(files: List<PickedFile>) { files.forEach { queue.trySend(it) } }

  private fun displayName(f: PickedFile): String {
    val t = f.mime.orEmpty().lowercase(Locale.ROOT)
    return if (t.startsWith("image/") && (f.name.isBlank() || Regex("""^image\.(png|jpe?g|webp|gif)$""", RegexOption.IGNORE_CASE).matches(f.name))) {
      "pasted-picture-${++pasted}." + t.substringAfter('/').substringBefore(';').replace("jpeg", "jpg").ifEmpty { "png" }
    } else f.name
  }

  private fun usedText(): Int = _items.value.filterIsInstance<TextItem>().sumOf { it.bytes }

  private suspend fun addOne(file: PickedFile) {
    val kind = MediaRules.kindOf(file.mime, file.name)
    if (kind == null) { toast(MediaRules.refusal(file.mime, file.name)); return }
    val name = MediaRules.cleanName(displayName(file))
    val cur = _items.value
    if (kind == FileKind.Image) {
      if (cur.count { it is ImageItem || (it is BusyItem && it.kind == BusyAs.Image) } >= Lim.IMAGES) { toast("You can attach up to ${Lim.IMAGES} pictures to one message."); return }
    } else if (cur.count { it is TextItem || (it is BusyItem && it.kind == BusyAs.Text) } >= Lim.TEXTS) { toast("You can attach up to ${Lim.TEXTS} files to one message."); return }
    val busy = BusyItem(++seq, name, if (kind == FileKind.Image) BusyAs.Image else BusyAs.Text)
    _items.update { it + busy }
    val alive = { _items.value.any { it.id == busy.id } }
    val progress = { note: String -> _items.update { l -> l.map { if (it.id == busy.id) busy.copy(note = note) else it } } }
    try {
      val done: AttItem = withContext(io) { process(kind, file, name, usedText(), alive, progress) }
      if (done is ImageItem) {
        val total = _items.value.filterIsInstance<ImageItem>().sumOf { it.size }
        if (total + done.size > Lim.IMG_TOTAL) throw AttachProblem("These pictures are too big together (at most ${Lim.IMG_TOTAL / 1048576} MB). Remove one or use smaller pictures.")
      }
      val placed: AttItem = when (done) { is ImageItem -> done.withId(busy.id); is TextItem -> done.copy(id = busy.id); else -> done }
      _items.update { l -> if (l.any { it.id == busy.id }) l.map { if (it.id == busy.id) placed else it } else l } // removed while it was being read: dropped
    } catch (e: CancellationException) {
      throw e
    } catch (e: AttachProblem) {
      drop(busy.id); toast(e.message ?: "\"$name\" could not be read.")
    } catch (e: Exception) {
      drop(busy.id); toast("\"$name\" could not be read.")
    } catch (e: OutOfMemoryError) {
      drop(busy.id); toast("\"$name\" is too big to read on this phone.")
    }
  }

  private fun drop(id: Long) { _items.update { l -> l.filter { it.id != id } } }

  private fun process(kind: FileKind, file: PickedFile, name: String, used: Int, alive: () -> Boolean, progress: (String) -> Unit): AttItem {
    when (kind) {
      FileKind.Image -> {
        val bytes = file.readCapped(Lim.IMG_FILE) ?: throw AttachProblem("\"$name\" is too big (over ${Lim.IMG_FILE / 1048576} MB). Pictures can be up to ${Lim.IMG_FILE / 1048576} MB.")
        return imageProcessor(name, if (file.size >= 0) file.size else bytes.size.toLong(), bytes)
      }
      FileKind.Text -> {
        val bytes = file.readCapped(Lim.EACH.toLong()) ?: throw AttachProblem("\"$name\" is too big (${MediaRules.kb(maxOf(file.size, Lim.EACH + 1L))}). Text files can be up to ${Lim.EACH / 1024} KB.")
        return TextProcessing.plainText(name, bytes.size.toLong(), bytes, used)
      }
      FileKind.Pdf -> {
        progress("Loading the PDF reader…")
        val f = file.copyTo(tempDir, Lim.PDF_FILE) ?: throw AttachProblem("\"$name\" is too big (over ${Lim.PDF_FILE / 1048576} MB). PDFs can be up to ${Lim.PDF_FILE / 1048576} MB.")
        try { return TextProcessing.pdf(name, f.length(), { pdfOpener(f) }, used, alive, progress) } finally { f.delete() }
      }
      FileKind.Zip -> {
        progress("Loading the ZIP reader…")
        val f = file.copyTo(tempDir, Lim.ZIP_FILE) ?: throw AttachProblem("\"$name\" is too big (over ${Lim.ZIP_FILE / 1048576} MB). ZIP files can be up to ${Lim.ZIP_FILE / 1048576} MB.")
        try { return TextProcessing.zip(name, f, used, progress) } finally { f.delete() }
      }
    }
  }

  // ---------------------------------------------------------------- links

  /** Adds a link chip; false (and a toast) when the address is not good (ATT-18). */
  fun addLink(raw: String): Boolean {
    val url = MediaRules.normalizeUrl(raw)
    if (url == null) { toast("That does not look like a web address."); return false }
    val cur = _items.value
    if (cur.any { it is LinkItem && it.url == url }) { toast("That link is already added."); return false }
    if (cur.count { it is LinkItem } >= Lim.LINKS) { toast("You can add up to ${Lim.LINKS} links to one message."); return false }
    _items.update { it + LinkItem(++seq, url, MediaRules.hostOf(url), _titles.value[url].orEmpty()) }
    fetchTitle(url)
    return true
  }

  private fun fetchTitle(url: String) {
    if (!requested.add(url)) return
    scope.launch {
      val t = try { withContext(io) { linkTitle(url).trim() } } catch (e: CancellationException) { throw e } catch (_: Exception) { "" }
      _titles.update { it + (url to t) }
      _items.update { l -> l.map { if (it is LinkItem && it.url == url) it.copy(title = t) else it } }
    }
  }

  private fun linkUrls(): Set<String> = _items.value.filterIsInstance<LinkItem>().map { it.url }.toSet()

  /** The addresses in the message text as chips (they stay in the text; ATT-20). */
  fun detected(text: String): List<LinkItem> =
    MediaRules.detectLinks(text, linkUrls()).mapIndexed { i, u -> LinkItem(-(i + 1L), u, MediaRules.hostOf(u), _titles.value[u].orEmpty(), detected = true) }

  /** Previews of typed addresses are fetched 900 ms after typing stops. */
  fun onTextChanged(text: String) {
    debounce?.cancel()
    if (!text.contains("http", ignoreCase = true)) return
    debounce = scope.launch {
      delay(900)
      detected(text).forEach { fetchTitle(it.url) }
    }
  }

  fun removeFromText(text: String, url: String): String = MediaRules.removeFromText(text, url)

  // ---------------------------------------------------------------- list

  fun remove(id: Long) { drop(id) }
  fun clear() { _items.value = emptyList() }

  /** Hands over everything for a send and empties the strip. [TakenAttachments.attachments] are the pictures and files (links go into the text). */
  fun take(): TakenAttachments {
    val items = _items.value
    val list = ArrayList<Attachment>()
    val meta = ArrayList<AttMeta>()
    for (i in items) when (i) {
      is ImageItem -> {
        list.add(Attachment.Image(i.name, i.mime, Base64.getEncoder().encodeToString(i.bytes), i.thumb.takeIf { it.isNotEmpty() }, i.size))
        meta.add(AttMeta("image", i.name, i.size, i.thumb, image = i.bytes))
      }
      is TextItem -> { list.add(Attachment.TextFile(i.name, i.text)); meta.add(AttMeta("text", i.name, i.bytes, text = i.text)) }
      else -> Unit
    }
    val links = items.filterIsInstance<LinkItem>().map { it.url }
    for (m in meta) rememberSent(m)
    _items.value = emptyList()
    return TakenAttachments(items.filter { it !is BusyItem }, links, list, meta)
  }

  // ---------------------------------------------------------------- what was sent (CHAT-04)

  private val sent = object : LinkedHashMap<String, AttMeta>() { override fun removeEldestEntry(e: MutableMap.MutableEntry<String, AttMeta>?) = size > 16 }
  private fun sentKey(kind: String, name: String, size: Long) = "$kind|$name|$size"
  @Synchronized private fun rememberSent(m: AttMeta) { sent[sentKey(m.kind, m.name, m.size.toLong())] = m }

  /**
   * What a tap on a picture or file of a bubble shows (CHAT-04, ATT-12): the full picture or the text while this phone still has what it sent (the last few things),
   * otherwise the small thumbnail the saved chat keeps, or only the name and size for a file.
   */
  @Synchronized fun previewForSent(kind: String, name: String, size: Long, thumb: String?): PreviewTarget {
    val m = sent[sentKey(kind, name, size)]
    return if (kind == "image") PreviewTarget.Picture(name, m?.image, m?.thumb?.ifEmpty { null } ?: thumb, 0, 0, size)
    else PreviewTarget.TextFile(name, m?.text?.ifEmpty { null } ?: "Attached file: $name (${MediaRules.kb(size)})\nThe text is not kept on this phone after the app is closed.")
  }

  /** A send failed before anything reached the model: the attachments go back to the strip (ATT-21). */
  fun restore(taken: TakenAttachments) { _items.update { taken.items + it } }
}
