package org.zohara.agent.chat

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * The home greeting (NAV-18): one message from the table shared with the desktop and the web app
 * (`crates/zo-core/data/greetings.json`, bundled as `assets/greetings.json`; a test keeps the copy equal to the source).
 * The rules are the same as `zo-core/src/greetings.rs` and the web page; the "golden" cases in the JSON are checked in all three.
 * Pure Kotlin: the clock and the random number are arguments, so tests are deterministic.
 */
object Greetings {
  @Serializable
  data class Message(val id: String, val text: String, val time: List<String> = emptyList(), val weekday: List<String> = emptyList(), val gap: List<String> = emptyList())

  @Serializable
  data class Golden(val hour: Int, val weekday: String, val gap: String, val ids: List<String>)

  @Serializable
  data class Table(
    val version: Int,
    val max_len: Int,
    val away_days: Long,
    val back_days: Long,
    val streak_days: Int,
    val messages: List<Message>,
    val golden: List<Golden> = emptyList(),
  )

  /** What is remembered between opens. */
  data class State(val lastDay: Long? = null, val streak: Int = 0, val lastId: String? = null)

  /** The local moment of an open: hour 0..23, weekday 0 = Monday .. 6 = Sunday, day = days since 1970-01-01 in local time. */
  data class Moment(val hour: Int, val weekday: Int, val day: Long)

  enum class Gap(val tag: String) { None("none"), First("first"), Away("away"), Back("back"), Streak("streak") }

  val WEEKDAYS = listOf("mon", "tue", "wed", "thu", "fri", "sat", "sun")

  private val json = Json { ignoreUnknownKeys = true }

  fun parse(text: String): Table = json.decodeFromString(Table.serializer(), text)

  fun gapOf(tag: String): Gap = Gap.entries.firstOrNull { it.tag == tag } ?: Gap.None

  fun slot(hour: Int): String = when (hour) {
    in 5..8 -> "early"
    in 9..11 -> "morning"
    in 12..13 -> "noon"
    in 14..17 -> "afternoon"
    in 18..20 -> "evening"
    in 21..23 -> "night"
    else -> "late"
  }

  /** The gap to report and the streak after an open on [today]. */
  fun visit(t: Table, st: State, today: Long): Pair<Gap, Int> {
    val last = st.lastDay ?: return Gap.First to 1
    val days = today - last
    if (days <= 0) return Gap.None to maxOf(st.streak, 1) // a second open the same day, or a clock set back
    val streak = if (days == 1L) st.streak + 1 else 1
    val gap = when {
      days >= t.away_days -> Gap.Away
      days >= t.back_days -> Gap.Back
      streak >= t.streak_days -> Gap.Streak
      else -> Gap.None
    }
    return gap to streak
  }

  private fun weekdayMatches(tags: List<String>, wd: Int): Boolean =
    tags.isEmpty() || tags.any { (it == "monday" && wd == 0) || (it == "friday" && wd == 4) || (it == "weekend" && wd >= 5) }

  /** The messages that fit this hour, weekday and gap, in table order. */
  fun candidates(t: Table, hour: Int, weekday: Int, gap: Gap): List<Message> {
    val s = slot(hour)
    val fits = { m: Message -> (m.time.isEmpty() || s in m.time) && weekdayMatches(m.weekday, weekday) }
    if (gap != Gap.None) {
      val special = t.messages.filter { gap.tag in it.gap }.filter(fits)
      if (special.isNotEmpty()) return special
    }
    return t.messages.filter { it.gap.isEmpty() }.filter(fits)
  }

  private fun weight(m: Message): Double = 1.0 + (if (m.weekday.isEmpty()) 0.0 else 2.0) + (if (m.time.isEmpty()) 0.0 else 1.0)

  /** One of [cands]: never [lastId] unless it is the only one, weighted towards the specific ones. [rnd] is in [0, 1). */
  fun pick(cands: List<Message>, lastId: String?, rnd: Double): Message? {
    val pool = if (cands.size > 1) cands.filter { it.id != lastId } else cands
    var x = rnd.coerceIn(0.0, 0.999999) * pool.sumOf { weight(it) }
    for (m in pool) {
      x -= weight(m)
      if (x < 0) return m
    }
    return pool.lastOrNull()
  }

  /** The text with the name put in; without a name ", {name}" (and a bare "{name}") disappears. */
  fun render(m: Message, name: String): String {
    val n = name.trim()
    return if (n.isEmpty()) m.text.replace(", {name}", "").replace("{name}", "").trim() else m.text.replace("{name}", n)
  }

  /** The first word of the display name, else the part of the e-mail before the @, capitalised ("" when neither is known). */
  fun firstName(displayName: String, email: String): String {
    val first = displayName.trim().split(Regex("\\s+")).firstOrNull().orEmpty()
    val raw = first.ifEmpty { email.substringBefore('@').trim() }
    return raw.replaceFirstChar { it.uppercase() }
  }

  /** The greeting for an open and the state to remember. */
  fun greet(t: Table, st: State, m: Moment, name: String, rnd: Double): Pair<String, State> {
    val (gap, streak) = visit(t, st, m.day)
    val chosen = pick(candidates(t, m.hour, m.weekday, gap), st.lastId, rnd)
    val next = State(lastDay = st.lastDay?.let { maxOf(it, m.day) } ?: m.day, streak = streak, lastId = chosen?.id)
    return (chosen?.let { render(it, name) } ?: "") to next
  }
}
