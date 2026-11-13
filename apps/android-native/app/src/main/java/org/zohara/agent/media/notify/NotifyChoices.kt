package org.zohara.agent.media.notify

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Two notification choices that have no place in the shared preferences: "Show the chat title in notifications" and "Sound" (Settings > Notifications, NOTIF-07). */
class NotifyChoices(context: Context) {
  private val p = context.applicationContext.getSharedPreferences("zo_notify", Context.MODE_PRIVATE)
  private val _showTitle = MutableStateFlow(p.getBoolean("show_title", false))
  private val _sound = MutableStateFlow(p.getBoolean("sound", true))
  val showTitle: StateFlow<Boolean> = _showTitle.asStateFlow()
  val sound: StateFlow<Boolean> = _sound.asStateFlow()

  /** the explanation after sign-in was shown once (INTER-15): never asked again by itself, the person can still turn it on in Settings > Notifications */
  var asked: Boolean
    get() = p.getBoolean("asked", false)
    set(v) { p.edit().putBoolean("asked", v).apply() }

  fun setShowTitle(on: Boolean) { p.edit().putBoolean("show_title", on).apply(); _showTitle.value = on }
  fun setSound(on: Boolean) { p.edit().putBoolean("sound", on).apply(); _sound.value = on }
}
