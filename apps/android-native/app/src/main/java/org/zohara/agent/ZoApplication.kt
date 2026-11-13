package org.zohara.agent

import android.app.Application
import org.zohara.agent.core.util.NotificationChannels

class ZoApplication : Application() {
  lateinit var container: AppContainer
    private set

  override fun onCreate() {
    super.onCreate()
    container = AppContainer(this)
    NotificationChannels.create(this)
    container.start()
    // media-link module: notifications, push registration and foreground tracking (the composer controls are plugged in by ZoApp, where the container is part of the composition)
    org.zohara.agent.media.MediaModule.of(container).install()
  }
}
