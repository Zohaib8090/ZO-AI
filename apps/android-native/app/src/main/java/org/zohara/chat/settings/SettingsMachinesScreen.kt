package org.zohara.chat.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.ErrorLine
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.PrimaryButton
import org.zohara.chat.core.design.SecondaryButton
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.core.design.ZoBottomSheet
import org.zohara.chat.core.design.ZoIconButton
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.design.ZoTextField
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.Machine

object MachineLogic {
  private val NAME = Regex("^[A-Za-z0-9._-]{1,40}$")

  /** "user@host:port · key saved · note" (SET-MAC-01). */
  fun describe(m: Machine): String =
    (if (m.user.isNotBlank()) m.user + "@" else "") + m.host + ":" + m.port + (if (m.hasKey) " · key saved" else " · no key") + (if (m.note.isNotBlank()) " · " + m.note else "")

  /** SET-MAC-02: "" when fine, else the sentence. The key is required (the server refuses a machine without it). */
  fun validate(name: String, host: String, user: String, port: String, key: String): String = when {
    !NAME.matches(name.trim()) -> "The name can have letters, digits, dots, underscores and dashes."
    host.isBlank() -> "Type the host: an address or a name."
    user.isBlank() -> "Type the user that logs in."
    (port.trim().toIntOrNull() ?: 0) !in 1..65535 -> "The port is a number from 1 to 65535."
    key.isBlank() -> "Paste the private key that this machine accepts."
    else -> ""
  }
}

/** Machines (SSH), administrators only. Parity rows: SET-MAC-01..04, 07. */
@Composable
fun SettingsMachinesScreen() = AdminOnly { MachinesBody() }

@Composable
private fun MachinesBody() {
  val container = LocalAppContainer.current
  val doer = rememberDoer()
  val loader = rememberLoader { container.api.machines() }
  val load by loader.state.collectAsState()
  var add by remember { mutableStateOf(false) }
  var remove by remember { mutableStateOf<Machine?>(null) }
  var testing by remember { mutableStateOf<String?>(null) }
  SettingsPage("Machines") {
    LoadBox(load, { loader.load() }) { machines ->
      MachinesContent(
        machines, testing,
        onTest = { m -> testing = m.name; doer.run(onFail = { testing = null; doer.say(it) }) { doer.say(container.api.machineTest(m.name)); testing = null } },
        onRemove = { remove = it }, onAdd = { add = true },
      )
    }
  }
  if (add) AddMachineSheet(onDismiss = { add = false }) { n, h, u, p, note, k -> container.api.machineAdd(n, h, u, p, note, k); loader.load() }
  remove?.let { m ->
    ConfirmSheet("Remove ${m.name}?", "Zohara Chat forgets this machine and its saved key.", "Remove", { doer.run(onDone = { loader.load() }) { container.api.machineRemove(m.name) } }, { remove = null })
  }
}

@Composable
fun MachinesContent(machines: List<Machine>, testing: String?, onTest: (Machine) -> Unit, onRemove: (Machine) -> Unit, onAdd: () -> Unit) {
  SectionHeader("Machines")
  SettingsCard {
    for (m in machines) {
      ActionRow(
        m.name, MachineLogic.describe(m),
        actions = {
          PillButton(if (testing == m.name) "Testing…" else "Test", { onTest(m) }, enabled = testing != m.name)
          ZoIconButton(ZoIcons.Trash, "Remove this machine", { onRemove(m) }, tint = ZoTheme.colors.danger, size = 36.dp)
        },
      )
    }
    ActionRow(
      "Add a machine", "Zohara Chat connects with SSH to run commands there. The private key is stored on the server and never shown again. Use a key made for Zohara Chat.",
      divider = false, actions = { PrimaryButton("Add machine…", onAdd) },
    )
  }
}

@Composable
fun AddMachineSheet(onDismiss: () -> Unit, submit: suspend (String, String, String, Int, String, String) -> Unit) {
  var name by remember { mutableStateOf("") }
  var host by remember { mutableStateOf("") }
  var user by remember { mutableStateOf("") }
  var port by remember { mutableStateOf("22") }
  var note by remember { mutableStateOf("") }
  var key by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  val clipboard = LocalClipboardManager.current
  ZoBottomSheet(
    "Add a machine", onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Add", {
        val bad = MachineLogic.validate(name, host, user, port, key)
        if (bad.isNotEmpty()) { error = bad } else {
          busy = true; error = null
          scope.launch { try { submit(name.trim(), host.trim(), user.trim(), port.trim().toInt(), note.trim(), key.trim()); onDismiss() } catch (e: ApiException) { error = e.text } finally { busy = false } }
        }
      }, loading = busy)
    },
  ) {
    Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
      ZoTextField(name, { name = it }, label = "Name", placeholder = "home-server", modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(host, { host = it }, label = "Host", placeholder = "192.168.1.20 or example.com", keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri, modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(user, { user = it }, label = "User", modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(port, { port = it.filter(Char::isDigit).take(5) }, label = "Port", keyboardType = androidx.compose.ui.text.input.KeyboardType.Number, modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(note, { note = it }, label = "Note (optional)", modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(key, { key = it }, label = "Private key", placeholder = "-----BEGIN OPENSSH PRIVATE KEY-----", singleLine = false, minLines = 5, mono = true, modifier = Modifier.fillMaxWidth())
      Text("From -----BEGIN to -----END. It must not need a passphrase.", style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(top = 5.dp))
      PillButton("Paste key", { key = clipboard.getText()?.text.orEmpty() }, modifier = Modifier.padding(top = 8.dp))
      error?.let { ErrorLine(it, Modifier.padding(top = 8.dp)) }
    }
  }
}
