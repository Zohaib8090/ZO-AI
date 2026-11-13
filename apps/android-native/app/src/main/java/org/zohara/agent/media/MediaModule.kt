package org.zohara.agent.media

import android.content.Context
import java.io.File
import java.util.WeakHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.zohara.agent.AppContainer
import org.zohara.agent.core.auth.AuthState
import org.zohara.agent.core.session.Gate
import org.zohara.agent.media.notify.ApiPushBackend
import org.zohara.agent.media.notify.AppVisibility
import org.zohara.agent.media.notify.FirebaseTokenSource
import org.zohara.agent.media.notify.NotificationPermission
import org.zohara.agent.media.notify.NotifyChoices
import org.zohara.agent.media.notify.NotifySettings
import org.zohara.agent.media.notify.PrefsPushStore
import org.zohara.agent.media.notify.PushRegistrar
import org.zohara.agent.media.notify.ZoNotifier

/**
 * The media part of the app, made once per [AppContainer] (`MediaModule.of(container)`): the attachments being written, the notifier, the push registration.
 * [install] is called once from `ZoApplication.onCreate`. The chat module uses [attachments], [notifier] and [keepRunning]; the settings module uses [push], [choices], [notifier].
 * See docs/MEDIA-LINK.md.
 */
class MediaModule private constructor(val container: AppContainer) {
  private val app = container.app
  private val scope = container.scope

  val attachments: AttachmentsController by lazy {
    AttachmentsController(
      scope = scope,
      io = Dispatchers.IO,
      tempDir = File(app.cacheDir, "attachments"),
      pdfOpener = { f -> PdfBoxSource.open(app, f) },
      linkTitle = { url -> container.api.linkPreview(url).title },
    )
  }

  // ---- voice settings as values (Settings > Appearance and permissions: VOICE-03, VOICE-04, VOICE-11) ----
  val voiceLanguage: StateFlow<String> by lazy { container.prefs.voiceLanguage.stateIn(scope, SharingStarted.Eagerly, "") }
  val voiceAutoSend: StateFlow<Boolean> by lazy { container.prefs.voiceAutoSend.stateIn(scope, SharingStarted.Eagerly, false) }
  /** "Read answers aloud" (off by default): the chat module reads each new answer when it ends, if it is still on screen (VOICE-11) */
  val readAloudAuto: StateFlow<Boolean> by lazy { container.prefs.readAloudAuto.stateIn(scope, SharingStarted.Eagerly, false) }
  private val speechRate: StateFlow<Float> by lazy { container.prefs.speechRate.stateIn(scope, SharingStarted.Eagerly, 1f) }

  /** the one reader of answers: only one reading at a time (VOICE-10) */
  val readAloud: ReadAloud by lazy {
    ReadAloud(AndroidTtsEngine(app), { voiceLanguage.value }, { speechRate.value }) { s -> scope.launch { container.toast.show(s) } }
  }

  /** "Show the chat title" and "Sound" (the other notification choices live in `ZoPrefs`) */
  val choices: NotifyChoices by lazy { NotifyChoices(app) }

  /** the notification choices as one value */
  val notifySettings: StateFlow<NotifySettings> by lazy {
    combine(container.prefs.notifAnswers, container.prefs.notifNeedsYou, container.prefs.notifDetails, choices.showTitle, choices.sound) { a, n, d, t, s ->
      NotifySettings(answers = a, needsYou = n, details = d, showTitle = t, sound = s)
    }.stateIn(scope, SharingStarted.Eagerly, NotifySettings())
  }

  val notifier: ZoNotifier by lazy { ZoNotifier(app, notifySettings) }

  private val pushStore by lazy { PrefsPushStore(app) }
  val push: PushRegistrar by lazy { PushRegistrar(ApiPushBackend(container.api), FirebaseTokenSource(), pushStore, { NotificationPermission.granted(app) }) }

  /** true while this phone is registered for push: the chat module sends `keep_running: true` then (NOTIF-05) */
  fun keepRunning(): Boolean = push.registered.value

  private var installed = false
  private var lastKey = ""
  private var lastAt = 0L
  private var lastOk = false

  /** Starts the background parts: foreground tracking, push registration after sign-in, removal at sign-out. */
  fun install() {
    if (installed) return
    installed = true
    AppVisibility.install(app)
    container.session.beforeSignOut.add { push.unregister(deleteToken = false) }
    scope.launch {
      combine(container.auth.state, container.session.gate, choices.showTitle, AppVisibility.foreground) { a, g, t, fg -> Triple((a as? AuthState.SignedIn)?.user?.uid, g is Gate.Ready, t to fg) }
        .distinctUntilChanged()
        .collect { (uid, ready, rest) ->
          if (uid == null || !ready) return@collect
          // "at every start" (NOTIF-15), but a registration that was fine a minute ago is not asked about again at every resume
          val key = uid + "|" + rest.first
          val now = System.currentTimeMillis()
          if (key == lastKey && lastOk && now - lastAt < 10 * 60_000L) return@collect
          val r = push.sync(uid, rest.first)
          lastKey = key; lastAt = now; lastOk = r is org.zohara.agent.media.notify.PushResult.Registered
        }
    }
    scope.launch { AppVisibility.foreground.collect { if (it) notifier.cancelAll() } } // open notifications are closed when the app comes back (NOTIF-02)
  }

  /** Settings: switch "Also notify me when the app is closed" on or off, then register or unregister. Returns what happened. */
  suspend fun setClosedPush(on: Boolean): org.zohara.agent.media.notify.PushResult {
    push.setEnabled(on)
    val uid = (container.auth.state.value as? AuthState.SignedIn)?.user?.uid ?: return org.zohara.agent.media.notify.PushResult.Off
    return push.sync(uid, choices.showTitle.value)
  }

  /** FCM made a new token (BG-05). */
  fun onNewToken(token: String) {
    val uid = (container.auth.state.value as? AuthState.SignedIn)?.user?.uid
    scope.launch { push.onNewToken(uid, token, choices.showTitle.value) }
  }

  companion object {
    private val instances = WeakHashMap<AppContainer, MediaModule>()
    @Synchronized fun of(container: AppContainer): MediaModule = instances.getOrPut(container) { MediaModule(container) }
  }
}
