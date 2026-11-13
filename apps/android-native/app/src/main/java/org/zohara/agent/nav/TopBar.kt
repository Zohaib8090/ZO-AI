package org.zohara.agent.nav

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme

/**
 * What the shared top bar shows right now. A screen calls [SetTopBar] to give its title (a chat's title) and its action buttons
 * (the chat's Context and Copy menu, NAV-17). Without a call the bar shows the title of the route.
 */
@Stable
class TopBarController {
  var title by mutableStateOf<String?>(null)
  var actions by mutableStateOf<(@Composable RowScope.() -> Unit)?>(null)
  var owner: Any? = null
}

val LocalTopBar = staticCompositionLocalOf { TopBarController() }

@Composable
fun SetTopBar(title: String?, actions: (@Composable RowScope.() -> Unit)? = null) {
  val ctl = LocalTopBar.current
  val me = remember { Any() }
  ctl.owner = me
  ctl.title = title
  ctl.actions = actions
  DisposableEffect(me) {
    onDispose {
      if (ctl.owner === me) { ctl.title = null; ctl.actions = null; ctl.owner = null }
    }
  }
}

/** The bar above every screen: the sidebar button always in the same place at the start (NAV-06), the title, and the screen's actions. */
@Composable
fun ZoTopBar(routeTitle: String, showMenuButton: Boolean, onMenu: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  val ctl = LocalTopBar.current
  Row(
    modifier.fillMaxWidth().background(c.bg).statusBarsPadding().height(52.dp).padding(horizontal = 6.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    if (showMenuButton) ZoIconButton(ZoIcons.Sidebar, "Open the sidebar", onMenu, tint = c.icon)
    else Spacer(Modifier.padding(start = 10.dp))
    Text(
      ctl.title ?: routeTitle,
      style = ZoTheme.type.popTitle.copy(fontSize = androidx.compose.ui.unit.TextUnit(15f, androidx.compose.ui.unit.TextUnitType.Sp)),
      color = c.text,
      maxLines = 1,
      overflow = TextOverflow.Ellipsis,
      modifier = Modifier.weight(1f).padding(horizontal = 6.dp),
    )
    ctl.actions?.invoke(this)
  }
}
