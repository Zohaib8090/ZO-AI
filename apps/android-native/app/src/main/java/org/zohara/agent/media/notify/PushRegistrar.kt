package org.zohara.agent.media.notify

import android.content.Context
import android.os.Build
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.tasks.await
import org.zohara.agent.BuildConfig
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.core.net.PushConfig
import org.zohara.agent.core.net.PushSubscription
import org.zohara.agent.core.net.ZoApi

/** The four push calls of the account API (NOTIFICATIONS.md): `push_config`, `push_subscribe {kind:"fcm"}`, `push_unsubscribe`, `push_list`. */
interface PushBackend {
  suspend fun config(): PushConfig
  suspend fun subscribe(token: String, label: String, showTitle: Boolean): String
  suspend fun unsubscribe(id: String)
  suspend fun list(): List<PushSubscription>
}

class ApiPushBackend(private val api: ZoApi) : PushBackend {
  override suspend fun config() = api.pushConfig()
  override suspend fun subscribe(token: String, label: String, showTitle: Boolean) = api.pushSubscribe(token, label, showTitle)
  override suspend fun unsubscribe(id: String) = api.pushUnsubscribe(id)
  override suspend fun list() = api.pushList()
}

/** The phone's FCM token. */
interface TokenSource {
  suspend fun token(): String?
  /** forgets the token so the next [token] is a new one: nothing that was registered with the old one can reach this phone */
  suspend fun deleteToken()
}

class FirebaseTokenSource : TokenSource {
  override suspend fun token(): String? = if (!BuildConfig.FIREBASE_CONFIGURED) null else try { FirebaseMessaging.getInstance().token.await() } catch (_: Exception) { null }
  override suspend fun deleteToken() { if (BuildConfig.FIREBASE_CONFIGURED) runCatching { FirebaseMessaging.getInstance().deleteToken().await() } }
}

/** What this phone remembers about its push registration (not secret; the token is only useful with the server's key). */
interface PushStore {
  /** the switch "Also notify me when the app is closed" */
  var enabled: Boolean
  /** the registration id the server gave; "" when not registered */
  var id: String
  /** whose registration it is (Firebase uid) */
  var uid: String
  var token: String
  var showTitle: Boolean
  fun clearRegistration() { id = ""; uid = ""; token = ""; showTitle = false }
}

class MemoryPushStore : PushStore {
  override var enabled = false
  override var id = ""
  override var uid = ""
  override var token = ""
  override var showTitle = false
}

class PrefsPushStore(context: Context) : PushStore {
  private val p = context.applicationContext.getSharedPreferences("zo_push", Context.MODE_PRIVATE)
  override var enabled: Boolean
    get() = p.getBoolean("enabled", false)
    set(v) { p.edit().putBoolean("enabled", v).apply() }
  override var id: String
    get() = p.getString("id", "").orEmpty()
    set(v) { p.edit().putString("id", v).apply() }
  override var uid: String
    get() = p.getString("uid", "").orEmpty()
    set(v) { p.edit().putString("uid", v).apply() }
  override var token: String
    get() = p.getString("token", "").orEmpty()
    set(v) { p.edit().putString("token", v).apply() }
  override var showTitle: Boolean
    get() = p.getBoolean("show_title", false)
    set(v) { p.edit().putBoolean("show_title", v).apply() }
}

/** What happened to the registration, for the Settings page. */
sealed interface PushResult {
  data object Off : PushResult
  data object NeedsPermission : PushResult
  /** `push_config.fcm` is false: this server cannot send to the app */
  data object ServerCannot : PushResult
  /** no token (no Firebase in this build, or no Google services on the phone) */
  data object NoToken : PushResult
  data class Registered(val id: String) : PushResult
  data class Failed(val message: String) : PushResult
}

/**
 * Registers this phone for push (NOTIF-03, NOTIF-15, NOTIF-16, NOTIF-17, BG-05):
 * after sign-in, when notifications are allowed and the server can send to the app, `push_subscribe {kind:"fcm", token, label, show_title}`;
 * at every start it checks the registration is still on the server (`push_list`) and registers again if not; a registration of another person on this phone is dropped
 * and the token renewed; a new token replaces the old one; sign-out and the switch off remove it (`push_unsubscribe`).
 */
class PushRegistrar(
  private val backend: PushBackend,
  private val tokens: TokenSource,
  private val store: PushStore,
  private val permission: () -> Boolean,
  private val label: String = deviceLabel(),
) {
  private val lock = Mutex()
  private val _registered = MutableStateFlow(store.enabled && store.id.isNotEmpty())
  /** true while this phone is registered: the chat module sends `keep_running` then (NOTIF-05) */
  val registered: StateFlow<Boolean> = _registered.asStateFlow()

  private fun publish() { _registered.value = store.enabled && store.id.isNotEmpty() }

  suspend fun sync(uid: String, showTitle: Boolean): PushResult = lock.withLock {
    try {
      if (!store.enabled) {
        if (store.id.isNotEmpty()) removeLocked(deleteToken = false)
        return@withLock PushResult.Off
      }
      if (!permission()) return@withLock PushResult.NeedsPermission
      val cfg = try { backend.config() } catch (e: ApiException) { return@withLock PushResult.Failed(e.text) }
      if (!cfg.fcm) return@withLock PushResult.ServerCannot
      if (store.uid.isNotEmpty() && store.uid != uid) { store.clearRegistration(); tokens.deleteToken() } // another person's registration: dropped, token renewed
      val token = tokens.token() ?: return@withLock PushResult.NoToken
      val known = try { backend.list() } catch (e: ApiException) { return@withLock PushResult.Failed(e.text) }
      val still = store.id.isNotEmpty() && known.any { it.id == store.id }
      if (still && store.token == token && store.showTitle == showTitle && store.uid == uid) return@withLock PushResult.Registered(store.id)
      val id = try { backend.subscribe(token, label, showTitle) } catch (e: ApiException) { return@withLock PushResult.Failed(if (e.status == 409) "This account already has 5 devices registered. Remove one in Settings > Notifications." else e.text) }
      store.id = id; store.uid = uid; store.token = token; store.showTitle = showTitle
      PushResult.Registered(id)
    } finally { publish() }
  }

  /** FCM made a new token (BG-05): the old registration goes, the new one comes. */
  suspend fun onNewToken(uid: String?, token: String, showTitle: Boolean): PushResult = lock.withLock {
    try {
      if (!store.enabled || uid == null || !permission()) { store.token = ""; return@withLock PushResult.Off }
      val old = store.id
      if (old.isNotEmpty()) runCatching { backend.unsubscribe(old) }
      val id = try { backend.subscribe(token, label, showTitle) } catch (e: ApiException) { store.clearRegistration(); return@withLock PushResult.Failed(e.text) }
      store.id = id; store.uid = uid; store.token = token; store.showTitle = showTitle
      PushResult.Registered(id)
    } finally { publish() }
  }

  /** Sign-out, the switch off: the server forgets this phone (NOTIF-17). [deleteToken] also renews the token. */
  suspend fun unregister(deleteToken: Boolean = false) = lock.withLock { removeLocked(deleteToken); publish() }

  private suspend fun removeLocked(deleteToken: Boolean) {
    val id = store.id
    if (id.isNotEmpty()) runCatching { backend.unsubscribe(id) }
    store.clearRegistration()
    if (deleteToken) tokens.deleteToken()
  }

  /** Settings > Notifications device list */
  suspend fun devices(): List<PushSubscription> = backend.list()

  /** Remove one device of the list; removing this phone also switches the push off. */
  suspend fun removeDevice(id: String) = lock.withLock {
    if (id == store.id) { removeLocked(false); store.enabled = false } else backend.unsubscribe(id)
    publish()
  }

  suspend fun setEnabled(on: Boolean) = lock.withLock { store.enabled = on; publish() }
  val enabled: Boolean get() = store.enabled
  val registrationId: String get() = store.id

  companion object {
    /** NOTIF-16: "Android: <model>", at most 60 characters */
    fun deviceLabel(model: String = Build.MODEL.orEmpty()): String = ("Android: " + model.filter { !it.isISOControl() }.trim()).trim().trimEnd(':').take(60)
  }
}
