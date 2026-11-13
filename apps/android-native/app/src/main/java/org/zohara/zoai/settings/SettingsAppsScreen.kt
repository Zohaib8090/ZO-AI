package org.zohara.zoai.settings

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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.SettingsRow
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoSwitch
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.McpServer

/** SET-03: a restricted account that lands on an administrator's page sees Model and profiles instead (the server refuses the ops too). */
@Composable
fun AdminOnly(content: @Composable () -> Unit) {
  val account = rememberAccount()
  if (account?.restricted != false) SettingsModelScreen() else content()
}

object McpLogic {
  const val READ_ONLY_TEXT = "Tools that only read run on their own; the rest ask first, unless the mode is Bypass."

  /** SET-APP-10: "" when the form can be sent, else the sentence. Only remote (https) servers can be added from a phone. */
  fun validate(name: String, address: String, existing: List<McpServer>): String {
    val n = name.trim(); val a = address.trim()
    if (n.isEmpty() || a.isEmpty()) return "Give it a name and an address or command."
    if (existing.any { it.name.equals(n, ignoreCase = true) }) return "A server with that name is already added."
    if (!a.startsWith("https://", true)) return "Type the https address of the server. A program on a computer can only be added in the desktop app."
    return ""
  }
}

/** Connected apps (MCP servers), administrators only. Parity rows: SET-APP-02..05, 07, 09, 10. */
@Composable
fun SettingsAppsScreen() = AdminOnly { AppsBody() }

@Composable
private fun AppsBody() {
  val container = LocalAppContainer.current
  val doer = rememberDoer()
  val loader = rememberLoader { container.api.mcpList() }
  val load by loader.state.collectAsState()
  var add by remember { mutableStateOf(false) }
  var paste by remember { mutableStateOf(false) }
  var remove by remember { mutableStateOf<McpServer?>(null) }
  SettingsPage("Connected apps") {
    LoadBox(load, { loader.load() }) { servers ->
      AppsContent(
        servers,
        onToggle = { s, on -> doer.run(onDone = { loader.load() }) { container.api.mcpToggle(s.name, on) } },
        onRemove = { remove = it }, onAdd = { add = true }, onPaste = { paste = true },
      )
    }
  }
  if (add) {
    val existing = (load as? Load.Ready)?.value.orEmpty()
    AddMcpSheet(existing, onDismiss = { add = false }) { name, address, token ->
      val msg = container.api.mcpAdd(name, address, token)
      loader.load()
      msg
    }
  }
  if (paste) PasteConfigSheet(onDismiss = { paste = false }) { cfg -> val m = container.api.mcpImport(cfg); loader.load(); m }
  remove?.let { s ->
    ConfirmSheet("Remove \"${s.name}\"?", "ZO-AI stops using this server and forgets its sign-in.", "Remove", { doer.run(onDone = { loader.load() }) { container.api.mcpRemove(s.name) } }, { remove = null })
  }
}

@Composable
fun AppsContent(servers: List<McpServer>, onToggle: (McpServer, Boolean) -> Unit, onRemove: (McpServer) -> Unit, onAdd: () -> Unit, onPaste: () -> Unit) {
  SectionHeader("MCP servers ZO-AI can use")
  SettingsCard {
    if (servers.isEmpty()) {
      ActionRow("No servers yet", "Add MCP servers on the computer; they sync here. You can also add a remote server below.")
    }
    for (s in servers) {
      SettingsRow(
        s.name, subtitle = s.command,
        control = {
          ZoIconButton(ZoIcons.Trash, "Remove this server", { onRemove(s) }, tint = ZoTheme.colors.danger, size = 36.dp)
          ZoSwitch(s.enabled, { onToggle(s, it) })
        },
      )
    }
    ActionRow(
      "Add a server", McpLogic.READ_ONLY_TEXT, divider = false,
      actions = { PrimaryButton("Add server…", onAdd); PillButton("Paste a config…", onPaste) },
    )
  }
}

@Composable
fun AddMcpSheet(existing: List<McpServer>, onDismiss: () -> Unit, submit: suspend (name: String, address: String, token: String?) -> String) {
  var name by remember { mutableStateOf("") }
  var address by remember { mutableStateOf("") }
  var token by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  ZoBottomSheet(
    "Add server", onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Test and add", {
        val bad = McpLogic.validate(name, address, existing)
        if (bad.isNotEmpty()) { error = bad } else {
          busy = true; error = null
          scope.launch { try { submit(name.trim(), address.trim(), token.ifBlank { null }); onDismiss() } catch (e: ApiException) { error = "It did not work: " + e.text } finally { busy = false } }
        }
      }, loading = busy)
    },
  ) {
    Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
      ZoTextField(name, { name = it }, label = "Name", modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(address, { address = it }, label = "Address of the server", placeholder = "https://example.com/mcp", keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri, modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(token, { token = it }, label = "Access token (only if the MCP server needs one)", password = true, modifier = Modifier.fillMaxWidth())
      Text(if (busy) "Connecting…" else McpLogic.READ_ONLY_TEXT, style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(top = 8.dp))
      error?.let { ErrorLine(it, Modifier.padding(top = 8.dp)) }
    }
  }
}

/** SET-APP-05: paste the JSON of a claude_desktop_config or similar; the server tries each server and answers a report. */
@Composable
fun PasteConfigSheet(onDismiss: () -> Unit, submit: suspend (String) -> String) {
  var text by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var report by remember { mutableStateOf<String?>(null) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  ZoBottomSheet(
    "Paste a config", onDismiss,
    footer = {
      SecondaryButton(if (report != null) "Done" else "Cancel", onDismiss)
      PrimaryButton("Add", {
        busy = true; error = null
        scope.launch { try { report = submit(text) } catch (e: ApiException) { error = e.text } finally { busy = false } }
      }, enabled = text.isNotBlank() && report == null, loading = busy)
    },
  ) {
    Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
      ZoTextField(text, { text = it }, label = "JSON of the servers", placeholder = "{\"mcpServers\": {\"name\": {\"url\": \"https://...\"}}}", singleLine = false, minLines = 6, mono = true, modifier = Modifier.fillMaxWidth())
      Text("Remote servers (with a url) are added. A server that starts a program is reported as not supported here.", style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(top = 6.dp))
      report?.let { Text(it, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.text2, modifier = Modifier.padding(top = 10.dp), overflow = TextOverflow.Clip) }
      error?.let { ErrorLine(it, Modifier.padding(top = 8.dp)) }
    }
  }
}
