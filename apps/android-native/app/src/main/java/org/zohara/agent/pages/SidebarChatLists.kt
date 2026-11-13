package org.zohara.agent.pages

import androidx.compose.foundation.background
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.ChatSummary

/**
 * The "Pinned" and "Recents" chat lists of the sidebar (NAV-09..12). The drawer calls this; [activeChatId] is the open chat (highlighted),
 * [onOpenChat] opens one and closes the drawer. A row has a "more" button and answers a long press with the same menu: Pin or Unpin, Share
 * conversation, Delete this chat (with its confirm). The lists load when the drawer is shown; while it is, the running count of the avatar badge
 * is read every 15 s (NAV-15).
 */
@Composable
fun SidebarChatLists(activeChatId: String?, onOpenChat: (String) -> Unit) {
  val hub = rememberPagesHub()
  val state by hub.chats.state.collectAsState()
  val actions = rememberChatActions(hub, activeChatId)
  RefreshWhileVisible(null) { hub.chats.refresh() }
  RefreshWhileVisible(15_000) { hub.activity.refresh() }
  val sections = remember(state.value) { PagesFormat.sections(state.value.orEmpty()) }
  SidebarChatListsContent(sections, activeChatId, { onOpenChat(it.id) }, actions::menu)
  actions.DeleteConfirm()
}

/** The lists without any state of their own (screenshot tests and the real thing use this). */
@Composable
fun SidebarChatListsContent(
  sections: PagesFormat.Sections,
  activeChatId: String?,
  onOpen: (ChatSummary) -> Unit,
  menu: (ChatSummary) -> List<MenuAction>,
) {
  Column(Modifier.fillMaxWidth()) {
    if (sections.pinned.isNotEmpty()) {
      SideLabel("Pinned")
      for (c in sections.pinned) SideRow(c, c.id == activeChatId, onOpen, menu(c))
    }
    if (sections.recents.isNotEmpty()) {
      SideLabel("Recents")
      for (c in sections.recents) SideRow(c, c.id == activeChatId, onOpen, menu(c))
    }
  }
}

@Composable
private fun SideLabel(text: String) {
  Text(text, style = ZoTheme.type.sectionHeader, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(start = 12.dp, end = 12.dp, top = 14.dp, bottom = 6.dp))
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SideRow(c: ChatSummary, active: Boolean, onOpen: (ChatSummary) -> Unit, actions: List<MenuAction>) {
  val colors = ZoTheme.colors
  var open by remember { mutableStateOf(false) }
  Row(
    Modifier.fillMaxWidth().clip(ZoTheme.shapes.navItem).background(if (active) colors.card2 else Color.Transparent)
      .combinedClickable(role = Role.Button, onClick = { onOpen(c) }, onLongClick = { open = true })
      .padding(start = 12.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Icon(ZoIcons.Chat, null, Modifier.size(18.dp), tint = if (active) colors.text else colors.icon)
    Text(
      PagesFormat.title(c), style = ZoTheme.type.sideItem, color = colors.text, maxLines = 1, overflow = TextOverflow.Ellipsis,
      modifier = Modifier.weight(1f).padding(start = 10.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
    )
    if (c.pinned) Icon(ZoIcons.Pin, "Pinned", Modifier.size(14.dp), tint = colors.dimmer)
    Box {
      ZoIconButton(ZoIcons.More, "More for ${PagesFormat.title(c)}", { open = true }, size = 36.dp, iconSize = 16.dp, tint = colors.dim)
      ActionMenu(open, { open = false }, actions)
    }
  }
}
