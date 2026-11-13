package org.zohara.agent.link

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import org.zohara.agent.core.net.LinkDevice
import org.zohara.agent.core.net.SendRequest
import org.zohara.agent.core.store.RunOn
import org.zohara.agent.core.store.ZoPrefs

/** Where messages run: the cloud, or one linked computer ([device] is its id, [name] its last known name). */
data class RunOnChoice(val device: String? = null, val name: String = "") {
  val isCloud: Boolean get() = device == null
  companion object { val Cloud = RunOnChoice() }
}

/** Where the choice is kept (LINK-02): the id in the shared preferences, the name beside it. */
interface RunOnStore {
  suspend fun load(): RunOnChoice
  suspend fun save(choice: RunOnChoice)
}

class PrefsRunOnStore(private val prefs: ZoPrefs, context: Context) : RunOnStore {
  private val names = context.applicationContext.getSharedPreferences("zo_link", Context.MODE_PRIVATE)
  override suspend fun load(): RunOnChoice {
    val id = prefs.runOn.first().device ?: return RunOnChoice.Cloud
    return RunOnChoice(id, names.getString("name:$id", "").orEmpty().take(60))
  }
  override suspend fun save(choice: RunOnChoice) {
    if (choice.device != null) names.edit().putString("name:${choice.device}", choice.name.take(60)).apply()
    prefs.setRunOn(RunOn(choice.device))
  }
}

/** The result of the look before sending to a computer (COMP-20). */
sealed interface SendCheck {
  data object Ok : SendCheck
  /** do not send; [message] is the toast, the text stays in the box */
  data class Stop(val message: String) : SendCheck
}

/**
 * Run on (LINK-01 to LINK-03, COMP-10, COMP-20): the choice, the link state, and the rules between them.
 * A computer that is no longer linked sends the choice back to the cloud with a toast; choosing an offline one warns.
 */
class RunOnController(
  val repo: LinkRepository,
  private val store: RunOnStore,
  private val scope: CoroutineScope,
  private val toast: (String) -> Unit,
) {
  private val _choice = MutableStateFlow(RunOnChoice.Cloud)
  val choice: StateFlow<RunOnChoice> = _choice.asStateFlow()
  val link: StateFlow<LinkState> get() = repo.state

  init { scope.launch { _choice.value = store.load() } }

  /** Picks the cloud ([device] = null) or a computer. Choosing one that is offline works but warns. */
  fun choose(device: LinkDevice?) {
    val c = if (device == null) RunOnChoice.Cloud else RunOnChoice(device.id, LinkFormat.name(device))
    _choice.value = c
    scope.launch { store.save(c) }
    if (device != null && !device.online) toast(LinkText.NOT_REACHABLE_NOW)
  }

  private fun toCloud() { _choice.value = RunOnChoice.Cloud; scope.launch { store.save(RunOnChoice.Cloud) } }

  /** `linkRefresh`: checks the computers, then applies the rules of the choice. True when the check worked. */
  suspend fun refresh(): Boolean {
    if (_choice.value.device == null && !repo.state.value.busy) { /* still check: the sheet and My computers need the list */ }
    val ok = repo.refresh()
    val c = _choice.value
    if (ok && c.device != null) {
      val d = repo.state.value.device(c.device)
      if (d == null) { toCloud(); toast(LinkText.VANISHED) }
      else if (LinkFormat.name(d) != c.name) { val n = RunOnChoice(d.id, LinkFormat.name(d)); _choice.value = n; scope.launch { store.save(n) } }
    }
    return ok
  }

  /** Before a send to a computer: look first, so an offline computer keeps the text in the box (COMP-20). The cloud needs no look. */
  suspend fun ensureReachable(): SendCheck {
    val c = _choice.value
    if (c.device == null) return SendCheck.Ok
    val ok = refresh()
    val now = _choice.value
    if (now.device == null) return SendCheck.Stop(LinkText.BACK_TO_CLOUD)
    if (!ok) return SendCheck.Stop(LinkText.COULD_NOT_CHECK_SEND)
    if (repo.state.value.device(now.device)?.online != true) return SendCheck.Stop(LinkText.NOT_REACHABLE)
    return SendCheck.Ok
  }

  /** the account was switched or deleted: forget the choice */
  fun reset() { _choice.value = RunOnChoice.Cloud; repo.clear() }
}
