package org.zohara.chat.nav

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.zohara.chat.core.design.ZoTheme

/**
 * Placeholder of a screen another module still has to build. It shows which matrix rows belong here, so nothing is forgotten.
 * Replace the body of the screen function (not its name or parameters, the navigation graph calls it).
 */
@Composable
fun StubScreen(screen: String, rows: String) {
  val c = ZoTheme.colors
  Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
    Text(screen, style = ZoTheme.type.pageTitle, color = c.text)
    Text("Not built yet. Parity rows: $rows", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 8.dp))
  }
}
