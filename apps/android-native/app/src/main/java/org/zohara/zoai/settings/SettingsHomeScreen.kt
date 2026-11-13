package org.zohara.zoai.settings

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
import org.zohara.zoai.core.design.SettingsCard
import org.zohara.zoai.core.design.SettingsRow
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.nav.LocalNav
import org.zohara.zoai.nav.SetTopBar

/** Settings home: the list of pages. Parity rows: SET-01, SET-03, SET-04. */
@Composable
fun SettingsHomeScreen() {
  val account = rememberAccount()
  val nav = LocalNav.current
  SetTopBar("Settings")
  SettingsHomeContent(SettingsPages.visible(account?.restricted ?: true)) { nav.settings(it.route) }
}

@Composable
fun SettingsHomeContent(pages: List<SettingsPageDef>, onOpen: (SettingsPageDef) -> Unit) {
  androidx.compose.foundation.layout.Box(Modifier.fillMaxSize(), contentAlignment = androidx.compose.ui.Alignment.TopCenter) {
    Column(Modifier.widthIn(max = 720.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
      Text("Settings", style = ZoTheme.type.pageTitle, color = ZoTheme.colors.text, modifier = Modifier.padding(top = 12.dp, bottom = 14.dp))
      SettingsCard {
        pages.forEachIndexed { i, p ->
          SettingsRow(
            p.title, subtitle = p.sub, onClick = { onOpen(p) }, divider = i != pages.lastIndex,
            control = { Text("Open  ›", style = ZoTheme.type.button, color = ZoTheme.colors.dim) },
          )
        }
      }
      androidx.compose.foundation.layout.Spacer(Modifier.padding(bottom = 32.dp))
    }
  }
}
