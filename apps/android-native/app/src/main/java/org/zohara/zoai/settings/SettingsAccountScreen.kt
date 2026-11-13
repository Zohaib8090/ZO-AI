package org.zohara.zoai.settings

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import java.time.LocalDate
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.DangerButton
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.AccountDevice
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ClaudeStatus
import org.zohara.zoai.core.net.Whoami
import org.zohara.zoai.core.util.openInBrowser
import org.zohara.zoai.nav.LocalNav
import org.zohara.zoai.nav.Routes

/** The pure rules of the Account page (SET-ACC-01..04). */
object AccountLogic {
  fun deviceKind(d: AccountDevice): String = when (d.kind) { "desktop" -> "Desktop app"; "cli" -> "Terminal"; else -> if (d.scope == "full") "Terminal" else "Desktop app" }

  fun deviceName(d: AccountDevice): String = d.name.ifBlank { deviceKind(d) }

  fun deviceLine(d: AccountDevice, now: Long = System.currentTimeMillis()): String =
    deviceKind(d) + " · last used " + (if (d.lastUsed > 0) formatAgo(toMillis(d.lastUsed), now) else "never") + (if (d.created > 0) " · signed in " + formatWhen(toMillis(d.created)) else "")

  fun dataText(w: Whoami): String =
    humanBytes(w.dataBytes) + (w.limits.dataBytes?.let { " of ${humanBytes(it)}" } ?: "") + ". Chats, memory and personalities."

  fun exportFileName(date: LocalDate): String = "zo-ai-export-$date.json"

  fun deleteDone(loginRemoved: Boolean): String =
    if (loginRemoved) "Your account and data were deleted."
    else "Your data was deleted, but your sign-in record could not be removed yet. Ask the operator (see Privacy) to finish it."

  /** SET-ACC-04: the button of the delete sheet works only on exactly DELETE. */
  fun deleteConfirmed(typed: String): Boolean = typed == "DELETE"

  fun consentText(w: Whoami): String = if (w.terms.version.isBlank()) "What is stored, where, and who can see it." else
    "What is stored, where, and who can see it. " + if (w.terms.accepted) "You accepted the notice and terms of version ${w.terms.version}." else "The notice and terms of version ${w.terms.version} are not accepted yet."
}

class AccountData(val me: Whoami, val devices: List<AccountDevice>, val claude: ClaudeStatus?)

/** Account. Parity rows: SET-ACC-01..05, 06 (Claude Code status), AUTH-28 link. */
@Composable
fun SettingsAccountScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val nav = LocalNav.current
  val doer = rememberDoer()
  val scope = rememberCoroutineScope()
  val loader = rememberLoader {
    coroutineScope {
      val w = async { container.api.whoami() }
      val d = async { try { container.api.devicesList() } catch (_: ApiException) { emptyList() } }
      val me = w.await()
      val cc = if (me.restricted) null else try { container.api.claudeStatus().takeIf { it.admin } } catch (_: ApiException) { null }
      AccountData(me, d.await(), cc)
    }
  }
  val load by loader.state.collectAsState()
  var revoke by remember { mutableStateOf<AccountDevice?>(null) }
  var deleteSheet by remember { mutableStateOf(false) }
  var exporting by remember { mutableStateOf(false) }
  var pendingExport by remember { mutableStateOf<String?>(null) }
  val saver = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
    val json = pendingExport
    pendingExport = null
    if (uri != null && json != null) scope.launch {
      try {
        withContext(Dispatchers.IO) { context.contentResolver.openOutputStream(uri)?.use { it.write(json.toByteArray()) } ?: error("no stream") }
        doer.say("Export saved")
      } catch (_: Exception) { doer.say("The file could not be saved.") }
    }
  }

  SettingsPage("Account") {
    LoadBox(load, { loader.load() }) { a ->
      AccountContent(
        a, exporting,
        onPrivacy = { openInBrowser(context, legalUrl(container.serverUrl, "privacy")) },
        onTerms = { openInBrowser(context, legalUrl(container.serverUrl, "terms")) },
        onRevoke = { revoke = it },
        onExport = {
          exporting = true
          doer.run(onDone = { exporting = false }, onFail = { exporting = false; doer.say(it) }) {
            pendingExport = container.api.exportAccount()
            saver.launch(AccountLogic.exportFileName(LocalDate.now()))
          }
        },
        onDelete = { deleteSheet = true },
        onApprove = { nav.settings(Routes.SETTINGS_APPROVE) },
        onAdvanced = { nav.settings(Routes.SETTINGS_ADVANCED) },
      )
    }
  }
  revoke?.let { d ->
    ConfirmSheet("Sign out ${AccountLogic.deviceName(d)}?", "It stops syncing and has to sign in again. Chats already synced stay.", "Sign out", { doer.run(onDone = { loader.load() }) { container.api.deviceRevoke(d.id) } }, { revoke = null })
  }
  if (deleteSheet) DeleteAccountSheet(onDismiss = { deleteSheet = false }) {
    val r = container.api.deleteAccount()
    // the screen is gone once the sign-out happens, so the app's scope shows the toast and signs out
    container.scope.launch {
      container.toast.show(AccountLogic.deleteDone(r.loginRemoved))
      container.session.signOut()
    }
  }
}

@Composable
fun AccountContent(
  a: AccountData, exporting: Boolean, onPrivacy: () -> Unit, onTerms: () -> Unit, onRevoke: (AccountDevice) -> Unit, onExport: () -> Unit,
  onDelete: () -> Unit, onApprove: () -> Unit, onAdvanced: () -> Unit,
) {
  val me = a.me
  SectionHeader("You")
  SettingsCard {
    ActionRow("Signed in as", me.email + if (me.admin) " · administrator" else "")
    ActionRow("Data stored for you", AccountLogic.dataText(me))
    limitsText(me.limits).takeIf { it.isNotBlank() }?.let { ActionRow("Limits", it) }
    ActionRow("Legal", AccountLogic.consentText(me), divider = false, actions = { PillButton("Privacy", onPrivacy); PillButton("Terms", onTerms) })
  }
  a.claude?.let { cc ->
    SectionHeader("Claude Code (your subscription)")
    ClaudeCodeCard(cc)
  }
  SectionHeader("Signed-in devices")
  SettingsCard {
    for (d in a.devices) {
      ActionRow(AccountLogic.deviceName(d), AccountLogic.deviceLine(d), actions = { DangerButton("Sign out", { onRevoke(d) }, outlined = true) })
    }
    ActionRow(
      if (a.devices.isEmpty()) "No signed-in devices" else "Sign in on another device",
      "Sign in on the desktop app to sync. Nothing is synced until you do. This phone is not listed; you sign out of it from the sidebar.", divider = false,
    )
  }
  SectionHeader("Approve a sign-in")
  SettingsCard {
    ActionRow("Approve a sign-in", "Type the code that the desktop app or a terminal shows to let it use your account.", divider = false, actions = { PillButton("Approve a sign-in…", onApprove) })
  }
  SectionHeader("Your data")
  SettingsCard {
    ActionRow("Export my data", "A JSON file with your chats, memory and personalities. API keys are never included.", actions = { SecondaryButton("Export my data", onExport, loading = exporting) })
    ActionRow(
      "Delete my account and all data", "Removes every chat, memory, file, device and your saved API keys from the server. This cannot be undone.", divider = false,
      actions = { DangerButton("Delete my account and all data…", onDelete, outlined = true) },
    )
  }
  SectionHeader("Advanced")
  SettingsCard { ActionRow("Server address", "Only for your own ZO-AI server.", divider = false, actions = { PillButton("Server address…", onAdvanced) }) }
}

/** SET-ACC-04: the account is only deleted when the person types DELETE. [submit] may throw an [ApiException]; it is shown in the sheet. */
@Composable
fun DeleteAccountSheet(onDismiss: () -> Unit, submit: suspend () -> Unit) {
  var typed by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  ZoBottomSheet(
    "Delete my account", onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      DangerButton("Delete everything", {
        busy = true; error = null
        scope.launch { try { submit(); onDismiss() } catch (e: ApiException) { error = e.text } finally { busy = false } }
      }, enabled = AccountLogic.deleteConfirmed(typed) && !busy, loading = busy)
    },
  ) {
    Column {
      Text(
        "This removes all your chats, memory, files, devices and saved API keys from the server, and your place in this service. It cannot be undone. Export first if you want a copy.",
        style = ZoTheme.type.body, color = ZoTheme.colors.text2,
      )
      androidx.compose.foundation.layout.Spacer(Modifier.size(12.dp))
      ZoTextField(typed, { typed = it }, label = "Type DELETE to confirm", modifier = Modifier.padding(bottom = 4.dp))
      error?.let { ErrorLine(it) }
    }
  }
}
