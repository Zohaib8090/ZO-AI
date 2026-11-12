package org.zohara.zoai.pages

import androidx.compose.runtime.Composable

/**
 * The "Pinned" and "Recents" chat lists of the sidebar (NAV-09..12). Parity rows: NAV-09, NAV-10, NAV-11, NAV-12.
 * The drawer calls this; [activeChatId] is the open chat (highlighted), [onOpenChat] opens one and closes the drawer.
 */
@Composable
fun SidebarChatLists(activeChatId: String?, onOpenChat: (String) -> Unit) {
  // filled in by the pages module
}
