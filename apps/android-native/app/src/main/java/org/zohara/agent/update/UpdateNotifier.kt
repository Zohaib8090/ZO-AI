package org.zohara.agent.update

import android.Manifest
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.first
import org.zohara.agent.MainActivity
import org.zohara.agent.R
import org.zohara.agent.core.store.ZoPrefs
import org.zohara.agent.core.util.NotificationChannels

/** ONE notification per new version, on the "App updates" channel. Tapping it opens Settings > About (`zohara-agent://about`). Nothing is downloaded. */
object UpdateNotifier {
  const val NOTIFICATION_ID = 7101

  /** Tell the person about [latest]? Only when it is newer than the running version and not the one already announced (or older than it). */
  fun shouldNotify(current: String, latest: String, lastNotified: String?): Boolean {
    if (!AppVersion.isNewer(current, latest)) return false
    return lastNotified.isNullOrBlank() || AppVersion.isNewer(lastNotified, latest)
  }

  fun contentIntent(context: Context): PendingIntent {
    val intent = Intent(Intent.ACTION_VIEW, Uri.parse("zohara-agent://about"), context, MainActivity::class.java)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    return PendingIntent.getActivity(context, NOTIFICATION_ID, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
  }

  fun canPost(context: Context): Boolean {
    if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return false
    return NotificationManagerCompat.from(context).areNotificationsEnabled()
  }

  /** Shows the notification when [outcome] is a version the person has not been told about yet, and remembers it. Returns whether one was shown. */
  suspend fun announce(context: Context, prefs: ZoPrefs, outcome: UpdateOutcome): Boolean {
    val a = outcome as? UpdateOutcome.Available ?: return false
    if (!shouldNotify(a.current, a.latest.version, prefs.updateNotified.first())) return false
    // not posted (permission off): not remembered either, so it can still be shown once it is allowed
    if (!canPost(context)) return false
    NotificationChannels.create(context)
    val first = a.latest.notes.lineSequence().map { it.trim() }.firstOrNull { it.isNotEmpty() }
    val text = "Version ${a.latest.version} is available (you have ${a.current})." + (first?.let { " " + it.take(120) } ?: "")
    val n = NotificationCompat.Builder(context, NotificationChannels.UPDATES)
      .setSmallIcon(R.drawable.ic_stat_zo)
      .setContentTitle(if (a.required) "Zohara Agent update required" else "Zohara Agent update available")
      .setContentText(text)
      .setStyle(NotificationCompat.BigTextStyle().bigText(text))
      .setContentIntent(contentIntent(context))
      .setAutoCancel(true)
      .setOnlyAlertOnce(true)
      .setCategory(NotificationCompat.CATEGORY_RECOMMENDATION)
      .setPriority(NotificationCompat.PRIORITY_LOW)
      .build()
    return try {
      NotificationManagerCompat.from(context).notify(NOTIFICATION_ID, n)
      prefs.setUpdateNotified(a.latest.version)
      true
    } catch (_: SecurityException) { false }
  }
}
