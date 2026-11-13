package org.zohara.zoai.link

import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.SendEvent
import org.zohara.zoai.core.net.ZoApi

/** What the chat screen should do with one event of a computer's answer. */
sealed interface DesktopUi {
  /** "Running on your computer (name)" under the message (the `stream` event) */
  data class Running(val name: String?) : DesktopUi
  /** the status row with a spinner: "Waiting for your approval on your computer: <summary>" (CHAT-37, INTER-04) */
  data class Wait(val summary: String) : DesktopUi
  /** the status row goes away (any later event) */
  data object ClearWait : DesktopUi
  /** the computer allows web answers: show the normal Allow / Deny card */
  data class ShowApprovalCard(val event: SendEvent.Approval) : DesktopUi
  /** an error row; [refreshLink] means the link state should be checked again */
  data class Problem(val text: String, val refreshLink: Boolean) : DesktopUi
}

/**
 * Turns the events of a turn into what the screen shows (LINK-04, LINK-09, CHAT-37, INTER-04). One per turn. In the cloud ([onComputer] false) approvals are cards and nothing else changes.
 * [approvalsFromWeb] is what the computer announced; `approval {local:true}` or a computer that does not allow it means "answer it on the computer".
 */
class DesktopStreamReducer(private val onComputer: Boolean, private val approvalsFromWeb: () -> Boolean) {
  fun reduce(ev: SendEvent): List<DesktopUi> = when (ev) {
    is SendEvent.Stream -> if (onComputer && ev.target == "desktop") listOf(DesktopUi.Running(ev.device?.take(60))) else emptyList()
    is SendEvent.Approval -> when {
      onComputer && (ev.local || ev.id == 0L || !approvalsFromWeb()) -> listOf(DesktopUi.Wait(ev.summary.ifEmpty { ev.tool }.take(300)))
      else -> listOf(DesktopUi.ShowApprovalCard(ev))
    }
    is SendEvent.Delta, is SendEvent.Text, is SendEvent.ToolStart, is SendEvent.ToolDone, is SendEvent.Done, is SendEvent.Stopped -> if (onComputer) listOf(DesktopUi.ClearWait) else emptyList()
    is SendEvent.Error -> when {
      ev.code == "desktop_offline" -> listOf(DesktopUi.Problem(LinkText.OFFLINE, refreshLink = true))
      else -> listOf(DesktopUi.Problem(ev.message, refreshLink = onComputer && ev.code.startsWith("desktop_")))
    }
    is SendEvent.ConnectionLost -> if (onComputer) listOf(DesktopUi.Problem(LinkText.LOST, refreshLink = true)) else emptyList()
    else -> emptyList()
  }
}

/** What happened to an answer posted to `/api/reply` (LINK-09, LINK-10, INTER-17). */
sealed interface ReplyOutcome {
  data object Sent : ReplyOutcome
  /** 404: the stream is over or is not yours */
  data object Gone : ReplyOutcome
  /** 409 `approvals_on_computer`: the computer did not allow web approvals: "Waiting for your approval on your computer" */
  data object OnComputer : ReplyOutcome
  data class Failed(val text: String) : ReplyOutcome
}

object LinkReply {
  /** runs one reply call and sorts the result; [call] returns false for a stream that is over */
  suspend fun run(call: suspend () -> Boolean): ReplyOutcome = try {
    if (call()) ReplyOutcome.Sent else ReplyOutcome.Gone
  } catch (e: ApiException) {
    if (e.status == 409 || e.text.contains("approvals_on_computer")) ReplyOutcome.OnComputer else ReplyOutcome.Failed(e.text)
  }

  suspend fun approve(api: ZoApi, stream: Long, id: Long, yes: Boolean) = run { api.replyApprove(stream, id, yes) }
  suspend fun answer(api: ZoApi, stream: Long, id: Long, answers: List<org.zohara.zoai.core.net.QuestionAnswer>) = run { api.replyAnswer(stream, id, answers) }

  /** Stop for a computer's job queues a stop job on the server: toast "Stop sent to your computer." (LINK-10) */
  suspend fun stop(api: ZoApi, stream: Long): String? = try {
    api.replyStop(stream)
    LinkText.STOP_SENT
  } catch (e: ApiException) { e.text }
}
