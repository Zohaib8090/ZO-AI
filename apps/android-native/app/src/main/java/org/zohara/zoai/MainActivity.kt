package org.zohara.zoai

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import kotlinx.coroutines.flow.MutableStateFlow
import org.zohara.zoai.nav.DeepLinks
import org.zohara.zoai.nav.ZoApp

/** The single activity (the whole app is Compose). */
class MainActivity : ComponentActivity() {
  private val pendingChat = MutableStateFlow<String?>(null)

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    enableEdgeToEdge()
    pendingChat.value = DeepLinks.pendingFrom(intent)
    val container = (application as ZoApplication).container
    setContent { ZoApp(container, pendingChat) }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    DeepLinks.pendingFrom(intent)?.let { pendingChat.value = it }
  }
}
