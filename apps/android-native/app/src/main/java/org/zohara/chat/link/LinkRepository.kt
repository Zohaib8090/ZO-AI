package org.zohara.chat.link

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.LinkDevice

/** What is known about the linked computers (the web's `links`). [ok] is true when the last check worked; [at] is when (0 = never). */
data class LinkState(val devices: List<LinkDevice> = emptyList(), val ok: Boolean = false, val at: Long = 0, val busy: Boolean = false) {
  fun device(id: String?): LinkDevice? = if (id == null) null else devices.firstOrNull { it.id == id }
}

/**
 * `POST /api/link/status` (LINK-03): at most 30 computers, ids as text, online only when the server says `online: true`. One check at a time.
 * The screens call [refresh] every 20 s while visible, on resume, and from "Check again".
 */
class LinkRepository(private val fetch: suspend () -> List<LinkDevice>, private val clock: () -> Long = System::currentTimeMillis) {
  private val _state = MutableStateFlow(LinkState())
  val state: StateFlow<LinkState> = _state.asStateFlow()

  /** true when the check worked */
  suspend fun refresh(): Boolean {
    if (_state.value.busy) return _state.value.ok
    _state.update { it.copy(busy = true) }
    return try {
      val list = fetch().filter { it.id.isNotEmpty() }.take(30)
      _state.value = LinkState(list, ok = true, at = clock(), busy = false)
      true
    } catch (e: ApiException) {
      _state.update { it.copy(ok = false, busy = false) }
      false
    } catch (e: kotlinx.coroutines.CancellationException) {
      _state.update { it.copy(busy = false) }
      throw e
    } catch (e: Exception) {
      _state.update { it.copy(ok = false, busy = false) }
      false
    }
  }

  fun clear() { _state.value = LinkState() }
}

/** Runs [block] now and then every [intervalMs] until cancelled (the 20 s presence poll, LINK-03). */
suspend fun pollEvery(intervalMs: Long, block: suspend () -> Unit) {
  while (true) {
    block()
    kotlinx.coroutines.delay(intervalMs)
  }
}

const val LINK_POLL_MS = 20_000L
