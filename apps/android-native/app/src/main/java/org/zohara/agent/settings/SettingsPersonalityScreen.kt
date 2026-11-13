package org.zohara.agent.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.ErrorLine
import org.zohara.agent.core.design.PillButton
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.SectionHeader
import org.zohara.agent.core.design.SettingsCard
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTextField
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.core.net.PersonalitiesResp
import org.zohara.agent.core.net.Personality

/** The rows of the Personality page: Plain first, then the built-in and own ones (SET-PRS-01, 02). */
data class PersonalityRow(val id: String, val name: String, val sub: String, val inUse: Boolean, val deletable: Boolean)

const val PLAIN_TEXT = "Zohara Agent's own manner: clear and to the point."

fun personalityRows(r: PersonalitiesResp): List<PersonalityRow> =
  listOf(PersonalityRow("", "Plain", PLAIN_TEXT, r.active.isEmpty(), false)) +
    r.items.map { PersonalityRow(it.id, it.name + if (it.generated) " · written by the AI" else "", it.prompt, r.active == it.id, !it.builtin) }

/** Personality. Parity rows: SET-PRS-01..06. */
@Composable
fun SettingsPersonalityScreen() {
  val container = LocalAppContainer.current
  val doer = rememberDoer()
  val loader = rememberLoader { container.api.personalities() }
  val load by loader.state.collectAsState()
  var describe by remember { mutableStateOf(false) }
  var write by remember { mutableStateOf(false) }
  var deleting by remember { mutableStateOf<PersonalityRow?>(null) }
  SettingsPage("Personality") {
    LoadBox(load, { loader.load() }) { r ->
      PersonalityContent(
        personalityRows(r),
        onUse = { id -> doer.run(onDone = { loader.load() }) { container.api.personalityUse(id) } },
        onDelete = { deleting = it },
        onWrite = { write = true },
        onDescribe = { describe = true },
      )
    }
  }
  if (describe) {
    InputSheet(
      "Make a personality", "Write it", { describe = false }, label = "What should it be like?", minLines = 4,
      placeholder = "A patient teacher who explains with small everyday examples and never rushes me", busyHint = "Working on it…",
    ) { text -> container.api.personalityGenerate(text); loader.load() }
  }
  if (write) WritePersonalitySheet(onDismiss = { write = false }) { name, prompt -> container.api.personalityAdd(name, prompt); loader.load() }
  deleting?.let { p ->
    ConfirmSheet("Delete ${p.name}?", "This personality is removed.", "Delete", { doer.run(onDone = { loader.load() }) { container.api.personalityRemove(p.id) } }, { deleting = null })
  }
}

@Composable
fun PersonalityContent(rows: List<PersonalityRow>, onUse: (String) -> Unit, onDelete: (PersonalityRow) -> Unit, onWrite: () -> Unit, onDescribe: () -> Unit) {
  SectionHeader("Personality")
  SettingsCard {
    for (r in rows) {
      ActionRow(
        r.name, r.sub,
        actions = {
          PillButton(if (r.inUse) "In use" else "Use", { onUse(r.id) }, enabled = !r.inUse, accent = !r.inUse)
          if (r.deletable) ZoIconButton(ZoIcons.Trash, "Delete this personality", { onDelete(r) }, tint = ZoTheme.colors.danger, size = 36.dp)
        },
      )
    }
    ActionRow(
      "Add a personality",
      "The AI can write one from a description (it uses the model you chose). A personality changes the voice only, never the rules or what Zohara Agent may do.",
      divider = false,
      actions = { PrimaryButton("Describe one…", onDescribe); PillButton("Write my own…", onWrite) },
    )
  }
}

/** "Write my own…": a name and four to seven sentences (SET-PRS-04). */
@Composable
fun WritePersonalitySheet(onDismiss: () -> Unit, submit: suspend (name: String, prompt: String) -> Unit) {
  var name by remember { mutableStateOf("") }
  var prompt by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  ZoBottomSheet(
    "Write a personality", onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Save", {
        busy = true; error = null
        scope.launch { try { submit(name, prompt); onDismiss() } catch (e: ApiException) { error = e.text } finally { busy = false } }
      }, loading = busy)
    },
  ) {
    Column {
      ZoTextField(name, { name = it }, label = "Name", modifier = Modifier.fillMaxWidth())
      androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
      ZoTextField(prompt, { prompt = it }, label = "How should it sound? (4 to 7 sentences)", singleLine = false, minLines = 5, modifier = Modifier.fillMaxWidth())
      error?.let { ErrorLine(it, Modifier.padding(top = 8.dp)) }
    }
  }
}
