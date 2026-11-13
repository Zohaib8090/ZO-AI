package org.zohara.chat.chat

import kotlinx.coroutines.flow.StateFlow
import org.zohara.chat.core.net.AttachInfo
import org.zohara.chat.core.net.Attachment
import org.zohara.chat.core.store.RunOn

enum class NeedsKind { Approval, Question }

/** What a send takes from the attachment strip. [restore] puts it back when nothing reached the model (ATT-21, CHAT-19). */
class Taken(val text: String, val attachments: List<Attachment>, val infos: List<AttachInfo>, val restore: () -> Unit) {
  companion object { fun plain(text: String) = Taken(text.trim(), emptyList(), emptyList(), {}) }
}

/** What the session asks of the app around it. [AppChatEnv] is the real one; tests use their own. */
interface ChatEnv {
  val restricted: Boolean
  /** the longest message in characters, or null for no limit (admin) */
  val messageLimit: Int?
  fun runOn(): RunOn
  fun keepRunning(): Boolean
  /** how many things are attached to the message being written (the media module's strip) */
  val attachmentCount: StateFlow<Int>
  /** takes the attachments for a send: the text to send (link chips added), the payload, the facts for the bubble, and how to put them back */
  fun takeForSend(typed: String): Taken
  fun toast(text: String)
  /** the chat lists should refresh, after [delayMs] */
  fun chatsChanged(delayMs: Long = 0)
  /** an approval or a question waits: a notification when the app is in the background (INTER-12) */
  fun needsYou(chatId: String?, kind: NeedsKind, streamId: Long?, id: Long, summary: String)
  /** that approval or question was answered or is over */
  fun needsYouDone(chatId: String?, kind: NeedsKind, id: Long)
  fun desktopOffline()
  /** COMP-19: attachments are still being read; a send must wait (the strip shows spinners) */
  val attachmentsBusy: Boolean get() = false
  /** COMP-20: true when [beforeSend] must be asked before a message goes to My computer */
  val checksBeforeSend: Boolean get() = false
  /** COMP-20: looks at the linked computer before a send; a sentence means do not send (the text stays in the box) */
  suspend fun beforeSend(): String? = null
  /** NOTIF-01, VOICE-11: an answer finished ([answer] is its text); a notification when the app is in the background, read aloud when that setting is on */
  fun answerFinished(chatId: String?, title: String, answer: String) {}
}
