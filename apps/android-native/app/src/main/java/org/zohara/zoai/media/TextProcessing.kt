package org.zohara.zoai.media

import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.util.zip.ZipFile
import org.zohara.zoai.media.MediaRules.clean
import org.zohara.zoai.media.MediaRules.cut
import org.zohara.zoai.media.MediaRules.kb
import org.zohara.zoai.media.MediaRules.utf8Len

/** A PDF opened for reading text, page by page. The real one is [PdfBoxSource]; tests use a fake. */
interface PdfTextSource : AutoCloseable {
  val pages: Int
  fun pageText(page: Int): String
}

/** The PDF needs a password. */
class PdfPasswordException : Exception()

/**
 * Text, code, PDF and ZIP files into [TextItem]s, with the limits and sentences of the web page (ATT-05 to ATT-08). Pure Kotlin, no Android classes.
 * [used] is how many bytes of text the message already holds (the 400 KB limit counts all of them).
 */
object TextProcessing {
  /** how many bytes one more file may have, after its own "Attached file: name" header of [extra] bytes (`aTextRoom`) */
  fun room(used: Int, extra: Int): Int = minOf(Lim.EACH, Lim.TOTAL - used) - extra

  fun head(name: String) = "Attached file: $name\n"

  /** `aTextItem`: cleans [body] and cuts it to fit. */
  fun item(name: String, body: String, used: Int, note: String = "", zip: Boolean = false): TextItem {
    val head = head(name)
    val room = room(used, utf8Len(head))
    if (room < 200) throw AttachProblem("There is no room left for \"$name\": the files of one message can hold up to ${Lim.TOTAL / 1024} KB of text.")
    var text = clean(body)
    var cutIt = false
    if (utf8Len(text) > room) { text = cut(text, room - 60) + "\n[… cut: the rest does not fit]"; cutIt = true }
    val full = head + text
    return TextItem(0, name, full, utf8Len(full), if (cutIt) "cut to fit" else note, zip)
  }

  /** UTF-8 text, or null when the bytes are not valid UTF-8 */
  fun decodeUtf8(bytes: ByteArray): String? = try {
    Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString()
  } catch (_: CharacterCodingException) { null }

  /** `aProcText`: a plain text file is added whole or not at all. */
  fun plainText(name: String, size: Long, bytes: ByteArray, used: Int): TextItem {
    if (size > Lim.EACH) throw AttachProblem("\"$name\" is too big (${kb(size)}). Text files can be up to ${Lim.EACH / 1024} KB.")
    if (bytes.any { it == 0.toByte() }) throw AttachProblem("\"$name\" is not a text file.")
    val s0 = decodeUtf8(bytes) ?: throw AttachProblem("\"$name\" is not UTF-8 text.")
    val s = s0.removePrefix("\uFEFF")
    if (utf8Len(clean(s)) > room(used, utf8Len(head(name)))) {
      throw AttachProblem("\"$name\" does not fit: the files of one message can hold up to ${Lim.TOTAL / 1024} KB of text in all (${Math.round(used / 1024.0)} KB are used).")
    }
    return item(name, s, used)
  }

  /** `aProcPdf`: up to 500 pages, "[Page N]" markers, cut to fit. [alive] turns false when the person removed the chip. */
  fun pdf(name: String, size: Long, open: () -> PdfTextSource, used: Int, alive: () -> Boolean = { true }, progress: (String) -> Unit = {}): TextItem {
    if (size > Lim.PDF_FILE) throw AttachProblem("\"$name\" is too big (${kb(size)}). PDFs can be up to ${Lim.PDF_FILE / 1048576} MB.")
    val src = try { open() } catch (_: PdfPasswordException) { throw AttachProblem("\"$name\" is password protected.") } catch (_: Exception) { throw AttachProblem("\"$name\" could not be read as a PDF.") }
    val room = room(used, utf8Len(head(name))) - 120
    val pages = src.pages
    val sb = StringBuilder()
    var read = 0
    src.use {
      var p = 1
      while (p <= pages && p <= Lim.PDF_PAGES) {
        if (!alive()) break
        progress("Page $p of $pages")
        val raw = try { src.pageText(p) } catch (_: Exception) { "" }
        val line = raw.replace(Regex("""[ \t]+\n"""), "\n").replace(Regex(""" {2,}"""), " ").trim()
        sb.append("\n[Page ").append(p).append("]\n").append(line).append('\n')
        read = p
        if (utf8Len(sb.toString()) > room) break
        p++
      }
    }
    var out = sb.toString()
    if (read < pages && utf8Len(out) > room - 60) out = cut(out, room - 60) // leave room for the sentence that says how many pages are in
    if (out.replace(Regex("""\[Page \d+]"""), "").isBlank()) throw AttachProblem("\"$name\" has no text that can be read (it may be a scan or pictures only).")
    val tail = if (read < pages) "\n[… only $read of $pages pages are included]" else ""
    val it2 = item(name, out.trim() + tail, used, note = "$pages ${if (pages == 1) "page" else "pages"}")
    return if (read < pages && it2.note != "cut to fit") it2.copy(note = "$read of $pages pages") else it2
  }

  /** `aProcZip`: at most 25 MB, 200 entries, 2 MB unpacked, 100 KB per entry; what comes out is counted, whatever the archive says. */
  fun zip(name: String, file: File, used: Int, progress: (String) -> Unit = {}): TextItem {
    if (file.length() > Lim.ZIP_FILE) throw AttachProblem("\"$name\" is too big (${kb(file.length())}). ZIP files can be up to ${Lim.ZIP_FILE / 1048576} MB.")
    val zf = try { ZipFile(file) } catch (_: Exception) { throw AttachProblem("\"$name\" could not be read as a ZIP file.") }
    zf.use {
      val entries = try { zf.entries().toList() } catch (_: Exception) { throw AttachProblem("\"$name\" could not be read as a ZIP file.") }
      if (entries.size > Lim.ZIP_ENTRIES) throw AttachProblem("\"$name\" has more than ${Lim.ZIP_ENTRIES} entries, which is too many to read.")
      val all = entries.filter { !it.isDirectory }
      val declared = { e: java.util.zip.ZipEntry -> maxOf(e.size, 0L) }
      val total = all.sumOf(declared)
      if (total > Lim.ZIP_TOTAL) throw AttachProblem("\"$name\" would unpack to ${kb(total)}, more than the ${Lim.ZIP_TOTAL / 1048576} MB that can be read.")
      val list = StringBuilder()
      val body = StringBuilder()
      var done = 0
      var usedZip = 0
      for (e in all) {
        val path = clean(e.name).take(160)
        val size = declared(e)
        progress("Entry ${++done} of ${all.size}")
        if (MediaRules.ARCHIVE_NAME.containsMatchIn(e.name)) { list.append("- $path (${kb(size)}, archive inside: not opened)\n"); continue }
        val texty = MediaRules.isTextName(path.substringAfterLast('/'))
        if (!texty) { list.append("- $path (${kb(size)}, not text: name only)\n"); continue }
        if (size > Lim.ZIP_ENTRY || usedZip + size > Lim.ZIP_TOTAL) { list.append("- $path (${kb(size)}, too big to include)\n"); continue }
        val bytes = try { readCapped(zf, e, Lim.ZIP_ENTRY) } catch (_: Exception) { null }
        if (bytes == null) { list.append("- $path (too big to include)\n"); continue }
        if (bytes.any { it == 0.toByte() }) { list.append("- $path (${kb(bytes.size)}, not text: name only)\n"); continue }
        val s = decodeUtf8(bytes)
        if (s == null) { list.append("- $path (${kb(bytes.size)}, not UTF-8: name only)\n"); continue }
        usedZip += bytes.size
        list.append("- $path (${kb(bytes.size)})\n")
        body.append("\n--- $path ---\n").append(s.removePrefix("\uFEFF")).append('\n')
      }
      val n = all.size
      val text = "ZIP archive with $n file${if (n == 1) "" else "s"}. Contents:\n$list" + if (body.isNotEmpty()) "\nText files from the archive:$body" else ""
      return item(name, text, used, note = "$n ${if (n == 1) "file" else "files"}", zip = true)
    }
  }

  /** reads an entry and counts the bytes that really come out; null when more than [cap] */
  private fun readCapped(zf: ZipFile, e: java.util.zip.ZipEntry, cap: Int): ByteArray? {
    zf.getInputStream(e).use { ins ->
      val out = java.io.ByteArrayOutputStream()
      val buf = ByteArray(8192)
      var n = 0
      while (true) {
        val r = ins.read(buf)
        if (r < 0) break
        n += r
        if (n > cap) return null
        out.write(buf, 0, r)
      }
      return out.toByteArray()
    }
  }
}
