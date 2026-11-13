package org.zohara.zoai.settings

import android.Manifest
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.R
import org.zohara.zoai.core.design.DangerButton
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.SettingsRow
import org.zohara.zoai.core.design.ZoSwitch
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.PushSubscription
import org.zohara.zoai.core.util.NotificationChannels
import org.zohara.zoai.media.MediaModule
import org.zohara.zoai.media.notify.NotificationPermission
import org.zohara.zoai.media.notify.PushResult

object NotifLogic {
  const val BLOCKED = "Notifications are blocked for ZO-AI. Allow them in the system settings (Notifications), then come back here."
  const val ASK = "ZO-AI shows a short notice when an answer finishes while the app is in the background, and when it needs your yes or no. Android asks for permission first. Nothing is sent anywhere for this: the notice is made on your phone."
  const val CLOSED = "ZO-AI keeps writing the answer on the server after you close the app (a few minutes at most), saves the chat and sends a push through Google's push service. The push only says that an answer is ready."
  const val DEVICES_NOTE = "You can register up to 5 devices. A device that your push service no longer knows is removed automatically."

  /** What to tell the person after the switch "Also notify me when the app is closed" was changed. */
  fun pushMessage(r: PushResult, wanted: Boolean): String = when (r) {
    is PushResult.Registered -> "Background notifications are on"
    PushResult.Off -> if (wanted) "Turn on notifications first." else "Background notifications are off"
    PushResult.NeedsPermission -> "Turn on notifications first."
    PushResult.ServerCannot -> "This server cannot send notifications to the app."
    PushResult.NoToken -> "This phone could not get a push address (Google services are missing or not set up in this build)."
    is PushResult.Failed -> r.message
  }

  fun deviceTitle(s: PushSubscription, thisId: String): String =
    s.label.ifBlank { if (s.kind == "fcm") "Android app" else "Browser" } + if (s.id == thisId) " (this device)" else ""

  fun deviceSub(s: PushSubscription): String = "Added " + if (s.created > 0) formatWhen(toMillis(s.created)) else "earlier"
}

/** What the Notifications page shows (stateless: screenshot tested). */
data class NotifState(
  val permission: Boolean,
  val denied: Boolean,
  val answers: Boolean,
  val needsYou: Boolean,
  val details: Boolean,
  val showTitle: Boolean,
  val sound: Boolean,
  val closed: Boolean,
  val admin: Boolean,
  val devices: List<PushSubscription>,
  val thisDevice: String,
)

class NotifActions(
  val onAsk: () -> Unit = {}, val onOpenSystem: () -> Unit = {},
  val onAnswers: (Boolean) -> Unit = {}, val onNeedsYou: (Boolean) -> Unit = {}, val onDetails: (Boolean) -> Unit = {},
  val onShowTitle: (Boolean) -> Unit = {}, val onSound: (Boolean) -> Unit = {}, val onTest: () -> Unit = {},
  val onClosed: (Boolean) -> Unit = {}, val onRemoveDevice: (PushSubscription) -> Unit = {},
)

/** Notifications. Parity rows: NOTIF-07, 08, INTER-15, INTER-16. */
@Composable
fun SettingsNotificationsScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val account = rememberAccount()
  val scope = rememberCoroutineScope()
  val doer = rememberDoer()
  val media = remember { MediaModule.of(container) }
  val answers by container.prefs.notifAnswers.collectAsState(true)
  val needs by container.prefs.notifNeedsYou.collectAsState(true)
  val details by container.prefs.notifDetails.collectAsState(false)
  val showTitle by media.choices.showTitle.collectAsState()
  val sound by media.choices.sound.collectAsState()
  var permission by remember { mutableStateOf(NotificationPermission.granted(context)) }
  var denied by remember { mutableStateOf(false) }
  var closed by remember { mutableStateOf(media.push.enabled) }
  var devices by remember { mutableStateOf<List<PushSubscription>>(emptyList()) }
  var tick by remember { mutableIntStateOf(0) }
  LaunchedEffect(tick, closed, permission) {
    devices = try { media.push.devices() } catch (_: ApiException) { emptyList() }
  }
  val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
    permission = NotificationPermission.granted(context)
    denied = !ok
    if (ok) doer.say("Notifications are on")
  }
  SettingsPage("Notifications") {
    SectionHeader("Install")
    SettingsCard { SettingsRow("Install ZO-AI as an app", subtitle = "You are already using the ZO-AI app.", divider = false) }
    NotificationsContent(
      NotifState(permission, denied, answers, needs, details, showTitle, sound, closed, account?.admin == true && account.restricted == false, devices, media.push.registrationId),
      NotifActions(
        onAsk = { if (Build.VERSION.SDK_INT >= 33) launcher.launch(Manifest.permission.POST_NOTIFICATIONS) else denied = true },
        onOpenSystem = { context.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) },
        onAnswers = { v -> scope.launch { container.prefs.setNotifAnswers(v) } },
        onNeedsYou = { v -> scope.launch { container.prefs.setNotifNeedsYou(v) } },
        onDetails = { v -> scope.launch { container.prefs.setNotifDetails(v) } },
        onShowTitle = { v -> media.choices.setShowTitle(v); scope.launch { if (media.push.enabled) { runCatching { media.setClosedPush(true) }; tick++ } } },
        onSound = { v -> media.choices.setSound(v) },
        onTest = { doer.say(if (sendTest(context, sound)) "Test notification sent" else "Android did not show it. Check the notification settings.") },
        onClosed = { on ->
          closed = on
          scope.launch {
            val r = media.setClosedPush(on)
            doer.say(NotifLogic.pushMessage(r, on))
            if (on && r !is PushResult.Registered) { media.setClosedPush(false); closed = false }
            tick++
          }
        },
        onRemoveDevice = { d -> scope.launch { try { media.push.removeDevice(d.id); if (d.id == media.push.registrationId || !media.push.enabled) closed = media.push.enabled } catch (e: ApiException) { doer.say(e.text) }; tick++ } },
      ),
    )
  }
}

/** The test notice: shown now, on the "Answers" channel. */
fun sendTest(context: Context, sound: Boolean): Boolean {
  if (!NotificationPermission.granted(context)) return false
  NotificationChannels.create(context)
  val n = NotificationCompat.Builder(context, NotificationChannels.ANSWERS)
    .setSmallIcon(R.drawable.ic_stat_zo).setContentTitle("ZO-AI").setContentText("This is a test notification. It works.")
    .setAutoCancel(true).setSilent(!sound).build()
  return try { NotificationManagerCompat.from(context).notify("zo-test", 1, n); true } catch (_: SecurityException) { false }
}

@Composable
fun NotificationsContent(s: NotifState, a: NotifActions) {
  SectionHeader("Notifications")
  if (!s.permission) {
    SettingsCard {
      ActionRow(
        "Tell me when an answer is ready", if (s.denied) NotifLogic.BLOCKED else NotifLogic.ASK, divider = false,
        actions = {
          if (s.denied) { PillButton("Blocked", {}, enabled = false); PrimaryButton("Open system settings", a.onOpenSystem) }
          else PrimaryButton("Turn on notifications", a.onAsk)
        },
      )
    }
    return
  }
  SettingsCard {
    SettingsRow("Answer finished while the app is in the background", subtitle = "A notice that the answer is ready. Nothing is shown while you are looking at the app.", control = { ZoSwitch(s.answers, a.onAnswers) })
    if (s.admin) SettingsRow("ZO-AI needs my approval", subtitle = "A notice when a step waits for your yes or no, or ZO-AI asks you a question, and the app is in the background.", control = { ZoSwitch(s.needsYou, a.onNeedsYou) })
    SettingsRow("Show details in notifications", subtitle = "Off: the notice only says that ZO-AI needs you. On: it also shows the command or the question (on your lock screen too).", control = { ZoSwitch(s.details, a.onDetails) })
    SettingsRow("Show the chat title in notifications", subtitle = "Off: the notice only says that an answer is ready. On: it also shows the chat's title (on your lock screen too).", control = { ZoSwitch(s.showTitle, a.onShowTitle) })
    SettingsRow("Sound", subtitle = "Use your system's notification sound.", divider = false, control = { ZoSwitch(s.sound, a.onSound) })
  }
  Spacer(Modifier.height(12.dp))
  SettingsCard { ActionRow("Test", "Shows a notification now, even though you are looking at the app.", divider = false, actions = { PillButton("Send a test notification", a.onTest) }) }
  SectionHeader("When the app is closed")
  SettingsCard {
    SettingsRow("Also notify me when the app is closed", subtitle = NotifLogic.CLOSED, divider = false, control = { ZoSwitch(s.closed, a.onClosed) })
  }
  if (s.devices.isNotEmpty()) {
    Spacer(Modifier.height(12.dp))
    SettingsCard {
      s.devices.forEachIndexed { i, d ->
        ActionRow(NotifLogic.deviceTitle(d, s.thisDevice), NotifLogic.deviceSub(d), divider = i != s.devices.lastIndex, actions = { DangerButton("Remove", { a.onRemoveDevice(d) }, outlined = true) })
      }
    }
    Note(NotifLogic.DEVICES_NOTE)
  }
}
