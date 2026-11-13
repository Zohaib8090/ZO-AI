package org.zohara.agent.nav

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import org.zohara.agent.core.design.ZoTheme

/** NAV-23: from this window width Settings and the lists show a list and the opened item side by side (a tablet, a fold, a phone on its side). */
const val TWO_PANE_MIN_DP = 840

/** True when the window is at least [TWO_PANE_MIN_DP] wide. (The window, not the content: the permanent sidebar from 900 dp does not change the layout of a page.) */
@Composable
fun isTwoPane(): Boolean = LocalConfiguration.current.screenWidthDp >= TWO_PANE_MIN_DP

/** A list on the left (fixed width) and the detail on the right, with a thin divider between them. */
@Composable
fun TwoPane(list: @Composable () -> Unit, detail: @Composable () -> Unit, modifier: Modifier = Modifier, listWidth: Dp = 300.dp) {
  Row(modifier.fillMaxSize()) {
    Box(Modifier.width(listWidth).fillMaxHeight()) { list() }
    Box(Modifier.width(1.dp).fillMaxHeight().background(ZoTheme.colors.line))
    Box(Modifier.weight(1f).fillMaxHeight()) { detail() }
  }
}
