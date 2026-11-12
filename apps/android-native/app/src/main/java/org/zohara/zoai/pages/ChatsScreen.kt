package org.zohara.zoai.pages

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Alignment
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.zohara.zoai.core.design.EmptyBox
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ChatSummary
import org.zohara.zoai.nav.LocalNav

/** Saved chats list. Parity rows: LIST-01..04, 08, 09, 12, 13. Refreshes on entry, on return to the app and every 60 s (LIST-04). */
@Composable
fun ChatsScreen() {
  val hub = rememberPagesHub()
  val state by hub.chats.state.collectAsState()
  val nav = LocalNav.current
  val actions = rememberChatActions(hub, activeChatId = null)
  RefreshWhileVisible(60_000) { hub.chats.refresh() }
  ChatsContent(state, remember(state) { PagesClock(System.currentTimeMillis()) }, onRefresh = hub.chats::refresh, onOpen = { nav.openChat(it.id) }, menu = actions::menu)
  actions.DeleteConfirm()
}

@Composable
fun ChatsContent(
  state: Loadable<List<ChatSummary>>,
  clock: PagesClock,
  onRefresh: () -> Unit,
  onOpen: (ChatSummary) -> Unit,
  menu: (ChatSummary) -> List<MenuAction>,
) {
  ListPage(
    state = state,
    title = "Chats",
    onRefresh = onRefresh,
    empty = { EmptyBox(PagesFormat.CHATS_EMPTY_TITLE, subtitle = PagesFormat.CHATS_EMPTY_TEXT, icon = ZoIcons.Chat) },
    footer = if ((state.value?.size ?: 0) >= PagesFormat.SERVICE_CHAT_LIMIT) ({
      Text(PagesFormat.OLDER_CHATS_NOTE, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = Modifier.padding(top = 4.dp))
    }) else null,
  ) { c -> ChatCardRow(c, clock, onOpen, menu(c)) }
}

@Composable
private fun ChatCardRow(c: ChatSummary, clock: PagesClock, onOpen: (ChatSummary) -> Unit, actions: List<MenuAction>) {
  var open by remember { mutableStateOf(false) }
  PageCard(
    icon = ZoIcons.Chat,
    name = PagesFormat.title(c),
    meta = clock.whenText(c.updated),
    onClick = { onOpen(c) },
    onLongClick = { open = true },
    trailing = {
      Row(verticalAlignment = Alignment.CenterVertically) {
        if (c.pinned) Icon(ZoIcons.Pin, "Pinned", Modifier.size(16.dp), tint = ZoTheme.colors.accent)
        Box {
          ZoIconButton(ZoIcons.More, "More for this chat", { open = true }, size = 36.dp, iconSize = 18.dp)
          ActionMenu(open, { open = false }, actions)
        }
      }
    },
  )
}
