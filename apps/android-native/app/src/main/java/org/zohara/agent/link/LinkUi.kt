package org.zohara.agent.link

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner as LifecycleOwnerLocal
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoIcon
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoSpinner
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.LinkDevice

/** the green of an online computer (`.pdot.on`, #3FB57A) and the amber of a waiting one (`.pdot.wait`) */
val OnlineGreen = Color(0xFF3FB57A)
val WaitAmber = Color(0xFFD9A441)

/** The 8 dp status dot (`.pdot`): green when online, grey when not. */
@Composable
fun PresenceDot(online: Boolean, modifier: Modifier = Modifier) {
  Box(modifier.size(8.dp).clip(CircleShape).background(if (online) OnlineGreen else ZoTheme.colors.dimmer))
}

/**
 * Polls the link status every 20 s while the screen is visible, and once on resume (LINK-03, BG-04). Put it in the composition of the composer and of My computers.
 */
@Composable
fun LinkPresence() {
  val module = LinkModule.of(LocalAppContainer.current)
  val owner = LifecycleOwnerLocal.current
  LaunchedEffect(owner, module) {
    owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
      pollEvery(LINK_POLL_MS) { module.runOn.refresh() }
    }
  }
}

/** The "Run on" control in the pills row (COMP-10): a dot and "Cloud" or "My computer (name)"; opens [RunOnSheet]. */
@Composable
fun RunOnPill(modifier: Modifier = Modifier) {
  val module = LinkModule.of(LocalAppContainer.current)
  val choice by module.runOn.choice.collectAsState()
  val link by module.runOn.link.collectAsState()
  var open by remember { mutableStateOf(false) }
  val on = !choice.isCloud
  val d = link.device(choice.device)
  val online = d?.online == true
  val name = if (d != null) LinkFormat.name(d) else choice.name
  RunOnPillView(on, online, name, modifier) { open = true }
  if (open) RunOnSheet(module) { open = false }
}

/** The pill without the module (also drawn by the screenshot tests). */
@Composable
fun RunOnPillView(on: Boolean, online: Boolean, name: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
  val c = ZoTheme.colors
  val label = if (on) LinkFormat.runLabel(name) else LinkText.CLOUD_TITLE
  val desc = if (on) (if (online) "Run on: $label, online" else "Run on: $label, not reachable") else "Run on: Cloud. Tap to run on your own computer instead."
  Row(
    modifier.heightIn(min = 40.dp).clip(ZoTheme.shapes.pill).border(1.dp, c.line, ZoTheme.shapes.pill)
      .clickable(role = Role.Button, onClick = onClick).semantics { contentDescription = desc }.padding(horizontal = 12.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    if (on) { PresenceDot(online); Spacer(Modifier.size(6.dp)) }
    Text(label, style = ZoTheme.type.cardMeta.copy(fontSize = 13.sp), color = c.text3, maxLines = 1)
    Spacer(Modifier.size(4.dp))
    ZoIcon(ZoIcons.Down, size = 14.dp, tint = c.dim)
  }
}

/** The sheet "Run on" (LINK-01): Cloud, one row per linked computer, the empty and error texts, "Check again". */
@Composable
fun RunOnSheet(module: LinkModule, onDismiss: () -> Unit) {
  val choice by module.runOn.choice.collectAsState()
  val link by module.runOn.link.collectAsState()
  val scope = rememberCoroutineScope()
  LaunchedEffect(Unit) { module.runOn.refresh() }
  RunOnSheetContent(link, choice, onChoose = { d -> module.runOn.choose(d); onDismiss() }, onCheck = { scope.launch { module.runOn.refresh() } }, onDismiss = onDismiss)
}

/** The body of the sheet, without the module (so it can be drawn in tests). */
@Composable
fun RunOnSheetContent(link: LinkState, choice: RunOnChoice, onChoose: (LinkDevice?) -> Unit, onCheck: () -> Unit, onDismiss: () -> Unit, now: Long = System.currentTimeMillis()) {
  ZoBottomSheet("Run on", onDismiss) {
    RunOnRows(link, choice, onChoose, onCheck, now)
  }
}

@Composable
fun RunOnRows(link: LinkState, choice: RunOnChoice, onChoose: (LinkDevice?) -> Unit, onCheck: () -> Unit, now: Long = System.currentTimeMillis()) {
  val c = ZoTheme.colors
  @Composable
  fun Row1(title: String, sub: String, dot: Boolean?, checked: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 4.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
      Column(Modifier.weight(1f)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
          if (dot != null) { PresenceDot(dot); Spacer(Modifier.size(8.dp)) }
          Text(title, style = ZoTheme.type.popTitle, color = c.text)
        }
        Text(sub, style = ZoTheme.type.popSub, color = c.dim)
      }
      if (checked) ZoIcon(ZoIcons.Check, tint = c.check, size = 18.dp)
    }
  }
  Row1(LinkText.CLOUD_TITLE, LinkText.CLOUD_SUB, null, choice.isCloud) { onChoose(null) }
  link.devices.forEach { d -> Row1(LinkFormat.runLabel(LinkFormat.name(d)), LinkFormat.statusLine(d, now), d.online, choice.device == d.id) { onChoose(d) } }
  if (link.ok && link.devices.isEmpty()) Text(LinkText.NO_COMPUTER, style = ZoTheme.type.popSub, color = c.dim, modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp))
  if (!link.ok && link.at == 0L && !link.busy) Text(LinkText.COULD_NOT_CHECK_LIST, style = ZoTheme.type.popSub, color = c.dim, modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp))
  org.zohara.agent.core.design.ZoDivider(Modifier.padding(vertical = 6.dp))
  Row1(if (link.busy) "Checking…" else "Check again", "", null, false) { if (!link.busy) onCheck() }
}

/** The note under the composer when My computer is chosen but not reachable (COMP-15): the offline message and "Check again". */
@Composable
fun RunNote(modifier: Modifier = Modifier) {
  val module = LinkModule.of(LocalAppContainer.current)
  val choice by module.runOn.choice.collectAsState()
  val link by module.runOn.link.collectAsState()
  val scope = rememberCoroutineScope()
  RunNoteContent(choice, link, { scope.launch { module.runOn.refresh() } }, modifier)
}

@Composable
fun RunNoteContent(choice: RunOnChoice, link: LinkState, onCheck: () -> Unit, modifier: Modifier = Modifier) {
  val online = link.device(choice.device)?.online == true
  val asking = !choice.isCloud && (link.at > 0 || !link.ok) && !online
  if (!asking) return
  val c = ZoTheme.colors
  Column(modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).background(c.card).border(1.dp, c.line12, ZoTheme.shapes.popover).padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
    Text(if (link.ok || link.at > 0) LinkText.OFFLINE else LinkText.COULD_NOT_CHECK_ONE, style = ZoTheme.type.cardMeta, color = c.text2)
    SecondaryButton(if (link.busy) "Checking…" else "Check again", onCheck, enabled = !link.busy)
  }
}

/** "Waiting for your approval on your computer: <summary>" with a spinner (CHAT-37). */
@Composable
fun WaitingApprovalRow(summary: String, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Row(
    modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).border(1.dp, c.line12, ZoTheme.shapes.popover).padding(horizontal = 14.dp, vertical = 10.dp)
      .semantics { contentDescription = LinkText.WAITING + if (summary.isNotBlank()) ": $summary" else "" },
    verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp),
  ) {
    ZoSpinner(size = 16.dp)
    Text(LinkText.WAITING + (if (summary.isNotBlank()) ": ${summary.take(300)}" else "."), style = ZoTheme.type.cardMeta, color = c.text2)
  }
}

/** "Running on your computer (name)" under the message (the `stream` event of a computer turn). */
@Composable
fun RunningOnLine(name: String?, modifier: Modifier = Modifier) {
  Text("Running on your computer" + (if (!name.isNullOrBlank()) " ($name)" else ""), style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = modifier)
}

// ---------------------------------------------------------------- permission mode

/** The sheet "What Zohara Agent may do" (PERM-03, LINK-11): on a computer only Manual and Auto, Bypass shown as Auto, and the note. */
@Composable
fun ModeSheet(modes: List<String>, current: String, onComputer: Boolean, onPick: (String) -> Unit, onDismiss: () -> Unit) {
  ZoBottomSheet("What Zohara Agent may do", onDismiss) { ModeRows(modes, current, onComputer, onPick) }
}

@Composable
fun ModeRows(modes: List<String>, current: String, onComputer: Boolean, onPick: (String) -> Unit) {
  val c = ZoTheme.colors
  modes.forEach { m ->
    Row(Modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).clickable(role = Role.Button) { onPick(m) }.padding(horizontal = 4.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
      Column(Modifier.weight(1f)) {
        Text(m, style = ZoTheme.type.popTitle, color = c.text)
        Text(LinkModes.INFO[m].orEmpty(), style = ZoTheme.type.popSub, color = c.dim)
      }
      if (m == LinkModes.shown(current, onComputer)) ZoIcon(ZoIcons.Check, tint = c.check, size = 18.dp)
    }
  }
  if (onComputer) Text(LinkText.MODE_NOTE, style = ZoTheme.type.popSub, color = c.dim, modifier = Modifier.padding(horizontal = 4.dp, vertical = 8.dp))
}

// ---------------------------------------------------------------- models on the computer

/**
 * "Models on your computer" (MODEL-32): the profiles, the providers and the models of the chosen computer, changed through `desktop_op`. API keys are never set here.
 */
@Composable
fun DesktopModelSheet(ops: DesktopOps, onDismiss: () -> Unit) {
  val c = ZoTheme.colors
  val scope = rememberCoroutineScope()
  var providers by remember { mutableStateOf<org.zohara.agent.core.net.ProvidersResp?>(null) }
  var profiles by remember { mutableStateOf<org.zohara.agent.core.net.ProfilesResp?>(null) }
  var models by remember { mutableStateOf<org.zohara.agent.core.net.ModelsResp?>(null) }
  var provider by remember { mutableStateOf("") }
  var message by remember { mutableStateOf("") }
  var loading by remember { mutableStateOf(true) }

  suspend fun loadModels(p: String) {
    provider = p
    models = null
    try { models = ops.models(p); message = "" } catch (e: DesktopOpException) { message = e.message.orEmpty() }
  }

  LaunchedEffect(Unit) {
    try {
      providers = ops.providers()
      profiles = try { ops.profiles() } catch (_: DesktopOpException) { null }
      val cur = providers?.current.orEmpty()
      if (cur.isNotEmpty()) loadModels(cur)
    } catch (e: DesktopOpException) { message = e.message.orEmpty() }
    loading = false
  }

  ZoBottomSheet(
    "Models on your computer", onDismiss,
    footer = { SecondaryButton("Close", onDismiss) },
    skipPartiallyExpanded = true,
  ) {
    Column(Modifier.heightIn(max = 520.dp).verticalScroll(rememberScrollState())) {
      Text(LinkText.MODEL_NOTE, style = ZoTheme.type.fieldHint, color = c.dimmer, modifier = Modifier.padding(bottom = 10.dp))
      if (loading) Row(Modifier.padding(vertical = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) { ZoSpinner(size = 16.dp); Text("Asking your computer…", style = ZoTheme.type.cardMeta, color = c.dim) }
      val prof = profiles?.profiles.orEmpty()
      if (prof.isNotEmpty()) {
        Text("Profiles", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(bottom = 4.dp))
        prof.forEach { x ->
          PickRow(x.name, "${x.provider} · ${x.model}", profiles?.active == x.id) {
            scope.launch {
              try { ops.profileUse(x.id); profiles = ops.profiles(); providers = ops.providers(); loadModels(providers?.current.orEmpty()) } catch (e: DesktopOpException) { message = e.message.orEmpty() }
            }
          }
        }
      }
      val provs = providers?.providers.orEmpty().filter { !it.hidden }
      if (provs.isNotEmpty()) {
        Text("Provider", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(top = 12.dp, bottom = 4.dp))
        FlowChips(provs.map { it.id to (it.name + if (it.hasKey) " ✓" else "") }, provider) { id -> scope.launch { loadModels(id) } }
      }
      val ms = models
      if (provider.isNotEmpty()) {
        Text("Model", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(top = 12.dp, bottom = 4.dp))
        if (ms == null && message.isEmpty()) ZoSpinner(size = 16.dp)
        if (ms != null && ms.models.isEmpty()) Text("No models listed", style = ZoTheme.type.cardMeta, color = c.dim)
        ms?.models?.forEach { m ->
          PickRow(m, null, m == ms.current) {
            scope.launch {
              try { ops.select(provider, m); models = ops.models(provider); message = "Now using $m on your computer." } catch (e: DesktopOpException) { message = e.message.orEmpty() }
            }
          }
        }
      }
      if (message.isNotEmpty()) Text(message, style = ZoTheme.type.cardMeta, color = c.text2, modifier = Modifier.padding(top = 10.dp))
    }
  }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FlowChips(items: List<Pair<String, String>>, selected: String, onPick: (String) -> Unit) {
  val c = ZoTheme.colors
  FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
    items.forEach { (id, label) ->
      val on = id == selected
      Text(
        label, style = ZoTheme.type.cardMeta.copy(fontSize = 13.sp), color = if (on) c.onAccent else c.text2,
        modifier = Modifier.clip(ZoTheme.shapes.pill).background(if (on) c.accent else Color.Transparent).border(1.dp, if (on) c.accent else c.line12, ZoTheme.shapes.pill)
          .clickable(role = Role.Button) { onPick(id) }.padding(horizontal = 12.dp, vertical = 8.dp),
      )
    }
  }
}

@Composable
private fun PickRow(title: String, sub: String?, checked: Boolean, onClick: () -> Unit) {
  val c = ZoTheme.colors
  Row(Modifier.fillMaxWidth().clip(ZoTheme.shapes.popover).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 4.dp, vertical = 9.dp), verticalAlignment = Alignment.CenterVertically) {
    Column(Modifier.weight(1f)) {
      Text(title, style = ZoTheme.type.popTitle, color = c.text, maxLines = 1)
      if (sub != null) Text(sub, style = ZoTheme.type.popSub, color = c.dim, maxLines = 1)
    }
    if (checked) ZoIcon(ZoIcons.Check, tint = c.check, size = 18.dp)
  }
}
