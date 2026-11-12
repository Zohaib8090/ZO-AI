package org.zohara.zoai

import android.app.Application
import org.zohara.zoai.core.util.NotificationChannels

class ZoApplication : Application() {
  lateinit var container: AppContainer
    private set

  override fun onCreate() {
    super.onCreate()
    container = AppContainer(this)
    NotificationChannels.create(this)
    container.start()
  }
}
