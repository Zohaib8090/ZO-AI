package org.zohara.agent.chat

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.auth.AuthState
import org.zohara.agent.nav.LocalNav

/** The home view: greeting over the composer (a new chat). Parity rows: NAV-18, COMP-*, PERM-*, CHAT-*. */
@Composable
fun HomeScreen() {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val hub = remember { ChatHub.of(container) }
  var session by remember { mutableStateOf(hub.hub.home()) }
  val state by session.state.collectAsState()
  val auth by container.auth.state.collectAsState()
  val user = (auth as? AuthState.SignedIn)?.user
  val context = LocalContext.current
  val greeting = remember(user, state.items.isEmpty()) { HomeGreeting.text(context, user?.displayName.orEmpty(), user?.email.orEmpty()) }
  // the server named the chat: it lives under its id now, and the screen opens it; the next home is empty (CHAT-12, CHAT-40)
  LaunchedEffect(state.chatId) {
    val id = state.chatId
    if (id != null) {
      hub.hub.promote()
      session = hub.hub.home()
      nav.openChat(id)
    }
  }
  ChatPane(session, hub, greeting, Modifier)
}
