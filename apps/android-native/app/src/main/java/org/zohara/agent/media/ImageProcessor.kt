package org.zohara.agent.media

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ImageDecoder
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.Rect
import android.os.Build
import androidx.exifinterface.media.ExifInterface
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.util.Base64
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import org.zohara.agent.core.net.Attachments

/**
 * Pictures like the web page (ATT-02, ATT-03, ATT-04): decode (honouring the EXIF turn, then dropping EXIF and location by re-drawing),
 * shrink to at most 1600 px on the long side, JPEG at 85 percent, PNG or WebP when the picture has transparency, retry at 75 percent of the size up to 4 times
 * when it is still over 3 MB. A small JPEG thumbnail (at most 20,000 characters as a data URI) is kept for the saved chat.
 */
object ImageProcessor {
  private val thumbPlan = listOf(Lim.THUMB_SIDE to 72, Lim.THUMB_SIDE to 50, 120 to 50, 96 to 40, 64 to 40)

  class Encoded(val bytes: ByteArray, val mime: String, val w: Int, val h: Int)

  /** [fileSize] is the size on disk (the 40 MB limit); [name] the display name. */
  fun process(name: String, fileSize: Long, bytes: ByteArray): ImageItem {
    if (fileSize > Lim.IMG_FILE) throw AttachProblem("\"$name\" is too big (${MediaRules.kb(fileSize)}). Pictures can be up to ${Lim.IMG_FILE / 1048576} MB.")
    val mimeIn = Attachments.sniffImageMime(bytes)
    val src = decode(bytes) ?: throw AttachProblem(if (isHeic(bytes)) MediaRules.heicRefusal(name) else "\"$name\" could not be read as a picture.")
    try {
      val sw = src.width
      val sh = src.height
      if (sw <= 0 || sh <= 0) throw AttachProblem("\"$name\" could not be read as a picture.")
      var scale = min(1.0, Lim.SIDE.toDouble() / max(sw, sh))
      val alpha = mimeIn != "image/jpeg" && src.hasAlpha() && transparent(src)
      var out: Encoded? = null
      var k = 0
      while (k < 4 && out == null) {
        val w = max(1, (sw * scale).roundToInt())
        val h = max(1, (sh * scale).roundToInt())
        var e: Encoded? = null
        if (alpha) {
          val keep = draw(src, w, h, white = false)
          e = compress(keep, Bitmap.CompressFormat.PNG, 100, "image/png")
          if (e.bytes.size > 1024 * 1024) {
            val wp = compress(keep, webpFormat(), 85, "image/webp")
            if (wp.bytes.isNotEmpty()) e = wp
          }
          keep.recycle()
          if (e.bytes.size > Lim.IMG_BYTES) {
            val flat = draw(src, w, h, white = true)
            e = compress(flat, Bitmap.CompressFormat.JPEG, 85, "image/jpeg")
            flat.recycle()
          }
        } else {
          val flat = draw(src, w, h, white = true)
          e = compress(flat, Bitmap.CompressFormat.JPEG, 85, "image/jpeg")
          flat.recycle()
        }
        if (e.bytes.size <= Lim.IMG_BYTES) out = Encoded(e.bytes, e.mime, w, h)
        scale *= 0.75
        k++
      }
      val enc = out ?: throw AttachProblem("\"$name\" is too big to send even after shrinking it.")
      val ext = when (enc.mime) { "image/png" -> "png"; "image/webp" -> "webp"; else -> "jpg" }
      return ImageItem(0, name.replace(Regex("""\.[A-Za-z0-9]{1,5}$"""), "") + "." + ext, enc.mime, enc.bytes, thumbnail(src), enc.w, enc.h)
    } finally {
      src.recycle()
    }
  }

  /** a JPEG data URI of at most 20,000 characters: 160 px at 72 and 50 percent, then 120, 96 and 64 px (ATT-03) */
  fun thumbnail(src: Bitmap): String {
    for ((side, q) in thumbPlan) {
      val k = min(1.0, side.toDouble() / max(src.width, src.height))
      val t = draw(src, max(1, (src.width * k).roundToInt()), max(1, (src.height * k).roundToInt()), white = true)
      val e = compress(t, Bitmap.CompressFormat.JPEG, q, "image/jpeg")
      t.recycle()
      val uri = "data:image/jpeg;base64," + Base64.getEncoder().encodeToString(e.bytes)
      if (uri.length <= Lim.THUMB_CHARS) return uri
    }
    return ""
  }

  private fun webpFormat(): Bitmap.CompressFormat =
    if (Build.VERSION.SDK_INT >= 30) Bitmap.CompressFormat.WEBP_LOSSY else @Suppress("DEPRECATION") Bitmap.CompressFormat.WEBP

  private fun compress(b: Bitmap, f: Bitmap.CompressFormat, q: Int, mime: String): Encoded {
    val o = ByteArrayOutputStream()
    b.compress(f, q, o)
    return Encoded(o.toByteArray(), mime, b.width, b.height)
  }

  private fun draw(src: Bitmap, w: Int, h: Int, white: Boolean): Bitmap {
    val out = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    val c = Canvas(out)
    if (white) c.drawColor(Color.WHITE)
    c.drawBitmap(src, Rect(0, 0, src.width, src.height), Rect(0, 0, w, h), Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG or Paint.DITHER_FLAG))
    return out
  }

  /** has the picture a pixel that is not opaque? (looked at on a 64 px copy, like the web page) */
  private fun transparent(src: Bitmap): Boolean {
    val w = min(64, src.width)
    val h = min(64, src.height)
    val s = draw(src, w, h, white = false)
    val px = IntArray(w * h)
    s.getPixels(px, 0, w, 0, 0, w, h)
    s.recycle()
    return px.any { (it ushr 24) < 250 }
  }

  private fun isHeic(b: ByteArray): Boolean = b.size >= 12 && String(b, 4, 4, Charsets.ISO_8859_1) == "ftyp" && String(b, 8, 4, Charsets.ISO_8859_1).let { it.startsWith("hei") || it.startsWith("mif") || it == "avif" }

  /** Decodes at about the final size (no huge bitmap for a 40 MB photo), turns it upright by its EXIF orientation. */
  fun decode(bytes: ByteArray): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth > 0 && bounds.outHeight > 0) {
      var sample = 1
      val long = max(bounds.outWidth, bounds.outHeight)
      while (long / (sample * 2) >= Lim.SIDE) sample *= 2
      val opts = BitmapFactory.Options().apply { inSampleSize = sample; inPreferredConfig = Bitmap.Config.ARGB_8888 }
      val bmp = try { BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts) } catch (_: OutOfMemoryError) { null }
      if (bmp != null) return upright(bmp, bytes)
    }
    // HEIC, AVIF and the like: the system decoder (Android 9 and newer)
    if (Build.VERSION.SDK_INT >= 28) {
      return try {
        ImageDecoder.decodeBitmap(ImageDecoder.createSource(ByteBuffer.wrap(bytes))) { dec, info, _ ->
          dec.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
          val long = max(info.size.width, info.size.height)
          var sample = 1
          while (long / (sample * 2) >= Lim.SIDE) sample *= 2
          if (sample > 1) dec.setTargetSampleSize(sample)
        }
      } catch (_: Exception) { null } catch (_: OutOfMemoryError) { null }
    }
    return null
  }

  private fun upright(bmp: Bitmap, bytes: ByteArray): Bitmap {
    val orientation = try { ExifInterface(ByteArrayInputStream(bytes)).getAttributeInt(ExifInterface.TAG_ORIENTATION, ExifInterface.ORIENTATION_NORMAL) } catch (_: Exception) { ExifInterface.ORIENTATION_NORMAL }
    val m = Matrix()
    when (orientation) {
      ExifInterface.ORIENTATION_ROTATE_90 -> m.postRotate(90f)
      ExifInterface.ORIENTATION_ROTATE_180 -> m.postRotate(180f)
      ExifInterface.ORIENTATION_ROTATE_270 -> m.postRotate(270f)
      ExifInterface.ORIENTATION_FLIP_HORIZONTAL -> m.postScale(-1f, 1f)
      ExifInterface.ORIENTATION_FLIP_VERTICAL -> m.postScale(1f, -1f)
      ExifInterface.ORIENTATION_TRANSPOSE -> { m.postRotate(90f); m.postScale(-1f, 1f) }
      ExifInterface.ORIENTATION_TRANSVERSE -> { m.postRotate(270f); m.postScale(-1f, 1f) }
      else -> return bmp
    }
    return try {
      val r = Bitmap.createBitmap(bmp, 0, 0, bmp.width, bmp.height, m, true)
      if (r !== bmp) bmp.recycle()
      r
    } catch (_: OutOfMemoryError) { bmp }
  }
}
