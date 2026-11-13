package org.zohara.chat.media.notify

import android.app.Activity
import android.app.Application
import android.os.Bundle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Is any screen of the app visible? Notifications are only made while it is not (NOTIF-01, INTER-12). Counts started activities. */
object AppVisibility {
  private val _foreground = MutableStateFlow(false)
  val foreground: StateFlow<Boolean> = _foreground.asStateFlow()
  private var started = 0
  private var installed = false

  @Synchronized
  fun install(app: Application) {
    if (installed) return
    installed = true
    app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
      override fun onActivityStarted(activity: Activity) { started++; _foreground.value = true }
      override fun onActivityStopped(activity: Activity) { started = (started - 1).coerceAtLeast(0); _foreground.value = started > 0 }
      override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) {}
      override fun onActivityResumed(activity: Activity) {}
      override fun onActivityPaused(activity: Activity) {}
      override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) {}
      override fun onActivityDestroyed(activity: Activity) {}
    })
  }

  /** for tests */
  fun set(value: Boolean) { _foreground.value = value }
}
