package org.zohara.chat.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.DangerButton
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.PrimaryButton
import org.zohara.chat.core.design.SecondaryButton
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.core.design.ZoBottomSheet
import org.zohara.chat.core.design.ZoIconButton
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.design.ZoSwitch
import org.zohara.chat.core.design.ZoTextField
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.MemoryItem
import org.zohara.chat.core.net.MemoryResp

/** The rules of the Memory page (SET-MEM-01, 03). */
object MemoryLogic {
  const val GROUP_LIMIT = 80
  val GROUPS = listOf("person" to "About you", "preference" to "Preferences", "project" to "Projects", "fact" to "Facts", "episode" to "Chat summaries")

  data class Group(val kind: String, val label: String, val shown: List<MemoryItem>)

  /** Pinned first, then newest; at most [GROUP_LIMIT] per group; empty groups are left out. */
  fun groups(items: List<MemoryItem>): List<Group> {
    val sorted = items.sortedWith(compareByDescending<MemoryItem> { it.pinned }.thenByDescending { it.updated })
    return GROUPS.mapNotNull { (k, label) ->
      val mine = sorted.filter { it.kind == k }
      if (mine.isEmpty()) null else Group(k, label, mine.take(GROUP_LIMIT))
    }
  }

  /** How many are kept but not listed. */
  fun omitted(items: List<MemoryItem>): Int = groups(items).sumOf { g -> items.count { it.kind == g.kind } - g.shown.size }

  fun counts(r: MemoryResp): Pair<Int, Int> { val summaries = r.items.count { it.kind == "episode" }; return (r.items.size - summaries) to summaries }

  fun explainer(r: MemoryResp): String {
    val (notes, summaries) = counts(r)
    return "$notes notes about you and $summaries chat summaries, shared with the computer and the phone. After every ${r.every} messages of yours, Zohara Chat reads the chat in the background and keeps what is worth remembering. Never passwords or keys."
  }
}

/** Memory. Parity rows: SET-MEM-01..05. */
@Composable
fun SettingsMemoryScreen() {
  val container = LocalAppContainer.current
  val doer = rememberDoer()
  val loader = rememberLoader { container.api.memory() }
  val load by loader.state.collectAsState()
  var chat by remember { mutableStateOf(false) }
  var add by remember { mutableStateOf(false) }
  var edit by remember { mutableStateOf<MemoryItem?>(null) }
  var forgetAll by remember { mutableStateOf(false) }
  SettingsPage("Memory") {
    LoadBox(load, { loader.load() }) { m ->
      var enabled by remember(m) { mutableStateOf(m.enabled) }
      MemoryContent(
        m, enabled,
        onEnabled = { on -> enabled = on; doer.run(onFail = { enabled = !on; doer.say(it) }) { container.api.memoryEnable(on) } },
        onChat = { chat = true },
        onEdit = { edit = it },
        onPin = { i -> doer.run(onDone = { loader.load() }) { container.api.memoryPin(i.id, !i.pinned) } },
        onForget = { i -> doer.run(onDone = { loader.load() }) { container.api.memoryForget(i.id) } },
        onAdd = { add = true },
        onForgetAll = { forgetAll = true },
      )
    }
  }
  if (chat) MemoryChatSheet(onDismiss = { chat = false; loader.load() })
  if (add) InputSheet("Add a note", "Add", { add = false }, label = "What should Zohara Chat remember?", placeholder = "I prefer short answers", minLines = 4) { t -> container.api.memoryAdd(t); loader.load() }
  edit?.let { i ->
    InputSheet("Edit note", "Save", { edit = null }, label = "What should Zohara Chat remember?", placeholder = "I prefer short answers", initial = i.text, minLines = 4) { t -> container.api.memoryUpdate(i.id, t); loader.load() }
  }
  if (forgetAll) ConfirmSheet("Forget everything?", "All notes and chat summaries are removed, on every device. Saved chats stay.", "Forget", { doer.run(onDone = { loader.load() }) { container.api.memoryClear() } }, { forgetAll = false })
}

@Composable
fun MemoryContent(
  m: MemoryResp, enabled: Boolean, onEnabled: (Boolean) -> Unit, onChat: () -> Unit, onEdit: (MemoryItem) -> Unit, onPin: (MemoryItem) -> Unit,
  onForget: (MemoryItem) -> Unit, onAdd: () -> Unit, onForgetAll: () -> Unit,
) {
  SectionHeader("Memory")
  SettingsCard {
    org.zohara.chat.core.design.SettingsRow(
      "Remember things between chats", subtitle = MemoryLogic.explainer(m),
      control = { ZoSwitch(enabled, onEnabled) },
    )
    ActionRow(
      "Edit by talking",
      "Tell Zohara Chat what to change, in your own words: \"join my preferences into one note\", \"forget the old laptop\", \"my name is spelled Zohaib\". This chat only edits the notes.",
      actions = { PrimaryButton("Chat about memory…", onChat) },
    )
    for (g in MemoryLogic.groups(m.items)) {
      Text(g.label, style = ZoTheme.type.sectionHeader, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(start = 16.dp, top = 14.dp, bottom = 2.dp))
      for (i in g.shown) {
        ActionRow(
          if (g.kind == "episode") i.title else "", i.text,
          actions = {
            if (g.kind != "episode") ZoIconButton(ZoIcons.Edit, "Edit this note", { onEdit(i) }, size = 36.dp)
            ZoIconButton(
              ZoIcons.Star, if (i.pinned) "Always tell the AI (pinned)" else "Pin: always tell the AI", { onPin(i) },
              tint = if (i.pinned) ZoTheme.colors.gold else ZoTheme.colors.icon, size = 36.dp,
            )
            ZoIconButton(ZoIcons.Trash, "Forget this", { onForget(i) }, tint = ZoTheme.colors.danger, size = 36.dp)
          },
        )
      }
    }
    val more = MemoryLogic.omitted(m.items)
    if (more > 0) ActionRow("…", "$more more are kept; the AI can search all of them.")
    ActionRow(
      "Your notes",
      "Pin a note to make sure the AI always knows it. Delete one that is wrong. You can also just tell Zohara Chat: \"remember that ...\" or \"forget that ...\".",
      divider = false,
      actions = { PillButton("Add a note…", onAdd); DangerButton("Forget everything", onForgetAll, outlined = true) },
    )
  }
}

/** SET-MEM-02: a small chat that only edits the notes. */
@Composable
fun MemoryChatSheet(onDismiss: () -> Unit) {
  val container = LocalAppContainer.current
  val scope = rememberCoroutineScope()
  class Turn(val you: String, reply: String) { var reply by mutableStateOf(reply) }
  val log = remember { mutableStateListOf<Turn>() }
  var text by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  val send = {
    val t = text.trim()
    if (t.isNotEmpty() && !busy) {
      text = ""
      val turn = Turn(t, "Working on it…")
      log.add(turn)
      busy = true
      scope.launch {
        try { turn.reply = container.api.memoryEdit(t).ifBlank { "Done." } } catch (e: ApiException) { turn.reply = e.text } finally { busy = false }
      }
    }
  }
  ZoBottomSheet(
    "Chat about memory", onDismiss,
    footer = { SecondaryButton("Done", onDismiss); PrimaryButton("Send", send, enabled = !busy && text.isNotBlank()) },
  ) {
    Column(Modifier.heightIn(max = 460.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
      Text("Say what to change. Zohara Chat edits the notes and tells you what it did.", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
      for (t in log) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
          Text(t.you, style = ZoTheme.type.body, color = ZoTheme.colors.text, modifier = Modifier.padding(8.dp).let { it })
        }
        Text(t.reply, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = Modifier.padding(horizontal = 8.dp))
      }
      ZoTextField(text, { text = it }, placeholder = "For example: join all my preferences into one note", modifier = Modifier.fillMaxWidth().padding(top = 6.dp))
    }
  }
}
