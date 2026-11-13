package org.zohara.zoai.media.notify

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.Text
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.Modifier
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.media.MediaModule

object NotificationAskText {
  const val TITLE = "Know when ZO-AI is done"
  const val BODY = "ZO-AI can tell you when an answer is ready, or when it needs a yes or a no from you, even when the app is closed. The notices never contain your messages or its answers. You can change this in Settings > Notifications."
}

/** The rule of the one-time question after sign-in (INTER-15): not when it was asked before, and not when notifications are already allowed. */
fun shouldAskAboutNotifications(alreadyAsked: Boolean, granted: Boolean): Boolean = !alreadyAsked && !granted

/**
 * Asked once after sign-in, with an explanation (INTER-15, NOTIF-03): "Turn on" asks Android for the permission (13 and later) and then switches on the notices for the
 * closed app (the registration happens in [org.zohara.zoai.media.MediaModule.setClosedPush]); "Not now" asks nothing and is not asked again. Put once in the signed-in shell.
 */
@Composable
fun NotificationAskOnce() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val module = remember(container) { MediaModule.of(container) }
  val scope = rememberCoroutineScope()
  var show by remember { mutableStateOf(shouldAskAboutNotifications(module.choices.asked, NotificationPermission.granted(context))) }
  val finish = { module.choices.asked = true; show = false }
  val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
    finish()
    if (ok) scope.launch { module.setClosedPush(true) }
  }
  if (!show) return
  ZoBottomSheet(NotificationAskText.TITLE, onDismiss = finish, footer = {
    SecondaryButton("Not now", finish)
    PrimaryButton("Turn on", { if (Build.VERSION.SDK_INT >= 33) launcher.launch(Manifest.permission.POST_NOTIFICATIONS) else { finish(); scope.launch { module.setClosedPush(true) } } })
  }) {
    Text(NotificationAskText.BODY, style = ZoTheme.type.body, color = ZoTheme.colors.text2, modifier = Modifier.verticalScroll(rememberScrollState()))
  }
}
