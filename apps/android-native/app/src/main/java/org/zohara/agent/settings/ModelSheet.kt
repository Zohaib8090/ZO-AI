package org.zohara.agent.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.ErrorLine
import org.zohara.agent.core.design.PillButton
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoChip
import org.zohara.agent.core.design.ZoIcon
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoSpinner
import org.zohara.agent.core.design.ZoTextField
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.ProviderInfo

/** What the sheet can do; the screen wires it to a [ModelSheetController], a screenshot test passes empty lambdas. */
class ModelSheetActions(
  val onProvider: (String) -> Unit = {},
  val onAddress: (String) -> Unit = {},
  val onAddressDone: () -> Unit = {},
  val onFilter: (String) -> Unit = {},
  val onChoose: (String) -> Unit = {},
  val onFetch: () -> Unit = {},
  val onTest: () -> Unit = {},
  val onSaveKey: (String) -> Unit = {},
  val onTogglePin: (String) -> Unit = {},
  val onEffort: (String) -> Unit = {},
  val onAddEndpoint: () -> Unit = {},
)

/**
 * The full model sheet as a bottom sheet (MODEL-04, 09..19, 23). Opened from "More models…" of the model menu and from Settings > Model and
 * profiles > Change. [onUsed] runs after "Use this model" worked.
 */
@Composable
fun ModelSheet(onDismiss: () -> Unit, onUsed: () -> Unit = {}) {
  val container = LocalAppContainer.current
  val account = rememberAccount()
  val restricted = account?.restricted ?: true
  val scope = rememberCoroutineScope()
  val ctl = remember {
    ModelSheetController(
      api = container.api, scope = scope, restricted = restricted, admin = account?.admin ?: false, cache = container.modelsCache,
      hiddenStore = HiddenStore(container.api, LocalSettings(container.app.getSharedPreferences("zo_settings_local", 0))),
      toast = { t -> scope.launch { container.toast.show(t) } },
      invalidate = { container.providersCache.clear(); container.profilesCache.clear(); container.modelsCache.clear() },
    )
  }
  DisposableEffect(ctl) { ctl.start(); onDispose { } }
  val s by ctl.state.collectAsState()
  var addEndpoint by remember { mutableStateOf(false) }
  val actions = remember(ctl) {
    ModelSheetActions(
      onProvider = ctl::selectProvider, onAddress = ctl::setAddress, onAddressDone = ctl::addressDone, onFilter = ctl::setFilter,
      onChoose = ctl::choose, onFetch = { ctl.fetchModels(force = true) }, onTest = ctl::testConnection, onSaveKey = { k -> ctl.saveKey(k) },
      onTogglePin = ctl::togglePin, onEffort = ctl::setEffort, onAddEndpoint = { addEndpoint = true },
    )
  }
  ZoBottomSheet(
    title = "Model and API key", onDismiss = onDismiss,
    footer = {
      SecondaryButton("Close", onDismiss)
      PrimaryButton("Use this model", { ctl.use { onUsed(); onDismiss() } }, enabled = s.canUse, loading = s.using)
    },
  ) { ModelSheetBody(s, restricted, actions) }
  if (addEndpoint) AddEndpointSheet(restricted = restricted, onDismiss = { addEndpoint = false }, onAdded = { addEndpoint = false; ctl.start() })
}

/** The content of the sheet (stateless: screenshot tested). */
@Composable
fun ModelSheetBody(s: ModelSheetState, restricted: Boolean, a: ModelSheetActions) {
  val c = ZoTheme.colors
  Column(Modifier.heightIn(max = 600.dp).verticalScroll(rememberScrollState())) {
    if (s.loading) {
      Text("Loading…", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(vertical = 16.dp))
      return@Column
    }
    if (s.loadError != null) { ErrorLine(s.loadError, Modifier.padding(vertical = 12.dp)); return@Column }

    Text("Provider", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(bottom = 6.dp))
    ProviderPicker(s, a.onProvider)
    s.claudeLine?.let { Text(it, style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 6.dp)) }
    s.hiddenHint?.let { Text(it, style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 6.dp)) }

    val p = s.provider
    if (p != null) {
      AddressSection(s, p, restricted, a)
      if (!ModelLogic.isLocal(p)) KeySection(s, p, restricted, a)
      ModelListSection(s, a)
      if (s.efforts.isNotEmpty()) EffortPicker(s, a.onEffort)
    }
    Row(Modifier.padding(top = 16.dp)) { PillButton("Add a custom endpoint…", a.onAddEndpoint) }
  }
}

@Composable
private fun ProviderPicker(s: ModelSheetState, onSelect: (String) -> Unit) {
  val c = ZoTheme.colors
  var open by remember { mutableStateOf(false) }
  Box {
    Row(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.field).background(c.input).border(1.dp, c.border, ZoTheme.shapes.field)
        .clickable(role = Role.DropdownList) { open = true }.padding(horizontal = 12.dp, vertical = 12.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      val cur = s.provider
      Text(cur?.let { ModelLogic.providerLabel(it) } ?: "Choose a provider…", style = ZoTheme.type.body.copy(fontSize = androidx.compose.ui.unit.TextUnit(14f, androidx.compose.ui.unit.TextUnitType.Sp)), color = if (cur == null) c.dimmer else c.text, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
      ZoIcon(ZoIcons.Down, tint = c.dim, size = 18.dp)
    }
    DropdownMenu(open, { open = false }, modifier = Modifier.background(c.popover).heightIn(max = 420.dp)) {
      for (g in ModelLogic.groups(s.providers)) {
        Text(g.label, style = ZoTheme.type.sectionHeader, color = c.dimmer, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        for (p in g.items) {
          DropdownMenuItem(
            text = { Text(ModelLogic.providerLabel(p), style = ZoTheme.type.body, color = c.text) },
            trailingIcon = if (p.id == s.providerId) ({ ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp) }) else null,
            onClick = { open = false; onSelect(p.id) },
          )
        }
      }
    }
  }
}

@Composable
private fun AddressSection(s: ModelSheetState, p: ProviderInfo, restricted: Boolean, a: ModelSheetActions) {
  val c = ZoTheme.colors
  var advanced by remember(p.id) { mutableStateOf(false) }
  val needs = p.needsUrl
  Column(Modifier.padding(top = 14.dp)) {
    if (!needs) {
      Row(
        Modifier.clickable(role = Role.Button) { advanced = !advanced }.padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
      ) {
        Text("Advanced: address", style = ZoTheme.type.fieldLabel, color = c.text2)
        Spacer(Modifier.width(6.dp))
        ZoIcon(if (advanced) ZoIcons.Down else ZoIcons.Chevron, tint = c.dim, size = 16.dp)
      }
    }
    if (needs || advanced) {
      ZoTextField(
        s.address, a.onAddress, label = if (needs) "Address (endpoint URL)" else null, placeholder = p.defaultUrl.ifBlank { "https://" },
        hint = s.addressError.ifBlank { ModelLogic.addressHint(p, restricted) }, isError = s.addressError.isNotBlank(),
        keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri, modifier = Modifier.fillMaxWidth(),
      )
      Row(Modifier.padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        SecondaryButton("Test connection", a.onTest, enabled = !s.testing, loading = s.testing)
        Spacer(Modifier.width(10.dp))
        Text(
          s.testText, style = ZoTheme.type.fieldHint,
          color = when (s.testOk) { true -> c.ok; false -> c.danger; null -> c.dimmer }, modifier = Modifier.weight(1f),
        )
      }
    }
  }
}

@Composable
private fun KeySection(s: ModelSheetState, p: ProviderInfo, restricted: Boolean, a: ModelSheetActions) {
  val c = ZoTheme.colors
  var key by remember(p.id) { mutableStateOf("") }
  Column(Modifier.padding(top = 14.dp)) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(bottom = 6.dp)) {
      Text("API key", style = ZoTheme.type.fieldLabel, color = c.text)
      Spacer(Modifier.width(8.dp))
      val saved = p.hasKey
      ZoChip(if (saved) "Key saved" else "No key yet", color = if (saved) c.okChip else c.warn)
    }
    Row(verticalAlignment = Alignment.Top) {
      ZoTextField(key, { key = it }, placeholder = "Paste the key (a saved one is used when this stays empty)", password = true, modifier = Modifier.weight(1f))
      Spacer(Modifier.width(8.dp))
      PrimaryButton("Save", { a.onSaveKey(key); key = "" }, enabled = key.isNotBlank() && !s.savingKey, loading = s.savingKey)
    }
    Text(
      if (restricted) "Bring your own key: it is stored encrypted on our server and only used to call your provider for you."
      else "Keys stay on the server, stored encrypted. Empty keeps the saved one.",
      style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 5.dp),
    )
  }
}

@Composable
private fun ModelListSection(s: ModelSheetState, a: ModelSheetActions) {
  val c = ZoTheme.colors
  Column(Modifier.padding(top = 14.dp)) {
    Row(verticalAlignment = Alignment.CenterVertically) {
      Text("Model", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.weight(1f))
      ZoIconButton(ZoIcons.Refresh, "Load the list again", a.onFetch, size = 32.dp, iconSize = 18.dp, enabled = !s.fetching)
    }
    Spacer(Modifier.size(6.dp))
    ZoTextField(
      s.filter, a.onFilter, placeholder = if (s.models.isEmpty() && !s.fetching) "Model name, or pick one below" else "Search models",
      modifier = Modifier.fillMaxWidth(),
    )
    val pins = s.pins
    // MODEL-08: Claude Code lists the newest model of each family and keeps the older ones under "More models" (a search shows all)
    val grouped = s.providerId == "claude-code" && s.filter.isBlank()
    val split = if (grouped) ModelLogic.splitMain(s.models) else s.models to emptyList()
    val items = ModelLogic.orderModels(split.first, pins, s.filter)
    val older = split.second
    var showOlder by remember(s.providerId) { mutableStateOf(false) }
    val custom = ModelLogic.customOffer(s.models, s.filter)
    Spacer(Modifier.size(8.dp))
    if (s.fetching) {
      Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 10.dp)) {
        ZoSpinner(size = 16.dp)
        Spacer(Modifier.width(8.dp))
        Text("Fetching the models list…", style = ZoTheme.type.fieldHint, color = c.dimmer)
      }
    } else {
      LazyColumn(
        Modifier.fillMaxWidth().heightIn(max = 260.dp).clip(ZoTheme.shapes.field).border(1.dp, c.line, ZoTheme.shapes.field),
      ) {
        items(items, key = { it }) { m -> ModelRow(m, m == s.chosen, m in pins, a) }
        if (older.isNotEmpty()) {
          item(key = "more-models") {
            Row(
              Modifier.fillMaxWidth().clickable(role = Role.Button) { showOlder = !showOlder }.padding(horizontal = 14.dp, vertical = 12.dp),
              verticalAlignment = Alignment.CenterVertically,
            ) {
              Text("More models", style = ZoTheme.type.body, color = c.text2, modifier = Modifier.weight(1f))
              ZoIcon(if (showOlder) ZoIcons.Down else ZoIcons.Chevron, tint = c.dim, size = 16.dp)
            }
          }
          if (showOlder) items(older, key = { "older:$it" }) { m -> ModelRow(m, m == s.chosen, m in pins, a) }
        }
        if (custom.isNotEmpty()) item(key = "custom:$custom") { CustomRow(custom, custom == s.chosen, a.onChoose) }
        if (items.isEmpty() && custom.isEmpty()) item(key = "none") {
          Text(
            if (s.models.isNotEmpty()) "No model matches your search." else "No models yet.",
            style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(14.dp),
          )
        }
      }
      if (s.fetchError.isNotBlank()) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
          Text(s.fetchError, style = ZoTheme.type.fieldHint, color = c.danger, modifier = Modifier.weight(1f))
          Spacer(Modifier.width(8.dp))
          PillButton("Retry", a.onFetch)
        }
      } else if (s.models.isNotEmpty()) {
        Text("${s.models.size} models. Tap the star to pin one to the model menu.", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 6.dp))
      }
    }
  }
}

@Composable
private fun ModelRow(name: String, chosen: Boolean, pinned: Boolean, a: ModelSheetActions) {
  val c = ZoTheme.colors
  Row(
    Modifier.fillMaxWidth().clickable(role = Role.RadioButton) { a.onChoose(name) }.semantics { selected = chosen }
      .background(if (chosen) c.active else androidx.compose.ui.graphics.Color.Transparent).padding(start = 14.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Text(name, style = ZoTheme.type.body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f).padding(vertical = 10.dp))
    if (chosen) ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp)
    ZoIconButton(
      ZoIcons.Star, if (pinned) "Unpin from the model menu" else "Pin to the model menu", { a.onTogglePin(name) },
      tint = if (pinned) c.gold else c.dimmer, size = 40.dp, iconSize = 18.dp,
    )
  }
}

@Composable
private fun CustomRow(text: String, chosen: Boolean, onChoose: (String) -> Unit) {
  val c = ZoTheme.colors
  Row(Modifier.fillMaxWidth().clickable(role = Role.RadioButton) { onChoose(text) }.background(if (chosen) c.active else androidx.compose.ui.graphics.Color.Transparent).padding(horizontal = 14.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
    Text("Use “$text” as the model name", style = ZoTheme.type.body, color = c.text, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
    if (chosen) ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp)
  }
}

@Composable
private fun EffortPicker(s: ModelSheetState, onEffort: (String) -> Unit) {
  val c = ZoTheme.colors
  var open by remember { mutableStateOf(false) }
  Column(Modifier.padding(top = 14.dp)) {
    Text("Effort", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(bottom = 6.dp))
    Box {
      Row(
        Modifier.fillMaxWidth().clip(ZoTheme.shapes.field).background(c.input).border(1.dp, c.border, ZoTheme.shapes.field)
          .clickable(role = Role.DropdownList) { open = true }.padding(horizontal = 12.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
      ) {
        Text(ModelLogic.effortLabel(s.effort), style = ZoTheme.type.body, color = c.text, modifier = Modifier.weight(1f))
        ZoIcon(ZoIcons.Down, tint = c.dim, size = 18.dp)
      }
      DropdownMenu(open, { open = false }, modifier = Modifier.background(c.popover)) {
        for (lvl in listOf("") + s.efforts) {
          DropdownMenuItem(
            text = { Text(ModelLogic.effortLabel(lvl), style = ZoTheme.type.body, color = c.text) },
            trailingIcon = if (lvl == s.effort) ({ ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp) }) else null,
            onClick = { open = false; onEffort(lvl) },
          )
        }
      }
    }
    Text("How hard it thinks before answering", style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(top = 5.dp))
  }
}

// ================================================================================================================ add a custom endpoint (MODEL-25)

@Composable
fun AddEndpointSheet(restricted: Boolean, onDismiss: () -> Unit, onAdded: () -> Unit) {
  val container = LocalAppContainer.current
  val scope = rememberCoroutineScope()
  val ctl = remember {
    AddEndpointController(
      container.api, scope, restricted, toast = { t -> scope.launch { container.toast.show(t) } },
      invalidate = { container.providersCache.clear(); container.profilesCache.clear(); container.modelsCache.clear() },
    )
  }
  val st by ctl.state.collectAsState()
  var name by remember { mutableStateOf("") }
  var address by remember { mutableStateOf("") }
  var key by remember { mutableStateOf("") }
  var model by remember { mutableStateOf("") }
  ZoBottomSheet(
    title = "Add a custom endpoint", onDismiss = onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Add and use", { ctl.add(name, address, key, model, onAdded) }, loading = st.busy)
    },
  ) {
    EndpointForm(
      name, { name = it }, address, { address = it }, key, { key = it }, model, { model = it },
      st.error, st.testText, st.testOk, st.testing, onTest = { ctl.test(address, key, model) },
    )
  }
}

@Composable
fun EndpointForm(
  name: String, onName: (String) -> Unit, address: String, onAddress: (String) -> Unit, key: String, onKey: (String) -> Unit,
  model: String, onModel: (String) -> Unit, error: String, testText: String, testOk: Boolean?, testing: Boolean, onTest: () -> Unit,
) {
  val c = ZoTheme.colors
  Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    Text("Any server that speaks the OpenAI chat API: vLLM, LM Studio, a company gateway, a proxy.", style = ZoTheme.type.fieldHint, color = c.dimmer)
    ZoTextField(name, onName, label = "Name", placeholder = "Work server, Home GPU…", modifier = Modifier.fillMaxWidth())
    ZoTextField(address, onAddress, label = "Address", placeholder = "https://my-host.example/v1", keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri, modifier = Modifier.fillMaxWidth())
    ZoTextField(key, onKey, label = "Key", password = true, placeholder = "Only if the server needs one", modifier = Modifier.fillMaxWidth())
    ZoTextField(model, onModel, label = "Model", placeholder = "Model name (Test connection can list them)", modifier = Modifier.fillMaxWidth())
    Row(verticalAlignment = Alignment.CenterVertically) {
      SecondaryButton("Test connection", onTest, enabled = !testing, loading = testing)
      Spacer(Modifier.width(10.dp))
      Text(testText, style = ZoTheme.type.fieldHint, color = when (testOk) { true -> c.ok; false -> c.danger; null -> c.dimmer }, modifier = Modifier.weight(1f))
    }
    if (error.isNotBlank()) ErrorLine(error)
  }
}
