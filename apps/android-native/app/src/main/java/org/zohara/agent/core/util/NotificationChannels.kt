package org.zohara.agent.core.util

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context

/** The channels of D10 / NOTIF-12: "Answers" (default importance), "Needs you" (high) and "App updates" (low: no sound; one notice per new version). Created at start; the person can tune them in system settings. */
object NotificationChannels {
  const val ANSWERS = "answers"
  const val NEEDS_YOU = "needs_you"
  const val UPDATES = "updates"

  fun create(context: Context) {
    val nm = context.getSystemService(NotificationManager::class.java) ?: return
    nm.createNotificationChannel(NotificationChannel(ANSWERS, "Answers", NotificationManager.IMPORTANCE_DEFAULT).apply { description = "Zohara Agent has finished an answer" })
    nm.createNotificationChannel(NotificationChannel(NEEDS_YOU, "Needs you", NotificationManager.IMPORTANCE_HIGH).apply { description = "Zohara Agent is waiting for your yes or an answer" })
    nm.createNotificationChannel(NotificationChannel(UPDATES, "App updates", NotificationManager.IMPORTANCE_LOW).apply { description = "A new version of Zohara Agent is available" })
  }
}
