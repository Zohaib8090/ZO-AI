package org.zohara.chat.core.util

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context

/** The two channels of D10 / NOTIF-12: "Answers" (default importance) and "Needs you" (high). Created at start; the person can tune them in system settings. */
object NotificationChannels {
  const val ANSWERS = "answers"
  const val NEEDS_YOU = "needs_you"

  fun create(context: Context) {
    val nm = context.getSystemService(NotificationManager::class.java) ?: return
    nm.createNotificationChannel(NotificationChannel(ANSWERS, "Answers", NotificationManager.IMPORTANCE_DEFAULT).apply { description = "Zohara Chat has finished an answer" })
    nm.createNotificationChannel(NotificationChannel(NEEDS_YOU, "Needs you", NotificationManager.IMPORTANCE_HIGH).apply { description = "Zohara Chat is waiting for your yes or an answer" })
  }
}
