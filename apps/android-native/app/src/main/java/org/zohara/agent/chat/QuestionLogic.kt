package org.zohara.agent.chat

import org.zohara.agent.core.net.QuestionAnswer
import org.zohara.agent.core.net.QuestionSpec
import org.zohara.agent.core.net.ZoApi

object QuestionLogic {
  const val MAX_OTHER = 2000

  /** the D key: "You decide" (optional shortcut) */
  fun isDecideKey(key: androidx.compose.ui.input.key.Key): Boolean = key == androidx.compose.ui.input.key.Key.D

  /** 1..6 for the number keys that pick an option (INTER-09), else null */
  fun digitOf(key: androidx.compose.ui.input.key.Key): Int? = when (key) {
    androidx.compose.ui.input.key.Key.One, androidx.compose.ui.input.key.Key.NumPad1 -> 1
    androidx.compose.ui.input.key.Key.Two, androidx.compose.ui.input.key.Key.NumPad2 -> 2
    androidx.compose.ui.input.key.Key.Three, androidx.compose.ui.input.key.Key.NumPad3 -> 3
    androidx.compose.ui.input.key.Key.Four, androidx.compose.ui.input.key.Key.NumPad4 -> 4
    androidx.compose.ui.input.key.Key.Five, androidx.compose.ui.input.key.Key.NumPad5 -> 5
    androidx.compose.ui.input.key.Key.Six, androidx.compose.ui.input.key.Key.NumPad6 -> 6
    else -> null
  }

  /** "Red, Blue - Other: teal"; several questions are joined with "; " (INTER-08). */
  fun summary(answers: List<QuestionAnswer>): String = answers.joinToString("; ") { a ->
    if (a.decide) return@joinToString "you left it to Zohara Agent"
    val labels = a.selected.joinToString(", ")
    val other = a.other?.takeIf { it.isNotBlank() }
    when {
      other == null -> labels
      labels.isEmpty() -> "Other: $other"
      else -> "$labels - Other: $other"
    }
  }

  /** Checks answers the way INTERACTION.md says before they are sent: one per question, only offered labels, other <= 2000, no control characters. */
  fun validate(specs: List<QuestionSpec>, answers: List<QuestionAnswer>): String? {
    if (answers.size != specs.size) return "Answer every question first."
    for ((i, a) in answers.withIndex()) {
      if (a.decide) {
        if (a.selected.isNotEmpty() || !a.other.isNullOrBlank()) return "You decide cannot be mixed with a choice."
        continue
      }
      val offered = specs[i].options.map { it.label }.toSet()
      if (a.selected.any { it !in offered }) return "That is not one of the choices."
      if (!specs[i].multiSelect && a.selected.size > 1) return "Pick one choice."
      if ((a.other?.length ?: 0) > MAX_OTHER) return "That answer is too long."
      if (a.selected.isEmpty() && a.other.isNullOrBlank()) return "Answer every question first."
    }
    return null
  }
}

/**
 * The state of a question card while the person fills it in (INTER-06): the page (question) shown, the picked labels and the "Other" text of
 * every question. Immutable; every change returns a new one.
 */
data class QuestionDraft(
  val specs: List<QuestionSpec>,
  val page: Int = 0,
  val selected: List<Set<String>> = specs.map { emptySet() },
  val otherOn: List<Boolean> = specs.map { false },
  val other: List<String> = specs.map { "" },
  /** "You decide" per question: Zohara Agent picks the best option itself and says which and why */
  val decide: List<Boolean> = specs.map { false },
) {
  val last: Boolean get() = page >= specs.lastIndex
  val first: Boolean get() = page == 0

  fun answered(i: Int): Boolean = decide[i] || selected[i].isNotEmpty() || (otherOn[i] && other[i].isNotBlank())
  val currentAnswered: Boolean get() = answered(page)
  val allAnswered: Boolean get() = specs.indices.all { answered(it) }

  fun toggle(i: Int, label: String): QuestionDraft {
    val multi = specs[i].multiSelect
    val cur = selected[i]
    val next = if (multi) (if (label in cur) cur - label else cur + label) else setOf(label)
    return copy(
      selected = selected.toMutableList().also { it[i] = next },
      otherOn = if (multi) otherOn else otherOn.toMutableList().also { it[i] = false },
      decide = decide.toMutableList().also { it[i] = false },
    )
  }

  /** the "You decide" row: on clears the choices and the Other text; picking anything else turns it off again */
  fun toggleDecide(i: Int): QuestionDraft {
    val on = !decide[i]
    return if (!on) copy(decide = decide.toMutableList().also { it[i] = false })
    else copy(
      decide = decide.toMutableList().also { it[i] = true },
      selected = selected.toMutableList().also { it[i] = emptySet() },
      otherOn = otherOn.toMutableList().also { it[i] = false },
      other = other.toMutableList().also { it[i] = "" },
    )
  }

  /** the "Other" row: ticking it (single select) clears the options, typing in it ticks it */
  fun toggleOther(i: Int): QuestionDraft {
    val multi = specs[i].multiSelect
    val on = !otherOn[i]
    return copy(
      otherOn = otherOn.toMutableList().also { it[i] = on },
      selected = if (!multi && on) selected.toMutableList().also { it[i] = emptySet() } else selected,
      decide = if (on) decide.toMutableList().also { it[i] = false } else decide,
    )
  }

  fun setOther(i: Int, text: String): QuestionDraft {
    val t = text.filter { !it.isISOControl() || it == '\n' || it == '\t' }.take(QuestionLogic.MAX_OTHER)
    val turnOn = t.isNotEmpty() || otherOn[i]
    val d = copy(
      other = other.toMutableList().also { it[i] = t }, otherOn = otherOn.toMutableList().also { it[i] = turnOn },
      decide = if (t.isNotEmpty()) decide.toMutableList().also { it[i] = false } else decide,
    )
    return if (!specs[i].multiSelect && t.isNotEmpty()) d.copy(selected = d.selected.toMutableList().also { it[i] = emptySet() }) else d
  }

  fun next() = copy(page = (page + 1).coerceAtMost(specs.lastIndex))
  fun back() = copy(page = (page - 1).coerceAtLeast(0))

  /** the labels in the order the options were offered */
  fun answers(): List<QuestionAnswer> = specs.indices.map { i ->
    if (decide[i]) return@map QuestionAnswer(emptyList(), null, decide = true)
    val labels = specs[i].options.map { it.label }.filter { it in selected[i] }
    val o = if (otherOn[i]) ZoApi.sanitizeOther(other[i]).takeIf { it.isNotBlank() } else null
    QuestionAnswer(labels, o)
  }

  companion object {
    /** keeps a half-filled card over rotation and process death (the draft is plain lists, so a Bundle takes it) */
    fun saver(specs: List<QuestionSpec>): androidx.compose.runtime.saveable.Saver<QuestionDraft, Any> = androidx.compose.runtime.saveable.listSaver(
      save = { d -> listOf(d.page, ArrayList(d.selected.map { ArrayList(it) }), ArrayList(d.otherOn), ArrayList(d.other), ArrayList(d.decide)) },
      restore = { l ->
        @Suppress("UNCHECKED_CAST")
        QuestionDraft(specs, l[0] as Int, (l[1] as List<List<String>>).map { it.toSet() }, l[2] as List<Boolean>, l[3] as List<String>, l[4] as List<Boolean>)
      },
    )
  }
}
