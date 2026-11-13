package org.zohara.agent.pages

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.pulltorefresh.PullToRefreshDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import java.util.Locale
import java.util.TimeZone
import org.zohara.agent.core.design.DangerButton
import org.zohara.agent.core.design.EmptyBox
import org.zohara.agent.core.design.ErrorLine
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoIconButton
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoTheme

/** What the pages need to print a time: "now" and the person's locale and zone (fixed in tests so a screenshot never changes). */
data class PagesClock(val now: Long, val locale: Locale = Locale.getDefault(), val zone: TimeZone = TimeZone.getDefault()) {
  fun relative(ts: Long) = PagesFormat.relative(ts, now, zone)
  fun whenText(ms: Long) = PagesFormat.whenText(ms, locale, zone)
}

// ------------------------------------------------------------------ holders that survive rotation

class HolderVm<T>(build: (CoroutineScope) -> T) : ViewModel() {
  val holder: T = build(viewModelScope)
}

/** A holder owned by the back stack entry: kept over rotation, dropped when the page is left. */
@Composable
fun <T> rememberHolder(key: String, build: (CoroutineScope) -> T): T {
  @Suppress("UNCHECKED_CAST")
  val vm = viewModel<HolderVm<T>>(HolderVm::class.java as Class<HolderVm<T>>, key = key, factory = viewModelFactory { initializer { HolderVm(build) } })
  return vm.holder
}

/** Runs [block] when the page comes to the front and every [everyMs] while it is there (null: only on entry and on return to the app). */
@Composable
fun RefreshWhileVisible(everyMs: Long?, block: () -> Unit) {
  val owner = LocalLifecycleOwner.current
  LaunchedEffect(owner, everyMs) {
    owner.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
      while (true) {
        block()
        if (everyMs == null) break
        delay(everyMs)
      }
    }
  }
}

// ------------------------------------------------------------------ layout pieces

/** The page title in serif with an optional action at the end (the web's `pageBox` head and the desktop's page title row). */
@Composable
fun PageHeader(title: String, modifier: Modifier = Modifier, action: (@Composable () -> Unit)? = null) {
  Row(modifier.fillMaxWidth().padding(top = 4.dp, bottom = 14.dp), verticalAlignment = Alignment.CenterVertically) {
    Text(title, style = ZoTheme.type.pageTitle, color = ZoTheme.colors.text, modifier = Modifier.weight(1f))
    action?.invoke()
  }
}

/** `.project-card`: icon, a name and a meta line, optionally a trailing control. Tap and long press are separate. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun PageCard(
  icon: ImageVector,
  name: String,
  modifier: Modifier = Modifier,
  meta: String? = null,
  metaLines: Int = 1,
  extra: String? = null,
  onClick: (() -> Unit)? = null,
  onLongClick: (() -> Unit)? = null,
  iconTint: Color = ZoTheme.colors.icon,
  trailing: (@Composable () -> Unit)? = null,
  /** NAV-23: the item shown in the right pane */
  selected: Boolean = false,
) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  Row(
    modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(if (selected) c.active else c.card).border(1.dp, c.line, ZoTheme.shapes.card)
      .then(if (onClick != null || onLongClick != null) Modifier.combinedClickable(role = Role.Button, onClick = onClick ?: {}, onLongClick = onLongClick) else Modifier)
      .padding(horizontal = 14.dp, vertical = 12.dp),
    verticalAlignment = Alignment.CenterVertically,
  ) {
    Icon(icon, null, Modifier.size(20.dp), tint = iconTint)
    Column(Modifier.weight(1f).padding(start = 14.dp)) {
      Text(name, style = t.cardName, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
      if (!meta.isNullOrBlank()) Text(meta, style = t.cardMeta, color = c.dim, maxLines = metaLines, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 2.dp))
      if (!extra.isNullOrBlank()) Text(extra, style = t.cardMeta, color = c.dim, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 2.dp))
    }
    if (trailing != null) {
      Spacer(Modifier.width(8.dp))
      trailing()
    }
  }
}

@Composable
fun LoadingLine(modifier: Modifier = Modifier) {
  Text("Loading…", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim, modifier = modifier.padding(vertical = 8.dp))
}

/** An error row that keeps the person moving: the server's sentence and a way to try again. */
@Composable
fun PageError(message: String, onRetry: (() -> Unit)?, modifier: Modifier = Modifier) {
  Column(modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    ErrorLine(message)
    if (onRetry != null) SecondaryButton("Try again", onRetry)
  }
}

/**
 * The scrolling body of a list page: pull to refresh, the header, then [content] items. First load shows "Loading…", a failed first load the error,
 * an empty list [empty]. The list is lazy, so a hundred artifacts or chats cost nothing until they are scrolled to.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun <T> ListPage(
  state: Loadable<List<T>>,
  title: String,
  onRefresh: () -> Unit,
  modifier: Modifier = Modifier,
  headerAction: (@Composable () -> Unit)? = null,
  beforeList: (LazyListScope.() -> Unit)? = null,
  empty: @Composable () -> Unit,
  footer: (@Composable () -> Unit)? = null,
  listState: LazyListState = rememberLazyListState(),
  item: @Composable (T) -> Unit,
) {
  val c = ZoTheme.colors
  val pull = rememberPullToRefreshState()
  PullToRefreshBox(
    isRefreshing = state.refreshing,
    onRefresh = onRefresh,
    state = pull,
    modifier = modifier.fillMaxSize(),
    indicator = { PullToRefreshDefaults.Indicator(state = pull, isRefreshing = state.refreshing, modifier = Modifier.align(Alignment.TopCenter), containerColor = c.card, color = c.accent) },
  ) {
    LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
      item { PageHeader(title, action = headerAction) }
      beforeList?.invoke(this)
      val list = state.value
      when {
        list == null && state.error != null -> item { PageError(state.error, onRefresh) }
        list == null -> item { LoadingLine() }
        list.isEmpty() -> item { empty() }
        else -> {
          for (x in list) item { item(x) }
          if (footer != null) item { footer() }
        }
      }
      // a reload failed but the old list is still shown: say so under it
      if (list != null && state.error != null) item { ErrorLine(state.error) }
    }
  }
}

// ------------------------------------------------------------------ confirm sheet and row menu

/** The confirm of a destructive step: a title, a sentence, the person's choice. */
@Composable
fun ConfirmSheet(title: String, text: String, confirmLabel: String, onConfirm: () -> Unit, onDismiss: () -> Unit, dismissLabel: String = "Cancel") {
  ZoBottomSheet(
    title = title,
    onDismiss = onDismiss,
    footer = {
      SecondaryButton(dismissLabel, onDismiss)
      DangerButton(confirmLabel, onConfirm)
    },
  ) {
    Text(text, style = ZoTheme.type.body, color = ZoTheme.colors.text2)
  }
}

data class MenuAction(val label: String, val icon: ImageVector, val danger: Boolean = false, val onClick: () -> Unit)

/** A small popover menu (long press or the "more" button of a row). */
@Composable
fun ActionMenu(expanded: Boolean, onDismiss: () -> Unit, actions: List<MenuAction>) {
  val c = ZoTheme.colors
  DropdownMenu(expanded = expanded, onDismissRequest = onDismiss, containerColor = c.popover, shape = ZoTheme.shapes.popover) {
    for (a in actions) {
      DropdownMenuItem(
        text = { Text(a.label, style = ZoTheme.type.navItem, color = if (a.danger) c.danger else c.text) },
        leadingIcon = { Icon(a.icon, null, Modifier.size(18.dp), tint = if (a.danger) c.danger else c.icon) },
        onClick = { onDismiss(); a.onClick() },
      )
    }
  }
}

/** The trailing "more" button of a row with its menu. */
@Composable
fun MoreMenuButton(actions: List<MenuAction>, description: String, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 36.dp) {
  var open by remember { mutableStateOf(false) }
  Box(modifier) {
    ZoIconButton(ZoIcons.More, description, { open = true }, size = size, iconSize = 18.dp)
    ActionMenu(open, { open = false }, actions)
  }
}

/** LINK-15: the name of the computer the phone runs on (from the link module's choice), or null while it is not known. */
@Composable
fun rememberComputerName(): String? {
  val container = org.zohara.agent.LocalAppContainer.current
  val choice by org.zohara.agent.link.LinkModule.of(container).runOn.choice.collectAsState()
  return choice.name.takeIf { choice.device != null && it.isNotBlank() }
}

/** NAV-23: what the right pane says before an item is chosen. */
@Composable
fun PickAnItem(text: String) {
  Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { EmptyBox(text, icon = ZoIcons.Chat) }
}
