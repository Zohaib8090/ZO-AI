package org.zohara.agent.link

import java.util.Locale
import org.zohara.agent.core.net.LinkDevice

/** The sentences of the Run on feature (the web page's `OFFLINE_MSG` and friends). */
object LinkText {
  const val OFFLINE = "Your computer is not reachable. Start Zohara Agent on your computer (it can run in the tray) and turn on Settings > Account and sync > Control this computer from the web."
  const val NOT_REACHABLE_NOW = "That computer is not reachable right now."
  const val VANISHED = "That computer is no longer linked. Back to the cloud."
  const val COULD_NOT_CHECK_ONE = "Could not check your computer. Check your connection."
  const val COULD_NOT_CHECK_LIST = "Could not check your computers."
  const val NOT_REACHABLE = "Your computer is not reachable."
  const val COULD_NOT_CHECK_SEND = "Could not check your computer."
  const val BACK_TO_CLOUD = "Back to the cloud: send again."
  const val NO_COMPUTER = "No computer is linked yet. On your computer: open Zohara Agent, sign in with this account, then turn on Settings > Account and sync > Control this computer from the web."
  const val CLOUD_TITLE = "Cloud"
  const val CLOUD_SUB = "The Zohara Agent server: chat, web search and memory."
  const val WAITING = "Waiting for your approval on your computer"
  const val STOP_SENT = "Stop sent to your computer."
  const val LOST = "The connection to your computer was lost. The task may still be running there: check Running now on your computer."
  const val ACCESS_WARNING = "A linked computer lets this app run Zohara Agent there, with that computer's files and tools. Choose it with the Run on control under the message box. Anyone who can sign in to your account on the web can run tools on this computer. Keep Ask-first on."
  const val PRESENCE = "A computer counts as online when Zohara Agent on it checked in within the last 40 seconds."
  const val NO_COMPUTER_PAGE = "Open Zohara Agent on your computer and sign in with this account. Then turn on Settings > Account and sync > Control this computer from the web. The app can run in the tray."
  const val MODEL_NOTE = "This changes the model on your computer. API keys are set in the desktop app, never here."
  const val MODE_NOTE = "Your computer may limit this: web sessions never run on Bypass unless you allowed it there."
  const val NO_CLAUDE = "Send your Claude sign-in from the desktop app (Settings > Account and sync) to use your Claude subscription here."
}

/** Times and names of linked computers (LINK-14). */
object LinkFormat {
  /** timestamps may be seconds or milliseconds: above 1e12 is milliseconds; 0 stays 0 */
  fun millis(t: Long): Long = if (t <= 0) 0 else if (t > 1_000_000_000_000L) t else t * 1000

  /** `ago`: "never", "just now", "5 min ago", "3 h ago", or the date after two days */
  fun ago(ms: Long, now: Long = System.currentTimeMillis(), locale: Locale = Locale.getDefault()): String {
    if (ms <= 0) return "never"
    val m = Math.round((now - ms) / 60000.0)
    return when {
      m < 1 -> "just now"
      m < 60 -> "$m min ago"
      m < 2880 -> "${Math.round(m / 60.0)} h ago"
      else -> java.text.DateFormat.getDateInstance(java.text.DateFormat.MEDIUM, locale).format(java.util.Date(ms))
    }
  }

  /** a computer's name as shown: at most 60 characters, "Computer" when it has none (`devName`) */
  fun name(d: LinkDevice?): String = d?.name?.filter { !it.isISOControl() }?.trim()?.take(60)?.ifEmpty { null } ?: "Computer"

  /** "My computer (name)" */
  fun runLabel(name: String?): String = if (name.isNullOrBlank()) "My computer" else "My computer ($name)"

  /** the sub line of a row of the Run on sheet (LINK-01) */
  fun statusLine(d: LinkDevice, now: Long = System.currentTimeMillis()): String =
    if (d.online) "Online" + (if (d.caps.claudeCode) " · Claude Code installed" else "")
    else "Not reachable" + (if (d.lastSeen > 0) " · last seen " + ago(millis(d.lastSeen), now) else "")

  /** the chips on My computers (LINK-05) */
  fun chips(d: LinkDevice): List<String> = buildList {
    add(if (d.caps.claudeCode) "Claude Code installed" else "Claude Code not found")
    if (d.caps.version.isNotBlank()) add("Zohara Agent " + d.caps.version.take(24))
    if (d.caps.os.isNotBlank()) add(d.caps.os.take(32))
    add(if (d.caps.approvalsFromWeb) "Approvals can be answered here" else "Approvals are answered on the computer")
  }
}

/** The modes on a computer: only Manual and Auto, Bypass is shown as Auto (PERM-03, LINK-11). */
object LinkModes {
  val INFO = mapOf("Manual" to "Asks before every change and command.", "Auto" to "Looking around runs; changes ask first.", "Bypass" to "Never asks.")

  fun available(onComputer: Boolean, restricted: Boolean): List<String> = if (restricted || onComputer) listOf("Manual", "Auto") else listOf("Manual", "Auto", "Bypass")
  fun shown(mode: String, onComputer: Boolean): String = if (onComputer && mode == "Bypass") "Auto" else mode.ifEmpty { "Manual" }
}
