package org.zohara.zoai.pages

import androidx.compose.foundation.layout.Column
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
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoBottomSheet
import org.zohara.zoai.core.design.ZoIcon
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme

/**
 * Projects page. Parity rows: PROJ-01, 02, 05, 12 (the workspace rows PROJ-06..11 need service ops that do not exist: docs/PAGES.md).
 * The list is the `projects` text parsed into cards; "New project" asks for a name and a goal. A restricted account gets the server's own refusal.
 */
@Composable
fun ProjectsScreen() {
  val hub = rememberPagesHub()
  val holder = rememberHolder("pages-projects") { ProjectsHolder(hub.api, it) }
  val state by holder.state.collectAsState()
  val toast = rememberToastLambda()
  RefreshWhileVisible(null) { holder.refresh() }
  ProjectsContent(state, onRefresh = holder::refresh, onCreate = { n, g -> holder.create(n, g).also { if (it == null) toast("Project created") } })
}

@Composable
fun ProjectsContent(
  state: Loadable<List<PagesFormat.Project>>,
  onRefresh: () -> Unit,
  onCreate: suspend (name: String, goal: String) -> String?,
  startWithSheet: Boolean = false,
  startWithProject: PagesFormat.Project? = null,
) {
  var creating by remember { mutableStateOf(startWithSheet) }
  var shown by remember { mutableStateOf(startWithProject) }
  var query by remember { mutableStateOf("") }
  val all = state.value
  val filtered = all?.let { PagesFormat.filterProjects(it, query) }
  val wide = org.zohara.zoai.nav.isTwoPane()
  val list: @Composable () -> Unit = { ListPage(
    state = if (filtered != null && query.isNotBlank()) state.copy(value = filtered) else state,
    title = "Projects",
    onRefresh = onRefresh,
    headerAction = { PrimaryButton("New project", { creating = true }, icon = ZoIcons.Plus) },
    beforeList = if (all.isNullOrEmpty()) null else ({
      item(key = "project-search") {
        ZoTextField(query, { query = it }, placeholder = "Search projects", trailing = { ZoIcon(ZoIcons.Search) })
      }
    }),
    empty = {
      if (!all.isNullOrEmpty()) EmptyBox("Nothing matches that.", icon = ZoIcons.Search)
      else EmptyBox(
        PagesFormat.PROJECTS_EMPTY_TITLE, subtitle = PagesFormat.PROJECTS_EMPTY_TEXT, icon = ZoIcons.Folder,
        action = { PrimaryButton("New project", { creating = true }) },
      )
    },
  ) { p -> PageCard(ZoIcons.Folder, p.name, meta = p.meta, onClick = { shown = p }, selected = wide && shown?.id == p.id) } }
  if (wide) {
    // NAV-23: the opened project is shown beside the list instead of in a sheet
    org.zohara.zoai.nav.TwoPane(list, detail = { shown?.let { ProjectDetail(it, Modifier.padding(16.dp)) } ?: PickAnItem("Pick a project to see it here") })
  } else {
    list()
    shown?.let { ProjectSheet(it) { shown = null } }
  }
  if (creating) NewProjectSheet(onCreate = onCreate, onDismiss = { creating = false })
}

/** PROJ-05: Name (required) and Goal. */
@Composable
fun NewProjectSheet(onCreate: suspend (String, String) -> String?, onDismiss: () -> Unit) {
  var name by remember { mutableStateOf("") }
  var goal by remember { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  val scope = rememberCoroutineScope()
  ZoBottomSheet(
    title = "New project",
    onDismiss = onDismiss,
    footer = {
      SecondaryButton("Cancel", onDismiss)
      PrimaryButton("Create project", {
        busy = true; error = null
        scope.launch {
          val e = onCreate(name, goal)
          busy = false
          if (e == null) onDismiss() else error = e
        }
      }, enabled = name.isNotBlank() && !busy, loading = busy)
    },
  ) {
    ZoTextField(name, { name = it }, label = "Name", placeholder = "Name your project")
    ZoTextField(goal, { goal = it }, label = "Goal", placeholder = "What is it for?", modifier = Modifier.padding(top = 12.dp))
    error?.let { ErrorLine(it, Modifier.padding(top = 10.dp)) }
  }
}

/**
 * Opening a project (PROJ-06): the desktop's workspace keeps instructions, context files, memory and per-project tasks in the desktop's own store;
 * the service has no op for them, so on the phone a card opens what the list knows, and says where the rest is.
 */
@Composable
fun ProjectSheet(p: PagesFormat.Project, onDismiss: () -> Unit) {
  ZoBottomSheet(title = p.name, onDismiss = onDismiss, footer = { SecondaryButton("Close", onDismiss) }) { ProjectDetailBody(p) }
}

/** The opened project in the right pane of the two-pane layout (NAV-23). */
@Composable
fun ProjectDetail(p: PagesFormat.Project, modifier: Modifier = Modifier) {
  Column(modifier) {
    Text(p.name, style = ZoTheme.type.sheetTitle, color = ZoTheme.colors.text, modifier = Modifier.padding(bottom = 10.dp))
    ProjectDetailBody(p)
  }
}

@Composable
fun ProjectDetailBody(p: PagesFormat.Project) {
  val c = ZoTheme.colors
  run {
    Column {
      Text(
        p.goal.ifBlank { "ZO-AI remembers the same knowledge every time you talk to it in this project." },
        style = ZoTheme.type.body, color = c.text,
      )
      if (p.memoryNotes.isNotBlank()) Text(p.memoryNotes, style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 8.dp))
      Text(
        "Instructions, context files, project memory and chats inside a project are kept in the desktop app.",
        style = ZoTheme.type.cardMeta, color = c.dimmer, modifier = Modifier.padding(top = 12.dp),
      )
    }
  }
}
