package org.zohara.zoai.chat

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.design.ZoLogo
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.nav.LocalNav
import org.zohara.zoai.nav.Routes

/**
 * The screen body shared by home and chat: the thread (or the hero when empty), the composer and the sheets. [hero] is the greeting text
 * for the home screen; a chat passes null.
 */
@Composable
fun ChatPane(session: ChatSession, hub: ChatHub.Instance, greetingText: String?, modifier: Modifier = Modifier, onOpenContext: ((() -> Unit) -> Unit)? = null, contextRequests: Int = 0) {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val context = LocalContext.current
  val clipboard = LocalClipboardManager.current
  val state by session.state.collectAsState()
  val draft by session.draft.collectAsState()
  val pills by hub.pills.ui.collectAsState()
  val toast = { text: String -> hub.env.toast(text) }

  // the draft survives rotation and process death (COMP-18)
  DraftKeeper(session.draft)
  // text shared into the app (PHN share-in) lands in the home composer
  LaunchedEffect(Unit) {
    if (greetingText != null) ChatHooks.prefill.collect { t ->
      if (t != null) { session.draft.value = (session.draft.value + (if (session.draft.value.isEmpty()) "" else "\n") + t).trimEnd(); ChatHooks.prefill.value = null }
    }
  }
  LaunchedEffect(Unit) { hub.pills.refresh() }

  var sheet by rememberSaveable { mutableStateOf("") }
  var compacting by remember { mutableStateOf(false) }
  var compactResult by remember { mutableStateOf<String?>(null) }
  LaunchedEffect(contextRequests) { if (contextRequests > 0) sheet = "context" }

  val attachmentCount by hub.env.attachmentCount.collectAsState()
  val actions = remember(session) {
    MessageActions(
      canRetry = true,
      onCopy = { clipboard.setText(AnnotatedString(it)); toast("Copied") },
      onShare = { t -> context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, t), null).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) },
      onEdit = { u -> session.edit(u.nth, u.text) },
      onRetry = { session.retry() },
      onSwitchTake = { nth, d -> session.switchTake(nth, d) },
      onChooseModel = { sheet = "model" },
      onReadAloud = null,
    )
  }
  val readAloud = ChatHooks.readAloud
  // CHAT-04: a tap on a picture or file of a bubble opens the preview (the media module's sheet)
  var preview by remember { mutableStateOf<org.zohara.zoai.media.PreviewTarget?>(null) }
  val onPicture: (org.zohara.zoai.core.net.AttachInfo) -> Unit = { a -> preview = org.zohara.zoai.media.MediaModule.of(container).attachments.previewForSent(a.kind, a.name, a.size, a.thumb) }
  val actionsNow = MessageActions(actions.canRetry, actions.onCopy, actions.onShare, actions.onEdit, actions.onRetry, actions.onSwitchTake, actions.onChooseModel, readAloud, onPicture)
  preview?.let { org.zohara.zoai.media.PreviewSheet(it) { preview = null } }

  val cb = ThreadCallbacks(
    messages = actionsNow,
    onShowEarlier = { session.showEarlier() },
    onOpenTool = { g, _ -> state.chatId?.let { nav.openToolOutput(it, g) } },
    onApprove = { k, yes -> session.approve(k, yes) },
    onAnswer = { k, a -> session.answer(k, a) },
    onSkip = { k -> session.skip(k) },
  )
  val model = ComposerModel(
    busy = state.busy, questionOpen = state.questionOpen, modelText = pills.modelText, mode = pills.mode, runsOnComputer = state.computerName,
    restricted = container.session.restricted, limit = if (container.session.restricted) 20_000 else null, showContext = state.chatId != null || state.items.isNotEmpty(), showRunNote = greetingText != null,
    onSend = { text -> if (session.submit(text)) session.draft.value = "" },
    onStop = { session.stop() },
    onModel = { sheet = "model"; hub.pills.refresh() },
    onMode = { sheet = "mode" },
    onContext = { sheet = "context" },
    onTerminal = if (container.session.admin && hub.env.runOn().isCloud) ({ nav.terminal() }) else null,
  )

  var pinnedPanel by rememberSaveable { mutableStateOf(false) }
  val contextView = ContextView(
    info = Transcript.context(state.items, pills.model), usage = state.usage,
    message = Transcript.contextMessage(pills.provider, pills.ready),
    compacting = compacting, result = compactResult,
  )
  val refresh = { state.chatId?.let { session.open(it, force = true) }; compactResult = null }
  val compact = {
    compacting = true; compactResult = null
    session.compact { err -> compacting = false; compactResult = err ?: "Session compacted."; if (err == null) toast("Session compacted.") }
  }

  androidx.compose.foundation.layout.BoxWithConstraints(modifier) {
    // CTX-08, CTX-11: on a tablet the context panel can stay open next to the chat
    val wide = maxWidth >= PANEL_MIN_WIDTH
    val panelOn = wide && pinnedPanel && (state.chatId != null || state.items.isNotEmpty())
    androidx.compose.foundation.layout.Row(Modifier.fillMaxSize()) {
      Box(Modifier.weight(1f)) {
        androidx.compose.runtime.CompositionLocalProvider(androidx.compose.ui.platform.LocalUriHandler provides remember(context) { ChatUriHandler(context) }) {
          ChatContent(state, cb, model, draft, { session.draft.value = it }, greetingText, container.session.restricted, attachmentCount > 0)
        }
      }
      if (panelOn) ContextPanel(contextView, onUnpin = { pinnedPanel = false }, onRefresh = refresh, onCompact = compact)
    }

    when (sheet) {
      // MODEL-32: on My computer the model controls act on that computer (desktop_op), so the sheet is the link module's
      "model" -> if (!hub.env.runOn().isCloud) org.zohara.zoai.link.DesktopModelSheet(org.zohara.zoai.link.LinkModule.of(container).desktop, onDismiss = { sheet = ""; hub.pills.refresh() }) else ModelSheet(
        pills, { sheet = "" },
        onPickModel = { p, m -> hub.pills.chooseModel(p, m) { err -> if (err != null) toast(err) }; sheet = "" },
        onPickProfile = { id -> hub.pills.chooseProfile(id) { err -> if (err != null) toast(err) }; sheet = "" },
        onMore = { sheet = ""; nav.navigate(Routes.SETTINGS_MODEL) },
      )
      "mode" -> if (!hub.env.runOn().isCloud) org.zohara.zoai.core.design.ZoBottomSheet("What ZO-AI may do", { sheet = "" }) {
        // PERM-03, LINK-11: only Manual and Auto on a computer, Bypass is shown as Auto, and a note says the computer may limit it
        org.zohara.zoai.link.ModeRows(org.zohara.zoai.link.LinkModes.available(true, container.session.restricted), pills.mode.api, true) { m -> hub.pills.setMode(PermMode.parse(m)) { err -> if (err != null) toast(err) }; sheet = "" }
      } else ModeSheet(pills.mode, hub.pills.bypassAllowed(), { sheet = "" }, onPick = { m -> hub.pills.setMode(m) { err -> if (err != null) toast(err) }; sheet = "" })
      "context" -> if (wide && pinnedPanel) sheet = "" else ContextSheet(
        contextView, { sheet = "" }, onRefresh = refresh, onCompact = compact,
        onPin = if (wide) ({ pinnedPanel = true; sheet = "" }) else null,
      )
    }
  }
}

/** Width from which the context panel can be pinned beside the chat (a tablet or a phone on its side with a large screen). */
val PANEL_MIN_WIDTH = 840.dp

/** The pinned context panel (CTX-11): the same content as the sheet, with Unpin. It reads the rows of the chat, so it follows every finished answer. */
@Composable
fun ContextPanel(view: ContextView, onUnpin: () -> Unit, onRefresh: () -> Unit, onCompact: () -> Unit, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  Column(modifier.width(340.dp).fillMaxSize().background(c.card).padding(16.dp)) {
    androidx.compose.foundation.layout.Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
      Text("Context", style = ZoTheme.type.sheetTitle, color = c.text, modifier = Modifier.weight(1f))
      org.zohara.zoai.core.design.SecondaryButton("Unpin", onUnpin, Modifier.heightIn(min = 48.dp))
    }
    Spacer(Modifier.height(8.dp))
    ContextSheetContent(view, onRefresh, onCompact)
  }
}

/**
 * COMP-18, NAV-19: the draft lives in the session (it survives rotation and moving between pages); this also puts it in the saved state of
 * the screen, so it comes back after the system ended the process.
 */
@Composable
fun DraftKeeper(draft: kotlinx.coroutines.flow.MutableStateFlow<String>) {
  var saved by rememberSaveable { mutableStateOf("") }
  LaunchedEffect(Unit) { if (draft.value.isEmpty() && saved.isNotEmpty()) draft.value = saved }
  val now by draft.collectAsState()
  LaunchedEffect(now) { saved = now }
}

/** The body without any state holder, so a screenshot test can draw exactly what the person sees. */
@Composable
fun ChatContent(
  state: ChatUiState, cb: ThreadCallbacks, model: ComposerModel, draft: String, onDraft: (String) -> Unit,
  greetingText: String?, restricted: Boolean, hasAttachments: Boolean, modifier: Modifier = Modifier,
) {
  val showHero = greetingText != null && state.items.isEmpty() && !state.loading
  Column(modifier.fillMaxSize()) {
    if (showHero) {
      Column(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        ZoLogo(size = 52.dp)
        Text(greetingText!!, style = ZoTheme.type.hero, color = ZoTheme.colors.text, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 14.dp, bottom = 22.dp))
        Composer(draft, onDraft, model, hasAttachments = hasAttachments)
        if (restricted) {
          Text(
            "This app: chat, web search and memory. Computer control runs in the desktop app, which syncs your chats here. To run on your own computer, choose My computer under the message box.",
            style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, textAlign = TextAlign.Center, modifier = Modifier.padding(top = 8.dp),
          )
        }
      }
    } else {
      Box(Modifier.weight(1f).fillMaxWidth()) { ThreadView(state, cb) }
      Composer(draft, onDraft, model, Modifier.padding(horizontal = 12.dp).navigationBarsPadding(), hasAttachments = hasAttachments)
    }
  }
}
