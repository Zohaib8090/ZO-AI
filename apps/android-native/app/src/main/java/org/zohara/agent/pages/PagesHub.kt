package org.zohara.agent.pages

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.zohara.agent.AppContainer
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.auth.AuthState

/**
 * What the pages share between screens: the chat list (Chats page, Search recents, the sidebar) and the activity list (Activity page,
 * the user menu sheet, the avatar badge). One per signed-in person: a new person gets a new hub, so a list never leaks to the next account.
 */
class PagesHub(val api: PagesApi, val scope: CoroutineScope, onRunning: (Int) -> Unit) {
  val chats = ChatsHolder(api, scope)
  val activity = ActivityHolder(api, scope, onCount = onRunning)
}

/** Keeps the hub of the current person; a different [uid] (or none) drops the old one. */
class PagesHubRegistry(private val factory: (CoroutineScope) -> PagesHub) {
  private var uid: String? = null
  private var hub: PagesHub? = null
  private var scope: CoroutineScope? = null

  @Synchronized
  fun get(forUid: String): PagesHub {
    val cur = hub
    if (cur != null && uid == forUid) return cur
    scope?.cancel()
    val s = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    scope = s
    uid = forUid
    return factory(s).also { hub = it }
  }

  @Synchronized
  fun drop() { scope?.cancel(); scope = null; hub = null; uid = null }
}

private val registries = java.util.WeakHashMap<AppContainer, PagesHubRegistry>()

/** The hub of the signed-in person (the pages only exist while somebody is signed in). */
fun hubOf(container: AppContainer): PagesHub {
  val reg = synchronized(registries) {
    registries.getOrPut(container) {
      PagesHubRegistry(
        factory = { s -> PagesHub(ZoPagesApi(container.api), s) { n -> container.runningCount.value = n } },
      )
    }
  }
  val uid = (container.auth.state.value as? AuthState.SignedIn)?.user?.uid ?: "anonymous"
  return reg.get(uid)
}

@Composable
fun rememberPagesHub(): PagesHub {
  val c = LocalAppContainer.current
  val auth by c.auth.state.collectAsState()
  val uid = (auth as? AuthState.SignedIn)?.user?.uid
  return remember(c, uid) { hubOf(c) }
}
