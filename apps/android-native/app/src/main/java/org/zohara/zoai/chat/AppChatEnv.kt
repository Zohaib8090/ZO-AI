package org.zohara.zoai.chat

import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.launch
import org.zohara.zoai.AppContainer
import org.zohara.zoai.core.net.AttachInfo
import org.zohara.zoai.core.net.Attachment
import org.zohara.zoai.core.store.RunOn
import org.zohara.zoai.media.AttMeta
import org.zohara.zoai.media.MediaModule

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
      runCatching { org.zohara.zoai.pages.hubOf(c).chats.refresh() }
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

  override fun desktopOffline() { c.scope.launch { runCatching { c.api.linkStatus() } } }
}
