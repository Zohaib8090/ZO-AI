package org.zohara.agent.media

/** One thing in the strip above the composer (ATT-11). Identity by [id]; the controller numbers them. */
sealed interface AttItem { val id: Long }

/** A picture, already shrunk and re-encoded (ATT-02). [thumb] is the small JPEG data URI kept in the saved chat (ATT-03); [w] and [h] are of the encoded picture. */
class ImageItem(override val id: Long, val name: String, val mime: String, val bytes: ByteArray, val thumb: String, val w: Int, val h: Int) : AttItem {
  val size: Int get() = bytes.size
  fun withId(i: Long) = ImageItem(i, name, mime, bytes, thumb, w, h)
}

/** A text, code, PDF or ZIP file as text. [text] is what the model sees ("Attached file: name" and the content). [note] is "3 pages", "cut to fit", "12 files". */
data class TextItem(override val id: Long, val name: String, val text: String, val bytes: Int, val note: String = "", val zip: Boolean = false) : AttItem

/** A web address the model reads with its web tool. [title] comes from `link_preview` (ATT-19); [detected] chips come from the text (ATT-20). */
data class LinkItem(override val id: Long, val url: String, val host: String, val title: String = "", val detected: Boolean = false) : AttItem

enum class BusyAs { Image, Text }

/** A file being read; [note] is "Reading…", "Page 3 of 12", "Entry 4 of 20" (ATT-11). */
data class BusyItem(override val id: Long, val name: String, val kind: BusyAs, val note: String = "Reading…") : AttItem

/** What the bubble of a sent message shows (CHAT-04): never the picture itself, only name, size and the small thumbnail. */
data class AttMeta(val kind: String, val name: String, val size: Int, val thumb: String = "", val text: String = "", val image: ByteArray? = null)
