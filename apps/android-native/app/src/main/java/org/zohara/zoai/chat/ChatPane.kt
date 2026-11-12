package org.zohara.zoai.chat

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
  var savedDraft by rememberSaveable { mutableStateOf("") }
  LaunchedEffect(Unit) { if (session.draft.value.isEmpty() && savedDraft.isNotEmpty()) session.draft.value = savedDraft }
  LaunchedEffect(draft) { savedDraft = draft }
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
  val actionsNow = if (readAloud != null) MessageActions(actions.canRetry, actions.onCopy, actions.onShare, actions.onEdit, actions.onRetry, actions.onSwitchTake, actions.onChooseModel, readAloud) else actions

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
  )

  ChatContent(state, cb, model, draft, { session.draft.value = it }, greetingText, container.session.restricted, attachmentCount > 0)

  when (sheet) {
    "model" -> ModelSheet(
      pills, { sheet = "" },
      onPickModel = { p, m -> hub.pills.chooseModel(p, m) { err -> if (err != null) toast(err) }; sheet = "" },
      onPickProfile = { id -> hub.pills.chooseProfile(id) { err -> if (err != null) toast(err) }; sheet = "" },
      onMore = { sheet = ""; nav.navigate(Routes.SETTINGS_MODEL) },
    )
    "mode" -> ModeSheet(pills.mode, hub.pills.bypassAllowed(), { sheet = "" }, onPick = { m -> hub.pills.setMode(m) { err -> if (err != null) toast(err) }; sheet = "" })
    "context" -> {
      val view = ContextView(
        info = Transcript.context(state.items, pills.model), usage = state.usage,
        message = Transcript.contextMessage(pills.provider, pills.ready),
        compacting = compacting, result = compactResult,
      )
      ContextSheet(
        view, { sheet = "" },
        onRefresh = { session.open(state.chatId ?: return@ContextSheet, force = true); compactResult = null },
        onCompact = {
          compacting = true; compactResult = null
          session.compact { err -> compacting = false; compactResult = err ?: "Session compacted."; if (err == null) toast("Session compacted.") }
        },
      )
    }
  }
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
