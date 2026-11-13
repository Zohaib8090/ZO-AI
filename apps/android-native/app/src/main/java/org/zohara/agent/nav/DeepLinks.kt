package org.zohara.agent.nav

import android.content.Intent

/**
 * NAV-20, NAV-22, AUTH-24: where an intent wants to go. Notifications and links open a chat (`zohara-agent://chat/<id>` or the `chat` extra), the
 * launcher shortcut "New chat" is `zohara-agent://new`, and `zohara-agent://device` opens "Approve a sign-in" (the link never carries the code).
 * Anything that is not `[\w-]{1,64}` as a chat id is ignored.
 */
object DeepLinks {
  const val EXTRA_CHAT = "chat"
  /** pending values that are not chat ids (they do not match the id pattern, so they can never be mistaken for one) */
  const val NEW_CHAT = "+new"
  const val APPROVE = "+device"
  /** the link scheme; [OLD_SCHEMES] (the names before the renames) are still accepted so old links and notifications keep working */
  const val SCHEME = "zohara-agent"
  const val OLD_SCHEME = "zoai"
  const val CHAT_SCHEME = "zohara-chat"
  val OLD_SCHEMES = setOf(CHAT_SCHEME, OLD_SCHEME)

  private val valid = Regex("""[\w-]{1,64}""")

  fun validChatId(id: String?): String? = id?.takeIf { valid.matches(it) }

  /** the pending value for [intent]: a chat id, [NEW_CHAT], [APPROVE], or null when the intent is just a launch */
  fun pendingFrom(intent: Intent?): String? {
    if (intent == null) return null
    validChatId(intent.getStringExtra(EXTRA_CHAT))?.let { return it }
    val data = intent.data ?: return null
    if (data.scheme != SCHEME && data.scheme !in OLD_SCHEMES) return null
    return when (data.host) {
      "chat" -> validChatId(data.lastPathSegment)
      "new" -> NEW_CHAT
      "device" -> APPROVE
      else -> null
    }
  }

  fun chatIdFrom(intent: Intent?): String? = pendingFrom(intent)?.let { validChatId(it) }
}
