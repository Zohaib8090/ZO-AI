package org.zohara.agent.media

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Popup
import kotlinx.coroutines.launch
import org.zohara.agent.AppContainer
import org.zohara.agent.chat.ChatHooks
import org.zohara.agent.link.LinkPresence
import org.zohara.agent.link.RunNote
import org.zohara.agent.link.RunOnPill

/**
 * Plugs the media and link controls into the chat composer through [ChatHooks] (the chat module's slots). Called once from `ZoApp`, where the container is part of
 * the composition. The composer card is wrapped once by [ChatHooks.composerHost]: that makes the one [ComposerMedia] (attachments, dictation, sheets) and hands it to the
 * slots drawn inside (the strip, the "+" button, the microphone, the text box modifiers) through [LocalComposerCtx]. See docs/MEDIA-LINK.md.
 */
object ChatPlug {
  private var installed = false

  /** for tests: the slots are installed again against another container */
  @Synchronized internal fun resetForTests() { installed = false }

  @Synchronized
  fun install(container: AppContainer) {
    if (installed) return
    installed = true
    val module = MediaModule.of(container)
    ChatHooks.composerHost = { text, setText, send, busy, content -> ComposerHost(text, setText, send, busy, content) }
    ChatHooks.attachmentChips = { LocalComposerCtx.current?.let { HookStrip(it) } }
    ChatHooks.attachButton = { LocalComposerCtx.current?.let { PlusButton(it.media) } }
    ChatHooks.micButton = { _, _ -> LocalComposerCtx.current?.let { MicButton(it.media, busy = it.busy) } }
    ChatHooks.fieldModifier = { LocalComposerCtx.current?.let { Modifier.attachmentPaste(it.media).attachmentDrop(it.media).mediaKeys(it.media) } ?: Modifier }
    ChatHooks.runOnPill = { LinkPresence(); RunOnPill() }
    ChatHooks.readAloudButton = { text -> ReadAloudButton("answer-" + text.hashCode(), text, size = 44.dp, iconSize = 17.dp) }
    ChatHooks.readAloud = { text -> module.readAloud.toggle("answer-" + text.hashCode(), text) }
    // text shared from another app goes to the box of the new message (ATT-22)
    container.scope.launch {
      ShareInbox.text.collect { t -> if (t != null) { ChatHooks.prefill.value = t; ShareInbox.consumeText() } }
    }
  }
}

/** What the slots inside the composer share: the controller, the text of the box and how to change it, and whether an answer is being written. */
class ComposerCtx(val media: ComposerMedia, val text: String, val setText: (String) -> Unit, val busy: Boolean)

val LocalComposerCtx = compositionLocalOf<ComposerCtx?> { null }

/** Wraps the composer card: makes the controller once, takes the words of dictation into the box, and draws the sheets. */
@Composable
private fun ComposerHost(text: String, setText: (String) -> Unit, send: () -> Unit, busy: Boolean, content: @Composable () -> Unit) {
  val textNow by rememberUpdatedState(text)
  val setTextNow by rememberUpdatedState(setText)
  val sendNow by rememberUpdatedState(send)
  var wantSend by remember { mutableStateOf(false) }
  val media = rememberComposerMedia({ textNow }, { setTextNow(it) }, { wantSend = true }, owner = true)
  LaunchedEffect(wantSend) {
    if (wantSend) {
      wantSend = false
      withFrameNanos { }; withFrameNanos { } // the box takes the last words first, then the send reads them
      sendNow()
    }
  }
  LaunchedEffect(busy) { media.onBusy(busy) } // a new answer stops listening and reading (VOICE-06, VOICE-12)
  DisposableEffect(media) {
    val mine: (String, String) -> String = { o, n -> media.onTextEdited(o, n) }
    ChatHooks.textEdited = mine
    onDispose { if (ChatHooks.textEdited === mine) ChatHooks.textEdited = null }
  }
  ReadAloudLifecycle()
  CompositionLocalProvider(LocalComposerCtx provides ComposerCtx(media, text, setText, busy)) { content() }
  ComposerMediaSheets(media, overlay = false)
  if (media.dropping) Popup(alignment = Alignment.Center) { DropCard() }
}

/** The strip above the text (COMP-17), the Run on note, and nothing else: dictation writes into the box itself. */
@Composable
private fun HookStrip(ctx: ComposerCtx) {
  Column(Modifier.fillMaxWidth()) {
    AttachmentStrip(ctx.media, ctx.text, ctx.setText)
    RunNote(Modifier.padding(bottom = 8.dp))
  }
}
