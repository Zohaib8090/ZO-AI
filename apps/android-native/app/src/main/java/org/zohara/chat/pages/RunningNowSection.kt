package org.zohara.chat.pages

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.chat.core.design.ZoDivider
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.nav.LocalNav

/**
 * "Running now" at the top of the user menu sheet (NAV-14, ACT-*): the running items with Stop, polled every 3 s while it is shown. Every read
 * also sets `AppContainer.runningCount`, which the avatar badge shows (NAV-15).
 */
@Composable
fun RunningNowSection() {
  val hub = rememberPagesHub()
  val ui by hub.activity.ui.collectAsState()
  val toast = rememberToastLambda()
  val scope = rememberCoroutineScope()
  val nav = LocalNav.current
  RefreshWhileVisible(3_000) { hub.activity.refresh() }
  val ticker = rememberTicker(ui.running.isNotEmpty())
  RunningNowContent(
    ui, ticker,
    ActivityActions(
      onStop = { i -> scope.launch { toast(hub.activity.stop(i.id)?.let { "Could not stop" } ?: "Stopped") } },
      onOpenChat = { nav.openChat(it) },
    ),
  )
}

@Composable
fun RunningNowContent(ui: ActivityUi, localNow: Long, actions: ActivityActions) {
  val c = ZoTheme.colors
  Column(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
    Text("Running now", style = ZoTheme.type.sectionHeader, color = c.dimmer, modifier = Modifier.padding(start = 6.dp))
    if (ui.running.isEmpty()) {
      Text(if (ui.loaded) "Nothing is running." else "Loading…", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(start = 6.dp, bottom = 4.dp))
    } else {
      for (i in ui.running) ActivityRow(i, PagesFormat.clock(PagesFormat.elapsedMs(i, ui.serverNow, ui.fetchedAt, localNow)), i.id in ui.stopSent, actions)
    }
    if (ui.clients.isNotEmpty()) Text(PagesFormat.connectionsLine(ui.clients.size), style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(start = 6.dp))
    ZoDivider(Modifier.padding(top = 4.dp))
  }
}
