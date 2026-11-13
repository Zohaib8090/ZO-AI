package org.zohara.chat.media.notify

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.zohara.chat.ZoApplication
import org.zohara.chat.media.MediaModule

/**
 * Receives FCM (NOTIF-04, BG-05). With a `notification` block the system shows the message itself while the app is in the background and the launcher intent carries
 * the data (`chat`: handled by DeepLinks in MainActivity); this service sees a message when the app is visible (then nothing is shown) and every data-only message.
 * Data: `{type:"answer", chat, title?}`; a chat id that is not `[\w-]{1,64}` is dropped; push payloads never contain message text.
 */
class ZoMessagingService : FirebaseMessagingService() {
  override fun onMessageReceived(message: RemoteMessage) {
    val c = (application as ZoApplication).container
    MediaModule.of(c).notifier.fromPush(message.data)
  }

  override fun onNewToken(token: String) {
    val c = (application as ZoApplication).container
    MediaModule.of(c).onNewToken(token)
  }
}
