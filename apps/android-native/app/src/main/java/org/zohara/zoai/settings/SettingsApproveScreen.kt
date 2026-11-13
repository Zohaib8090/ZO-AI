package org.zohara.zoai.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.DangerButton
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.ZoSwitch
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.DeviceLookup
import org.zohara.zoai.core.net.ErrorKind

object ApproveLogic {
  /** "abcd1234" -> "ABCD-1234": letters and digits only, upper case, a dash after four, at most eight. */
  fun formatCode(raw: String): String {
    val c = raw.filter { it.isLetterOrDigit() && it.code < 128 }.uppercase().take(8)
    return if (c.length > 4) c.take(4) + "-" + c.drop(4) else c
  }

  fun codeComplete(formatted: String) = formatted.count { it.isLetterOrDigit() } == 8

  fun kindText(kind: String) = when (kind) {
    "desktop" -> "Desktop app (syncs chats and memory)"
    "cli" -> "Terminal (full access to your account)"
    else -> kind
  }

  /** the person has to tick the box for a terminal and for a request from another network */
  fun strict(i: DeviceLookup) = !i.sameNetwork || i.kind == "cli"

  const val NOT_FOUND = "That code was not found. It may have expired (they last 10 minutes) or been used. Check it and try again, or start again in the app."
}

private sealed interface Step {
  data object Entry : Step
  data class Confirm(val code: String, val info: DeviceLookup) : Step
  data class Done(val approved: Boolean) : Step
}

/** Approve a sign-in of the desktop app or a terminal with a code (SET-ACC-05, AUTH-24, AUTH-25). */
@Composable
fun SettingsApproveScreen() {
  val container = LocalAppContainer.current
  val scope = rememberCoroutineScope()
  var step by remember { mutableStateOf<Step>(Step.Entry) }
  var code by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf("") }
  var sure by remember { mutableStateOf(false) }
  val email = rememberAccount()?.email.orEmpty()
  SettingsPage("Approve a sign-in") {
    when (val st = step) {
      Step.Entry -> Column {
        ZoTextField(
          code, { code = ApproveLogic.formatCode(it); error = "" }, label = "Code shown on your device", placeholder = "ABCD-EFGH",
          hint = "Open ZO-AI on your computer, or run zo login in a terminal. It shows a code like ABCD-EFGH. Type it here.",
          modifier = Modifier.fillMaxWidth(),
        )
        if (error.isNotBlank()) ErrorLine(error, Modifier.padding(top = 10.dp))
        Spacer(Modifier.height(10.dp))
        PrimaryButton("Look up", {
          busy = true; error = ""
          scope.launch {
            try { val i = container.api.deviceLookup(code); sure = false; step = Step.Confirm(code, i) }
            catch (e: ApiException) { error = if (e.status == 404) ApproveLogic.NOT_FOUND else e.text }
            finally { busy = false }
          }
        }, enabled = ApproveLogic.codeComplete(code), loading = busy)
      }
      is Step.Confirm -> ConfirmView(st.info, email.ifBlank { st.info.email }, sure, { sure = it }, busy, error,
        onDecide = { ok ->
          busy = true; error = ""
          scope.launch {
            try { container.api.deviceApprove(st.code, ok); step = Step.Done(ok) }
            catch (e: ApiException) { error = e.text }
            finally { busy = false }
          }
        },
        onAgain = { code = ""; error = ""; step = Step.Entry })
      is Step.Done -> Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth().padding(top = 16.dp)) {
        Text(if (st.approved) "Approved" else "Denied", style = ZoTheme.type.sheetTitle, color = ZoTheme.colors.text)
        Text(
          if (st.approved) "You can go back to the app. It signs in within a few seconds." else "Nothing was signed in.",
          style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = Modifier.padding(top = 6.dp),
        )
        Spacer(Modifier.height(14.dp))
        SecondaryButton("Approve another", { code = ""; step = Step.Entry })
      }
    }
  }
}

@Composable
fun ConfirmView(info: DeviceLookup, account: String, sure: Boolean, onSure: (Boolean) -> Unit, busy: Boolean, error: String, onDecide: (Boolean) -> Unit, onAgain: () -> Unit) {
  val c = ZoTheme.colors
  val strict = ApproveLogic.strict(info)
  Column {
    Text("This device is asking to sign in to your account", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(bottom = 8.dp))
    SettingsCard {
      ActionRow("Your account", account.ifBlank { "unknown" })
      ActionRow("Name", info.name)
      ActionRow("Type", ApproveLogic.kindText(info.kind))
      ActionRow("From address", info.ip.ifBlank { "unknown" })
      ActionRow("Asked at", if (info.requestedAt > 0) formatWhen(toMillis(info.requestedAt)) else "unknown", divider = false)
    }
    if (!info.sameNetwork) Warn("This request came from a different network than the one you are using now.", " If you started it on your own computer on another network, that can be fine. If you did not, press Deny.")
    Warn("Only approve if you started this on your own device,", " and this is the account you mean ($account). The name above is chosen by whoever asked, so it proves nothing. If someone else sent you here, press Deny.")
    if (strict) {
      Row(Modifier.padding(vertical = 10.dp).clickable(role = Role.Checkbox) { onSure(!sure) }, verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        ZoSwitch(sure, null)
        Text(
          "I started this myself on my own computer" + (if (info.kind == "cli") ", and I understand a terminal gets full access to this account" else "") + ".",
          style = ZoTheme.type.body, color = c.text2, modifier = Modifier.weight(1f),
        )
      }
    }
    if (error.isNotBlank()) ErrorLine(error, Modifier.padding(bottom = 8.dp))
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.padding(top = 6.dp)) {
      DangerButton("Deny", { onDecide(false) }, enabled = !busy, outlined = true)
      PrimaryButton("Approve", { onDecide(true) }, enabled = !busy && (!strict || sure))
    }
    Spacer(Modifier.size(10.dp))
    Text("Type a different code", style = ZoTheme.type.button, color = c.accent, modifier = Modifier.clickable(role = Role.Button, onClick = onAgain).padding(vertical = 8.dp))
  }
}

@Composable
private fun Warn(bold: String, rest: String) {
  val c = ZoTheme.colors
  Text(
    androidx.compose.ui.text.buildAnnotatedString {
      pushStyle(androidx.compose.ui.text.SpanStyle(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold)); append(bold); pop(); append(rest)
    },
    style = ZoTheme.type.cardMeta, color = c.text2,
    modifier = Modifier.padding(top = 10.dp).fillMaxWidth().background(c.warn.copy(alpha = 0.14f), RoundedCornerShape(10.dp)).padding(12.dp),
  )
}
