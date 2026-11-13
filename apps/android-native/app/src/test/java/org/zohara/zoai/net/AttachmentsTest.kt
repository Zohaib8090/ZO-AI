package org.zohara.zoai.core.net

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AttachmentsTest {
  @Test fun namesAreCleaned() {
    assertEquals("file", Attachments.cleanName("///"))
    assertEquals("ab.txt", Attachments.cleanName("a/b\\\"<>.txt").let { if (it == "ab.txt") it else it })
    assertEquals(120, Attachments.cleanName("x".repeat(500)).length)
  }

  @Test fun sniffsImages() {
    assertEquals("image/png", Attachments.sniffImageMime(byteArrayOf(0x89.toByte(), 'P'.code.toByte(), 'N'.code.toByte(), 'G'.code.toByte(), 13, 10, 26, 10)))
    assertEquals("image/jpeg", Attachments.sniffImageMime(byteArrayOf(0xFF.toByte(), 0xD8.toByte(), 0xFF.toByte(), 0)))
    assertNull(Attachments.sniffImageMime("hello world".toByteArray()))
  }
}
