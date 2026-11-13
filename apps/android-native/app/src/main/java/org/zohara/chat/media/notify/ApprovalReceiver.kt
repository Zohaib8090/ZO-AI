package org.zohara.chat.media.notify

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import kotlinx.coroutines.launch
import org.zohara.chat.ZoApplication
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.media.MediaModule

/** What the notification buttons need from the server: `POST /api/reply {stream, op:"approve", id, yes}`. */
fun interface ApproveApi { suspend fun approve(stream: Long, id: Long, yes: Boolean): Boolean }

object ApprovalActions {
  const val NOT_WAITING = "Zohara Chat is no longer waiting for this."
  const val ON_COMPUTER = "Waiting for your approval on your computer."

  /** Sends the answer and returns the sentence that replaces the buttons (INTER-04: 409 `approvals_on_computer` means the computer must answer). */
  suspend fun send(api: ApproveApi, stream: Long, id: Long, yes: Boolean): String = try {
    if (api.approve(stream, id, yes)) (if (yes) "Allowed." else "Denied.") else NOT_WAITING
  } catch (e: ApiException) {
    if (e.status == 409 || e.text.contains("approvals_on_computer")) ON_COMPUTER else "Could not send your answer: ${e.text}"
  } catch (e: Exception) {
    "Could not send your answer."
  }
}

/** The Allow and Deny buttons of an approval notification (INTER-12, D10). Needs the signed-in session: the API client adds the token. */
class ApprovalReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != ACTION) return
    val chat = intent.getStringExtra(EXTRA_CHAT) ?: return
    val stream = intent.getLongExtra(EXTRA_STREAM, 0)
    val id = intent.getLongExtra(EXTRA_ID, -1)
    val yes = intent.getBooleanExtra(EXTRA_YES, false)
    if (stream == 0L || id < 0) return
    val pending = goAsync()
    val container = (context.applicationContext as ZoApplication).container
    container.scope.launch {
      try {
        val text = ApprovalActions.send({ s, i, y -> container.api.replyApprove(s, i, y) }, stream, id, yes)
        MediaModule.of(container).notifier.resolved(chat, text)
      } finally {
        pending.finish()
      }
    }
  }

  companion object {
    const val ACTION = "org.zohara.chat.action.APPROVAL"
    const val EXTRA_CHAT = "chat"
    const val EXTRA_STREAM = "stream"
    const val EXTRA_ID = "id"
    const val EXTRA_YES = "yes"

    fun pending(context: Context, chat: String, stream: Long, id: Long, yes: Boolean): PendingIntent {
      val i = Intent(ACTION).setComponent(ComponentName(context, ApprovalReceiver::class.java))
        .setData(Uri.parse("zohara-chat-action://approval/$stream/$id/${if (yes) "allow" else "deny"}"))
        .putExtra(EXTRA_CHAT, chat).putExtra(EXTRA_STREAM, stream).putExtra(EXTRA_ID, id).putExtra(EXTRA_YES, yes)
      return PendingIntent.getBroadcast(context, 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }
  }
}
