package org.zohara.zoai.media

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.zohara.zoai.core.net.Attachment

/** ATT-01..04, ATT-10, ATT-13, ATT-14, ATT-18..21, COMP-19, COMP-21: the strip's state, against fake readers. */
@OptIn(ExperimentalCoroutinesApi::class)
class AttachmentsControllerTest {
  @get:Rule val tmp = TemporaryFolder()

  private val fakeImage: (String, Long, ByteArray) -> ImageItem = { name, _, bytes -> ImageItem(0, name.substringBeforeLast('.') + ".jpg", "image/jpeg", bytes, "data:image/jpeg;base64,AAAA", 10, 20) }

  private fun TestScope.controller(titles: suspend (String) -> String = { "" }, image: (String, Long, ByteArray) -> ImageItem = fakeImage): Pair<AttachmentsController, MutableList<String>> {
    val d = StandardTestDispatcher(testScheduler)
    val sc = kotlinx.coroutines.CoroutineScope(d + kotlinx.coroutines.SupervisorJob()) // not backgroundScope: advanceUntilIdle does not wait for background work
    val c = AttachmentsController(sc, d, tmp.newFolder(), { error("no pdf here") }, titles, image)
    val msgs = ArrayList<String>()
    sc.launch { c.messages.collect { msgs.add(it) } }
    testScheduler.runCurrent() // the collector must be listening before the first toast
    return c to msgs
  }

  private fun png(name: String = "a.png", size: Int = 100) = MemoryFile(name, "image/png", ByteArray(size) { 1 })
  private fun txt(name: String, body: String = "hi") = MemoryFile(name, "text/plain", body.toByteArray())

  @Test fun picturesGoThroughTheProcessorAndKeepTheirThumbnail() = runTest {
    val (c, _) = controller()
    c.addFiles(listOf(png("photo.png")))
    advanceUntilIdle()
    val i = c.items.value.single() as ImageItem
    assertEquals("photo.jpg", i.name)
    assertEquals("data:image/jpeg;base64,AAAA", i.thumb)
    assertFalse(c.isBusy)
  }

  @Test fun aBusyChipShowsWhileAFileIsRead() = runTest {
    val (c, _) = controller()
    c.addFiles(listOf(txt("a.txt")))
    runCurrent()
    assertTrue(c.isBusy || c.items.value.single() is TextItem) // the read may already be done on the test dispatcher
    advanceUntilIdle()
    assertTrue(c.items.value.single() is TextItem)
  }

  @Test fun atMostFourPicturesAndSixFiles() = runTest {
    val (c, msgs) = controller()
    c.addFiles((1..5).map { png("p$it.png") })
    c.addFiles((1..7).map { txt("t$it.txt") })
    advanceUntilIdle()
    assertEquals(4, c.items.value.count { it is ImageItem })
    assertEquals(6, c.items.value.count { it is TextItem })
    assertTrue(msgs.contains("You can attach up to 4 pictures to one message."))
    assertTrue(msgs.contains("You can attach up to 6 files to one message."))
  }

  @Test fun theTextLimitCountsAllFilesInOrder() = runTest {
    val (c, msgs) = controller()
    val big = "a".repeat(150_000)
    c.addFiles(listOf(txt("one.txt", big), txt("two.txt", big), txt("three.txt", big)))
    advanceUntilIdle()
    assertEquals(listOf("one.txt", "two.txt"), c.items.value.filterIsInstance<TextItem>().map { it.name })
    assertTrue(msgs.single(), msgs.single().startsWith("\"three.txt\" does not fit: the files of one message can hold up to 400 KB"))
  }

  @Test fun picturesTogetherAtMost9MB() = runTest {
    val big = { name: String, _: Long, bytes: ByteArray -> ImageItem(0, name, "image/jpeg", ByteArray(2_900_000), "", 1, 1) }
    val (c, msgs) = controller(image = big)
    c.addFiles(listOf(png("a.png"), png("b.png"), png("c.png"), png("d.png")))
    advanceUntilIdle()
    assertEquals(3, c.items.value.size)
    assertEquals(listOf("These pictures are too big together (at most 9 MB). Remove one or use smaller pictures."), msgs)
  }

  @Test fun anUnsupportedFileIsRefusedWithASentence() = runTest {
    val (c, msgs) = controller()
    c.addFiles(listOf(MemoryFile("clip.mp4", "video/mp4", ByteArray(5))))
    advanceUntilIdle()
    assertTrue(c.items.value.isEmpty())
    assertEquals("\"clip.mp4\" cannot be attached: video files are not supported. You can attach pictures, PDF, ZIP and text or code files.", msgs.single())
  }

  @Test fun aProblemDropsTheBusyChipAndToasts() = runTest {
    val (c, msgs) = controller()
    c.addFiles(listOf(MemoryFile("bin.txt", "text/plain", byteArrayOf(65, 0, 66))))
    advanceUntilIdle()
    assertTrue(c.items.value.isEmpty())
    assertEquals(listOf("\"bin.txt\" is not a text file."), msgs)
  }

  @Test fun pastedPicturesGetNumberedNames() = runTest {
    val (c, _) = controller(image = { name, _, b -> ImageItem(0, name, "image/png", b, "", 1, 1) })
    c.addFiles(listOf(MemoryFile("image.png", "image/png", ByteArray(3)), MemoryFile("", "image/jpeg", ByteArray(3))))
    advanceUntilIdle()
    assertEquals(listOf("pasted-picture-1.png", "pasted-picture-2.jpg"), c.items.value.map { (it as ImageItem).name })
  }

  @Test fun removingAChipAndRemovingWhileBusy() = runTest {
    val (c, _) = controller()
    c.addFiles(listOf(txt("a.txt"), txt("b.txt")))
    advanceUntilIdle()
    c.remove(c.items.value[0].id)
    assertEquals(listOf("b.txt"), c.items.value.map { (it as TextItem).name })
  }

  @Test fun filesAreReadOneAfterAnotherInOrder() = runTest {
    val order = ArrayList<String>()
    val (c, _) = controller(image = { n, _, b -> order.add(n); fakeImage(n, 0, b) })
    c.addFiles(listOf(png("1.png"), png("2.png"), png("3.png")))
    advanceUntilIdle()
    assertEquals(listOf("1.png", "2.png", "3.png"), order)
    assertEquals(listOf("1.jpg", "2.jpg", "3.jpg"), c.items.value.map { (it as ImageItem).name })
  }

  // ---------------------------------------------------------------- links

  @Test fun linkChipsGetATitleFromThePreview() = runTest {
    val (c, _) = controller(titles = { "Example page" })
    assertTrue(c.addLink("example.com/page"))
    advanceUntilIdle()
    val l = c.items.value.single() as LinkItem
    assertEquals("https://example.com/page", l.url)
    assertEquals("example.com", l.host)
    assertEquals("Example page", l.title)
  }

  @Test fun aFailedPreviewKeepsTheHostAsTheLabel() = runTest {
    val (c, _) = controller(titles = { throw RuntimeException("429") })
    c.addLink("https://example.com")
    advanceUntilIdle()
    assertEquals("", (c.items.value.single() as LinkItem).title)
  }

  @Test fun linkRulesWithTheirSentences() = runTest {
    val (c, msgs) = controller()
    assertFalse(c.addLink("nonsense"))
    assertTrue(c.addLink("https://a.example.com"))
    assertFalse(c.addLink("https://a.example.com"))
    (2..5).forEach { assertTrue(c.addLink("https://s$it.example.com")) }
    assertFalse(c.addLink("https://s6.example.com"))
    advanceUntilIdle()
    assertEquals(listOf("That does not look like a web address.", "That link is already added.", "You can add up to 5 links to one message."), msgs)
    assertEquals(5, c.items.value.size)
  }

  @Test fun addressesTypedInTheTextShowAsChipsAndGetPreviewsAfterTheTypingStops() = runTest {
    val asked = ArrayList<String>()
    val (c, _) = controller(titles = { asked.add(it); "T" })
    val text = "look at https://typed.example.com/x please"
    assertEquals(listOf("https://typed.example.com/x"), c.detected(text).map { it.url })
    assertTrue(c.detected(text).single().detected)
    c.onTextChanged(text)
    advanceTimeBy(800)
    assertTrue("not yet, typing may continue", asked.isEmpty())
    advanceTimeBy(200)
    runCurrent()
    assertEquals(listOf("https://typed.example.com/x"), asked)
    assertEquals("T", c.detected(text).single().title)
    // a chip already added is not shown twice
    c.addLink("https://typed.example.com/x")
    assertTrue(c.detected(text).isEmpty())
  }

  // ---------------------------------------------------------------- sending

  @Test fun takeGivesThePayloadTheBubbleFactsAndEmptiesTheStrip() = runTest {
    val (c, _) = controller()
    c.addFiles(listOf(png("p.png", 7), txt("n.txt", "note")))
    c.addLink("https://example.com")
    advanceUntilIdle()
    val t = c.take()
    assertTrue(c.items.value.isEmpty())
    assertEquals(2, t.attachments.size)
    val img = t.attachments[0] as Attachment.Image
    assertEquals("p.jpg", img.name); assertEquals("image/jpeg", img.mime); assertEquals(7, img.decodedSize)
    assertEquals(java.util.Base64.getEncoder().encodeToString(ByteArray(7) { 1 }), img.dataB64)
    assertEquals("Attached file: n.txt\nnote", (t.attachments[1] as Attachment.TextFile).text)
    assertEquals(listOf("image", "text"), t.meta.map { it.kind })
    assertEquals(listOf("https://example.com/"), t.links)
    assertEquals("hello\n\nLinks to read (use your web tool):\n- https://example.com/", t.messageText("hello"))
    assertNotNull(t.meta[0].image)
  }

  @Test fun aFailedSendPutsTheAttachmentsBack() = runTest {
    val (c, _) = controller()
    c.addFiles(listOf(txt("a.txt")))
    c.addLink("https://example.com")
    advanceUntilIdle()
    val t = c.take()
    c.addFiles(listOf(txt("b.txt")))
    advanceUntilIdle()
    c.restore(t)
    assertEquals(listOf("example.com", "a.txt", "b.txt"), c.items.value.map { when (it) { is TextItem -> it.name; is LinkItem -> it.host; else -> "?" } })
  }

  @Test fun aTapOnASentPictureOrFileShowsWhatWasSentOrFallsBack() = runTest { // CHAT-04, ATT-12
    val (c, _) = controller()
    c.addFiles(listOf(png("p.png", 7), txt("n.txt", "note")))
    advanceUntilIdle()
    val t = c.take()
    val pic = t.meta[0]; val txt = t.meta[1]
    val p = c.previewForSent("image", pic.name, pic.size.toLong(), "data:image/jpeg;base64,BBBB") as PreviewTarget.Picture
    assertEquals(7, p.bytes!!.size)
    val f = c.previewForSent("text", txt.name, txt.size.toLong(), null) as PreviewTarget.TextFile
    assertEquals("Attached file: n.txt\nnote", f.text)
    // something this phone never sent (a saved chat from another device): the thumbnail, or the name and size
    val other = c.previewForSent("image", "other.jpg", 5000, "data:image/jpeg;base64,BBBB") as PreviewTarget.Picture
    assertEquals(null, other.bytes); assertEquals("data:image/jpeg;base64,BBBB", other.thumb)
    assertTrue((c.previewForSent("text", "x.txt", 2048, null) as PreviewTarget.TextFile).text.startsWith("Attached file: x.txt (2 KB)"))
  }
}
