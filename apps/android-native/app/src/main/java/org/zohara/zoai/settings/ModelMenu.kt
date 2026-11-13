package org.zohara.zoai.settings

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.net.ZoApi
import org.zohara.zoai.core.store.TtlCache
import org.zohara.zoai.nav.LocalNav
import org.zohara.zoai.nav.Routes

data class ModelMenuState(
  val loading: Boolean = true,
  val error: String? = null,
  val groups: List<ModelLogic.MenuGroup> = emptyList(),
  val activeProfile: String = "",
  val currentModel: String = "",
  val currentProviderName: String = "",
  val hasProvider: Boolean = false,
  val efforts: List<String> = emptyList(),
  val effort: String = "",
) {
  val showInUseNow: Boolean get() = hasProvider && ModelLogic.needsInUseNow(groups, activeProfile, currentModel)
}

/** The logic of the model menu (MODEL-01..07, 34): pinned models grouped by profile; one tap switches profile and model. */
class ModelMenuController(
  private val api: ZoApi,
  private val scope: CoroutineScope,
  private val modelsCache: TtlCache<String, ModelsResp>,
  private val toast: (String) -> Unit,
  private val invalidate: () -> Unit,
) {
  private val _state = MutableStateFlow(ModelMenuState())
  val state: StateFlow<ModelMenuState> = _state

  fun start() {
    scope.launch {
      try {
        coroutineScope {
          val st = async { api.status() }
          val pf = async { api.profiles() }
          val pv = async { api.providers() }
          val status = st.await(); val prof = pf.await(); val prov = pv.await()
          val name = { id: String -> prov.providers.firstOrNull { it.id == id }?.name?.ifBlank { id } ?: id }
          val cached = status.provider.takeIf { it.isNotBlank() }?.let { modelsCache.get(ModelLogic.cacheKey(it, "")) }
          val active = prof.profiles.firstOrNull { it.id == prof.active }
          _state.value = ModelMenuState(
            loading = false, groups = ModelLogic.menuGroups(prof, name), activeProfile = prof.active, currentModel = status.model,
            currentProviderName = name(status.provider), hasProvider = status.ready || status.provider.isNotBlank(),
            efforts = cached?.efforts.orEmpty(), effort = active?.effort ?: cached?.effort.orEmpty(),
          )
        }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(loading = false, error = e.text) }
      }
    }
  }

  /** One tap: `profile_use`, then `select {model}` (web2 `modelPopover`). */
  fun pick(profileId: String, model: String, onDone: () -> Unit) {
    scope.launch {
      try {
        api.profileUse(profileId)
        api.select(model = model)
        invalidate()
        toast("Using $model")
        onDone()
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        toast(e.text)
      }
    }
  }

  fun setEffort(level: String) {
    scope.launch {
      try {
        api.select(effort = level)
        invalidate()
        _state.update { it.copy(effort = level) }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        toast(e.text)
      }
    }
  }
}

/**
 * The model menu of the model pill, as a bottom sheet: pinned models grouped by profile, "More models…" opens the full sheet.
 * Reusable: the chat screen calls `ModelMenuSheet(onDismiss, onMore = { showModelSheet = true }, onChanged = { refresh the pill })`, or just
 * [ModelPickerHost], which also hosts the full sheet.
 */
@Composable
fun ModelMenuSheet(onDismiss: () -> Unit, onMore: () -> Unit, onChanged: () -> Unit = {}) {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val scope = rememberCoroutineScope()
  val ctl = remember {
    ModelMenuController(
      container.api, scope, container.modelsCache, toast = { t -> scope.launch { container.toast.show(t) } },
      invalidate = { container.providersCache.clear(); container.profilesCache.clear(); container.modelsCache.clear() },
    )
  }
  DisposableEffect(ctl) { ctl.start(); onDispose { } }
  val s by ctl.state.collectAsState()
  ZoBottomSheet(title = if (s.loading || s.hasProvider) "Pinned models" else "Choose a provider to begin", onDismiss = onDismiss) {
    ModelMenuBody(
      s,
      onPick = { id, m -> ctl.pick(id, m) { onChanged(); onDismiss() } },
      onMore = { onDismiss(); onMore() },
      onOpenSettings = { onDismiss(); nav.settings(Routes.SETTINGS_MODEL) },
      onEffort = ctl::setEffort,
    )
  }
}

/** The pill's menu and the full sheet together; [visible] is the state of the caller. */
@Composable
fun ModelPickerHost(visible: Boolean, onDismiss: () -> Unit, onChanged: () -> Unit = {}) {
  var full by remember { mutableStateOf(false) }
  if (visible && !full) ModelMenuSheet(onDismiss = onDismiss, onMore = { full = true }, onChanged = onChanged)
  if (full) ModelSheet(onDismiss = { full = false; onDismiss() }, onUsed = onChanged)
}

@Composable
fun ModelMenuBody(
  s: ModelMenuState,
  onPick: (profileId: String, model: String) -> Unit,
  onMore: () -> Unit,
  onOpenSettings: () -> Unit,
  onEffort: (String) -> Unit,
) {
  val c = ZoTheme.colors
  Column(Modifier.heightIn(max = 560.dp).verticalScroll(rememberScrollState())) {
    when {
      s.loading -> Text("Loading…", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(vertical = 16.dp))
      s.error != null -> ErrorLine(s.error, Modifier.padding(vertical = 12.dp))
      else -> {
        if (s.showInUseNow) {
          MenuHeader("In use now")
          MenuRow(s.currentModel, s.currentProviderName, true) {}
        }
        if (s.groups.isEmpty()) {
          if (s.hasProvider) Text("Nothing pinned yet. Open the full list and tap the star next to a model.", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(vertical = 10.dp))
          MenuLink("Pin models in Settings", "Star the models you use and they show here, one tap away", onOpenSettings)
        }
        for (g in s.groups) {
          MenuHeader(g.header)
          for (m in g.models) MenuRow(m, g.provider, g.profileId == s.activeProfile && m == s.currentModel) { onPick(g.profileId, m) }
        }
        if (s.efforts.isNotEmpty()) EffortRow(s.effort, s.efforts, onEffort)
        MenuLink(
          if (s.hasProvider) "More models…" else "Provider, key and address…",
          if (s.hasProvider) "All models, key and address" else "Choose a provider to begin", onMore,
        )
      }
    }
  }
}

@Composable
private fun MenuHeader(text: String) {
  Text(text, style = ZoTheme.type.sectionHeader, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(start = 2.dp, top = 12.dp, bottom = 4.dp))
}

@Composable
private fun MenuRow(title: String, sub: String, current: Boolean, onClick: () -> Unit) {
  val c = ZoTheme.colors
  Row(
    Modifier.fillMaxWidth().clip(ZoTheme.shapes.navItem).clickable(role = Role.Button, onClick = onClick).semantics { selected = current }.padding(horizontal = 4.dp, vertical = 8.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Column(Modifier.weight(1f)) {
      Text(title, style = ZoTheme.type.popTitle, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
      if (sub.isNotBlank()) Text(sub, style = ZoTheme.type.popSub, color = c.dim, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
    if (current) ZoIcon(ZoIcons.Check, tint = c.accent, size = 18.dp)
  }
}

@Composable
private fun MenuLink(title: String, sub: String, onClick: () -> Unit) {
  val c = ZoTheme.colors
  Box(Modifier.padding(top = 10.dp)) {
    Row(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.navItem).border(1.dp, c.line, ZoTheme.shapes.navItem).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 12.dp, vertical = 10.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      Column(Modifier.weight(1f)) {
        Text(title, style = ZoTheme.type.popTitle, color = c.text)
        Text(sub, style = ZoTheme.type.popSub, color = c.dim)
      }
      ZoIcon(ZoIcons.Chevron, tint = c.dim, size = 16.dp)
    }
  }
}

@Composable
private fun EffortRow(effort: String, efforts: List<String>, onEffort: (String) -> Unit) {
  val c = ZoTheme.colors
  var open by remember { mutableStateOf(false) }
  Column(Modifier.padding(top = 10.dp).fillMaxWidth().clip(ZoTheme.shapes.navItem).border(1.dp, c.line, ZoTheme.shapes.navItem)) {
    Row(Modifier.fillMaxWidth().clickable(role = Role.Button) { open = !open }.padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
      Column(Modifier.weight(1f)) {
        Text("Effort · " + ModelLogic.effortLabel(effort), style = ZoTheme.type.popTitle, color = c.text)
        Text("How hard it thinks before answering", style = ZoTheme.type.popSub, color = c.dim)
      }
      ZoIcon(if (open) ZoIcons.Down else ZoIcons.Chevron, tint = c.dim, size = 16.dp)
    }
    if (open) {
      for (lvl in listOf("") + efforts) {
        Row(
          Modifier.fillMaxWidth().background(c.card2).clickable(role = Role.RadioButton) { open = false; onEffort(lvl) }.padding(horizontal = 16.dp, vertical = 10.dp),
          verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween,
        ) {
          Text(ModelLogic.effortLabel(lvl), style = ZoTheme.type.body, color = c.text, modifier = Modifier.weight(1f))
          if (lvl == effort) ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp)
          else Spacer(Modifier.width(16.dp))
        }
      }
    }
  }
}
