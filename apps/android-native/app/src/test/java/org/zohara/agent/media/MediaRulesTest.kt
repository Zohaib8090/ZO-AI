package org.zohara.agent.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** ATT-06, ATT-09, ATT-18, ATT-20, ATT-23, COMP-21: the pure rules, with the web page's own sentences. */
class MediaRulesTest {
  @Test fun kindsByTypeAndName() {
    assertEquals(FileKind.Pdf, MediaRules.kindOf("application/pdf", "x"))
    assertEquals(FileKind.Pdf, MediaRules.kindOf(null, "Report.PDF"))
    assertEquals(FileKind.Zip, MediaRules.kindOf("application/x-zip-compressed", "a.bin"))
    assertEquals(FileKind.Zip, MediaRules.kindOf("application/octet-stream", "a.zip"))
    assertEquals(FileKind.Image, MediaRules.kindOf("image/png", "a"))
    assertEquals(FileKind.Image, MediaRules.kindOf("image/heic", "IMG.HEIC"))
    assertEquals(FileKind.Image, MediaRules.kindOf(null, "photo.jpeg"))
    assertEquals(FileKind.Image, MediaRules.kindOf("application/octet-stream", "photo.webp"))
    assertEquals(FileKind.Text, MediaRules.kindOf("text/markdown", "a"))
    assertEquals(FileKind.Text, MediaRules.kindOf("application/json", "a"))
    assertEquals(FileKind.Text, MediaRules.kindOf("application/octet-stream", "main.rs"))
    assertEquals(FileKind.Text, MediaRules.kindOf(null, "Dockerfile"))
    assertEquals(FileKind.Text, MediaRules.kindOf(null, "README"))
    assertNull(MediaRules.kindOf("video/mp4", "a.mp4"))
    assertNull(MediaRules.kindOf("application/vnd.openxmlformats-officedocument.wordprocessingml.document", "a.docx"))
    assertNull(MediaRules.kindOf(null, "setup.exe"))
    assertNull(MediaRules.kindOf(null, "noextension"))
  }

  @Test fun refusalsNameWhatIsNotSupported() {
    val tail = " files are not supported. You can attach pictures, PDF, ZIP and text or code files."
    assertEquals("\"clip.mp4\" cannot be attached: video$tail", MediaRules.refusal("video/mp4", "clip.mp4"))
    assertEquals("\"a.mp3\" cannot be attached: audio$tail", MediaRules.refusal("audio/mpeg", "a.mp3"))
    assertEquals("\"a.docx\" cannot be attached: .docx (Office)$tail", MediaRules.refusal(null, "a.docx"))
    assertEquals("\"a.exe\" cannot be attached: program$tail", MediaRules.refusal(null, "a.exe"))
    assertEquals("\"a.7z\" cannot be attached: .7z archive$tail", MediaRules.refusal(null, "a.7z"))
    assertEquals("\"thing\" cannot be attached: application/x-foo$tail", MediaRules.refusal("application/x-foo", "thing"))
    assertTrue(MediaRules.heicRefusal("a.heic").contains("HEIC"))
  }

  @Test fun namesAreCleaned() {
    assertEquals("a b.txt", MediaRules.cleanName("a/b.txt").replace("a b", "a b"))
    assertEquals("file", MediaRules.cleanName("  "))
    assertEquals("file", MediaRules.cleanName(null))
    assertEquals(120, MediaRules.cleanName("x".repeat(300)).length)
    assertFalse(MediaRules.cleanName("a\"<b>&c`d\u0007e").any { it in "\"<>&`\u0007" })
  }

  @Test fun sizesLikeTheWeb() {
    assertEquals("12 B", MediaRules.kb(12))
    assertEquals("3 KB", MediaRules.kb(3 * 1024))
    assertEquals("1.5 MB", MediaRules.kb(1_572_864))
  }

  @Test fun thumbnailsMustBeSmallDataUris() {
    assertTrue(MediaRules.safeThumb("data:image/jpeg;base64,/9j/4AAQ"))
    assertTrue(MediaRules.safeThumb("data:image/png;base64,iVBOR=="))
    assertFalse(MediaRules.safeThumb("data:image/svg+xml;base64,AAAA"))
    assertFalse(MediaRules.safeThumb("data:image/jpeg;base64,AA AA"))
    assertFalse(MediaRules.safeThumb("https://example.com/a.png"))
    assertFalse(MediaRules.safeThumb(null))
    assertFalse(MediaRules.safeThumb("data:image/jpeg;base64," + "A".repeat(20_000)))
  }

  @Test fun webAddresses() {
    assertEquals("https://example.com/", MediaRules.normalizeUrl("example.com"))
    assertEquals("https://example.com/a?b=1#c", MediaRules.normalizeUrl("https://Example.com/a?b=1#c"))
    assertEquals("http://example.com:8080/x", MediaRules.normalizeUrl("http://example.com:8080/x"))
    assertEquals("https://example.com/", MediaRules.normalizeUrl("https://example.com:443"))
    assertNull(MediaRules.normalizeUrl("not a url"))
    assertNull(MediaRules.normalizeUrl("hello"))
    assertNull(MediaRules.normalizeUrl("https://user:pw@example.com"))
    assertNull(MediaRules.normalizeUrl("https://localhost/x"))
    assertNull(MediaRules.normalizeUrl("ftp://example.com"))
    assertNull(MediaRules.normalizeUrl("https://example.com/" + "a".repeat(2000)))
    assertEquals("http://192.168.1.2/", MediaRules.normalizeUrl("http://192.168.1.2"))
  }

  @Test fun hostsLoseWww() { assertEquals("example.com", MediaRules.hostOf("https://www.example.com/x")) }

  @Test fun addressesInTheTextBecomeChips() {
    val t = "see https://a.example.com/x, and (https://b.example.com/y). Also https://a.example.com/x again http://c.example.org!"
    assertEquals(listOf("https://a.example.com/x", "https://b.example.com/y", "http://c.example.org/"), MediaRules.detectLinks(t, emptySet()))
    assertEquals(listOf("https://b.example.com/y", "http://c.example.org/"), MediaRules.detectLinks(t, setOf("https://a.example.com/x")))
    val many = (1..9).joinToString(" ") { "https://s$it.example.com" }
    assertEquals(5, MediaRules.detectLinks(many, emptySet()).size)
  }

  @Test fun theChipsXRemovesTheAddressFromTheText() {
    val t = "read https://a.example.com/x, please   thanks"
    assertEquals("read , please thanks", MediaRules.removeFromText(t, "https://a.example.com/x"))
  }

  @Test fun linkChipsAreAddedToTheMessage() {
    assertEquals("", MediaRules.linkText(emptyList()))
    assertEquals("Links to read (use your web tool):\n- https://a.example.com/\n- https://b.example.com/", MediaRules.linkText(listOf("https://a.example.com/", "https://b.example.com/")))
    val taken = TakenAttachments(emptyList(), listOf("https://a.example.com/"), emptyList(), emptyList())
    assertEquals("hi\n\nLinks to read (use your web tool):\n- https://a.example.com/", taken.messageText(" hi "))
    assertEquals("Links to read (use your web tool):\n- https://a.example.com/", taken.messageText("  "))
  }

  @Test fun textIsCleanedAndCutByBytes() {
    assertEquals("a b\tc\nd", MediaRules.clean("a\u0000b\tc\nd"))
    assertEquals("x�y", MediaRules.clean("x\uD83Dy"))
    val s = "é".repeat(100) // 2 bytes each
    assertEquals(50, MediaRules.cut(s, 100).length)
    assertEquals(49, MediaRules.cut(s, 99).length)
    val e = "😀".repeat(10) // 4 bytes each, a surrogate pair
    assertEquals("😀".repeat(2), MediaRules.cut(e, 9))
    assertEquals("short", MediaRules.cut("short", 100))
  }
}
