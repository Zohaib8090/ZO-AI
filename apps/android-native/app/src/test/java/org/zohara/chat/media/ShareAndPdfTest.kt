package org.zohara.chat.media

import android.content.Intent
import android.net.Uri
import androidx.test.core.app.ApplicationProvider
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** ATT-22, PHN-08: what other apps share; ATT-07: a real PDF read with PdfBox. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ShareAndPdfTest {
  @get:Rule val tmp = TemporaryFolder()
  private val context get() = ApplicationProvider.getApplicationContext<android.app.Application>()

  private fun copy(u: Uri, mime: String?): PickedFile? = if (u.toString().contains("bad")) null else MemoryFile(u.lastPathSegment.orEmpty(), mime, ByteArray(3))

  @Test fun aSharedAddressBecomesALinkChipAndLeavesTheTextEmpty() {
    val i = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "https://example.com/article")
    val s = ShareParser.parse(context, i, ::copy)!!
    assertEquals("", s.text)
    assertEquals(listOf("https://example.com/article"), s.links)
    assertTrue(s.files.isEmpty())
  }

  @Test fun sharedTextWithWordsGoesToTheBox() {
    val i = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, "  Read this https://example.com/a and tell me  ")
    val s = ShareParser.parse(context, i, ::copy)!!
    assertEquals("Read this https://example.com/a and tell me", s.text)
    assertTrue(s.links.isEmpty())
  }

  @Test fun sharedFilesGoThroughTheCopyAndKeepTheirType() {
    val i = Intent(Intent.ACTION_SEND).setType("image/png").putExtra(Intent.EXTRA_STREAM, Uri.parse("content://x/pic.png"))
    val s = ShareParser.parse(context, i, ::copy)!!
    assertEquals(1, s.files.size)
    assertEquals("image/png", s.files[0].mime)
    assertFalse(s.skippedSome)
  }

  @Test fun aWildcardTypeIsNotPassedAsAHint() {
    val i = Intent(Intent.ACTION_SEND_MULTIPLE).setType("*/*").putParcelableArrayListExtra(Intent.EXTRA_STREAM, arrayListOf(Uri.parse("content://x/a.txt")))
    assertNull(ShareParser.parse(context, i, ::copy)!!.files.single().mime)
  }

  @Test fun tooManyOrUnreadableFilesAreSkippedWithTheNotice() {
    val uris = ArrayList((1..12).map { Uri.parse("content://x/f$it.txt") })
    val s = ShareParser.parse(context, Intent(Intent.ACTION_SEND_MULTIPLE).setType("text/plain").putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris), ::copy)!!
    assertEquals(10, s.files.size)
    assertTrue(s.skippedSome)
    val bad = ShareParser.parse(context, Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_STREAM, Uri.parse("content://x/bad.txt")), ::copy)!!
    assertTrue(bad.files.isEmpty()); assertTrue(bad.skippedSome)
    assertEquals("Some shared files were too big or too many and were skipped.", ShareInbox.SKIPPED)
  }

  @Test fun otherIntentsAreIgnored() {
    assertNull(ShareParser.parse(context, Intent(Intent.ACTION_VIEW), ::copy))
  }

  @Test fun theInboxHoldsTextUntilTheComposerTakesIt() {
    ShareInbox.consumeText(); ShareInbox.consumeOpen()
    val c = SharedContent("note", emptyList(), emptyList(), false)
    val toasts = ArrayList<String>()
    // a module is not needed when there are no files and no links
    ShareInbox.deliver(null, c) { toasts.add(it) }
    // in the app the chat plug takes the text at once and puts it in the box of the new message (ChatHooks.prefill); here the application did install it
    org.robolectric.shadows.ShadowLooper.idleMainLooper()
    assertTrue(ShareInbox.text.value == "note" || org.zohara.chat.chat.ChatHooks.prefill.value == "note")
    assertEquals(1, ShareInbox.openRequests.value)
    ShareInbox.consumeText(); ShareInbox.consumeOpen()
    assertNull(ShareInbox.text.value)
    assertEquals(0, ShareInbox.openRequests.value)
  }

  // ---------------------------------------------------------------- PDF

  private fun pdf(text: String): File {
    val content = "BT /F1 18 Tf 20 100 Td ($text) Tj ET"
    val sb = StringBuilder("%PDF-1.4\n")
    val offsets = ArrayList<Int>()
    fun obj(s: String) { offsets.add(sb.length); sb.append(s).append("\n") }
    obj("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj")
    obj("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj")
    obj("3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj")
    obj("4 0 obj\n<< /Length ${content.length} >>\nstream\n$content\nendstream\nendobj")
    obj("5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj")
    val xref = sb.length
    sb.append("xref\n0 6\n0000000000 65535 f \n")
    offsets.forEach { sb.append(String.format("%010d 00000 n \n", it)) }
    sb.append("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n$xref\n%%EOF\n")
    return tmp.newFile("t.pdf").also { it.writeBytes(sb.toString().toByteArray(Charsets.ISO_8859_1)) }
  }

  @Test fun aRealPdfGivesItsText() {
    val f = pdf("Hello from the PDF")
    val item = TextProcessing.pdf("t.pdf", f.length(), { PdfBoxSource.open(context, f) }, 0)
    assertTrue(item.text, item.text.contains("[Page 1]"))
    assertTrue(item.text, item.text.contains("Hello from the PDF"))
    assertEquals("1 page", item.note)
  }

  @Test fun aBrokenPdfIsRefusedWithTheSentence() {
    val f = tmp.newFile("bad.pdf").also { it.writeText("this is not a pdf at all") }
    try { TextProcessing.pdf("bad.pdf", f.length(), { PdfBoxSource.open(context, f) }, 0); org.junit.Assert.fail() } catch (e: AttachProblem) { assertEquals("\"bad.pdf\" could not be read as a PDF.", e.message) }
    assertNotNull(f)
  }
}
