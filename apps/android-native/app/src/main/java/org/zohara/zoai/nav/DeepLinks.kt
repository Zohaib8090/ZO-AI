package org.zohara.zoai.nav

import android.content.Intent

/** NAV-20: a chat id from a notification or a link (`zoai://chat/<id>`); anything that is not `[\w-]{1,64}` is ignored. */
object DeepLinks {
  const val EXTRA_CHAT = "chat"
  private val valid = Regex("""[\w-]{1,64}""")

  fun validChatId(id: String?): String? = id?.takeIf { valid.matches(it) }

  fun chatIdFrom(intent: Intent?): String? {
    if (intent == null) return null
    validChatId(intent.getStringExtra(EXTRA_CHAT))?.let { return it }
    val data = intent.data ?: return null
    if (data.scheme == "zoai" && data.host == "chat") return validChatId(data.lastPathSegment)
    return null
  }
}
