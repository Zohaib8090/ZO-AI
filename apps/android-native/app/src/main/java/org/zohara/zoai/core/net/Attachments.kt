package org.zohara.zoai.core.net

import java.util.Base64
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** An attachment of a message (API.md 3.2). The payload is plain JSON (base64 for pictures): there is no multipart. */
sealed interface Attachment {
  val name: String

  data class Image(override val name: String, val mime: String, val dataB64: String, val thumb: String? = null, val decodedSize: Int = 0) : Attachment
  data class TextFile(override val name: String, val text: String) : Attachment
}

class AttachmentException(message: String) : Exception(message)

/** Builders and the same limits the server enforces (`zo_core::attach::parse`), so a bad file is refused on the phone with a sentence. */
object Attachments {
  const val MAX_IMAGES = 4
  const val MAX_FILES = 6
  const val MAX_IMAGE_BYTES = 3 * 1024 * 1024
  const val MAX_IMAGES_TOTAL = 9 * 1024 * 1024
  const val MAX_TEXT_BYTES = 200 * 1024
  const val MAX_TEXT_TOTAL = 400 * 1024
  const val MAX_THUMB_CHARS = 20_000
  const val MAX_NAME = 120

  /** Names lose control characters and `/ \ " < >` and are cut to 120 characters. */
  fun cleanName(raw: String): String {
    val s = raw.filter { !it.isISOControl() && it !in "/\\\"<>" }.trim()
    return (if (s.isEmpty()) "file" else s).take(MAX_NAME)
  }

  /** png, jpeg, webp or gif by the first bytes; null for anything else. */
  fun sniffImageMime(b: ByteArray): String? = when {
    b.size >= 8 && b[0] == 0x89.toByte() && b[1] == 'P'.code.toByte() && b[2] == 'N'.code.toByte() && b[3] == 'G'.code.toByte() -> "image/png"
    b.size >= 3 && b[0] == 0xFF.toByte() && b[1] == 0xD8.toByte() && b[2] == 0xFF.toByte() -> "image/jpeg"
    b.size >= 12 && String(b, 0, 4, Charsets.ISO_8859_1) == "RIFF" && String(b, 8, 4, Charsets.ISO_8859_1) == "WEBP" -> "image/webp"
    b.size >= 6 && (String(b, 0, 6, Charsets.ISO_8859_1) == "GIF87a" || String(b, 0, 6, Charsets.ISO_8859_1) == "GIF89a") -> "image/gif"
    else -> null
  }

  /** A picture from its bytes. [thumbJpeg] is an optional small JPEG (a thumbnail the chat keeps). */
  fun image(name: String, bytes: ByteArray, thumbJpeg: ByteArray? = null): Attachment.Image {
    val mime = sniffImageMime(bytes) ?: throw AttachmentException("That picture is not a PNG, JPEG, WebP or GIF.")
    if (bytes.size > MAX_IMAGE_BYTES) throw AttachmentException("That picture is bigger than 3 MB.")
    val thumb = thumbJpeg?.let { "data:image/jpeg;base64," + Base64.getEncoder().encodeToString(it) }?.takeIf { it.length <= MAX_THUMB_CHARS }
    return Attachment.Image(cleanName(name), mime, Base64.getEncoder().encodeToString(bytes), thumb, bytes.size)
  }

  /** A text file: the model sees "Attached file: <name>" and then the text (the web does the same). */
  fun textFile(name: String, content: String): Attachment.TextFile {
    val clean = cleanName(name)
    if (content.any { it.isISOControl() && it != '\n' && it != '\t' && it != '\r' }) throw AttachmentException("That file is not plain text.")
    val body = "Attached file: $clean\n$content"
    if (body.toByteArray(Charsets.UTF_8).size > MAX_TEXT_BYTES) throw AttachmentException("That file is bigger than 200 KB.")
    return Attachment.TextFile(clean, body)
  }

  /** Checks the whole list; returns a sentence for the person, or null when it is fine. */
  fun validate(list: List<Attachment>): String? {
    val images = list.filterIsInstance<Attachment.Image>()
    val texts = list.filterIsInstance<Attachment.TextFile>()
    if (images.size > MAX_IMAGES) return "At most 4 pictures can go with one message."
    if (texts.size > MAX_FILES) return "At most 6 files can go with one message."
    if (images.sumOf { it.decodedSize } > MAX_IMAGES_TOTAL) return "The pictures together are bigger than 9 MB."
    if (texts.sumOf { it.text.toByteArray(Charsets.UTF_8).size } > MAX_TEXT_TOTAL) return "The files together are bigger than 400 KB."
    return null
  }

  fun toJson(a: Attachment): JsonObject = when (a) {
    is Attachment.Image -> buildJsonObject {
      put("kind", "image"); put("mime", a.mime); put("name", a.name); put("data_b64", a.dataB64)
      if (a.thumb != null) put("thumb", a.thumb)
    }
    is Attachment.TextFile -> buildJsonObject { put("kind", "text"); put("name", a.name); put("text", a.text) }
  }

  fun toJson(list: List<Attachment>): JsonArray = JsonArray(list.map { toJson(it) })
}
