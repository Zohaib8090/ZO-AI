package org.zohara.chat.media

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import androidx.exifinterface.media.ExifInterface
import java.io.ByteArrayOutputStream
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** ATT-02, ATT-03, ATT-04: the picture pipeline, with real decoding and encoding (Robolectric native graphics). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class ImageProcessorTest {
  @get:Rule val tmp = TemporaryFolder()

  private fun bitmap(w: Int, h: Int, alphaHalf: Boolean = false): Bitmap {
    val b = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    for (y in 0 until h step 1) for (x in 0 until w step 1) {
      val a = if (alphaHalf && x < w / 2) 0 else 255
      b.setPixel(x, y, Color.argb(a, (x * 255 / w), (y * 255 / h), 128))
    }
    return b
  }

  private fun bytes(b: Bitmap, f: Bitmap.CompressFormat, q: Int = 90): ByteArray = ByteArrayOutputStream().also { b.compress(f, q, it) }.toByteArray()

  private fun decode(b: ByteArray) = BitmapFactory.decodeByteArray(b, 0, b.size)

  @Test fun bigPicturesAreShrunkTo1600AndBecomeJpeg() {
    val src = bytes(bitmap(3000, 2000), Bitmap.CompressFormat.JPEG)
    val r = ImageProcessor.process("holiday.jpeg", src.size.toLong(), src)
    assertEquals("image/jpeg", r.mime)
    assertEquals("holiday.jpg", r.name)
    assertEquals(1600, r.w)
    assertEquals(1067, r.h)
    val out = decode(r.bytes)
    assertEquals(1600, out.width)
    assertTrue(r.bytes.size <= Lim.IMG_BYTES)
    assertEquals(0xFF.toByte(), r.bytes[0]); assertEquals(0xD8.toByte(), r.bytes[1])
  }

  @Test fun smallPicturesKeepTheirSize() {
    val src = bytes(bitmap(300, 200), Bitmap.CompressFormat.PNG, 100)
    val r = ImageProcessor.process("small.png", src.size.toLong(), src)
    assertEquals(300, r.w); assertEquals(200, r.h)
    assertEquals("image/jpeg", r.mime) // opaque: JPEG at 85 percent, like the web page
    assertEquals("small.jpg", r.name)
  }

  @Test fun transparentPicturesStayPng() {
    val src = bytes(bitmap(200, 200, alphaHalf = true), Bitmap.CompressFormat.PNG, 100)
    val r = ImageProcessor.process("logo.png", src.size.toLong(), src)
    assertEquals("image/png", r.mime)
    assertEquals("logo.png", r.name)
    assertTrue(decode(r.bytes).hasAlpha())
  }

  @Test fun theExifTurnIsHonouredAndTheMetadataIsGone() {
    val f = File(tmp.root, "turned.jpg")
    f.writeBytes(bytes(bitmap(300, 200), Bitmap.CompressFormat.JPEG))
    ExifInterface(f.absolutePath).apply {
      setAttribute(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_ROTATE_90.toString())
      setAttribute(ExifInterface.TAG_GPS_LATITUDE, "52/1,30/1,0/1")
      setAttribute(ExifInterface.TAG_MAKE, "SecretCam")
      saveAttributes()
    }
    val src = f.readBytes()
    assertTrue("the test file carries a make", String(src, Charsets.ISO_8859_1).contains("SecretCam"))
    val r = ImageProcessor.process("turned.jpg", src.size.toLong(), src)
    assertEquals(200, r.w); assertEquals(300, r.h) // turned upright: portrait now
    assertEquals(200, decode(r.bytes).width)
    val text = String(r.bytes, Charsets.ISO_8859_1)
    assertTrue("no EXIF block left", !text.contains("SecretCam") && !text.contains("Exif"))
  }

  @Test fun theThumbnailIsASmallJpegDataUri() {
    val src = bytes(bitmap(1200, 800), Bitmap.CompressFormat.JPEG)
    val r = ImageProcessor.process("t.jpg", src.size.toLong(), src)
    assertTrue(r.thumb, r.thumb.startsWith("data:image/jpeg;base64,"))
    assertTrue(r.thumb.length <= Lim.THUMB_CHARS)
    assertTrue(MediaRules.safeThumb(r.thumb))
    val t = decodeBase64(r.thumb)
    assertTrue(maxOf(t.width, t.height) <= Lim.THUMB_SIDE)
  }

  private fun decodeBase64(u: String): Bitmap {
    val b = java.util.Base64.getDecoder().decode(u.substringAfter(','))
    return decode(b)
  }

  @Test fun notAPictureIsSaidSo() {
    try { ImageProcessor.process("fake.png", 5, "hello".toByteArray()); fail() } catch (e: AttachProblem) { assertEquals("\"fake.png\" could not be read as a picture.", e.message) }
  }

  @Test fun over40MegabytesIsRefused() {
    try { ImageProcessor.process("huge.jpg", 41L * 1024 * 1024, ByteArray(10)); fail() } catch (e: AttachProblem) { assertEquals("\"huge.jpg\" is too big (41.0 MB). Pictures can be up to 40 MB.", e.message) }
  }

  @Test fun theNameKeepsItsBaseAndTakesTheNewExtension() {
    val src = bytes(bitmap(50, 50), Bitmap.CompressFormat.PNG, 100)
    assertEquals("my.photo.jpg", ImageProcessor.process("my.photo.png", src.size.toLong(), src).name)
    assertEquals("noext.jpg", ImageProcessor.process("noext", src.size.toLong(), src).name)
  }
}
