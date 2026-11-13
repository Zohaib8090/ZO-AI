package org.zohara.zoai.media

import java.io.ByteArrayOutputStream
import java.io.File
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** ATT-05, ATT-07, ATT-08: text, PDF and ZIP files, with the limits and sentences of the web page. */
class TextProcessingTest {
  @get:Rule val tmp = TemporaryFolder()

  private fun problem(block: () -> Unit): String = try { block(); fail("expected a problem"); "" } catch (e: AttachProblem) { e.message.orEmpty() }

  // ---------------------------------------------------------------- plain text

  @Test fun plainTextGetsTheAttachedFileHeader() {
    val t = TextProcessing.plainText("a.txt", 5, "hello".toByteArray(), 0)
    assertEquals("Attached file: a.txt\nhello", t.text)
    assertEquals(t.text.toByteArray().size, t.bytes)
    assertEquals("", t.note)
  }

  @Test fun aByteOrderMarkIsStrippedAndControlCharactersBecomeSpaces() {
    val t = TextProcessing.plainText("a.txt", 9, byteArrayOf(0xEF.toByte(), 0xBB.toByte(), 0xBF.toByte()) + "a\u0007b".toByteArray(), 0)
    assertEquals("Attached file: a.txt\na b", t.text)
  }

  @Test fun errorsAreTheWebSentences() {
    assertEquals("\"a.bin\" is not a text file.", problem { TextProcessing.plainText("a.bin", 3, byteArrayOf(65, 0, 66), 0) })
    assertEquals("\"a.txt\" is not UTF-8 text.", problem { TextProcessing.plainText("a.txt", 2, byteArrayOf(0xC3.toByte(), 0x28), 0) })
    assertEquals("\"big.txt\" is too big (201 KB). Text files can be up to 200 KB.", problem { TextProcessing.plainText("big.txt", 201L * 1024, ByteArray(0), 0) })
    val m = problem { TextProcessing.plainText("c.txt", 150_000, ByteArray(150_000) { 'a'.code.toByte() }, 300 * 1024) }
    assertEquals("\"c.txt\" does not fit: the files of one message can hold up to 400 KB of text in all (300 KB are used).", m)
  }

  @Test fun noRoomLeftIsSaidAsSuch() {
    val m = problem { TextProcessing.item("x.txt", "abc", 400 * 1024 - 50) }
    assertTrue(m, m.startsWith("There is no room left for \"x.txt\""))
  }

  @Test fun longTextIsCutToFitWithANote() {
    val t = TextProcessing.item("l.txt", "x".repeat(250_000), 0)
    assertEquals("cut to fit", t.note)
    assertTrue(t.text.endsWith("[… cut: the rest does not fit]"))
    assertTrue(t.bytes <= Lim.EACH)
  }

  // ---------------------------------------------------------------- PDF

  private class FakePdf(val texts: List<String>, var closed: Boolean = false) : PdfTextSource {
    override val pages get() = texts.size
    override fun pageText(page: Int) = texts[page - 1]
    override fun close() { closed = true }
  }

  @Test fun pdfPagesHaveMarkers() {
    val src = FakePdf(listOf("Hello  world \n second line", "Page two"))
    val t = TextProcessing.pdf("doc.pdf", 1000, { src }, 0)
    assertEquals("Attached file: doc.pdf\n[Page 1]\nHello world\n second line\n\n[Page 2]\nPage two", t.text)
    assertEquals("2 pages", t.note)
    assertTrue(src.closed)
    assertEquals("1 page", TextProcessing.pdf("d.pdf", 10, { FakePdf(listOf("x")) }, 0).note)
  }

  @Test fun pdfProgressAndRemoval() {
    val notes = ArrayList<String>()
    TextProcessing.pdf("d.pdf", 10, { FakePdf(listOf("a", "b", "c")) }, 0, progress = { notes.add(it) })
    assertEquals(listOf("Page 1 of 3", "Page 2 of 3", "Page 3 of 3"), notes)
    var n = 0
    val m = problem { TextProcessing.pdf("d.pdf", 10, { FakePdf(listOf("a", "b", "c")) }, 0, alive = { n++ < 0 }) }
    assertTrue(m, m.contains("has no text"))
  }

  @Test fun pdfCutToFitSaysHowManyPagesAreIn() {
    val page = "w".repeat(60_000)
    val t = TextProcessing.pdf("long.pdf", 10, { FakePdf(List(10) { page }) }, 0)
    assertTrue(t.text, t.text.contains("[… only "))
    assertTrue(t.note, t.note == "cut to fit" || t.note.matches(Regex("""\d+ of 10 pages""")))
    assertTrue(t.bytes <= Lim.EACH)
  }

  @Test fun pdfErrors() {
    assertEquals("\"a.pdf\" is password protected.", problem { TextProcessing.pdf("a.pdf", 10, { throw PdfPasswordException() }, 0) })
    assertEquals("\"a.pdf\" could not be read as a PDF.", problem { TextProcessing.pdf("a.pdf", 10, { throw IllegalStateException() }, 0) })
    assertEquals("\"a.pdf\" has no text that can be read (it may be a scan or pictures only).", problem { TextProcessing.pdf("a.pdf", 10, { FakePdf(listOf("", "  ")) }, 0) })
    assertEquals("\"a.pdf\" is too big (31.0 MB). PDFs can be up to 30 MB.", problem { TextProcessing.pdf("a.pdf", 31L * 1048576, { FakePdf(listOf("x")) }, 0) })
  }

  @Test fun pdfStopsAt500Pages() {
    val t = TextProcessing.pdf("many.pdf", 10, { FakePdf(List(600) { "p" }) }, 0)
    assertTrue(t.text, t.text.contains("[Page 500]"))
    assertFalse(t.text.contains("[Page 501]"))
    assertTrue(t.text.contains("[… only 500 of 600 pages are included]"))
  }

  // ---------------------------------------------------------------- ZIP

  private fun zip(entries: List<Pair<String, ByteArray>>, stored: Boolean = false): File {
    val out = ByteArrayOutputStream()
    ZipOutputStream(out).use { z ->
      for ((n, b) in entries) {
        val e = ZipEntry(n)
        if (stored) { e.method = ZipEntry.STORED; e.size = b.size.toLong(); e.compressedSize = b.size.toLong(); e.crc = java.util.zip.CRC32().also { it.update(b) }.value }
        z.putNextEntry(e); z.write(b); z.closeEntry()
      }
    }
    return tmp.newFile().also { it.writeBytes(out.toByteArray()) }
  }

  @Test fun zipListsEverythingAndIncludesTextOnly() {
    val f = zip(listOf("src/a.kt" to "fun a() {}".toByteArray(), "logo.png" to byteArrayOf(1, 2, 3), "inner.zip" to byteArrayOf(4), "bin.txt" to byteArrayOf(65, 0, 66), "bad.txt" to byteArrayOf(0xC3.toByte(), 0x28), "README" to "read me".toByteArray()))
    val t = TextProcessing.zip("pack.zip", f, 0)
    assertTrue(t.zip)
    assertEquals("6 files", t.note)
    assertTrue(t.text.startsWith("Attached file: pack.zip\nZIP archive with 6 files. Contents:\n"))
    assertTrue(t.text, t.text.contains("- src/a.kt (10 B)\n"))
    assertTrue(t.text, t.text.contains("- logo.png (3 B, not text: name only)\n"))
    assertTrue(t.text, t.text.contains("- inner.zip (1 B, archive inside: not opened)\n"))
    assertTrue(t.text, t.text.contains("- bin.txt (3 B, not text: name only)\n"))
    assertTrue(t.text, t.text.contains("- bad.txt (2 B, not UTF-8: name only)\n"))
    assertTrue(t.text, t.text.contains("\nText files from the archive:\n--- src/a.kt ---\nfun a() {}\n"))
    assertTrue(t.text, t.text.contains("--- README ---\nread me\n"))
    assertFalse(t.text.contains("--- logo.png ---"))
  }

  @Test fun zipTooManyEntries() {
    val f = zip((1..201).map { "f$it.txt" to "x".toByteArray() })
    assertEquals("\"many.zip\" has more than 200 entries, which is too many to read.", problem { TextProcessing.zip("many.zip", f, 0) })
  }

  @Test fun zipThatWouldUnpackTooBig() {
    val f = zip(listOf("a.txt" to ByteArray(1_500_000) { 'a'.code.toByte() }, "b.txt" to ByteArray(1_500_000) { 'b'.code.toByte() }))
    val m = problem { TextProcessing.zip("bomb.zip", f, 0) }
    assertTrue(m, m.startsWith("\"bomb.zip\" would unpack to 2.9 MB, more than the 2 MB that can be read."))
  }

  @Test fun zipEntriesOverTheEntryLimitAreNamedNotRead() {
    val f = zip(listOf("big.txt" to ByteArray(150_000) { 'a'.code.toByte() }, "ok.txt" to "fine".toByteArray()))
    val t = TextProcessing.zip("z.zip", f, 0)
    assertTrue(t.text, t.text.contains("- big.txt (146 KB, too big to include)\n"))
    assertTrue(t.text, t.text.contains("--- ok.txt ---\nfine"))
    assertFalse(t.text.contains("aaaaaaaa"))
  }

  @Test fun zipThatLiesAboutItsSizeIsCountedByWhatComesOut() {
    // the central directory says 10 bytes, the data is 300 KB: the real bytes are counted (the entry is "too big to include")
    val real = ByteArray(300_000) { 'q'.code.toByte() }
    val f = zip(listOf("liar.txt" to real, "ok.txt" to "fine".toByteArray()))
    val b = f.readBytes()
    // patch the "uncompressed size" (offset 24) of the first central directory header (signature 50 4B 01 02)
    var i = 0
    while (i < b.size - 4 && !(b[i] == 0x50.toByte() && b[i + 1] == 0x4B.toByte() && b[i + 2] == 0x01.toByte() && b[i + 3] == 0x02.toByte())) i++
    b[i + 24] = 10; b[i + 25] = 0; b[i + 26] = 0; b[i + 27] = 0
    f.writeBytes(b)
    val t = TextProcessing.zip("liar.zip", f, 0)
    assertTrue(t.text, t.text.contains("- liar.txt (too big to include)\n"))
    assertFalse(t.text.contains("qqqqqqqq"))
    assertTrue(t.text.contains("--- ok.txt ---\nfine"))
  }

  @Test fun zipErrors() {
    assertEquals("\"x.zip\" could not be read as a ZIP file.", problem { TextProcessing.zip("x.zip", tmp.newFile().also { it.writeText("not a zip") }, 0) })
  }

  @Test fun zipProgressIsReportedPerEntry() {
    val notes = ArrayList<String>()
    TextProcessing.zip("p.zip", zip(listOf("a.txt" to "a".toByteArray(), "b.txt" to "b".toByteArray())), 0) { notes.add(it) }
    assertEquals(listOf("Entry 1 of 2", "Entry 2 of 2"), notes)
  }
}
