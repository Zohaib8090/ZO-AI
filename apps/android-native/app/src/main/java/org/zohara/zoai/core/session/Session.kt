package org.zohara.zoai.core.session

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.zohara.zoai.core.auth.AuthRepository
import org.zohara.zoai.core.auth.AuthState
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.net.Whoami
import org.zohara.zoai.core.net.ZoApi
import org.zohara.zoai.core.store.TtlCache
import org.zohara.zoai.core.store.ZoPrefs

/** What the person sees once signed in (AUTH-16..20). */
sealed interface Gate {
  /** `whoami` is being read */
  data object Loading : Gate
  /** the terms are not accepted: show "Before you start" for [version] */
  data class Consent(val version: String, val error: String? = null) : Gate
  /** the app is open. [account] is null when `whoami` failed (non-403): the app then behaves as restricted until known. */
  data class Ready(val account: Whoami?) : Gate
}

/** The top banner of NET-03. */
enum class NetBanner { Offline, SignedOut }

/**
 * Who is signed in and what that allows. One per app. Reacts to the global errors of the API client:
 * terms_required opens the consent screen, 401 shows "Signed out or not allowed.", blocked and deleted accounts are signed out with the message.
 */
class Session(
  private val auth: AuthRepository,
  private val api: ZoApi,
  private val prefs: ZoPrefs,
  private val scope: CoroutineScope,
  /** forget every cache that belongs to the person */
  private val caches: List<TtlCache<*, *>> = emptyList(),
) {
  private val _gate = MutableStateFlow<Gate>(Gate.Loading)
  val gate: StateFlow<Gate> = _gate.asStateFlow()

  private val _banner = MutableStateFlow<NetBanner?>(null)
  val banner: StateFlow<NetBanner?> = _banner.asStateFlow()

  /** a sentence to show on the login screen after a refusal (blocked, deleted) */
  private val _loginMessage = MutableStateFlow<String?>(null)
  val loginMessage: StateFlow<String?> = _loginMessage.asStateFlow()

  private var loadJob: Job? = null
  private var lastUid: String? = null

  init {
    api.errorListener = ::onApiError
    api.successListener = { if (_banner.value == NetBanner.Offline) _banner.value = null }
  }

  /** Call when the auth state changes. */
  fun onAuthState(state: AuthState) {
    when (state) {
      is AuthState.SignedIn -> if (lastUid != state.user.uid || _gate.value is Gate.Loading) { lastUid = state.user.uid; load() }
      else -> {
        lastUid = null
        loadJob?.cancel()
        _gate.value = Gate.Loading
        _banner.value = null
        caches.forEach { it.clear() }
      }
    }
  }

  /** AUTH-19: `whoami` first, consent when the terms are not accepted, else the app. */
  fun load() {
    loadJob?.cancel()
    _gate.value = Gate.Loading
    loadJob = scope.launch {
      try {
        val me = api.whoami()
        _gate.value = if (!me.terms.accepted && me.terms.version.isNotBlank()) Gate.Consent(me.terms.version) else Gate.Ready(me)
      } catch (e: ApiException) {
        // the error listener already handled terms, 401, blocked and deleted; anything else still opens the app (restricted until known)
        if (_gate.value is Gate.Loading) _gate.value = Gate.Ready(null)
      }
    }
  }

  /** AUTH-17: accept the terms of [version], then continue. */
  fun acceptTerms(version: String, onError: (String) -> Unit = {}) {
    scope.launch {
      try {
        api.acceptTerms(version)
        prefs.setConsentVersion(version)
        load()
      } catch (e: ApiException) {
        onError(e.text)
      }
    }
  }

  val account: Whoami? get() = (_gate.value as? Gate.Ready)?.account
  /** until `whoami` is known the app behaves as restricted (AUTH-20) */
  val restricted: Boolean get() = account?.restricted ?: true
  val admin: Boolean get() = account?.admin ?: false

  fun signOut(message: String? = null) {
    scope.launch {
      _loginMessage.value = message
      prefs.clearAccountData()
      auth.signOut()
    }
  }

  fun clearLoginMessage() { _loginMessage.value = null }
  fun dismissBanner() { _banner.value = null }

  private fun onApiError(e: ApiException) {
    when (e.kind) {
      ErrorKind.Offline -> _banner.value = NetBanner.Offline
      ErrorKind.Unauthorized -> _banner.value = NetBanner.SignedOut
      ErrorKind.TermsRequired -> _gate.update { Gate.Consent(e.termsVersion ?: (it as? Gate.Consent)?.version ?: "") }
      ErrorKind.Blocked, ErrorKind.Deleted -> signOut(e.text)
      else -> Unit
    }
  }
}
