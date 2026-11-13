package org.zohara.agent.settings

import android.content.SharedPreferences
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.core.net.ErrorKind
import org.zohara.agent.core.net.ModelsResp
import org.zohara.agent.core.net.ProfilesResp
import org.zohara.agent.core.net.ProviderInfo
import org.zohara.agent.core.net.ProvidersResp
import org.zohara.agent.core.net.StatusInfo
import org.zohara.agent.core.net.ZoApi
import org.zohara.agent.core.store.TtlCache

/** Small phone-only choices that have no server home (the fallback of the hidden providers when the server has no `settings_set`). */
class LocalSettings(private val prefs: SharedPreferences) {
  fun hiddenProviders(): List<String> = prefs.getStringSet("hidden_providers", emptySet()).orEmpty().sorted()
  fun setHiddenProviders(list: Collection<String>) { prefs.edit().putStringSet("hidden_providers", list.toSet()).apply() }
}

/**
 * MODEL-29: the providers the person hid. They live in the synced settings document (`settings_get`, `settings_set`); a server that does not
 * have the ops yet answers "unknown request", and then the choice stays on this phone.
 */
class HiddenStore(private val api: ZoApi, private val local: LocalSettings?) {
  /** false once the server said it has no `settings_set`: the choice is only on this phone */
  var onServer: Boolean = true
    private set

  suspend fun get(): List<String> = try {
    val r = api.settingsGet()
    onServer = true
    r.hiddenProviders
  } catch (e: ApiException) {
    if (e.kind != ErrorKind.UnknownOp) throw e
    onServer = false
    local?.hiddenProviders().orEmpty()
  }

  suspend fun set(list: List<String>) {
    if (onServer) {
      try { api.settingsSet(hiddenProviders = list); return } catch (e: ApiException) { if (e.kind != ErrorKind.UnknownOp) throw e; onServer = false }
    }
    local?.setHiddenProviders(list)
  }
}

data class ModelSheetState(
  val loading: Boolean = true,
  val loadError: String? = null,
  val providers: List<ProviderInfo> = emptyList(),
  val hiddenHint: String? = null,
  val providerId: String = "",
  val address: String = "",
  val addressError: String = "",
  val models: List<String> = emptyList(),
  val chosen: String = "",
  val filter: String = "",
  val fetching: Boolean = false,
  val fetchError: String = "",
  val efforts: List<String> = emptyList(),
  val effort: String = "",
  val pins: Set<String> = emptySet(),
  val testing: Boolean = false,
  val testText: String = "",
  val testOk: Boolean? = null,
  val savingKey: Boolean = false,
  val using: Boolean = false,
  val claudeLine: String? = null,
) {
  val provider: ProviderInfo? get() = providers.firstOrNull { it.id == providerId }
  val canUse: Boolean get() = chosen.isNotBlank() && providerId.isNotBlank() && !using
}

/**
 * The logic of the "Model and API key" sheet (MODEL-09..19, 23, 24). One per open sheet; [state] is what the screen shows.
 * Rules are those of the web2 sheet: the selection is live, "Use this model" saves it, the star pins to the active profile.
 */
class ModelSheetController(
  private val api: ZoApi,
  private val scope: CoroutineScope,
  private val restricted: Boolean,
  private val admin: Boolean,
  private val cache: TtlCache<String, ModelsResp>,
  private val hiddenStore: HiddenStore,
  private val toast: (String) -> Unit,
  /** forget the cached providers, profiles and model lists of the app */
  private val invalidate: () -> Unit,
) {
  private val _state = MutableStateFlow(ModelSheetState())
  val state: StateFlow<ModelSheetState> = _state

  private var prov = ProvidersResp()
  private var prof = ProfilesResp()
  private var status = StatusInfo()
  private var seq = 0
  private var dirty = false
  private var fetchJob: Job? = null

  fun start() {
    scope.launch {
      _state.update { it.copy(loading = true, loadError = null) }
      try {
        coroutineScope {
          val a = async { api.providers() }
          val b = async { api.profiles() }
          val c = async { api.status() }
          val d = async { hiddenStore.get() }
          val cc = async { if (admin && !restricted) try { api.claudeStatus() } catch (_: ApiException) { null } else null }
          prov = a.await(); prof = b.await(); status = c.await()
          val hidden = (d.await() + prov.hiddenProviders).distinct()
          val claude = cc.await()
          val list = ModelLogic.sheetProviders(prov.providers, hidden, prov.current, restricted, claude != null && claude.admin && claude.present)
          _state.update {
            it.copy(
              providers = list, providerId = prov.current.takeIf { id -> list.any { p -> p.id == id } } ?: "",
              hiddenHint = ModelLogic.hiddenHint(prov.providers, hidden, prov.current, restricted),
              chosen = status.model, pins = ModelLogic.activePins(prof).toSet(),
              claudeLine = if (claude != null && claude.admin) claudeLine(claude.present, claude.setAt) else null,
            )
          }
        }
        val p = _state.value.provider
        if (p != null) {
          _state.update { it.copy(address = ModelLogic.savedAddress(p, prov.current)) }
          fetchModels()
        }
        _state.update { it.copy(loading = false) }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(loading = false, loadError = e.text) }
      }
    }
  }

  // ---- provider and address

  fun selectProvider(id: String) {
    val p = _state.value.providers.firstOrNull { it.id == id } ?: return
    dirty = false
    seq++
    _state.update {
      it.copy(
        providerId = id, address = ModelLogic.savedAddress(p, prov.current), addressError = "", models = emptyList(),
        chosen = if (id == prov.current) status.model else "", filter = "", fetchError = "", fetching = false,
        efforts = emptyList(), effort = "", testText = "", testOk = null,
      )
    }
    // a custom endpoint needs the address first; every other provider lists its models at once
    if (p.needsUrl && _state.value.address.isBlank()) return
    fetchModels()
  }

  fun setAddress(text: String) { dirty = true; _state.update { it.copy(address = text, addressError = "", testText = "", testOk = null) } }

  /** called when the address field loses focus or the person presses done */
  fun addressDone() {
    val s = _state.value; val p = s.provider ?: return
    val bad = ModelLogic.validateAddress(s.address, p.needsUrl, restricted)
    _state.update { it.copy(addressError = bad) }
    if (bad.isEmpty()) fetchModels()
  }

  fun setFilter(text: String) { _state.update { it.copy(filter = text) } }
  fun choose(model: String) { _state.update { it.copy(chosen = model) } }
  fun setEffort(level: String) { _state.update { it.copy(effort = level) } }

  // ---- model list (MODEL-14, 15, 16)

  fun fetchModels(force: Boolean = false) {
    val s = _state.value
    val p = s.provider
    if (p == null) { _state.update { it.copy(models = emptyList()) }; return }
    val bad = ModelLogic.validateAddress(s.address, p.needsUrl, restricted)
    if (bad.isNotEmpty()) { _state.update { it.copy(models = emptyList(), fetchError = bad, fetching = false) }; return }
    val my = ++seq
    fetchJob?.cancel()
    _state.update { it.copy(fetching = true, fetchError = "") }
    val addr = s.address.trim()
    fetchJob = scope.launch {
      try {
        val r = cache.getOrLoad(ModelLogic.cacheKey(p.id, addr), force) { api.models(p.id, addr.ifBlank { null }) }
        if (my != seq) return@launch
        _state.update {
          val models = r.models
          val sameProvider = p.id == prov.current
          val chosen = when {
            it.chosen in models -> it.chosen
            it.chosen.isNotEmpty() && models.isEmpty() && sameProvider -> it.chosen
            sameProvider && r.current in models -> r.current
            else -> ""
          }
          it.copy(
            fetching = false, models = models, fetchError = ModelLogic.listMessage(r), chosen = chosen,
            address = if (r.baseUrl.isNotBlank() && !dirty && it.address.isBlank() && p.needsUrl) r.baseUrl else it.address,
            efforts = r.efforts, effort = r.effort.orEmpty(),
          )
        }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        if (my != seq) return@launch
        _state.update { it.copy(fetching = false, models = emptyList(), fetchError = e.text) }
      }
    }
  }

  // ---- key

  fun saveKey(key: String, onSaved: () -> Unit = {}) {
    val p = _state.value.provider ?: return
    val k = key.trim()
    if (k.isEmpty()) { toast("Paste the key first."); return }
    _state.update { it.copy(savingKey = true) }
    scope.launch {
      try {
        api.setKey(p.id, k)
        prov = prov.copy(providers = prov.providers.map { x -> if (x.id == p.id) x.copy(hasKey = true) else x })
        _state.update { s -> s.copy(savingKey = false, providers = s.providers.map { x -> if (x.id == p.id) x.copy(hasKey = true) else x }) }
        invalidate()
        toast("Key saved")
        onSaved()
        fetchModels(force = true)
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(savingKey = false) }
        toast(e.text)
      }
    }
  }

  // ---- test connection (MODEL-13)

  fun testConnection() {
    val s = _state.value; val p = s.provider ?: return
    val bad = ModelLogic.validateAddress(s.address, p.needsUrl, restricted)
    if (bad.isNotEmpty()) { _state.update { it.copy(testOk = false, testText = bad) }; return }
    _state.update { it.copy(testing = true, testOk = null, testText = "Testing…") }
    scope.launch {
      try {
        val r = api.modelTest(p.id, s.chosen.ifBlank { null }, s.address.trim().ifBlank { null })
        _state.update { it.copy(testing = false, testOk = r.works, testText = r.message.ifBlank { if (r.works) "Connected." else "The test failed." }) }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(testing = false, testOk = false, testText = if (e.kind == ErrorKind.UnknownOp) "This server cannot test connections yet." else e.text) }
      }
    }
  }

  // ---- pins (MODEL-19)

  fun togglePin(model: String) {
    val s = _state.value; val p = s.provider ?: return
    val on = model !in s.pins
    scope.launch {
      try {
        val active = prof.profiles.firstOrNull { it.id == prof.active }
        var id = prof.active
        var note = if (on) "Pinned" else "Unpinned"
        if (on && (active == null || active.provider != p.id)) {
          api.select(provider = p.id, model = model, baseUrl = s.address.trim())
          id = api.profileCreate(p.name.ifBlank { p.id })
          note = "Pinned to a new profile, " + p.name
        }
        api.profilePin(id, model, on)
        invalidate()
        prof = api.profiles()
        _state.update { it.copy(pins = ModelLogic.activePins(prof).toSet()) }
        toast(note)
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        toast(e.text)
      }
    }
  }

  // ---- use (MODEL-23)

  /** Saves the choice. [onUsed] runs when it worked (the sheet closes). */
  fun use(onUsed: () -> Unit) {
    val s = _state.value; val p = s.provider ?: return
    if (s.chosen.isBlank()) return
    val bad = ModelLogic.validateAddress(s.address, p.needsUrl, restricted)
    if (bad.isNotEmpty()) { _state.update { it.copy(addressError = bad) }; toast(bad); return }
    _state.update { it.copy(using = true) }
    scope.launch {
      try {
        api.select(provider = p.id, model = s.chosen, effort = if (s.efforts.isNotEmpty()) s.effort else null, baseUrl = s.address.trim())
        invalidate()
        _state.update { it.copy(using = false) }
        toast("Using " + s.chosen)
        onUsed()
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(using = false) }
        toast(e.text)
      }
    }
  }
}

/** The sheet "Add a custom endpoint" (MODEL-25): Name, Address, Key, Model, Test connection, Add and use. */
class AddEndpointController(
  private val api: ZoApi,
  private val scope: CoroutineScope,
  private val restricted: Boolean,
  private val toast: (String) -> Unit,
  private val invalidate: () -> Unit,
) {
  data class State(val error: String = "", val testText: String = "", val testOk: Boolean? = null, val busy: Boolean = false, val testing: Boolean = false)

  private val _state = MutableStateFlow(State())
  val state: StateFlow<State> = _state

  /** Makes the endpoint the selected one, with its key: the server can only test what it has saved. */
  private suspend fun apply(address: String, key: String, model: String) {
    api.select(provider = "custom", model = model.ifBlank { null }, baseUrl = address.trim())
    if (key.isNotBlank()) api.setKey("custom", key.trim())
  }

  fun test(address: String, key: String, model: String) {
    val bad = ModelLogic.validateNewEndpoint(address, restricted)
    if (bad.isNotEmpty()) { _state.update { it.copy(error = bad) }; return }
    _state.update { it.copy(error = "", testing = true, testOk = null, testText = "Testing…") }
    scope.launch {
      try {
        apply(address, key, model)
        val r = api.modelTest("custom", model.ifBlank { null }, address.trim())
        _state.update { it.copy(testing = false, testOk = r.works, testText = r.message) }
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(testing = false, testOk = false, testText = if (e.kind == ErrorKind.UnknownOp) "This server cannot test connections yet." else e.text) }
      }
    }
  }

  fun add(name: String, address: String, key: String, model: String, onDone: () -> Unit) {
    val bad = ModelLogic.validateNewEndpoint(address, restricted)
    if (bad.isNotEmpty()) { _state.update { it.copy(error = bad) }; return }
    if (name.isBlank()) { _state.update { it.copy(error = "Give it a name first.") }; return }
    _state.update { it.copy(error = "", busy = true) }
    scope.launch {
      try {
        apply(address, key, model)
        api.profileCreate(name.trim())
        invalidate()
        _state.update { it.copy(busy = false) }
        toast("Added " + name.trim())
        onDone()
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.update { it.copy(busy = false, error = e.text) }
      }
    }
  }
}
