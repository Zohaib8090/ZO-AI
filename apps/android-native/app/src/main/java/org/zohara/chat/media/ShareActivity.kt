package org.zohara.chat.media

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.zohara.chat.MainActivity
import org.zohara.chat.ZoApplication

/**
 * The share target (ATT-22, PHN-08): other apps share text, a link, pictures, PDF, ZIP, JSON or text files to Zohara Chat. This invisible activity copies the files into the
 * app's cache (the sender's permission ends with this activity), puts them in the attachment strip, hands the text and links to [ShareInbox], and opens the main
 * screen, which shows a new message.
 */
class ShareActivity : ComponentActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    val container = (application as ZoApplication).container
    val source = intent
    lifecycleScope.launch {
      val content = withContext(Dispatchers.IO) { ShareParser.parse(this@ShareActivity, source) }
      if (content != null) ShareInbox.deliver(MediaModule.of(container), content) { s -> container.scope.launch { container.toast.show(s) } }
      startActivity(Intent(this@ShareActivity, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT).putExtra(ShareInbox.EXTRA_SHARED, true))
      finish()
    }
  }
}
