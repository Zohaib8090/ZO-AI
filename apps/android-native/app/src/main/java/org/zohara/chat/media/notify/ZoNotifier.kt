package org.zohara.chat.media.notify

import android.Manifest
import android.annotation.SuppressLint
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.StateFlow
import org.zohara.chat.MainActivity
import org.zohara.chat.R
import org.zohara.chat.core.util.NotificationChannels
import org.zohara.chat.nav.DeepLinks

/** The person's notification choices, as one value (Settings > Notifications). */
data class NotifySettings(
  /** "Answer finished while the app is in the background" */
  val answers: Boolean = true,
  /** "Zohara Chat needs my approval": approvals and questions */
  val needsYou: Boolean = true,
  /** "Show details in notifications": the command or question text (default off) */
  val details: Boolean = false,
  /** "Show the chat title in notifications" (default off) */
  val showTitle: Boolean = false,
  /** "Sound": off makes the notifications silent */
  val sound: Boolean = true,
)

object NotificationPermission {
  /** Are notifications allowed? (Android 13+: the POST_NOTIFICATIONS permission; before: the app's notification switch.) */
  fun granted(context: Context): Boolean =
    if (Build.VERSION.SDK_INT >= 33) ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
    else NotificationManagerCompat.from(context).areNotificationsEnabled()
}

/**
 * Makes the notifications (NOTIF-01, NOTIF-06, INTER-12): "Answers" when an answer finishes while the app is in the background (one per chat, same tag as the
 * server's push so they replace each other), and on "Needs you" the approvals (Allow and Deny buttons) and questions (Answer opens the chat).
 * Nothing is shown while the app is visible, when the permission is missing, or when the person switched that kind off.
 */
@SuppressLint("MissingPermission") // every notification is made after [permission] was checked
class ZoNotifier(
  private val context: Context,
  private val settings: StateFlow<NotifySettings>,
  private val foreground: () -> Boolean = { AppVisibility.foreground.value },
  private val permission: () -> Boolean = { NotificationPermission.granted(context) },
  private val post: (tag: String, id: Int, n: android.app.Notification) -> Unit = { tag, id, n -> NotificationManagerCompat.from(context).apply { runCatching { notify(tag, id, n) } } },
) {
  private val nm: NotificationManager? get() = context.getSystemService(NotificationManager::class.java)

  private fun tagAnswer(chat: String) = "zo-chat-$chat"
  private fun tagNeed(chat: String) = "zo-need-$chat"
  private fun id(tag: String) = tag.hashCode()

  /** the intent a tap sends to the app: opens the chat (NAV-20) */
  fun openChatIntent(chat: String, extra: Map<String, String> = emptyMap()): PendingIntent {
    val uri = Uri.parse("${DeepLinks.SCHEME}://chat/${Uri.encode(chat)}")
    val i = Intent(Intent.ACTION_VIEW, uri).setClass(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    i.putExtra(DeepLinks.EXTRA_CHAT, chat)
    extra.forEach { (k, v) -> i.putExtra(k, v) }
    return PendingIntent.getActivity(context, id(uri.toString() + extra), i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
  }

  private fun builder(channel: String, t: NotifyText.Text, chat: String, tap: PendingIntent): NotificationCompat.Builder =
    NotificationCompat.Builder(context, channel)
      .setSmallIcon(R.drawable.ic_stat_zo)
      .setContentTitle(t.title)
      .setContentText(t.body)
      .setContentIntent(tap)
      .setAutoCancel(true)
      .setOnlyAlertOnce(true)
      .setSilent(!settings.value.sound)
      .setCategory(NotificationCompat.CATEGORY_MESSAGE)
      .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)

  /** An answer finished. [chatTitle] is only used when the person turned titles on. Returns true when a notification was shown. */
  fun answerFinished(chat: String, chatTitle: String?): Boolean {
    val chatId = DeepLinks.validChatId(chat) ?: return false
    val s = settings.value
    if (foreground() || !s.answers || !permission()) return false
    val t = NotifyText.answer(s.showTitle, chatTitle)
    val tag = tagAnswer(chatId)
    post(tag, id(tag), builder(NotificationChannels.ANSWERS, t, chatId, openChatIntent(chatId)).setPriority(NotificationCompat.PRIORITY_DEFAULT).build())
    return true
  }

  /** The app is not running and a data message came from FCM: `{type, chat, title?}` (NOTIF-04). Unknown types and invalid chat ids are dropped. */
  fun fromPush(data: Map<String, String>): Boolean {
    val chat = DeepLinks.validChatId(data["chat"]) ?: return false
    return when (data["type"]) {
      "answer" -> answerFinished(chat, data["title"])
      "needs_you", "approval", "question" -> needsYouGeneric(chat, data["title"])
      else -> false
    }
  }

  private fun needsYouGeneric(chat: String, title: String?): Boolean {
    val s = settings.value
    if (foreground() || !s.needsYou || !permission()) return false
    val t = NotifyText.approval(false, null)
    val tag = tagNeed(chat)
    post(tag, id(tag), builder(NotificationChannels.NEEDS_YOU, t, chat, openChatIntent(chat)).setPriority(NotificationCompat.PRIORITY_HIGH).build())
    return true
  }

  /** The model wants to do something that needs a yes (INTER-12). Buttons Allow and Deny answer through [ApprovalReceiver]. */
  fun approval(chat: String, stream: Long, approvalId: Long, summary: String?, tool: String? = null): Boolean {
    val chatId = DeepLinks.validChatId(chat) ?: return false
    val s = settings.value
    if (foreground() || !s.needsYou || !permission()) return false
    val t = NotifyText.approval(s.details, summary ?: tool)
    val tag = tagNeed(chatId)
    val b = builder(NotificationChannels.NEEDS_YOU, t, chatId, openChatIntent(chatId))
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .addAction(0, "Allow", ApprovalReceiver.pending(context, chatId, stream, approvalId, true))
      .addAction(0, "Deny", ApprovalReceiver.pending(context, chatId, stream, approvalId, false))
    post(tag, id(tag), b.build())
    return true
  }

  /** The model asks a question (INTERACTION.md): "Answer" opens the chat where the question card waits. */
  fun question(chat: String, stream: Long, questionId: Long, firstQuestion: String?): Boolean {
    val chatId = DeepLinks.validChatId(chat) ?: return false
    val s = settings.value
    if (foreground() || !s.needsYou || !permission()) return false
    val t = NotifyText.question(s.details, firstQuestion)
    val tag = tagNeed(chatId)
    val tap = openChatIntent(chatId, mapOf(EXTRA_QUESTION to questionId.toString(), EXTRA_STREAM to stream.toString()))
    post(tag, id(tag), builder(NotificationChannels.NEEDS_YOU, t, chatId, tap).setPriority(NotificationCompat.PRIORITY_HIGH).addAction(0, "Answer", tap).build())
    return true
  }

  /** The decision was made (in the app or from the notification): the notice goes away. */
  fun cancelNeedsYou(chat: String) { nm?.cancel(tagNeed(chat), id(tagNeed(chat))) }
  fun cancelAnswer(chat: String) { nm?.cancel(tagAnswer(chat), id(tagAnswer(chat))) }

  /** "Open notifications are closed on resume" (NOTIF-02). */
  fun cancelAll() { nm?.cancelAll() }

  /** Settings > Notifications > "Send a test notification". Ignores the foreground rule so the person sees it. */
  fun test(): Boolean {
    if (!permission()) return false
    val t = NotifyText.Text("Zohara Chat", "This is a test notification. It works.")
    post("zo-test", id("zo-test"), builder(NotificationChannels.ANSWERS, t, "test", openChatIntent("test")).build())
    return true
  }

  /** shows [text] in place of the buttons after an approval was sent from the notification, or why it could not be */
  fun resolved(chat: String, text: String) {
    val tag = tagNeed(chat)
    if (!permission()) return
    post(tag, id(tag), builder(NotificationChannels.NEEDS_YOU, NotifyText.Text("Zohara Chat", text), chat, openChatIntent(chat)).setTimeoutAfter(8000).build())
  }

  companion object {
    const val EXTRA_QUESTION = "question"
    const val EXTRA_STREAM = "stream"
  }
}
