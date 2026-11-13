package org.zohara.agent.chat

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.agent.core.net.QuestionOption
import org.zohara.agent.core.net.QuestionSpec
import org.zohara.agent.core.net.SendEvent

/** Rows CHAT-38/39/42/45/50, NAV-19, PERM-04, SRV-11, INTER-14 at the state-holder level (no screen). */
@OptIn(ExperimentalCoroutinesApi::class)
class ChatSessionMoreTest {
  private class Clock(var now: Long = 10_000L) : () -> Long { override fun invoke() = now }

  private fun TestScope.make(be: FakeBackend = FakeBackend(), env: FakeEnv = FakeEnv(), clock: Clock = Clock()) =
    Triple(ChatSession(be, backgroundScope, env, clock = clock), be, env)

  // CHAT-42
  @Test fun aChatAnotherDeviceChangedIsReadAgainWhenTheScreenComesBack() = runTest(UnconfinedTestDispatcher()) {
    val clock = Clock()
    val (s, be, _) = make(clock = clock)
    be.chats["c1"] = detail("c1", "Title", entry("msg-user", "hello"), entry("msg-ai", "old answer"))
    s.open("c1"); advanceUntilIdle()
    assertEquals("old answer", (s.state.value.items.last() as AiItem).text)
    // the other device added a message
    be.chats["c1"] = detail("c1", "Title", entry("msg-user", "hello"), entry("msg-ai", "old answer"), entry("msg-user", "more"), entry("msg-ai", "from the laptop"))
    s.refreshIfIdle(); advanceUntilIdle()
    assertEquals("old answer", (s.state.value.items.last() as AiItem).text) // opened a moment ago: not again at once
    clock.now += 5_000
    s.refreshIfIdle(); advanceUntilIdle()
    assertEquals("from the laptop", (s.state.value.items.last() as AiItem).text)
  }

  @Test fun nothingIsOverwrittenWhileAnAnswerRuns() = runTest(UnconfinedTestDispatcher()) {
    val clock = Clock()
    val (s, be, _) = make(clock = clock)
    be.chats["c1"] = detail("c1", "T", entry("msg-user", "x"))
    val ch = be.newStream()
    s.submit("x"); ch.send(SendEvent.Chat("c1", "T")); ch.send(SendEvent.Delta("typing"))
    be.chats["c1"] = detail("c1", "T", entry("msg-user", "other device"))
    clock.now += 60_000
    s.refreshIfIdle(); advanceUntilIdle()
    assertEquals("typing", (s.state.value.items.last() as AiItem).text)
    ch.close()
  }

  // CHAT-39 (adapted): the person may leave; the answer goes on in its own session
  @Test fun leavingForAnotherChatDoesNotStopTheRunningAnswer() = runTest(UnconfinedTestDispatcher()) {
    val envs = FakeEnv()
    val backends = mutableListOf<FakeBackend>()
    val hub = ChatHub({
      FakeBackend().also { b -> b.chats["c2"] = detail("c2", "Other", entry("msg-user", "hi"), entry("msg-ai", "hello")); backends += b }.let { ChatSession(it, backgroundScope, envs) }
    })
    val home = hub.home()
    val be = backends[0]
    val ch = be.newStream()
    home.submit("long task")
    ch.send(SendEvent.Chat("c1", "Long task")); ch.send(SendEvent.Delta("part one"))
    hub.promote()
    // the person opens another chat
    val other = hub.forId("c2"); advanceUntilIdle()
    assertEquals("hello", (other.state.value.items.last() as AiItem).text)
    ch.send(SendEvent.Delta(" and part two"))
    val running = hub.forId("c1")
    assertEquals("part one and part two", (running.state.value.items.last() as AiItem).text)
    assertTrue(running.state.value.busy)
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    assertFalse(running.state.value.busy)
    assertTrue(envs.toasts.isEmpty())
  }

  // CHAT-50, CHAT-38
  @Test fun theTitleIsEmptyOnHomeNewChatAfterTheFirstSendAndTheServersTitleAfterThat() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    assertEquals("", Transcript.displayTitle(s.state.value.title, s.state.value.items.isNotEmpty() || s.state.value.busy))
    val ch = be.newStream()
    s.submit("What is the weather in Lahore today and tomorrow")
    assertEquals("New chat", Transcript.displayTitle(s.state.value.title, s.state.value.items.isNotEmpty() || s.state.value.busy))
    ch.send(SendEvent.Chat("c9", "What is the weather in Lahore today and"))
    assertEquals("What is the weather in Lahore today and", s.state.value.title)
    ch.close(); advanceUntilIdle()
  }

  // CHAT-45
  @Test fun openingAChatEndsLoadingWithItsRows() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    be.chats["c1"] = detail("c1", "T", entry("msg-user", "x"))
    s.open("c1"); advanceUntilIdle()
    assertFalse(s.state.value.loading)
    assertEquals(1, s.state.value.items.size)
  }

  // NAV-19: the draft is kept in the session; a failed send puts it back
  @Test fun theDraftStaysInTheSessionUntilTheMessageWentOut() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    s.draft.value = "half written"
    assertEquals("half written", s.draft.value)
    be.newStream()
    assertTrue(s.submit(s.draft.value))
    s.draft.value = ""
    assertEquals("", s.draft.value)
  }

  // PERM-04: the saved mode applies; the request carries no per-message mode (the transport can: see ZoApiTest)
  @Test fun theRequestKeepsTheSavedModeAndCarriesNoPerMessageMode() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    val ch = be.newStream()
    s.submit("hi")
    assertNull(be.requests.single().mode)
    ch.close(); advanceUntilIdle()
  }

  // SRV-11: a card nobody answered in time is over; the server says so with 404 (reply returns false)
  @Test fun anApprovalAndAQuestionThatTimedOutBecomeNoAnswer() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    val ch = be.newStream()
    s.submit("go"); ch.send(SendEvent.Stream(7)); ch.send(SendEvent.Chat("c1", "go"))
    ch.send(SendEvent.Approval(1, "zohara_shell", "zohara_shell\ncmd: ls"))
    be.replyResult = { false }
    s.approve(s.state.value.items.filterIsInstance<ApprovalItem>().single().key, true)
    assertEquals("No answer: denied", s.state.value.items.filterIsInstance<ApprovalItem>().single().result)
    ch.send(SendEvent.Question(2, listOf(QuestionSpec("Which?", "H", false, listOf(QuestionOption("A", ""), QuestionOption("B", ""))))))
    s.skip(s.state.value.items.filterIsInstance<QuestionItem>().single().key)
    assertEquals("No answer was given", s.state.value.items.filterIsInstance<QuestionItem>().single().result)
    assertFalse(s.state.value.questionOpen)
    ch.close(); advanceUntilIdle()
  }

  // INTER-14: answered somewhere else: the notification goes away when the answer goes on
  @Test fun theNotificationIsCancelledWhenTheAnswerContinuesAfterAnApproval() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    s.submit("go"); ch.send(SendEvent.Stream(7)); ch.send(SendEvent.Chat("c1", "go"))
    ch.send(SendEvent.Approval(1, "zohara_shell", "zohara_shell\ncmd: ls"))
    assertEquals(1, env.needs.size)
    assertTrue(env.done.isEmpty())
    ch.send(SendEvent.ToolDone("zohara_shell", true, "ok")) // the desktop answered
    assertEquals(listOf("Approval c1 1"), env.done)
    ch.send(SendEvent.Delta("fine")); // not cancelled twice
    assertEquals(1, env.done.size)
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
  }

  @Test fun aQuestionNoticeIsCancelledWhenTheTurnEndsWithoutAnAnswer() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    s.submit("go"); ch.send(SendEvent.Stream(7)); ch.send(SendEvent.Chat("c1", "go"))
    ch.send(SendEvent.Question(3, listOf(QuestionSpec("Which?", "H", false, listOf(QuestionOption("A", ""), QuestionOption("B", ""))))))
    assertTrue(env.needs.single().startsWith("Question c1 7 3"))
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    assertEquals(listOf("Question c1 3"), env.done)
  }
}
