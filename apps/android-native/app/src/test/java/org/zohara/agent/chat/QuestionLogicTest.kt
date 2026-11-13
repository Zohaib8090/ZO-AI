package org.zohara.agent.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.agent.core.net.QuestionAnswer
import org.zohara.agent.core.net.QuestionOption
import org.zohara.agent.core.net.QuestionSpec

class QuestionLogicTest {
  private val single = QuestionSpec("Which colour?", "Colour", false, listOf(QuestionOption("Red", "warm"), QuestionOption("Blue", "cool")))
  private val multi = QuestionSpec("Which fruits?", "Fruit", true, listOf(QuestionOption("Apple"), QuestionOption("Pear"), QuestionOption("Plum")))

  @Test fun singleSelectKeepsOneChoiceAndOtherClearsIt() {
    var d = QuestionDraft(listOf(single))
    assertFalse(d.currentAnswered)
    d = d.toggle(0, "Red").toggle(0, "Blue")
    assertEquals(setOf("Blue"), d.selected[0])
    d = d.setOther(0, "teal")
    assertTrue(d.selected[0].isEmpty())
    assertEquals(listOf(QuestionAnswer(emptyList(), "teal")), d.answers())
    d = d.toggle(0, "Red")
    assertFalse(d.otherOn[0])
    assertEquals(listOf(QuestionAnswer(listOf("Red"), null)), d.answers())
  }

  @Test fun multiSelectToggles_andAnswersKeepOfferedOrder() {
    var d = QuestionDraft(listOf(multi))
    d = d.toggle(0, "Plum").toggle(0, "Apple").toggle(0, "Pear").toggle(0, "Pear")
    assertEquals(listOf("Apple", "Plum"), d.answers()[0].selected)
    d = d.setOther(0, "kiwi")
    assertEquals(setOf("Apple", "Plum"), d.selected[0])
    assertEquals("kiwi", d.answers()[0].other)
  }

  @Test fun otherOffMeansNoOtherEvenWithText() {
    var d = QuestionDraft(listOf(multi)).toggle(0, "Apple").setOther(0, "kiwi")
    d = d.toggleOther(0)
    assertNull(d.answers()[0].other)
  }

  @Test fun severalQuestionsPageThroughBackAndNext() {
    var d = QuestionDraft(listOf(single, multi))
    assertTrue(d.first); assertFalse(d.last)
    d = d.toggle(0, "Red").next()
    assertEquals(1, d.page); assertTrue(d.last)
    assertFalse(d.allAnswered)
    d = d.toggle(1, "Apple")
    assertTrue(d.allAnswered)
    d = d.back()
    assertEquals(0, d.page)
    assertEquals(2, d.answers().size)
    assertEquals(d, d.back())
  }

  @Test fun controlCharactersAreStrippedAndOtherIsCutAt2000() {
    val d = QuestionDraft(listOf(single)).setOther(0, "a\u0000b\u0007c" + "x".repeat(3000))
    assertEquals(2000, d.other[0].length)
    assertTrue(d.other[0].startsWith("abc"))
    assertEquals(2000, d.answers()[0].other!!.length)
  }

  @Test fun validateChecksCountOfferedLabelsAndLength() {
    assertNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(listOf("Red")))))
    assertNotNull(QuestionLogic.validate(listOf(single), emptyList()))
    assertNotNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(listOf("Green")))))
    assertNotNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(listOf("Red", "Blue")))))
    assertNotNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(emptyList(), null))))
    assertNotNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(emptyList(), "x".repeat(2001)))))
    assertNull(QuestionLogic.validate(listOf(multi), listOf(QuestionAnswer(listOf("Apple", "Pear")))))
  }

  @Test fun youDecideStandsAloneAndIsAnAnswer() {
    var d = QuestionDraft(listOf(single, multi))
    assertFalse(d.answered(0))
    d = d.toggle(0, "Red").setOther(0, "teal").toggleDecide(0)
    assertTrue(d.decide[0] && d.selected[0].isEmpty() && !d.otherOn[0] && d.other[0].isEmpty())
    assertTrue(d.answered(0))
    assertEquals(QuestionAnswer(emptyList(), null, decide = true), d.answers()[0])
    // choosing anything clears it, also in a multi-select
    assertFalse(d.toggle(0, "Blue").decide[0])
    assertFalse(d.toggleOther(0).decide[0])
    assertFalse(d.setOther(0, "x").decide[0])
    val m = d.toggleDecide(1)
    assertTrue(m.decide[1])
    assertFalse(m.toggle(1, "Apple").decide[1])
    assertFalse("a second tap turns it off", d.toggleDecide(0).decide[0])
    // validate and the summary
    assertNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(emptyList(), null, decide = true))))
    assertNotNull(QuestionLogic.validate(listOf(single), listOf(QuestionAnswer(listOf("Red"), null, decide = true))))
    assertEquals("you left it to Zohara Agent; Apple", QuestionLogic.summary(listOf(QuestionAnswer(emptyList(), null, true), QuestionAnswer(listOf("Apple")))))
    assertTrue(QuestionLogic.isDecideKey(androidx.compose.ui.input.key.Key.D))
  }

  @Test fun summaryTextsFollowTheContract() {
    assertEquals("Red", QuestionLogic.summary(listOf(QuestionAnswer(listOf("Red")))))
    assertEquals("Red, Blue - Other: teal", QuestionLogic.summary(listOf(QuestionAnswer(listOf("Red", "Blue"), "teal"))))
    assertEquals("Other: teal", QuestionLogic.summary(listOf(QuestionAnswer(emptyList(), "teal"))))
    assertEquals("Red; Apple", QuestionLogic.summary(listOf(QuestionAnswer(listOf("Red")), QuestionAnswer(listOf("Apple")))))
    val q = QuestionItem(1, 7, listOf(single), QuestionState.Answered("Red"))
    assertEquals("You answered: Red", q.result)
    assertEquals("Skipped", q.copy(state = QuestionState.Skipped).result)
    assertEquals("No answer was given", q.copy(state = QuestionState.Expired).result)
  }

  @Test fun approvalResultLines() {
    val a = ApprovalItem(1, 1, "zohara_shell", "zohara_shell\ncmd: rm x", state = ApprovalState.Allowed)
    assertEquals("Allowed: zohara_shell cmd: rm x", a.result)
    assertEquals("Denied: zohara_shell cmd: rm x", a.copy(state = ApprovalState.Denied).result)
    assertEquals("No answer: denied", a.copy(state = ApprovalState.NoAnswer).result)
  }
}
