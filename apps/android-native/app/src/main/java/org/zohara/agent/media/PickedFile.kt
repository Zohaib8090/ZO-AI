package org.zohara.agent.media

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.io.InputStream

/** One file the person chose, pasted, took or shared. Reading is lazy; the controller reads it on a worker thread. */
interface PickedFile {
  val name: String
  /** the mime type the source says, may be null or `application/octet-stream` */
  val mime: String?
  /** the size the source says, or -1 when unknown */
  val size: Long
  fun open(): InputStream

  /** All bytes, or null when there are more than [cap]. */
  fun readCapped(cap: Long): ByteArray? {
    open().use { ins ->
      val out = java.io.ByteArrayOutputStream()
      val buf = ByteArray(16 * 1024)
      var n = 0L
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

  /** Copies it to [dir] (for the PDF and ZIP readers that need a file); null when it is bigger than [cap]. The caller deletes the file. */
  fun copyTo(dir: File, cap: Long): File? {
    dir.mkdirs()
    val f = File.createTempFile("att", ".tmp", dir)
    var ok = false
    try {
      open().use { ins ->
        f.outputStream().use { out ->
          val buf = ByteArray(64 * 1024)
          var n = 0L
          while (true) {
            val r = ins.read(buf)
            if (r < 0) break
            n += r
            if (n > cap) return null
            out.write(buf, 0, r)
          }
        }
      }
      ok = true
      return f
    } finally {
      if (!ok) f.delete()
    }
  }
}

/** An in-memory file (tests, pasted text, the clipboard). */
class MemoryFile(override val name: String, override val mime: String?, private val data: ByteArray) : PickedFile {
  override val size: Long get() = data.size.toLong()
  override fun open(): InputStream = data.inputStream()
}

/** A file behind a content or file Uri. */
class UriFile(private val resolver: ContentResolver, private val uri: Uri, override val name: String, override val mime: String?, override val size: Long) : PickedFile {
  override fun open(): InputStream = resolver.openInputStream(uri) ?: throw java.io.FileNotFoundException(uri.toString())
}

object UriFiles {
  /** Looks up name, size and type of [uri]. [fallbackName] is used when the source gives none (a keyboard's picture, a camera file). */
  fun resolve(context: Context, uri: Uri, hintMime: String? = null, fallbackName: String? = null): PickedFile {
    val r = context.contentResolver
    var name: String? = null
    var size = -1L
    try {
      r.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
        if (c.moveToFirst()) {
          val ni = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
          val si = c.getColumnIndex(OpenableColumns.SIZE)
          if (ni >= 0 && !c.isNull(ni)) name = c.getString(ni)
          if (si >= 0 && !c.isNull(si)) size = c.getLong(si)
        }
      }
    } catch (_: Exception) { /* some providers refuse the query: the name then comes from the uri */ }
    val mime = hintMime ?: try { r.getType(uri) } catch (_: Exception) { null }
    val n = name?.takeIf { it.isNotBlank() } ?: uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() } ?: fallbackName ?: "file"
    return UriFile(r, uri, n, mime, size)
  }
}
