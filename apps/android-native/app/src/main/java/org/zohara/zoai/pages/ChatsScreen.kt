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
import org.zohara.zoai.nav.TwoPane
import org.zohara.zoai.nav.isTwoPane
import androidx.compose.runtime.saveable.rememberSaveable

/** Saved chats list. Parity rows: LIST-01..04, 08, 09, 12, 13. Refreshes on entry, on return to the app and every 60 s (LIST-04). */
@Composable
fun ChatsScreen() {
  val hub = rememberPagesHub()
  val state by hub.chats.state.collectAsState()
  val nav = LocalNav.current
  val actions = rememberChatActions(hub, activeChatId = null)
  RefreshWhileVisible(60_000) { hub.chats.refresh() }
  val clock = remember(state) { PagesClock(System.currentTimeMillis()) }
  if (isTwoPane()) {
    // NAV-23: the list on the left, the chat that was tapped on the right
    var selected by rememberSaveable { mutableStateOf<String?>(null) }
    TwoPane(
      list = { ChatsContent(state, clock, onRefresh = hub.chats::refresh, onOpen = { selected = it.id }, selectedId = selected, menu = actions::menu) },
      detail = { selected?.let { id -> org.zohara.zoai.chat.ChatScreen(id) } ?: PickAnItem("Pick a chat to read it here") },
    )
  } else {
    ChatsContent(state, clock, onRefresh = hub.chats::refresh, onOpen = { nav.openChat(it.id) }, menu = actions::menu)
  }
  actions.DeleteConfirm()
}

@Composable
fun ChatsContent(
  state: Loadable<List<ChatSummary>>,
  clock: PagesClock,
  onRefresh: () -> Unit,
  onOpen: (ChatSummary) -> Unit,
  selectedId: String? = null,
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
  ) { c -> ChatCardRow(c, clock, onOpen, menu(c), c.id == selectedId) }
}

@Composable
private fun ChatCardRow(c: ChatSummary, clock: PagesClock, onOpen: (ChatSummary) -> Unit, actions: List<MenuAction>, selected: Boolean = false) {
  var open by remember { mutableStateOf(false) }
  PageCard(
    icon = ZoIcons.Chat,
    name = PagesFormat.title(c),
    meta = clock.whenText(c.updated),
    onClick = { onOpen(c) },
    selected = selected,
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
