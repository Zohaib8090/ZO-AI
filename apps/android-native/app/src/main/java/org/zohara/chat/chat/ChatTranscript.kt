package org.zohara.chat.chat

import org.zohara.chat.core.net.ChatEntry
import org.zohara.chat.core.net.TakeInfo

/** Pure helpers: saved chat to rows, copy text, titles, greeting, model names, context estimates. */
object Transcript {
  /**
   * The saved chat (`op chat`) as rows. Consecutive tool lines become one collapsed group (the desktop rule, CHAT-29); an AI or user entry or
   * an error ends it. [keys] gives the unique keys.
   */
  fun fromSaved(entries: List<ChatEntry>, takes: List<TakeInfo>, keys: () -> Long): List<ChatItem> {
    val out = ArrayList<ChatItem>()
    var group: MutableList<ToolLine>? = null
    var groupKey = 0L
    fun closeGroup() {
      group?.let { out.add(ToolGroupItem(groupKey, it.toList(), active = false)) }
      group = null
    }
    fun addLine(l: ToolLine) {
      if (group == null) { group = ArrayList(); groupKey = keys() }
      group!!.add(l)
    }
    var nth = 0
    for (e in entries) {
      val text = e.text
      when {
        e.kind == "msg-tool" && text.startsWith("▸ ") -> addLine(ToolLine(LineKind.Start, text.removePrefix("▸ ").trim()))
        e.kind == "msg-tool-out" -> addLine(ToolLine(if (text.startsWith("✗")) LineKind.Fail else LineKind.Ok, text.removePrefix("✓").removePrefix("✗").trim()))
        e.kind == "msg-error" && text.startsWith("✗ ") ->
          if (isVisionNotice(text)) { closeGroup(); out.add(NoticeItem(keys(), text.removePrefix("✗ ").trim())) }
          else addLine(ToolLine(LineKind.Fail, text.removePrefix("✗ ").trim()))
        e.kind == "msg-user" -> {
          closeGroup()
          out.add(UserItem(keys(), text, nth, e.attach, takes.firstOrNull { it.nth == nth && it.count > 1 }))
          nth++
        }
        e.kind == "msg-ai" -> { closeGroup(); out.add(AiItem(keys(), text)) }
        e.kind == "msg-error" -> { closeGroup(); out.add(ErrorItem(keys(), text, retry = true)) }
        e.kind == "msg-tool" -> { closeGroup(); out.add(NoticeItem(keys(), text)) }
        else -> { closeGroup(); if (text.isNotBlank()) out.add(NoticeItem(keys(), text)) }
      }
    }
    closeGroup()
    return out
  }

  fun isVisionNotice(text: String) = text.contains("cannot look at pictures")

  /** "name  detail" from a `tool_start` summary: the first line is the tool, the second the detail (CHAT-30). */
  fun toolLineFromSummary(name: String, summary: String): String {
    val lines = summary.lines().map { it.trim() }.filter { it.isNotEmpty() }
    val head = lines.firstOrNull() ?: name
    val detail = lines.getOrNull(1).orEmpty()
    return if (detail.isEmpty()) head.ifEmpty { name } else "$head  $detail"
  }

  /** Copy whole conversation (CHAT-26): "You: ..." and "Zohara Chat: ..." blocks separated by blank lines; tools and errors are left out. */
  fun copyAll(items: List<ChatItem>): String = items.mapNotNull {
    when (it) {
      is UserItem -> "You: ${it.text}"
      is AiItem -> "Zohara Chat: ${it.text}"
      else -> null
    }
  }.joinToString("\n\n")

  /** The title shown for a chat (CHAT-38, CHAT-50). */
  fun displayTitle(title: String, hasItems: Boolean): String = title.ifBlank { if (hasItems) "New chat" else "" }

  /** Greeting (NAV-18): desktop hours; the first word of the name, else the part of the e-mail before the @, capitalised. */
  fun greeting(hour: Int, displayName: String, email: String): String {
    val part = when (hour) { in 5..11 -> "Good morning"; in 12..17 -> "Good afternoon"; else -> "Good evening" }
    val first = displayName.trim().split(Regex("\\s+")).firstOrNull().orEmpty().ifEmpty {
      email.substringBefore('@').trim().replaceFirstChar { it.uppercase() }
    }
    return if (first.isEmpty()) part else "$part, $first"
  }

  /** Claude model ids shown the way the desktop does ("claude-opus-5-5" -> "Opus 5.5"); other models as they are (COMP-08). */
  fun niceName(model: String): String {
    val m = model.trim()
    val family = listOf("sonnet", "opus", "haiku", "fable")
    if (m in family) return m.replaceFirstChar { it.uppercase() }
    if (!m.startsWith("claude-")) return m
    val parts = m.removePrefix("claude-").split('-').filter { it.isNotEmpty() }
    val fam = parts.firstOrNull() ?: return m
    val nums = parts.drop(1).takeWhile { it.all(Char::isDigit) && it.length <= 2 }
    val name = fam.replaceFirstChar { it.uppercase() }
    return if (nums.isEmpty()) name else "$name ${nums.joinToString(".")}"
  }

  /** The text of the model pill (COMP-08). */
  fun pillText(ready: Boolean, model: String, provider: String, claudeCode: Boolean = provider == "claude-code"): String = when {
    ready && model.isNotBlank() -> niceName(model)
    ready && claudeCode -> "Claude"
    ready && provider.isNotBlank() -> provider
    else -> "Choose a model"
  }

  // ---- context sheet (CTX-01..05, CTX-09) ----
  /** the sentence that replaces the numbers (CTX-09), or null when the numbers can be shown */
  fun contextMessage(provider: String, ready: Boolean): String? = when {
    provider == "claude-code" -> "In Claude Code mode, Claude Code keeps this conversation and compacts it by itself, so Zohara Chat has nothing to show here."
    !ready -> "Choose a provider first."
    else -> null
  }

  const val SYSTEM_TOKENS = 2400
  const val TOOL_TOKENS = 3200

  fun windowFor(model: String): Int {
    val m = model.lowercase()
    var w = 128_000
    if (Regex("claude|gpt-4o|gemini").containsMatchIn(m)) w = 200_000
    if (Regex("gemini-1\\.5|gemini-2").containsMatchIn(m)) w = 1_000_000
    return w
  }

  fun historyTokens(items: List<ChatItem>): Int {
    val chars = items.sumOf {
      when (it) {
        is UserItem -> it.text.length + 1
        is AiItem -> it.text.length + 1
        is ToolGroupItem -> it.lines.sumOf { l -> l.text.length + 1 }
        is ErrorItem -> it.text.length
        is NoticeItem -> it.text.length
        else -> 0
      }
    }
    return Math.max(0, Math.round(chars / 3.8f))
  }

  data class ContextInfo(val used: Int, val window: Int, val history: Int, val system: Int, val tools: Int, val free: Int, val percent: Double, val buffer: Int = 0)

  fun context(items: List<ChatItem>, model: String): ContextInfo {
    val window = windowFor(model)
    val history = historyTokens(items)
    val used = history + SYSTEM_TOKENS + TOOL_TOKENS
    val pct = Math.min(100.0, Math.max(1.0, used.toDouble() / window * 100))
    // CTX-01: the auto-compact buffer is the last 20 percent of the window (compaction starts at 80, see untilCompact); free is what is left before it
    val buffer = Math.min((window * 0.2).toInt(), Math.max(0, window - used))
    return ContextInfo(used, window, history, SYSTEM_TOKENS, TOOL_TOKENS, Math.max(0, window - used - buffer), Math.round(pct * 10) / 10.0, buffer)
  }

  /** "N until auto-compact" (CTX-05): the desktop compacts at 80 percent of the window. */
  fun untilCompact(info: ContextInfo): Int = Math.max(0, (info.window * 0.8).toInt() - info.used)
}
