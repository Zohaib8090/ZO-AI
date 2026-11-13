package org.zohara.agent.chat

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.RowScope
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.nav.LocalNav
import org.zohara.agent.nav.SetTopBar

/** A chat: thread, streaming, composer. Parity rows: CHAT-*, COMP-*, INTER-*, CTX-*, PERM-*. */
@Composable
fun ChatScreen(chatId: String) {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val inst = remember { ChatHub.of(container) }
  val session = remember(chatId) { inst.hub.forId(chatId) }
  val state by session.state.collectAsState()
  val clipboard = LocalClipboardManager.current
  var menu by remember { mutableStateOf(false) }
  var contextRequests by remember { mutableIntStateOf(0) }
  var pinned by remember { mutableStateOf<Boolean?>(null) }
  var confirmDelete by remember { mutableStateOf(false) }

  LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { session.refreshIfIdle(); inst.pills.refresh() }
  LaunchedEffect(menu) {
    if (menu && pinned == null) pinned = try { container.api.chats().firstOrNull { it.id == chatId }?.pinned } catch (_: Exception) { null }
  }

  val title = Transcript.displayTitle(state.title, state.items.isNotEmpty() || state.busy)
  SetTopBar(title) {
    Box {
      ZoIconButton(ZoIcons.More, "More", { menu = true }, tint = ZoTheme.colors.icon)
      ChatMenu(
        menu, { menu = false }, pinned,
        ChatMenuActions(
          onContext = { contextRequests++ },
          onCopyAll = { clipboard.setText(AnnotatedString(Transcript.copyAll(state.items))); inst.env.toast("Copied") },
          onPin = { val on = pinned != true; session.pin(on); pinned = on },
          onTerminal = if (container.session.admin) ({ nav.terminal() }) else null,
          onDelete = { confirmDelete = true },
        ),
      )
    }
  }

  ChatPane(session, inst, greetingText = null, contextRequests = contextRequests)

  if (confirmDelete) {
    val c = ZoTheme.colors
    AlertDialog(
      onDismissRequest = { confirmDelete = false },
      containerColor = c.popover,
      title = { Text("Delete this chat?", color = c.text) },
      text = { Text("It is removed from your account on every device. This cannot be undone.", color = c.text2) },
      confirmButton = {
        TextButton({ confirmDelete = false; session.delete { ok -> if (ok) { inst.hub.remove(chatId); nav.home() } } }) { Text("Delete", color = c.danger) }
      },
      dismissButton = { TextButton({ confirmDelete = false }) { Text("Cancel", color = c.text2) } },
    )
  }
}
