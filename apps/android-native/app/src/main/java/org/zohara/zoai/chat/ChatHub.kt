package org.zohara.zoai.chat

import kotlinx.coroutines.launch
import org.zohara.zoai.AppContainer
import org.zohara.zoai.core.auth.AuthState

/**
 * All chat sessions of the app. [home] is the session of the home screen (a new chat); when it gets a chat id [promote] moves it to the
 * list and makes a new empty home. [forId] gives the session of an opened chat (made and loaded on first use). Idle sessions are dropped
 * beyond [max]; one that is answering is never dropped.
 */
class ChatHub(private val make: () -> ChatSession, private val max: Int = 8) {
  private val sessions = LinkedHashMap<String, ChatSession>(16, 0.75f, true)
  private var home: ChatSession = make()

  @Synchronized fun home(): ChatSession = home

  /** The home session started a chat: it now lives under its id. Returns that id's session. */
  @Synchronized fun promote(): ChatSession {
    val s = home
    val id = s.chatId
    if (id != null) { sessions[id] = s; home = make(); trim() }
    return s
  }

  @Synchronized fun forId(id: String): ChatSession {
    sessions[id]?.let { return it }
    if (home.chatId == id) return promote()
    val s = make()
    s.open(id)
    sessions[id] = s
    trim()
    return s
  }

  @Synchronized fun remove(id: String) { sessions.remove(id) }

  @Synchronized fun clear() { sessions.clear(); home = make() }

  private fun trim() {
    if (sessions.size <= max) return
    val it = sessions.entries.iterator()
    while (it.hasNext() && sessions.size > max) {
      val e = it.next()
      if (!e.value.state.value.busy) it.remove()
    }
  }

  companion object {
    @Volatile private var instance: Instance? = null

    /** the app's hub, made once from the container */
    fun of(container: AppContainer): Instance = instance ?: synchronized(this) {
      instance ?: Instance(container).also { instance = it }
    }
  }

  /** The hub plus the shared pill state, bound to the real container. */
  class Instance(container: AppContainer) {
    val backend = ZoChatBackend(container.api)
    val env = AppChatEnv(container)
    val hub = ChatHub({ ChatSession(backend, container.scope, env) })
    val pills = PillsState(backend, container.scope, env)

    init {
      container.scope.launch { container.auth.state.collect { if (it !is AuthState.SignedIn) hub.clear() } }
    }
  }
}
