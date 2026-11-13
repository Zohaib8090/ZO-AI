package org.zohara.chat.chat

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.launch
import org.zohara.chat.AppContainer
import org.zohara.chat.core.net.AttachInfo
import org.zohara.chat.core.net.Attachment
import org.zohara.chat.core.store.RunOn
import org.zohara.chat.media.AttMeta
import org.zohara.chat.media.MediaModule

/**
 * The real [ChatEnv]: reads the account and the run-on choice, takes attachments from the media module's strip, and hands approvals and
 * questions to the media module's notifier (which knows about the foreground, the notification choices and the Allow / Deny buttons).
 */
class AppChatEnv(private val c: AppContainer) : ChatEnv {
  @Volatile private var runOnNow: RunOn = RunOn.Cloud
  private val media by lazy { MediaModule.of(c) }

  init {
    c.scope.launch { c.prefs.runOn.collect { runOnNow = it } }
  }

  override val restricted: Boolean get() = c.session.restricted
  override val messageLimit: Int? get() = c.session.account?.limits?.messageChars ?: if (c.session.restricted) 20_000 else null
  override fun runOn(): RunOn = runOnNow
  /** COMP-22: only when background push is registered for this device */
  override fun keepRunning(): Boolean = media.keepRunning()

  override val attachmentCount: StateFlow<Int> by lazy { media.attachments.items.map { it.size }.stateIn(c.scope, SharingStarted.Eagerly, 0) }

  override fun takeForSend(typed: String): Taken {
    val t = media.attachments.take()
    return Taken(
      text = t.messageText(typed), attachments = t.attachments,
      infos = t.meta.map { m: AttMeta -> AttachInfo(kind = if (m.kind == "image") "image" else "text", name = m.name, size = m.size.toLong(), thumb = m.thumb.ifEmpty { null }) },
      restore = { media.attachments.restore(t) },
    )
  }

  override fun toast(text: String) { c.scope.launch { c.toast.show(text) } }

  override fun chatsChanged(delayMs: Long) {
    c.scope.launch {
      if (delayMs > 0) delay(delayMs)
      ChatHooks.chatsChanged.tryEmit(delayMs)
      // the pages module keeps the chat list (Chats page, sidebar): ask it to read again
      runCatching { org.zohara.chat.pages.hubOf(c).chats.refresh() }
    }
  }

  override fun needsYou(chatId: String?, kind: NeedsKind, streamId: Long?, id: Long, summary: String) {
    val chat = chatId ?: return
    val stream = streamId ?: return
    when (kind) {
      NeedsKind.Approval -> media.notifier.approval(chat, stream, id, summary)
      NeedsKind.Question -> media.notifier.question(chat, stream, id, summary)
    }
  }

  override fun needsYouDone(chatId: String?, kind: NeedsKind, id: Long) { chatId?.let { media.notifier.cancelNeedsYou(it) } }

  override val attachmentsBusy: Boolean get() = media.attachments.isBusy
  override val checksBeforeSend: Boolean get() = true
  override suspend fun beforeSend(): String? = when (val r = org.zohara.chat.link.LinkModule.of(c).runOn.ensureReachable()) { is org.zohara.chat.link.SendCheck.Stop -> r.message; else -> null }
  override fun answerFinished(chatId: String?, title: String, answer: String) {
    chatId?.let { media.notifier.answerFinished(it, title) } // only when the app is in the background
    // VOICE-11: "Read answers aloud" reads each new answer when it ends, if that chat is still on screen and the answer has text (same id as the speaker button, so it shows Stop)
    val (route, routeChat) = ChatHooks.screen.value
    val shown = routeChat == chatId || (route == org.zohara.chat.nav.Routes.HOME && chatId != null && ChatHub.of(c).hub.home().chatId == chatId)
    if (readAloudNow(org.zohara.chat.media.notify.AppVisibility.foreground.value, media.readAloudAuto.value, shown, answer)) {
      c.scope.launch(kotlinx.coroutines.Dispatchers.Main) { media.readAloud.speak("answer-" + answer.hashCode(), answer) }
    }
  }
  override fun desktopOffline() { c.scope.launch { runCatching { c.api.linkStatus() } } }

  companion object {
    /** VOICE-11 (web `send`: acc is not empty and the element is still connected): the app is on screen, the setting is on, the chat is shown, the answer has text. */
    fun readAloudNow(foreground: Boolean, auto: Boolean, chatShown: Boolean, answer: String): Boolean = foreground && auto && chatShown && answer.isNotBlank()
  }
}
