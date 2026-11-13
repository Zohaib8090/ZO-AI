package org.zohara.chat.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.ZoIconButton
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.core.net.QuestionAnswer

/** Grey placeholder rows while a chat loads (CHAT-45): one bubble on the right, then lines of an answer. */
@Composable
fun LoadingSkeleton(modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  fun bar(w: Float, h: Int = 12) = @Composable { Box(Modifier.fillMaxWidth(w).height(h.dp).clip(ZoTheme.shapes.field).background(c.line8)) }
  androidx.compose.foundation.layout.Column(
    modifier.widthIn(max = 760.dp).fillMaxWidth().padding(horizontal = 16.dp, vertical = 16.dp).semantics { contentDescription = "Loading the chat" },
    verticalArrangement = Arrangement.spacedBy(10.dp),
  ) {
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) { Box(Modifier.fillMaxWidth(0.55f).height(40.dp).clip(ZoTheme.shapes.bubble).background(c.line8)) }
    bar(0.92f)(); bar(0.8f)(); bar(0.6f)()
    androidx.compose.foundation.layout.Spacer(Modifier.height(8.dp))
    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) { Box(Modifier.fillMaxWidth(0.4f).height(40.dp).clip(ZoTheme.shapes.bubble).background(c.line8)) }
    bar(0.85f)(); bar(0.5f)()
  }
}

/** What the thread needs from the screen besides the rows. */
class ThreadCallbacks(
  val messages: MessageActions,
  val onShowEarlier: () -> Unit,
  val onOpenTool: (group: Int, line: Int) -> Unit,
  val onApprove: (key: Long, yes: Boolean) -> Unit,
  val onAnswer: (key: Long, answers: List<QuestionAnswer>) -> Unit,
  val onSkip: (key: Long) -> Unit,
)

/**
 * The conversation (CHAT-01): a lazy list, centred and 760 dp wide at most. It follows new content to the end until the person scrolls up,
 * and then shows a button to jump back down (CHAT-01 auto-scroll rule). Only the last rows are drawn at first; "Show earlier" adds more.
 */
@Composable
fun ThreadView(state: ChatUiState, cb: ThreadCallbacks, modifier: Modifier = Modifier, listState: LazyListState = rememberLazyListState()) {
  val c = ZoTheme.colors
  val shown = remember(state.items, state.visibleFrom) { state.items.drop(state.hiddenCount) }
  val groupOrdinals = remember(state.items) {
    var n = 0
    state.items.filterIsInstance<ToolGroupItem>().associate { it.key to n++ }
  }
  val lastAi = remember(state.items) { state.items.lastOrNull { it is AiItem }?.key }
  val lastUser = state.lastUser?.key
  val lastKey = state.items.lastOrNull { it !is ThinkingItem }?.key
  var editingNth by rememberSaveable { mutableStateOf<Int?>(null) }
  var following by remember { mutableStateOf(true) }
  val scope = androidx.compose.runtime.rememberCoroutineScope()

  val atEnd by remember { derivedStateOf { !listState.canScrollForward } }
  LaunchedEffect(atEnd) { if (atEnd) following = true }
  val connection = remember {
    object : NestedScrollConnection {
      override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
        if (source == NestedScrollSource.UserInput && available.y > 0f) following = false
        return Offset.Zero
      }
    }
  }
  val total = shown.size + (if (state.hiddenCount > 0) 1 else 0)
  val lastLen = (state.items.lastOrNull() as? AiItem)?.text?.length ?: (state.items.lastOrNull() as? ToolGroupItem)?.lines?.size ?: 0
  LaunchedEffect(total, lastKey, lastLen, state.busy) {
    if (following && total > 0) listState.scrollToItem(total - 1, scrollOffset = Int.MAX_VALUE / 2)
  }

  Box(modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
    LazyColumn(
      state = listState,
      modifier = Modifier.widthIn(max = 760.dp).fillMaxSize().nestedScroll(connection).testTag("thread"),
      contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 12.dp),
      verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
      if (state.hiddenCount > 0) item(key = "earlier") {
        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) { PillButton("Show earlier (${state.hiddenCount})", cb.onShowEarlier) }
      }
      items(shown, key = { it.key }, contentType = { it::class }) { item ->
        val canRetry = !state.busy && item.key == lastKey
        when (item) {
          is UserItem -> {
            androidx.compose.foundation.layout.Column {
              UserBubble(item, cb.messages, state.busy, editing = editingNth == item.nth, onStartEdit = { editingNth = item.nth }, onEditDone = { editingNth = null })
              if (item.key == lastUser && !state.busy && editingNth != item.nth) UserActionRow(item, cb.messages, onEdit = { editingNth = item.nth })
            }
          }
          is AiItem -> androidx.compose.foundation.layout.Column {
            AiMessage(item, cb.messages)
            if (item.key == lastAi && !state.busy && item.text.isNotBlank()) AiActionRow(item.text, cb.messages, showRetry = true)
          }
          is ToolGroupItem -> ToolGroupView(item, onOpen = { line -> cb.onOpenTool(groupOrdinals[item.key] ?: 0, line) })
          is ErrorItem -> ErrorRow(item, canRetry, cb.messages)
          is StoppedItem -> StoppedRow(canRetry, cb.messages)
          is NoticeItem -> NoticeRow(item.text)
          is ThinkingItem -> ThinkingRow()
          is WaitingItem -> WaitingRow(item.summary)
          is ApprovalItem -> ApprovalCard(item, onAnswer = { yes -> cb.onApprove(item.key, yes) })
          is QuestionItem -> QuestionCard(item, onSend = { cb.onAnswer(item.key, it) }, onSkip = { cb.onSkip(item.key) })
        }
      }
    }
    if (state.loading && state.items.isEmpty()) LoadingSkeleton(Modifier.align(Alignment.TopCenter))
    if (!following && !atEnd) {
      ZoIconButton(
        ZoIcons.Down, "Jump to the newest", { following = true; scope.launch { if (total > 0) listState.scrollToItem(total - 1, scrollOffset = Int.MAX_VALUE / 2) } },
        modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 8.dp).shadow(4.dp, CircleShape).clip(CircleShape).border(1.dp, c.line12, CircleShape),
        size = 40.dp, background = c.card2,
      )
    }
  }
}
