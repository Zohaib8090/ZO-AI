package org.zohara.chat.chat

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.Profile

/** The three modes of PERM-01 with the desktop's words. */
enum class PermMode(val api: String, val label: String, val text: String) {
  Manual("manual", "Manual", "Zohara Chat asks before every change and every command."),
  Auto("auto", "Auto", "Looking around runs on its own; changes and other commands ask first."),
  Bypass("bypass", "Bypass", "Nothing asks. Use with care.");

  companion object {
    fun parse(s: String?): PermMode = entries.firstOrNull { it.api.equals(s?.trim(), ignoreCase = true) } ?: Manual
  }
}

data class PillsUi(
  val modelText: String = "Choose a model",
  val ready: Boolean = false,
  val model: String = "",
  val provider: String = "",
  val mode: PermMode = PermMode.Manual,
  val profiles: List<Profile> = emptyList(),
  val activeProfile: String = "",
  val loading: Boolean = true,
  val computerOnline: Boolean = true,
  val error: String? = null,
)

/**
 * The model pill and the mode pill (COMP-07..09, PERM-*). One per app, read by the home and the chat composers. With a computer chosen
 * (Run on) the same values come from that computer; when it cannot be reached the pill says so.
 */
class PillsState(private val backend: PillsBackend, private val scope: CoroutineScope, private val env: ChatEnv) {
  private val _ui = MutableStateFlow(PillsUi())
  val ui: StateFlow<PillsUi> = _ui.asStateFlow()
  private var job: Job? = null

  fun refresh() {
    job?.cancel()
    job = scope.launch {
      val device = env.runOn().device
      try {
        if (device == null) {
          val st = backend.status()
          val profiles = try { backend.profiles() } catch (e: ApiException) { null }
          _ui.update {
            PillsUi(
              modelText = Transcript.pillText(st.ready, st.model, st.provider), ready = st.ready, model = st.model, provider = st.provider,
              mode = PermMode.parse(st.mode), profiles = profiles?.profiles.orEmpty(), activeProfile = profiles?.active.orEmpty(), loading = false,
            )
          }
        } else {
          val st = backend.computerStatus(device)
          _ui.update {
            PillsUi(
              modelText = if (st.ready) st.model.ifBlank { st.provider } else "Choose a model", ready = st.ready, model = st.model, provider = st.provider,
              mode = PermMode.parse(st.mode), loading = false,
            )
          }
        }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _ui.update { it.copy(loading = false, error = e.text, modelText = if (device != null) "Computer not reachable" else it.modelText, computerOnline = device == null) }
      }
    }
  }

  /** Picks a pinned model of a profile: switches the profile, then the model. */
  fun chooseModel(profileId: String?, model: String, done: (String?) -> Unit = {}) {
    scope.launch {
      try {
        val device = env.runOn().device
        if (device != null) backend.computerSelect(device, model)
        else {
          if (profileId != null && profileId != _ui.value.activeProfile) backend.profileUse(profileId)
          backend.select(model)
        }
        refresh()
        done(null)
      } catch (e: CancellationException) { throw e } catch (e: ApiException) { done(e.text) }
    }
  }

  fun chooseProfile(profileId: String, done: (String?) -> Unit = {}) {
    scope.launch {
      try { backend.profileUse(profileId); refresh(); done(null) } catch (e: CancellationException) { throw e } catch (e: ApiException) { done(e.text) }
    }
  }

  /** Bypass is refused for restricted accounts and on a computer (PERM-02): the server says so; the sheet hides the row too. */
  fun setMode(mode: PermMode, done: (String?) -> Unit = {}) {
    val before = _ui.value.mode
    _ui.update { it.copy(mode = mode) }
    scope.launch {
      try {
        val device = env.runOn().device
        val saved = if (device != null) backend.computerMode(device, mode.api) else backend.mode(mode.api)
        _ui.update { it.copy(mode = PermMode.parse(saved)) }
        done(null)
      } catch (e: CancellationException) { throw e } catch (e: ApiException) {
        _ui.update { it.copy(mode = before) }
        done(e.text)
      }
    }
  }

  fun bypassAllowed(): Boolean = !env.restricted && env.runOn().isCloud
}
