package org.zohara.zoai.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.SettingsRow
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoSwitch
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ClaudeStatus
import org.zohara.zoai.core.net.Profile
import org.zohara.zoai.core.net.ProfilesResp
import org.zohara.zoai.core.net.ProvidersResp
import org.zohara.zoai.core.net.StatusInfo
import org.zohara.zoai.core.net.ZoApi
import org.zohara.zoai.nav.LocalNav
import org.zohara.zoai.nav.Routes

/** Everything the Model page shows (SET-MOD). */
data class ModelPageData(
  val status: StatusInfo,
  val profiles: ProfilesResp,
  val providers: ProvidersResp,
  val hidden: List<String>,
  /** false: this server has no `settings_set`, the choice stays on this phone */
  val hiddenOnServer: Boolean,
  /** only for administrators */
  val claude: ClaudeStatus?,
)

suspend fun loadModelPage(api: ZoApi, hiddenStore: HiddenStore, restricted: Boolean): ModelPageData = coroutineScope {
  val st = async { api.status() }
  val pf = async { api.profiles() }
  val pv = async { api.providers() }
  val hd = async { hiddenStore.get() }
  val cc = async { if (restricted) null else try { api.claudeStatus().takeIf { it.admin } } catch (_: ApiException) { null } }
  val prov = pv.await()
  ModelPageData(st.await(), pf.await(), prov, (hd.await() + prov.hiddenProviders).distinct(), hiddenStore.onServer, cc.await())
}

/** Model and profiles. Parity rows: SET-MOD, MODEL-26..30, 36, 37, 18, 25. */
@Composable
fun SettingsModelScreen() {
  val container = LocalAppContainer.current
  val account = rememberAccount()
  val restricted = account?.restricted ?: true
  val doer = rememberDoer()
  val nav = LocalNav.current
  val store = remember { HiddenStore(container.api, LocalSettings(container.app.getSharedPreferences("zo_settings_local", 0))) }
  val loader = rememberLoader(restricted) { loadModelPage(container.api, store, restricted) }
  val load by loader.state.collectAsState()
  var sheet by remember { mutableStateOf(false) }
  var addEndpoint by remember { mutableStateOf(false) }
  var rename by remember { mutableStateOf<Profile?>(null) }
  var delete by remember { mutableStateOf<Profile?>(null) }
  var newProfile by remember { mutableStateOf(false) }
  val invalidate = { container.providersCache.clear(); container.profilesCache.clear(); container.modelsCache.clear() }
  val refresh = { invalidate(); loader.load() }

  SettingsPage("Model and profiles") {
    LoadBox(load, onRetry = { loader.load() }) { d ->
      var hidden by remember(d) { mutableStateOf(d.hidden.toSet()) }
      ModelPageContent(
        d, restricted, hidden,
        ModelPageActions(
          onChange = { sheet = true },
          onUse = { p -> doer.run(onDone = refresh) { container.api.profileUse(p.id) } },
          onRename = { rename = it },
          onDelete = { delete = it },
          onNew = { newProfile = true },
          onAddEndpoint = { addEndpoint = true },
          onUnpin = { p, m -> doer.run(onDone = refresh) { container.api.profilePin(p, m, false) } },
          onToggleProvider = { id, shown ->
            val next = if (shown) hidden - id else hidden + id
            val before = hidden
            hidden = next
            doer.run(onFail = { hidden = before; doer.say(it) }, onDone = { invalidate() }) { store.set(next.toList()) }
            val name = d.providers.providers.firstOrNull { it.id == id }?.name ?: id
            doer.say(if (shown) "$name shown" else "$name hidden")
          },
          onOpenSync = { nav.settings(Routes.SETTINGS_SYNC) },
        ),
      )
    }
  }
  if (sheet) ModelSheet(onDismiss = { sheet = false }, onUsed = refresh)
  if (addEndpoint) AddEndpointSheet(restricted, onDismiss = { addEndpoint = false }, onAdded = { addEndpoint = false; refresh() })
  rename?.let { p ->
    InputSheet("Rename profile", "Save", { rename = null }, label = "Name", placeholder = "Work, Home, Free models…", initial = p.name, maxLength = 40) { name ->
      container.api.profileRename(p.id, name); refresh()
    }
  }
  if (newProfile) {
    InputSheet("New profile", "Create", { newProfile = false }, label = "Name", placeholder = "Work, Home, Free models…", hint = "It starts from the provider and model in use now.", maxLength = 40) { name ->
      container.api.profileCreate(name); refresh()
    }
  }
  delete?.let { p ->
    ConfirmSheet("Delete the profile \"${p.name}\"?", "Its saved choices and the API key it kept are removed from the server.", "Delete", { doer.run(onDone = refresh) { container.api.profileDelete(p.id) } }, { delete = null })
  }
}

class ModelPageActions(
  val onChange: () -> Unit = {},
  val onUse: (Profile) -> Unit = {},
  val onRename: (Profile) -> Unit = {},
  val onDelete: (Profile) -> Unit = {},
  val onNew: () -> Unit = {},
  val onAddEndpoint: () -> Unit = {},
  val onUnpin: (profileId: String, model: String) -> Unit = { _, _ -> },
  val onToggleProvider: (id: String, shown: Boolean) -> Unit = { _, _ -> },
  val onOpenSync: () -> Unit = {},
)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ModelPageContent(d: ModelPageData, restricted: Boolean, hidden: Set<String>, a: ModelPageActions) {
  val c = ZoTheme.colors
  val provName = { id: String -> d.providers.providers.firstOrNull { it.id == id }?.name?.ifBlank { id } ?: id }

  SectionHeader("Model")
  SettingsCard {
    val active = d.profiles.profiles.firstOrNull { it.id == d.profiles.active }
    val line = if (d.status.ready || d.status.provider.isNotBlank()) {
      provName(d.status.provider) + " · " + d.status.model.ifBlank { "no model chosen" } + (active?.let { " · profile ${it.name}" } ?: "")
    } else "Nothing chosen yet"
    SettingsRow("Provider and model", subtitle = line, control = { PrimaryButton("Change", a.onChange) })
    SettingsRow("API keys", subtitle = "Keys stay on the server, stored encrypted.", divider = false)
  }

  SectionHeader("Profiles")
  SettingsCard {
    if (d.profiles.profiles.isEmpty()) {
      ActionRow(
        "No profiles yet",
        "A profile is a preset: a provider, a model, its address, how hard it thinks and the models you pinned. Switch between them in one tap from the model button.",
      )
    }
    for (p in d.profiles.profiles) {
      val on = d.profiles.active == p.id
      ActionRow(
        p.name, ModelLogic.profileLine(p, provName(p.provider)),
        extra = {
          ModelLogic.visionLine(p)?.let { Text(it, style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 4.dp)) }
          if (p.pinned.isNotEmpty()) {
            FlowRow(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
              p.pinned.forEach { m -> PinChip(m) { a.onUnpin(p.id, m) } }
            }
          }
        },
        actions = {
          PillButton(if (on) "In use" else "Use", { a.onUse(p) }, enabled = !on, accent = !on)
          ZoIconButton(ZoIcons.Edit, "Rename this profile", { a.onRename(p) }, size = 36.dp)
          ZoIconButton(ZoIcons.Trash, "Delete this profile and its key", { a.onDelete(p) }, tint = c.danger, size = 36.dp)
        },
      )
    }
    ActionRow(
      if (d.profiles.profiles.isEmpty()) "" else "Add another", if (d.profiles.profiles.isEmpty()) null else "Save the current settings as a profile.", divider = false,
      actions = {
        PrimaryButton("New profile…", a.onNew)
        PillButton("Add a custom endpoint…", a.onAddEndpoint)
      },
    )
  }
  Note("Profiles, pinned models and MCP addresses sync from your desktop when you are signed in there.")
  Row(Modifier.padding(start = 6.dp)) { PillButton("Open Desktop sync", a.onOpenSync) }

  SectionHeader("Pinned models")
  SettingsCard {
    val rows = ModelLogic.pinRows(d.profiles, provName)
    if (rows.isEmpty()) {
      ActionRow("Nothing pinned yet", "Open Change, find a model and tap the star next to it. Pinned models show in the model menu, one tap away.", divider = false)
    }
    rows.forEachIndexed { i, r ->
      SettingsRow(
        r.model, subtitle = "${r.profileName} · ${r.provider}", divider = i != rows.lastIndex,
        control = { PillButton("Unpin", { a.onUnpin(r.profileId, r.model) }) },
      )
    }
  }

  SectionHeader("Providers in the model picker")
  SettingsCard {
    val shown = ModelLogic.shownProviders(d.providers.providers, emptyList(), null, restricted)
    shown.forEachIndexed { i, p ->
      val inUse = p.id == d.providers.current
      val pins = d.profiles.profiles.filter { it.provider == p.id }.sumOf { it.pinned.size }
      SettingsRow(
        p.name.ifBlank { p.id }, subtitle = ModelLogic.providerSub(p, pins, inUse), divider = i != shown.lastIndex,
        control = {
          Row(Modifier.semantics { contentDescription = "Show in the model picker: ${p.name}" }) {
            ZoSwitch(checked = p.id !in hidden, onCheckedChange = { a.onToggleProvider(p.id, it) }, enabled = !inUse)
          }
        },
      )
    }
  }
  Note(
    "Switch off the providers you never use so the pickers stay short. The one in use always shows. " +
      if (d.hiddenOnServer) "This choice follows you to your other devices when sync is on." else "On this server the choice is kept on this phone only.",
  )

  d.claude?.let { cc ->
    SectionHeader("Claude Code (your subscription)")
    ClaudeCodeCard(cc)
  }
}

/** LINK-CC: the Claude Code card for administrators. */
@Composable
fun ClaudeCodeCard(cc: ClaudeStatus) {
  SettingsCard {
    SettingsRow(if (cc.present) "Ready on the server" else "Not set up", subtitle = claudeLine(cc.present, cc.setAt), divider = false)
  }
}

@Composable
private fun PinChip(model: String, onUnpin: () -> Unit) {
  val c = ZoTheme.colors
  Row(
    Modifier.clip(ZoTheme.shapes.pill).border(1.dp, c.line12, ZoTheme.shapes.pill).padding(start = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    ZoIcon(ZoIcons.Star, tint = c.gold, size = 14.dp)
    Spacer(Modifier.width(4.dp))
    Text(model, style = ZoTheme.type.fieldHint, color = c.text2)
    Box1(onUnpin, "Remove $model from the model menu")
  }
}

@Composable
private fun Box1(onClick: () -> Unit, description: String) {
  androidx.compose.foundation.layout.Box(
    Modifier.size(32.dp).clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = description },
    contentAlignment = Alignment.Center,
  ) { ZoIcon(ZoIcons.Close, tint = ZoTheme.colors.dim, size = 14.dp) }
}
