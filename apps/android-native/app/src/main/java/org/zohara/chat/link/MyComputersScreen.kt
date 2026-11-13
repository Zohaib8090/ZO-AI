package org.zohara.chat.link

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.launch
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.PrimaryButton
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.core.design.SettingsRow
import org.zohara.chat.core.design.ZoChip
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.core.net.ClaudeStatus
import org.zohara.chat.core.net.LinkDevice
import org.zohara.chat.nav.LocalNav

/** The text of the Claude Code card (LINK-CC, MODEL-21): ready on the server, or how to send the sign-in. */
fun claudeLine(cc: ClaudeStatus?, locale: java.util.Locale = java.util.Locale.getDefault()): String {
  if (cc == null || !cc.present) return LinkText.NO_CLAUDE
  val whenText = if (cc.setAt > 0) DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, locale).format(Date(LinkFormat.millis(cc.setAt))) else "earlier"
  return "Claude Code (your subscription) is ready on the server. Sign-in sent $whenText. It lasts about a year; send a new one from the desktop app before then."
}

/**
 * "Claude Code (your subscription)" for admins (LINK-CC, SET-ACC-06): status only, no token handling on the phone. Shows nothing for other people or when the status
 * could not be read. Used on My computers and on the Account and Model pages of the settings module.
 */
@Composable
fun ClaudeCodeCard(modifier: Modifier = Modifier) {
  val module = LinkModule.of(LocalAppContainer.current)
  val cc by module.claude.collectAsState()
  LaunchedEffect(module) { module.loadClaude() }
  val s = cc ?: return
  if (!s.admin) return
  Column(modifier) {
    SectionHeader("Claude Code (your subscription)")
    SettingsCard { SettingsRow(if (s.present) "Ready on the server" else "Not set up", subtitle = claudeLine(s), divider = false) }
  }
}

/** One computer on the page (LINK-05): name with a status dot, "Online now" or "Not reachable · last seen …", chips, and the button "Run chats here" / "In use". */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ComputerRow(d: LinkDevice, inUse: Boolean, onUse: () -> Unit, now: Long = System.currentTimeMillis(), divider: Boolean = true) {
  SettingsRow(
    LinkFormat.name(d),
    subtitle = if (d.online) "Online now" else "Not reachable" + (if (d.lastSeen > 0) " · last seen " + LinkFormat.ago(LinkFormat.millis(d.lastSeen), now) else ""),
    divider = false,
    leading = { PresenceDot(d.online) },
    control = { PillButton(if (inUse) "In use" else "Run chats here", onUse, enabled = !inUse && d.online, accent = !inUse && d.online) },
  )
  FlowRow(Modifier.padding(start = 36.dp, end = 16.dp, bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
    LinkFormat.chips(d).forEach { ZoChip(it) }
  }
  if (divider) Box(Modifier.padding(start = 16.dp).fillMaxWidth().height(1.dp).background(ZoTheme.colors.line))
}

/** Settings > My computers and the Claude Code status card (LINK-05, LINK-CC). */
@Composable
fun MyComputersScreen() {
  val module = LinkModule.of(LocalAppContainer.current)
  val nav = LocalNav.current
  val choice by module.runOn.choice.collectAsState()
  val link by module.runOn.link.collectAsState()
  val scope = rememberCoroutineScope()
  LaunchedEffect(Unit) { module.runOn.refresh() }
  org.zohara.chat.nav.SetTopBar("Settings")
  LinkPresence()
  androidx.compose.foundation.layout.Column(androidx.compose.ui.Modifier.fillMaxSize()) {
    org.zohara.chat.settings.SettingsBackLink(androidx.compose.ui.Modifier.padding(horizontal = 16.dp))
    MyComputersContent(
      link, choice,
      onUse = { d -> module.runOn.choose(d); nav.home() },
      onCheck = { scope.launch { module.runOn.refresh() } },
    ) { ClaudeCodeCard() }
  }
}

/** The page without the module, so it can be drawn in tests. */
@Composable
fun MyComputersContent(link: LinkState, choice: RunOnChoice, onUse: (LinkDevice) -> Unit, onCheck: () -> Unit, now: Long = System.currentTimeMillis(), extra: @Composable () -> Unit = {}) {
  val c = ZoTheme.colors
  Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).navigationBarsPadding()) {
    Text("My computers", style = ZoTheme.type.pageTitle, color = c.text, modifier = Modifier.padding(top = 8.dp))
    Spacer(Modifier.size(12.dp))
    SettingsCard { SettingsRow("Run Zohara Chat on your own computer", subtitle = LinkText.ACCESS_WARNING, divider = false) }
    if (!link.ok && link.at == 0L) {
      Spacer(Modifier.size(12.dp))
      SettingsCard {
        SettingsRow("Could not check your computers", subtitle = "Check your connection and try again.", divider = false, control = { PrimaryButton(if (link.busy) "Checking…" else "Try again", onCheck, enabled = !link.busy) })
      }
    } else {
      SectionHeader("Linked computers")
      SettingsCard {
        if (link.devices.isEmpty()) SettingsRow("No computer is linked", subtitle = LinkText.NO_COMPUTER_PAGE)
        link.devices.forEach { d -> ComputerRow(d, inUse = choice.device == d.id, onUse = { onUse(d) }, now = now) }
        SettingsRow("Presence", subtitle = LinkText.PRESENCE, divider = false, control = { PillButton(if (link.busy) "Checking…" else "Check again", onCheck, enabled = !link.busy) })
      }
    }
    extra()
    Spacer(Modifier.size(24.dp))
  }
}
