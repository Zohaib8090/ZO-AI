package org.zohara.zoai.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.zohara.zoai.core.design.EmptyBox
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.DangerButton
import org.zohara.zoai.core.design.SectionHeader
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.SettingsRow
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ActivityItem
import org.zohara.zoai.core.store.RunOn
import org.zohara.zoai.nav.LocalNav

fun kindIcon(kind: String): ImageVector = when (kind) {
  "answer" -> ZoIcons.Chat
  "claude_code", "terminal" -> ZoIcons.Terminal
  "scheduled" -> ZoIcons.Alarm
  "memory_job" -> ZoIcons.Context
  "command" -> ZoIcons.Bolt
  "sync" -> ZoIcons.Refresh
  "client" -> ZoIcons.Globe
  else -> ZoIcons.Activity
}

/** A clock that ticks every second while the screen is shown (the running time of an item counts up live). */
@Composable
fun rememberTicker(active: Boolean): Long {
  val t by produceState(System.currentTimeMillis(), active) {
    while (active) {
      value = System.currentTimeMillis()
      delay(1000)
    }
  }
  return t
}

/** What the page does with Stop and Open chat: kept apart from the state so tests can drive it. */
class ActivityActions(val onStop: (ActivityItem) -> Unit, val onOpenChat: (String) -> Unit)

/**
 * Activity page. Parity rows: ACT-01..09, INTER-11. Running now (with live clocks and Stop), Connected, Background services; read every 3 s while
 * it is shown. A restricted account sees only its own items because the server scopes `activity` (ACT-09).
 */
@Composable
fun ActivityScreen() {
  val hub = rememberPagesHub()
  val ui by hub.activity.ui.collectAsState()
  val runOn by LocalAppContainerPrefsRunOn()
  val computerName = rememberComputerName()
  val nav = LocalNav.current
  val toast = rememberToastLambda()
  val scope = rememberCoroutineScope()
  RefreshWhileVisible(3_000) { hub.activity.refresh() }
  val ticker = rememberTicker(ui.running.isNotEmpty())
  ActivityContent(
    ui, ticker, runOn,
    ActivityActions(
      onStop = { i -> scope.launch { toast(hub.activity.stop(i.id)?.let { "Could not stop" } ?: "Stopped") } },
      onOpenChat = { nav.openChat(it) },
    ),
    onRetry = hub.activity::refresh,
    computerName = computerName,
  )
}

@Composable
private fun LocalAppContainerPrefsRunOn(): androidx.compose.runtime.State<RunOn> {
  val c = org.zohara.zoai.LocalAppContainer.current
  return c.prefs.runOn.collectAsState(initial = RunOn.Cloud)
}

@Composable
fun ActivityContent(ui: ActivityUi, localNow: Long, runOn: RunOn, actions: ActivityActions, onRetry: () -> Unit = {}, computerName: String? = null) {
  val computer = computerName ?: PagesFormat.COMPUTER_FALLBACK
  val c = ZoTheme.colors
  LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    item { PageHeader("Activity") }
    // ---- Running now
    item { SectionHeader("Running now", Modifier.padding(top = 0.dp)) }
    when {
      !ui.loaded -> item { LoadingLine() }
      ui.error != null && ui.items.isEmpty() -> item { PageError(ui.error, onRetry) }
      ui.running.isEmpty() -> item { EmptyBox(PagesFormat.ACTIVITY_EMPTY_TITLE, subtitle = PagesFormat.ACTIVITY_EMPTY_TEXT, icon = ZoIcons.Activity) }
      else -> for (i in ui.running) item(key = "run-${i.id}") {
        ActivityRow(i, PagesFormat.clock(PagesFormat.elapsedMs(i, ui.serverNow, ui.fetchedAt, localNow)), i.id in ui.stopSent, actions)
      }
    }
    // ---- Connected (ACT-06)
    item { SectionHeader("Connected") }
    item {
      SettingsCard {
        SettingsRow("This phone", subtitle = "This app (Active)")
        if (!runOn.isCloud) SettingsRow("Linked computer", subtitle = "$computer (Online and ready)")
        SettingsRow("Web app and command line", subtitle = PagesFormat.clientsSeenText(ui.clientsSeen, ui.serverNow), divider = ui.clients.isNotEmpty())
        if (ui.clients.isNotEmpty()) SettingsRow("Connected clients", subtitle = PagesFormat.connectionsLine(ui.clients.size), divider = false)
      }
    }
    // ---- Background services (ACT-07)
    item { SectionHeader("Background services") }
    item {
      SettingsCard {
        for ((name, text) in PagesFormat.BACKGROUND_SERVICES) SettingsRow(name, subtitle = text)
        SettingsRow("Desktop link", subtitle = if (runOn.isCloud) PagesFormat.DESKTOP_LINK_STANDBY else "Connected to $computer", divider = false)
      }
    }
  }
}

/** One running item: the dot, "<title>", "<Kind> from <source> · detail", the running time, and Stop (or "Cannot be stopped"), Open chat. */
@Composable
fun ActivityRow(i: ActivityItem, clock: String, stopSent: Boolean, actions: ActivityActions) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  Column(
    Modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(c.card).border(1.dp, c.line, ZoTheme.shapes.card).padding(14.dp),
    verticalArrangement = Arrangement.spacedBy(8.dp),
  ) {
    androidx.compose.foundation.layout.Row(verticalAlignment = Alignment.Top) {
      Box(Modifier.padding(top = 5.dp, end = 12.dp).size(8.dp).clip(CircleShape).background(c.ok))
      androidx.compose.material3.Icon(kindIcon(i.kind), null, Modifier.size(18.dp), tint = c.icon)
      Column(Modifier.weight(1f).padding(start = 10.dp)) {
        Text(i.title.ifBlank { PagesFormat.kindWord(i.kind) }, style = t.cardName, color = c.text, maxLines = 2)
        Text(PagesFormat.activitySub(i), style = t.cardMeta, color = c.dim, maxLines = 2, modifier = Modifier.padding(top = 2.dp))
      }
      Text(clock, style = t.cardMeta, color = c.text2, modifier = Modifier.padding(start = 8.dp))
    }
    androidx.compose.foundation.layout.Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
      if (i.canStop) {
        if (stopSent) Text("Stop sent", style = t.cardMeta, color = c.dim) else DangerButton("Stop", { actions.onStop(i) }, outlined = true)
      } else Text("Cannot be stopped", style = t.cardMeta, color = c.dimmer)
      if (!i.chatId.isNullOrBlank()) SecondaryButton("Open chat", { actions.onOpenChat(i.chatId) })
    }
  }
}
