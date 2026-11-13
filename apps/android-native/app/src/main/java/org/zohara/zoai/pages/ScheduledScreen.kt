package org.zohara.zoai.pages

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
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
import org.zohara.zoai.core.design.EmptyBox
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.PillButton
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.net.ScheduledTask

/**
 * Scheduled tasks. Parity rows: SCHED-01..04, 08, 09 (Run now, the on/off switch and the last result need service ops that do not exist).
 * Results of a scheduled run are normal chats named "Scheduled: <title>"; they appear in Recents (SCHED-08).
 */
@Composable
fun ScheduledScreen() {
  val hub = rememberPagesHub()
  val holder = rememberHolder("pages-scheduled") { ScheduledHolder(hub.api, it) }
  val state by holder.state.collectAsState()
  val toast = rememberToastLambda()
  val scope = rememberCoroutineScope()
  RefreshWhileVisible(null) { holder.refresh() }
  ScheduledContent(
    state, remember(state) { PagesClock(System.currentTimeMillis()) }, onRefresh = holder::refresh,
    onCancel = { id -> scope.launch { holder.cancel(id)?.let(toast) } },
    onAdd = { f -> holder.add(f).fold({ toast("Task created"); null }, { it.sentence() }) },
  )
}

@Composable
fun ScheduledContent(
  state: Loadable<List<ScheduledTask>>,
  clock: PagesClock,
  onRefresh: () -> Unit,
  onCancel: (String) -> Unit,
  onAdd: suspend (TaskForm) -> String?,
  startWithSheet: Boolean = false,
  startWithConfirm: ScheduledTask? = null,
) {
  var creating by remember { mutableStateOf(startWithSheet) }
  var cancelling by remember { mutableStateOf(startWithConfirm) }
  var shown by remember { mutableStateOf<ScheduledTask?>(null) }
  val wide = org.zohara.zoai.nav.isTwoPane()
  val list: @Composable () -> Unit = { ListPage(
    state = state,
    title = "Scheduled",
    onRefresh = onRefresh,
    headerAction = { PrimaryButton("New task", { creating = true }, icon = ZoIcons.Plus) },
    empty = { EmptyBox(PagesFormat.SCHEDULED_EMPTY_TITLE, subtitle = PagesFormat.SCHEDULED_EMPTY_TEXT, icon = ZoIcons.Alarm) },
  ) { t ->
    PageCard(
      ZoIcons.Alarm, PagesFormat.taskTitle(t), meta = PagesFormat.taskWhen(t, clock.locale, clock.zone),
      extra = t.prompt.takeIf { it.isNotBlank() && it != PagesFormat.taskTitle(t) },
      onClick = if (wide) ({ shown = t }) else null, selected = wide && shown?.id == t.id,
      trailing = { ZoIconButton(ZoIcons.Trash, "Cancel this task", { cancelling = t }, size = 36.dp, iconSize = 18.dp) },
    )
  } }
  if (wide) {
    // NAV-23: the task that was tapped is described beside the list
    org.zohara.zoai.nav.TwoPane(list, detail = {
      shown?.let { t ->
        androidx.compose.foundation.layout.Column(Modifier.padding(16.dp)) {
          Text(PagesFormat.taskTitle(t), style = ZoTheme.type.sheetTitle, color = ZoTheme.colors.text)
          Text(PagesFormat.taskWhen(t, clock.locale, clock.zone), style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = Modifier.padding(top = 4.dp, bottom = 10.dp))
          if (t.prompt.isNotBlank()) Text(t.prompt, style = ZoTheme.type.body, color = ZoTheme.colors.text)
          androidx.compose.foundation.layout.Column(Modifier.padding(top = 14.dp)) { SecondaryButton("Cancel this task", { cancelling = t }) }
        }
      } ?: PickAnItem("Pick a task to see it here")
    })
  } else list()
  cancelling?.let { t ->
    ConfirmSheet(
      PagesFormat.CANCEL_TASK_TITLE, PagesFormat.CANCEL_TASK_TEXT, PagesFormat.CANCEL_TASK_BUTTON,
      onConfirm = { cancelling = null; onCancel(t.id) }, onDismiss = { cancelling = null }, dismissLabel = "Keep it",
    )
  }
  if (creating) NewTaskSheet(onAdd, onDismiss = { creating = false })
}

/** SCHED-04: what, when, repeat; the own interval wins over the choice. The server reads the time; its answer is shown when it cannot. */
@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun NewTaskSheet(onAdd: suspend (TaskForm) -> String?, onDismiss: () -> Unit) {
  var form by remember { mutableStateOf(TaskForm()) }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  val c = ZoTheme.colors
  ZoBottomSheet(
    title = "New scheduled task",
    onDismiss = onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Create task", {
        busy = true; error = null
        scope.launch {
          val e = onAdd(form)
          busy = false
          if (e == null) onDismiss() else error = e
        }
      }, enabled = form.valid && !busy, loading = busy)
    },
  ) {
    ZoTextField(
      form.prompt, { form = form.copy(prompt = it) }, label = "What should ZO-AI do?",
      placeholder = "e.g. check my disk space and tell me if it is above 90%", singleLine = false, minLines = 2,
    )
    ZoTextField(
      form.whenText, { form = form.copy(whenText = it) }, label = "When", placeholder = "tomorrow 9am, in 2 hours, 2026-11-05 18:30",
      modifier = Modifier.padding(top = 12.dp),
    )
    Text("Repeat", style = ZoTheme.type.fieldLabel, color = c.text, modifier = Modifier.padding(top = 12.dp, bottom = 6.dp))
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
      for ((label, value) in PagesFormat.REPEAT_CHOICES) PillButton(label, { form = form.copy(repeatChoice = value) }, accent = form.repeatChoice == value && form.ownInterval.isBlank())
    }
    ZoTextField(
      form.ownInterval, { form = form.copy(ownInterval = it) }, placeholder = "or your own: every 3 days, every 90 minutes, every 2 months",
      hint = "Your own interval wins over the choice above. At least 5 minutes.", modifier = Modifier.padding(top = 10.dp),
    )
    error?.let { ErrorLine(it, Modifier.padding(top = 10.dp)) }
  }
}
