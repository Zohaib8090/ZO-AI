package org.zohara.zoai.chat

import org.zohara.zoai.core.net.AttachInfo
import org.zohara.zoai.core.net.QuestionSpec
import org.zohara.zoai.core.net.TakeInfo

/** One row of the thread. [key] is unique inside a chat and never reused, so the list keeps its scroll position. */
sealed interface ChatItem { val key: Long }

/** [nth] counts user messages from 0 (the number `send.edit` and `op switch` use). [take] is set when this message has more than one take. */
data class UserItem(override val key: Long, val text: String, val nth: Int, val attach: List<AttachInfo> = emptyList(), val take: TakeInfo? = null) : ChatItem

/** [live] is true while deltas are still arriving (CHAT-11). */
data class AiItem(override val key: Long, val text: String, val live: Boolean = false) : ChatItem

enum class LineKind { Start, Ok, Fail }

/** One line of a tool group: [text] is what is shown (without the marker), [kind] says which marker it had. */
data class ToolLine(val kind: LineKind, val text: String)

/** The collapsed box of tool runs (CHAT-27..31). */
data class ToolGroupItem(override val key: Long, val lines: List<ToolLine>, val active: Boolean) : ChatItem {
  val count: Int get() = lines.count { it.kind == LineKind.Start }
  val shell: Boolean get() = lines.any { it.text.contains("zohara_shell") }

  /** "Working on your computer…", "Running a task in the terminal…" while active, "Ran 1 command" / "Ran N commands" after. */
  val head: String
    get() = if (active) (if (shell) "Running a task in the terminal…" else "Working on your computer…")
    else if (count == 1) "Ran 1 command" else "Ran $count commands"
}

data class ErrorItem(override val key: Long, val text: String, val code: String = "", val retry: Boolean = true, val chooseModel: Boolean = false) : ChatItem
data class StoppedItem(override val key: Long) : ChatItem
/** A grey inline row: vision notice, "summarized", "Running on your computer (x)". */
data class NoticeItem(override val key: Long, val text: String) : ChatItem
data class ThinkingItem(override val key: Long) : ChatItem
/** "Waiting for your approval on your computer: <summary>" with a spinner (CHAT-37). */
data class WaitingItem(override val key: Long, val summary: String) : ChatItem

enum class ApprovalState { Pending, Allowed, Denied, NoAnswer }

data class ApprovalItem(
  override val key: Long, val id: Long, val tool: String, val summary: String,
  val onComputer: Boolean = false, val state: ApprovalState = ApprovalState.Pending, val sending: Boolean = false,
) : ChatItem {
  /** The collapsed line (INTER-01, INTER-08). */
  val result: String
    get() = when (state) {
      ApprovalState.Allowed -> "Allowed: ${firstLine(summary, tool)}"
      ApprovalState.Denied -> "Denied: ${firstLine(summary, tool)}"
      ApprovalState.NoAnswer -> "No answer: denied"
      ApprovalState.Pending -> ""
    }

  companion object {
    /** the summary on one line: its first two lines (the tool and what it does), at most 140 characters */
    fun firstLine(summary: String, tool: String): String =
      summary.lineSequence().map { it.trim() }.filter { it.isNotEmpty() }.take(2).joinToString(" ").take(140).ifEmpty { tool }
  }
}

sealed interface QuestionState {
  data object Pending : QuestionState
  data class Answered(val text: String) : QuestionState
  data object Skipped : QuestionState
  data object Expired : QuestionState
}

data class QuestionItem(override val key: Long, val id: Long, val questions: List<QuestionSpec>, val state: QuestionState = QuestionState.Pending, val sending: Boolean = false) : ChatItem {
  val result: String
    get() = when (val s = state) {
      is QuestionState.Answered -> "You answered: ${s.text}"
      QuestionState.Skipped -> "Skipped"
      QuestionState.Expired -> "No answer was given"
      QuestionState.Pending -> ""
    }
}

/** What a send was running on, for the status line and the pills. */
data class Usage(val input: Long, val output: Long)

data class ChatUiState(
  val chatId: String? = null,
  val title: String = "",
  val items: List<ChatItem> = emptyList(),
  val loading: Boolean = false,
  val busy: Boolean = false,
  val streamId: Long? = null,
  val usage: Usage? = null,
  /** takes of the loaded chat: user message number -> info */
  val takes: List<TakeInfo> = emptyList(),
  /** the thread shows only items from this index on ("Show earlier" lowers it) */
  val visibleFrom: Int = 0,
  /** name of the computer when this answer runs there */
  val computerName: String? = null,
) {
  val isEmpty: Boolean get() = items.isEmpty() && !loading
  /** a question card waits: the composer is off (COMP-16) */
  val questionOpen: Boolean get() = items.any { it is QuestionItem && it.state == QuestionState.Pending }
  val approvalOpen: Boolean get() = items.any { it is ApprovalItem && it.state == ApprovalState.Pending }
  val lastUser: UserItem? get() = items.lastOrNull { it is UserItem } as UserItem?
  val hiddenCount: Int get() = visibleFrom.coerceIn(0, items.size)
}
