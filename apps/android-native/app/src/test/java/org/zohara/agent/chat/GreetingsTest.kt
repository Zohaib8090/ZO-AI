package org.zohara.agent.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.io.File
import kotlin.random.Random

/** NAV-18: the home greeting. Mirrors the tests of crates/zo-core/src/greetings.rs; the golden cases are shared. */
class GreetingsTest {
  private val bundled = File("src/main/assets/greetings.json")
  private val source = File("../../../crates/zo-core/data/greetings.json")
  private val t = Greetings.parse(bundled.readText())
  private fun msg(id: String) = t.messages.first { it.id == id }

  @Test fun theBundledCopyEqualsTheSharedSource() {
    assumeTrue("the shared source is not next to this checkout", source.exists())
    assertEquals("run: cp crates/zo-core/data/greetings.json apps/android-native/app/src/main/assets/greetings.json", source.readText(), bundled.readText())
  }

  @Test fun tableHas18To20DistinctShortMessages() {
    assertTrue(t.messages.size in 18..20)
    assertEquals(t.messages.size, t.messages.map { it.id }.toSet().size)
    assertEquals(t.messages.size, t.messages.map { it.text.lowercase() }.toSet().size)
    t.messages.forEach {
      assertTrue(it.text, it.text.length <= t.max_len)
      if ("{name}" in it.text) assertTrue(it.text, ", {name}" in it.text)
    }
  }

  @Test fun everyHourAndWeekdayHasAtLeastTwoCandidates() {
    for (h in 0..23) for (d in 0..6) assertTrue("$h $d", Greetings.candidates(t, h, d, Greetings.Gap.None).size >= 2)
  }

  @Test fun specialGapsReturnOnlyTheirOwnMessages() {
    for (g in listOf(Greetings.Gap.First, Greetings.Gap.Away, Greetings.Gap.Back, Greetings.Gap.Streak)) {
      for (h in 0..23) for (d in 0..6) {
        val c = Greetings.candidates(t, h, d, g)
        assertTrue(c.isNotEmpty())
        assertTrue(c.all { g.tag in it.gap })
      }
    }
    assertTrue(Greetings.candidates(t, 10, 2, Greetings.Gap.None).all { it.gap.isEmpty() })
  }

  @Test fun nameIsUsedInSomeAndNeverInAll() {
    val named = t.messages.count { "{name}" in it.text }
    assertTrue(named >= 4 && named * 2 <= t.messages.size)
    for (h in 0..23) for (d in 0..6) assertTrue(Greetings.candidates(t, h, d, Greetings.Gap.None).any { "{name}" !in it.text })
  }

  @Test fun nameIsSubstitutedOrDroppedCleanly() {
    assertEquals("Good morning, Zohaib", Greetings.render(msg("morning-1"), "Zohaib"))
    assertEquals("Good morning", Greetings.render(msg("morning-1"), ""))
    assertEquals("Good morning", Greetings.render(msg("morning-1"), "  "))
    assertEquals("Still up?", Greetings.render(msg("late-1"), ""))
    assertEquals("Welcome. Let's make something.", Greetings.render(msg("first-1"), ""))
    assertEquals("Welcome, Ali. Let's make something.", Greetings.render(msg("first-1"), "Ali"))
    t.messages.forEach {
      val bare = Greetings.render(it, "")
      assertFalse(it.text, "{" in bare || " ," in bare || "  " in bare)
    }
  }

  @Test fun firstNameFromDisplayNameOrEmail() {
    assertEquals("Zohaib", Greetings.firstName("Zohaib Baig", "z@x.org"))
    assertEquals("Ali", Greetings.firstName("", "ali@x.org"))
    assertEquals("Ali", Greetings.firstName("  ", "ali@x.org"))
    assertEquals("", Greetings.firstName("", ""))
  }

  @Test fun pickerNeverRepeatsTheLastMessage() {
    for (h in listOf(0, 6, 10, 13, 16, 19, 22)) for (d in 0..6) {
      val c = Greetings.candidates(t, h, d, Greetings.Gap.None)
      for (last in c) for (i in 0 until 50) assertNotEquals(last.id, Greetings.pick(c, last.id, i / 50.0)!!.id)
    }
    val c = Greetings.candidates(t, 10, 2, Greetings.Gap.None)
    assertEquals(c.map { it.id }.toSet(), (0 until 100).map { Greetings.pick(c, null, it / 100.0)!!.id }.toSet())
    assertEquals(c.first().id, Greetings.pick(c, null, 0.0)!!.id)
    assertEquals(c.last().id, Greetings.pick(c, null, 1.0)!!.id)
    assertEquals(Greetings.pick(c, null, 0.3)!!.id, Greetings.pick(c, null, 0.3)!!.id)
    assertEquals(null, Greetings.pick(emptyList(), null, 0.5))
  }

  @Test fun manyOpensInARowDoNotRepeat() {
    var st = Greetings.State()
    val rnd = Random(7)
    for (i in 0 until 400) {
      val m = Greetings.Moment((i * 7) % 24, i % 7, 20000L + i)
      val (gap, _) = Greetings.visit(t, st, m.day)
      val (text, next) = Greetings.greet(t, st, m, "Ali", rnd.nextDouble())
      assertTrue(text.isNotEmpty())
      if (st.lastId != null && Greetings.candidates(t, m.hour, m.weekday, gap).size > 1) assertNotEquals("open $i", st.lastId, next.lastId)
      st = next
    }
  }

  @Test fun gapsFollowTheDaysSinceTheLastOpen() {
    fun s(last: Long?, streak: Int) = Greetings.State(last, streak, null)
    assertEquals(Greetings.Gap.First to 1, Greetings.visit(t, s(null, 0), 100))
    assertEquals(Greetings.Gap.None to 3, Greetings.visit(t, s(100, 3), 100))
    assertEquals(Greetings.Gap.None to 2, Greetings.visit(t, s(100, 1), 101))
    assertEquals(Greetings.Gap.Streak to 5, Greetings.visit(t, s(100, 4), 101))
    assertEquals(Greetings.Gap.None to 1, Greetings.visit(t, s(100, 9), 102))
    assertEquals(Greetings.Gap.Back to 1, Greetings.visit(t, s(100, 9), 103))
    assertEquals(Greetings.Gap.Back to 1, Greetings.visit(t, s(100, 9), 113))
    assertEquals(Greetings.Gap.Away to 1, Greetings.visit(t, s(100, 9), 114))
    assertEquals(Greetings.Gap.None to 9, Greetings.visit(t, s(100, 9), 90))
  }

  @Test fun firstOpenEverIsTheWelcome() {
    val (text, next) = Greetings.greet(t, Greetings.State(), Greetings.Moment(9, 2, 20000), "Ali", 0.4)
    assertEquals("Welcome, Ali. Let's make something.", text)
    assertEquals(Greetings.State(20000, 1, "first-1"), next)
  }

  @Test fun goldenCasesAreTheSharedContract() {
    assertTrue(t.golden.size >= 8)
    for (g in t.golden) {
      val wd = Greetings.WEEKDAYS.indexOf(g.weekday)
      assertTrue(wd >= 0)
      assertEquals("${g.hour} ${g.weekday} ${g.gap}", g.ids, Greetings.candidates(t, g.hour, wd, Greetings.gapOf(g.gap)).map { it.id })
    }
    assertNotNull(t.golden.firstOrNull())
  }
}
