package org.zohara.chat.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.core.design.SettingsRow
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.nav.LocalNav
import org.zohara.chat.nav.SetTopBar

/** Settings home: the list of pages. Parity rows: SET-01, SET-03, SET-04. */
@Composable
fun SettingsHomeScreen() {
  val account = rememberAccount()
  val nav = LocalNav.current
  SetTopBar("Settings")
  SettingsHomeContent(SettingsPages.visible(account?.restricted ?: true), onOpen = { nav.settings(it.route) })
}

/**
 * NAV-23: at 840 dp and wider the list of pages stays on the left and the selected page is drawn on the right. [page] is the route of the page
 * to show (the Settings home itself shows the first page of the list); [content] draws it.
 */
@Composable
fun SettingsTwoPane(page: String?, content: @Composable (route: String) -> Unit) {
  val account = rememberAccount()
  val nav = LocalNav.current
  val pages = SettingsPages.visible(account?.restricted ?: true)
  val selected = pages.firstOrNull { it.route == page } ?: pages.first()
  SetTopBar("Settings")
  org.zohara.chat.nav.TwoPane(
    list = { SettingsHomeContent(pages, onOpen = { nav.settingsInPane(it.route) }, selected = selected.route, fillWidth = true) },
    detail = { androidx.compose.runtime.CompositionLocalProvider(LocalSettingsEmbedded provides true) { content(selected.route) } },
    listWidth = 300.dp,
  )
}

@Composable
fun SettingsHomeContent(pages: List<SettingsPageDef>, selected: String? = null, fillWidth: Boolean = false, onOpen: (SettingsPageDef) -> Unit) {
  androidx.compose.foundation.layout.Box(Modifier.fillMaxSize(), contentAlignment = androidx.compose.ui.Alignment.TopCenter) {
    Column((if (fillWidth) Modifier else Modifier.widthIn(max = 720.dp)).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
      Text("Settings", style = ZoTheme.type.pageTitle, color = ZoTheme.colors.text, modifier = Modifier.padding(top = 12.dp, bottom = 14.dp))
      SettingsCard {
        pages.forEachIndexed { i, p ->
          SettingsRow(
            p.title, subtitle = p.sub, onClick = { onOpen(p) }, divider = i != pages.lastIndex,
            modifier = if (p.route == selected) Modifier.background(ZoTheme.colors.active) else Modifier,
            control = { Text("Open  ›", style = ZoTheme.type.button, color = if (p.route == selected) ZoTheme.colors.accent else ZoTheme.colors.dim) },
          )
        }
      }
      androidx.compose.foundation.layout.Spacer(Modifier.padding(bottom = 32.dp))
    }
  }
}
