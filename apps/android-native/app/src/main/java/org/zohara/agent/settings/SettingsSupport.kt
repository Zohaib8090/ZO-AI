package org.zohara.agent.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.DangerButton
import org.zohara.agent.core.design.ErrorLine
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoTextField
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.core.net.ErrorKind
import org.zohara.agent.core.net.Limits
import org.zohara.agent.core.net.Whoami
import org.zohara.agent.core.session.Gate
import org.zohara.agent.nav.LocalNav
import org.zohara.agent.nav.Routes
import org.zohara.agent.nav.SetTopBar

// ================================================================================================================ the page list (SET-01, SET-03)

/** One entry of the Settings home. [adminOnly] pages are hidden for restricted accounts (`ADMIN_PAGES` of the web). */
data class SettingsPageDef(val id: String, val route: String, val title: String, val sub: String, val adminOnly: Boolean = false)

object SettingsPages {
  val all: List<SettingsPageDef> = listOf(
    SettingsPageDef("model", Routes.SETTINGS_MODEL, "Model and profiles", "Which AI answers, its key, and saved presets."),
    SettingsPageDef("personality", Routes.SETTINGS_PERSONALITY, "Personality", "How Zohara Agent talks to you."),
    SettingsPageDef("memory", Routes.SETTINGS_MEMORY, "Memory", "What Zohara Agent remembers about you between chats."),
    SettingsPageDef("general", Routes.SETTINGS_GENERAL, "Appearance and permissions", "Light or dark, and what Zohara Agent may do on its own."),
    SettingsPageDef("apps", Routes.SETTINGS_APPS, "Connected apps", "Tools Zohara Agent can use (MCP servers).", adminOnly = true),
    SettingsPageDef("machines", Routes.SETTINGS_MACHINES, "Machines", "Computers Zohara Agent can reach over SSH.", adminOnly = true),
    SettingsPageDef("sync", Routes.SETTINGS_SYNC, "Desktop sync", "Share chats and memory with the desktop app."),
    SettingsPageDef("computers", Routes.SETTINGS_COMPUTERS, "My computers", "Computers you can run Zohara Agent on from this app."),
    SettingsPageDef("notifications", Routes.SETTINGS_NOTIFICATIONS, "Notifications", "Be told when an answer is ready or Zohara Agent needs you."),
    SettingsPageDef("account", Routes.SETTINGS_ACCOUNT, "Account", "Your data, signed-in devices, export and delete."),
    SettingsPageDef("data", Routes.SETTINGS_DATA, "Data", "Saved chats."),
    SettingsPageDef("about", Routes.SETTINGS_ABOUT, "About", "Version."),
  )

  fun visible(restricted: Boolean): List<SettingsPageDef> = all.filter { !(restricted && it.adminOnly) }

  /** SET-03: a restricted account that opens an admin page, or an unknown page, lands on Model and profiles. */
  fun resolve(id: String, restricted: Boolean): SettingsPageDef = visible(restricted).firstOrNull { it.id == id } ?: all.first()
}

// ================================================================================================================ formatting

/** `ago` of the web: never / just now / N min ago / N h ago / the date. */
fun formatAgo(ms: Long, nowMs: Long = System.currentTimeMillis()): String {
  if (ms <= 0) return "never"
  val m = Math.round((nowMs - ms) / 60000.0)
  return when {
    m < 1 -> "just now"
    m < 60 -> "$m min ago"
    m < 2880 -> "${Math.round(m / 60.0)} h ago"
    else -> DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))
  }
}

fun formatWhen(ms: Long): String = if (ms <= 0) "" else DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(ms))

/** Seconds or milliseconds since 1970 (the server sends both, API.md) as milliseconds. */
fun toMillis(t: Long): Long = if (t <= 0) 0 else if (t > 1_000_000_000_000L) t else t * 1000

/** `human` of the web: 1.5 MB, 12 KB. */
fun humanBytes(bytes: Long): String {
  var n = bytes.toDouble().coerceAtLeast(0.0)
  val units = listOf("B", "KB", "MB", "GB")
  var i = 0
  while (n >= 1024 && i < 3) { n /= 1024; i++ }
  return if (i == 0) "${n.toLong()} B" else (if (n < 10) String.format(java.util.Locale.US, "%.1f", n) else String.format(java.util.Locale.US, "%.0f", n)) + " " + units[i]
}

/** "Requests per minute: 60 · Answers at the same time: 2 · ..." (SET-ACC-01). Only the limits the server sent. */
fun limitsText(l: Limits): String = buildList {
  if (l.requestsPerMin > 0) add("Requests per minute: ${l.requestsPerMin}")
  if (l.concurrentSends > 0) add("Answers at the same time: ${l.concurrentSends}")
  l.messageChars?.let { add("Longest message (characters): $it") }
  l.dataBytes?.let { add("Storage: ${humanBytes(it)}") }
  l.toolSteps?.let { add("Steps per answer: $it") }
  l.answerSecs?.let { add("Longest answer (seconds): $it") }
}.joinToString(" · ")

/** The voice languages of the web (`VOICE_LANGS`, VOICE-03). The first entry (empty tag) follows the device. */
val VOICE_LANGS: List<Pair<String, String>> = listOf(
  "" to "Device default", "en-US" to "English (US)", "en-GB" to "English (UK)", "ur-PK" to "Urdu", "hi-IN" to "Hindi", "ar-SA" to "Arabic",
  "bn-BD" to "Bengali", "es-ES" to "Spanish", "fr-FR" to "French", "de-DE" to "German", "it-IT" to "Italian", "pt-BR" to "Portuguese (Brazil)",
  "tr-TR" to "Turkish", "ru-RU" to "Russian", "id-ID" to "Indonesian", "zh-CN" to "Chinese (Mandarin)", "ja-JP" to "Japanese", "ko-KR" to "Korean",
)

/** The permission modes the person may pick: restricted accounts have no Bypass. */
fun permissionModes(restricted: Boolean): List<String> = if (restricted) listOf("Manual", "Auto") else listOf("Manual", "Auto", "Bypass")

/** The text under "Permission mode" (PERM-05). */
fun permissionModeText(restricted: Boolean): String =
  if (restricted) "Manual asks before every step. Auto lets looking around run and asks before changes."
  else "Manual asks before every change and command. Auto lets looking around run and asks before changes. Bypass never asks. The same rule applies to every app connected to Zohara Agent."

/** The Claude Code line of the web (`claudeLine`, LINK-CC). */
fun claudeLine(present: Boolean, setAtSec: Long, whenText: (Long) -> String = { formatWhen(toMillis(it)) }): String =
  if (!present) "Send your Claude sign-in from the desktop app (Settings > Account and sync) to use your Claude subscription here."
  else "Claude Code (your subscription) is ready on the server. Sign-in sent " + (if (setAtSec > 0) whenText(setAtSec) else "earlier") +
    ". It lasts about a year; send a new one from the desktop app before then."

// ================================================================================================================ loading and acting

sealed interface Load<out T> {
  data object Loading : Load<Nothing>
  data class Failed(val message: String) : Load<Nothing>
  data class Ready<T>(val value: T) : Load<T>
}

/** Loads one page's data. [load] shows "Loading…" the first time; a [reload] keeps the old data on screen until the new data is there. */
class Loader<T>(private val scope: CoroutineScope, private val fetch: suspend () -> T) {
  private val _state = MutableStateFlow<Load<T>>(Load.Loading)
  val state: StateFlow<Load<T>> = _state
  private var job: Job? = null

  fun load() {
    job?.cancel()
    job = scope.launch {
      if (_state.value !is Load.Ready) _state.value = Load.Loading
      try {
        _state.value = Load.Ready(fetch())
      } catch (e: CancellationException) {
        throw e
      } catch (e: ApiException) {
        _state.value = Load.Failed(e.text)
      }
    }
  }

  /** Replace the data without a trip to the server (after a local change). */
  fun set(value: T) { _state.value = Load.Ready(value) }
}

/** Runs the buttons of a page: [run] does the work, shows the server's sentence as a toast when it fails, and returns whether it worked. */
class Doer(private val scope: CoroutineScope, private val toast: (String) -> Unit) {
  fun run(onDone: () -> Unit = {}, onFail: (String) -> Unit = toast, block: suspend () -> Unit): Job = scope.launch {
    try {
      block()
      onDone()
    } catch (e: CancellationException) {
      throw e
    } catch (e: ApiException) {
      onFail(e.text)
    }
  }
  fun say(text: String) = toast(text)
}

@Composable
fun rememberDoer(): Doer {
  val scope = rememberCoroutineScope()
  val container = LocalAppContainer.current
  return remember(scope) { Doer(scope) { text -> scope.launch { container.toast.show(text) } } }
}

@Composable
fun <T> rememberLoader(vararg keys: Any?, fetch: suspend () -> T): Loader<T> {
  val scope = rememberCoroutineScope()
  val loader = remember(*keys) { Loader(scope, fetch) }
  LaunchedEffect(loader) { loader.load() }
  return loader
}

/** The account of the person; recomposes when `whoami` arrives. Until it is known the app behaves as restricted (AUTH-20). */
@Composable
fun rememberAccount(): Whoami? {
  val container = LocalAppContainer.current
  val gate by container.session.gate.collectAsState()
  return (gate as? Gate.Ready)?.account
}

// ================================================================================================================ page scaffold and rows

/** True while a settings page is drawn in the right pane of the two-pane layout (NAV-23): the back link is hidden. */
val LocalSettingsEmbedded = androidx.compose.runtime.staticCompositionLocalOf { false }

/** The "‹ Settings" link at the top of every page under Settings (SET-02). */
@Composable
fun SettingsBackLink(modifier: Modifier = Modifier) {
  // NAV-23: beside the list of pages there is nothing to go back to
  if (LocalSettingsEmbedded.current) return
  val c = ZoTheme.colors
  val nav = LocalNav.current
  Row(modifier.padding(top = 8.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
    Text(
      "‹ Settings", style = ZoTheme.type.button, color = c.link,
      modifier = Modifier
        .clickable(role = Role.Button) { if (!nav.back()) nav.settings() }
        .semantics { contentDescription = "Back to Settings" }
        .padding(end = 14.dp, top = 8.dp, bottom = 8.dp),
    )
  }
}

/** A settings page: "‹ Settings" back button, the serif title, the scrolling body (SET-02). */
@Composable
fun SettingsPage(title: String, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
  val c = ZoTheme.colors
  val nav = LocalNav.current
  SetTopBar("Settings")
  Box(modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
    Column(Modifier.widthIn(max = 720.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).navigationBarsPadding()) {
      SettingsBackLink()
      Text(title, style = ZoTheme.type.pageTitle, color = c.text, modifier = Modifier.padding(bottom = 8.dp))
      content()
      Spacer(Modifier.height(32.dp))
    }
  }
}

/** The body of a page while its data loads: "Loading…", an error with Try again, or the content. */
@Composable
fun <T> LoadBox(load: Load<T>, onRetry: () -> Unit, content: @Composable (T) -> Unit) {
  val c = ZoTheme.colors
  when (load) {
    Load.Loading -> Text("Loading…", style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(vertical = 16.dp))
    is Load.Failed -> Column(Modifier.padding(vertical = 12.dp)) {
      ErrorLine(load.message)
      Spacer(Modifier.height(10.dp))
      SecondaryButton("Try again", onRetry)
    }
    is Load.Ready -> content(load.value)
  }
}

/** A settings row whose buttons sit under the text (they wrap on a narrow phone). [extra] is shown between the text and the buttons. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ActionRow(
  title: String,
  subtitle: String? = null,
  modifier: Modifier = Modifier,
  divider: Boolean = true,
  extra: (@Composable () -> Unit)? = null,
  actions: (@Composable () -> Unit)? = null,
) {
  val c = ZoTheme.colors
  Column(modifier) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp)) {
      if (title.isNotEmpty()) Text(title, style = ZoTheme.type.cardName, color = c.text)
      if (!subtitle.isNullOrBlank()) Text(subtitle, style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 2.dp))
      extra?.invoke()
      if (actions != null) FlowRow(Modifier.padding(top = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) { actions() }
    }
    if (divider) org.zohara.agent.core.design.ZoDivider(Modifier.padding(start = 16.dp))
  }
}

/** A short explanation under a card (`.note`, `.field-hint`). */
@Composable
fun Note(text: String, modifier: Modifier = Modifier) {
  Text(text, style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dim, modifier = modifier.padding(horizontal = 6.dp, vertical = 8.dp))
}

// ================================================================================================================ sheets

/** A yes or no question as a bottom sheet (`confirmBox`). */
@Composable
fun ConfirmSheet(title: String, text: String, confirmLabel: String, onConfirm: () -> Unit, onDismiss: () -> Unit, danger: Boolean = true) {
  ZoBottomSheet(
    title = title, onDismiss = onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      if (danger) DangerButton(confirmLabel, { onDismiss(); onConfirm() }) else PrimaryButton(confirmLabel, { onDismiss(); onConfirm() })
    },
  ) { Text(text, style = ZoTheme.type.body, color = ZoTheme.colors.text2) }
}

/**
 * A sheet with one text field and a button (rename, new profile, add or edit a note, write a personality...). [submit] runs when the button is
 * pressed; an [ApiException] from it is shown in the sheet and the sheet stays open, otherwise the sheet closes.
 */
@Composable
fun InputSheet(
  title: String,
  confirmLabel: String,
  onDismiss: () -> Unit,
  label: String? = null,
  placeholder: String = "",
  initial: String = "",
  hint: String? = null,
  minLines: Int = 1,
  maxLength: Int = 0,
  /** shown instead of [hint] while [submit] runs ("Working on it…") */
  busyHint: String? = null,
  submit: suspend (String) -> Unit,
) {
  var value by remember { mutableStateOf(initial) }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  val go: () -> Unit = {
    busy = true; error = null
    scope.launch {
      try { submit(value); onDismiss() } catch (e: ApiException) { error = e.text } finally { busy = false }
    }
  }
  ZoBottomSheet(
    title = title, onDismiss = onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton(confirmLabel, { go() }, loading = busy)
    },
  ) {
    ZoTextField(
      value, { value = if (maxLength > 0) it.take(maxLength) else it }, label = label, placeholder = placeholder, hint = if (busy && busyHint != null) busyHint else if (error == null) hint else null,
      singleLine = minLines <= 1, minLines = minLines, modifier = Modifier.fillMaxWidth(),
      // SYS-06: Enter on the keyboard confirms a one-line sheet (a note stays multi-line)
      keyboardOptions = if (minLines <= 1) KeyboardOptions(imeAction = ImeAction.Done) else null,
      keyboardActions = KeyboardActions(onDone = { if (!busy) go() }),
    )
    error?.let { ErrorLine(it, Modifier.padding(top = 8.dp)) }
  }
}

/** Standard "ok" outcome of a call: the same sentence the server gave. */
fun ApiException.isOldServer(): Boolean = kind == ErrorKind.UnknownOp
