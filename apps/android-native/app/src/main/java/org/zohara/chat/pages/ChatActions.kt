package org.zohara.chat.pages

import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.nav.LocalNav

/**
 * What the person can do with a chat row: pin or unpin (LIST-08), share the conversation as text (LIST-12) and delete it after a confirm
 * (LIST-09, NAV-11). Shared by the Chats page and the sidebar lists. Deleting the chat that is open goes home.
 */
@Stable
class ChatActions(
  private val chats: ChatsHolder,
  private val scope: CoroutineScope,
  private val activeChatId: String?,
  private val goHome: () -> Unit,
  private val toast: (String) -> Unit,
  private val share: (title: String, text: String) -> Unit,
) {
  var pendingDelete by mutableStateOf<ChatSummary?>(null)
    private set

  fun menu(c: ChatSummary): List<MenuAction> = listOf(
    MenuAction(if (c.pinned) "Unpin" else "Pin this chat", ZoIcons.Pin) { togglePin(c) },
    MenuAction("Share conversation", ZoIcons.Send) { shareChat(c) },
    MenuAction("Delete this chat", ZoIcons.Trash, danger = true) { pendingDelete = c },
  )

  fun togglePin(c: ChatSummary) {
    scope.launch { chats.pin(c.id, !c.pinned)?.let(toast) }
  }

  fun shareChat(c: ChatSummary) {
    scope.launch {
      chats.transcript(c.id).fold({ share(PagesFormat.title(c), it) }, { toast(it.sentence()) })
    }
  }

  fun dismissDelete() { pendingDelete = null }

  fun confirmDelete() {
    val c = pendingDelete ?: return
    pendingDelete = null
    scope.launch {
      val err = chats.delete(c.id)
      if (err != null) toast(err) else if (c.id == activeChatId) goHome()
    }
  }
}

@Composable
fun rememberChatActions(hub: PagesHub, activeChatId: String?): ChatActions {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val scope = rememberCoroutineScope()
  val context = LocalContext.current
  val toast = rememberToastLambda()
  return remember(hub, activeChatId, nav, scope) {
    ChatActions(hub.chats, scope, activeChatId, goHome = { nav.home() }, toast = toast, share = { title, text ->
      val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_SUBJECT, title).putExtra(Intent.EXTRA_TEXT, text)
      runCatching { context.startActivity(Intent.createChooser(send, "Share conversation").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
    })
  }
}

/** The delete confirm of a chat (LIST-09): "Delete this chat?" */
@Composable
fun ChatActions.DeleteConfirm() {
  pendingDelete?.let {
    ConfirmSheet(PagesFormat.DELETE_CHAT_TITLE, PagesFormat.DELETE_CHAT_TEXT, "Delete", onConfirm = ::confirmDelete, onDismiss = ::dismissDelete)
  }
}

@Composable
fun rememberToastLambda(): (String) -> Unit {
  val container = LocalAppContainer.current
  return org.zohara.chat.core.design.rememberToast(container.toast)
}
