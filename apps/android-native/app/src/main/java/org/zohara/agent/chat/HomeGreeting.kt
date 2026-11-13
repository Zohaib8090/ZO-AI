package org.zohara.agent.chat

import android.content.Context
import java.time.LocalDateTime

/**
 * The glue between [Greetings] (pure) and the phone: reads `assets/greetings.json`, remembers the last open in SharedPreferences
 * ("zo_greeting") and keeps the message chosen for this launch, so the home screen does not change while the app is open.
 */
object HomeGreeting {
  private const val PREFS = "zo_greeting"
  private var chosen: Greetings.Message? = null
  private var table: Greetings.Table? = null

  /** A fallback if the bundled table cannot be read (it always can; this only keeps the home screen from crashing). */
  private const val PLAIN = "Welcome"

  fun text(context: Context, displayName: String, email: String, now: LocalDateTime = LocalDateTime.now(), rnd: Double = Math.random()): String {
    val t = table ?: runCatching { Greetings.parse(context.assets.open("greetings.json").bufferedReader().use { it.readText() }) }.getOrNull()?.also { table = it } ?: return PLAIN
    if (chosen == null) {
      val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
      val st = Greetings.State(
        lastDay = if (prefs.contains("last_day")) prefs.getLong("last_day", 0) else null,
        streak = prefs.getInt("streak", 0),
        lastId = prefs.getString("last_id", null),
      )
      val moment = Greetings.Moment(now.hour, now.dayOfWeek.value - 1, now.toLocalDate().toEpochDay())
      val (gap, streak) = Greetings.visit(t, st, moment.day)
      val pick = Greetings.pick(Greetings.candidates(t, moment.hour, moment.weekday, gap), st.lastId, rnd)
      chosen = pick
      prefs.edit().apply {
        putLong("last_day", st.lastDay?.let { maxOf(it, moment.day) } ?: moment.day)
        putInt("streak", streak)
        if (pick != null) putString("last_id", pick.id)
      }.apply()
    }
    return chosen?.let { Greetings.render(it, Greetings.firstName(displayName, email)) } ?: PLAIN
  }
}
