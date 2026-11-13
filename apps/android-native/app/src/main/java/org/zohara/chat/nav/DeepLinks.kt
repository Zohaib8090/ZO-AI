package org.zohara.chat.nav

import android.content.Intent

/**
 * NAV-20, NAV-22, AUTH-24: where an intent wants to go. Notifications and links open a chat (`zohara-chat://chat/<id>` or the `chat` extra), the
 * launcher shortcut "New chat" is `zohara-chat://new`, and `zohara-chat://device` opens "Approve a sign-in" (the link never carries the code).
 * Anything that is not `[\w-]{1,64}` as a chat id is ignored.
 */
object DeepLinks {
  const val EXTRA_CHAT = "chat"
  /** pending values that are not chat ids (they do not match the id pattern, so they can never be mistaken for one) */
  const val NEW_CHAT = "+new"
  const val APPROVE = "+device"
  /** the link scheme; [OLD_SCHEME] (the name before the rename) is still accepted so old links and notifications keep working */
  const val SCHEME = "zohara-chat"
  const val OLD_SCHEME = "zoai"

  private val valid = Regex("""[\w-]{1,64}""")

  fun validChatId(id: String?): String? = id?.takeIf { valid.matches(it) }

  /** the pending value for [intent]: a chat id, [NEW_CHAT], [APPROVE], or null when the intent is just a launch */
  fun pendingFrom(intent: Intent?): String? {
    if (intent == null) return null
    validChatId(intent.getStringExtra(EXTRA_CHAT))?.let { return it }
    val data = intent.data ?: return null
    if (data.scheme != SCHEME && data.scheme != OLD_SCHEME) return null
    return when (data.host) {
      "chat" -> validChatId(data.lastPathSegment)
      "new" -> NEW_CHAT
      "device" -> APPROVE
      else -> null
    }
  }

  fun chatIdFrom(intent: Intent?): String? = pendingFrom(intent)?.let { validChatId(it) }
}
