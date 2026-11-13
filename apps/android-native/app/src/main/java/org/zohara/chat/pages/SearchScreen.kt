package org.zohara.chat.pages

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.zohara.chat.core.design.EmptyBox
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.design.ZoTextField
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.core.net.SearchHit
import org.zohara.chat.nav.LocalNav

/** What the Search page shows for a state (LIST-05, LIST-06): kept pure so a test can pin it down. */
sealed interface SearchDisplay {
  /** an empty box: the recent chats, or "No chats yet" when there are none (yet) */
  data class Recents(val chats: List<ChatSummary>, val loaded: Boolean) : SearchDisplay
  data object Searching : SearchDisplay
  data class Hits(val hits: List<SearchHit>) : SearchDisplay
  data object NoHits : SearchDisplay
  data class Failed(val message: String) : SearchDisplay

  companion object {
    fun of(ui: SearchUi, recent: List<ChatSummary>?, recentsError: Boolean = false): SearchDisplay = when {
      ui.query.isBlank() -> Recents(recent.orEmpty(), loaded = recent != null || recentsError)
      ui.error != null -> Failed(ui.error)
      ui.hits == null -> Searching
      // the old hits stay up while a newer text is being searched, so the list does not flash
      ui.hits.isEmpty() -> if (ui.loading) Searching else NoHits
      else -> Hits(ui.hits)
    }
  }
}

/** Search in chats. Parity rows: LIST-05, LIST-06. 200 ms after the last key it asks the service (up to 50 hits); an empty box shows the recent chats. */
@Composable
fun SearchScreen() {
  val hub = rememberPagesHub()
  val holder = rememberHolder("pages-search") { SearchHolder(hub.api, it) }
  val ui by holder.ui.collectAsState()
  val chats by hub.chats.state.collectAsState()
  val nav = LocalNav.current
  RefreshWhileVisible(null) { hub.chats.refresh() }
  SearchContent(ui, chats.value, remember(ui.hits, chats.value) { PagesClock(System.currentTimeMillis()) }, holder::setQuery) { nav.openChat(it) }
}

@Composable
fun SearchContent(ui: SearchUi, recent: List<ChatSummary>?, clock: PagesClock, onQuery: (String) -> Unit, onOpen: (String) -> Unit) {
  val display = SearchDisplay.of(ui, recent)
  // LIST-15: the box takes the keyboard focus when the page opens (desktop grab_focus, web $('q').focus())
  val focus = remember { androidx.compose.ui.focus.FocusRequester() }
  LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
    item { PageHeader("Search") }
    item {
      // the field is inside a lazy list: ask for the focus once it is really composed
      androidx.compose.runtime.LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
      ZoTextField(ui.query, onQuery, focusRequester = focus, placeholder = PagesFormat.SEARCH_PLACEHOLDER, trailing = { org.zohara.chat.core.design.ZoIcon(ZoIcons.Search) })
    }
    when (display) {
      is SearchDisplay.Recents ->
        if (display.chats.isEmpty()) {
          if (display.loaded) item { EmptyBox(PagesFormat.SEARCH_EMPTY_TITLE, subtitle = PagesFormat.SEARCH_EMPTY_TEXT, icon = ZoIcons.Chat) }
          else item { LoadingLine() }
        } else {
          item { SectionHeader("Recent chats", Modifier.padding(top = 4.dp)) }
          items(display.chats, key = { it.id }) { c -> PageCard(ZoIcons.Chat, PagesFormat.title(c), meta = clock.relative(c.updated), onClick = { onOpen(c.id) }) }
        }
      SearchDisplay.Searching -> item { LoadingLine() }
      is SearchDisplay.Hits -> items(display.hits, key = { it.id }) { h ->
        PageCard(ZoIcons.Chat, h.title.ifBlank { "Untitled" }, meta = h.snippet, metaLines = 2, extra = clock.relative(h.updated), onClick = { onOpen(h.id) })
      }
      SearchDisplay.NoHits -> item { EmptyBox(PagesFormat.SEARCH_NONE_TITLE, subtitle = PagesFormat.SEARCH_NONE_TEXT, icon = ZoIcons.Search) }
      is SearchDisplay.Failed -> item { PageError(display.message, onRetry = { onQuery(ui.query) }) }
    }
  }
}
