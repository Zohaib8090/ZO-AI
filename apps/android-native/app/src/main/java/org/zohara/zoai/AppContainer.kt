package org.zohara.zoai

import android.app.Application
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.preferencesDataStoreFile
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import org.zohara.zoai.core.auth.AuthRepository
import org.zohara.zoai.core.design.ToastState
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.net.Profile
import org.zohara.zoai.core.net.ProfilesResp
import org.zohara.zoai.core.net.ProvidersResp
import org.zohara.zoai.core.net.ZoApi
import org.zohara.zoai.core.net.ZoHttp
import org.zohara.zoai.core.session.Session
import org.zohara.zoai.core.store.TtlCache
import org.zohara.zoai.core.store.ZoPrefs

/**
 * The app's objects, made once (no DI framework). Screens read it through [LocalAppContainer]:
 * `val c = LocalAppContainer.current; c.api.chats()`.
 */
class AppContainer(val app: Application) {
  val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

  private val dataStore: DataStore<Preferences> by lazy { PreferenceDataStoreFactory.create(scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)) { app.preferencesDataStoreFile("zo_prefs") } }
  val prefs: ZoPrefs by lazy { ZoPrefs(dataStore, BuildConfig.DEFAULT_SERVER_URL) }

  val auth = AuthRepository(app, BuildConfig.FIREBASE_CONFIGURED, BuildConfig.GOOGLE_WEB_CLIENT_ID)

  /** the server address in use (Settings > Advanced can change it); read by the API client on every call */
  @Volatile var serverUrl: String = BuildConfig.DEFAULT_SERVER_URL
    private set

  val http = ZoHttp.newClient(auth, localTestMode = { BuildConfig.DEBUG && serverUrl.startsWith("http://") })
  val api = ZoApi(http, { serverUrl })

  /** 30 s caches (core.store): providers, profiles, models */
  val providersCache = TtlCache<String, ProvidersResp>()
  val profilesCache = TtlCache<String, ProfilesResp>()
  val modelsCache = TtlCache<String, ModelsResp>()

  val session = Session(auth, api, prefs, scope, listOf(providersCache, profilesCache, modelsCache))
  val toast = ToastState()

  /** the number of running items (Activity), set by the pages module, shown as the avatar badge (NAV-15) */
  val runningCount = MutableStateFlow(0)

  init {
    scope.launch { prefs.serverUrl.collect { if (ZoHttp.isAcceptableServer(it, BuildConfig.DEBUG)) serverUrl = ZoHttp.normalizeServer(it) } }
  }

  /** Start background parts: the Firebase session. */
  fun start() {
    auth.start()
    scope.launch { auth.state.collect { session.onAuthState(it) } }
  }
}

val LocalAppContainer = staticCompositionLocalOf<AppContainer> { error("no AppContainer") }
