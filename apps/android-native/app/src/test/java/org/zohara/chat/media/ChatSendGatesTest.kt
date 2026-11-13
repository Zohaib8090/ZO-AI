package org.zohara.chat.media

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.chat.chat.ChatEnv
import org.zohara.chat.chat.ChatSession
import org.zohara.chat.chat.FakeBackend
import org.zohara.chat.chat.FakeEnv
import org.zohara.chat.core.net.SendEvent
import org.zohara.chat.core.store.RunOn

/** COMP-19, COMP-20, NOTIF-01: what the chat session asks the media and link modules before and after a send. */
@OptIn(ExperimentalCoroutinesApi::class)
class ChatSendGatesTest {
  private class Env(val base: FakeEnv, var busy: Boolean = false, var stop: String? = null) : ChatEnv by base {
    val finished = ArrayList<String>()
    var looks = 0
    override val attachmentsBusy: Boolean get() = busy
    override val checksBeforeSend: Boolean get() = true
    override suspend fun beforeSend(): String? { looks++; return stop }
    override fun answerFinished(chatId: String?, title: String, answer: String) { finished += "$chatId|$title|$answer" }
  }

  @Test fun sendWaitsWhileAttachmentsAreBeingRead() = runTest(UnconfinedTestDispatcher()) {
    val env = Env(FakeEnv(), busy = true)
    val be = FakeBackend()
    val s = ChatSession(be, backgroundScope, env)
    assertFalse(s.submit("hello"))
    assertEquals(listOf("Still reading your attachments. One moment."), env.base.toasts)
    assertTrue(be.requests.isEmpty())
  }

  @Test fun aComputerThatIsNotReachableKeepsTheTextAndSendsNothing() = runTest(UnconfinedTestDispatcher()) {
    val base = FakeEnv().also { it.run = RunOn("d1") }
    val env = Env(base, stop = "Your computer is not reachable.")
    val be = FakeBackend()
    val s = ChatSession(be, backgroundScope, env)
    s.draft.value = "run it"
    assertFalse(s.submit("run it"))
    advanceUntilIdle()
    assertEquals(1, env.looks)
    assertEquals(listOf("Your computer is not reachable."), base.toasts)
    assertEquals("run it", s.draft.value)
    assertTrue(be.requests.isEmpty())
  }

  @Test fun aReachableComputerGetsTheMessageAndTheBoxIsCleared() = runTest(UnconfinedTestDispatcher()) {
    val base = FakeEnv().also { it.run = RunOn("d1") }
    val env = Env(base)
    val be = FakeBackend()
    val ch = be.newStream()
    val s = ChatSession(be, backgroundScope, env)
    s.draft.value = "run it"
    assertFalse(s.submit("run it")) // the look comes first; the box is cleared when the message goes out
    advanceUntilIdle()
    assertEquals("desktop", be.requests.single().target)
    assertEquals("d1", be.requests.single().device)
    assertEquals("", s.draft.value)
    ch.close()
  }

  @Test fun theCloudIsNotChecked() = runTest(UnconfinedTestDispatcher()) {
    val env = Env(FakeEnv())
    val be = FakeBackend()
    val ch = be.newStream()
    val s = ChatSession(be, backgroundScope, env)
    assertTrue(s.submit("hi"))
    assertEquals(0, env.looks)
    ch.close()
  }

  @Test fun anAnswerThatEndsWithDoneTellsTheNotifier() = runTest(UnconfinedTestDispatcher()) {
    val env = Env(FakeEnv())
    val be = FakeBackend()
    val ch = be.newStream()
    val s = ChatSession(be, backgroundScope, env)
    assertTrue(s.submit("hi"))
    ch.send(SendEvent.Chat("c1", "A title")); ch.send(SendEvent.Text("answer")); ch.send(SendEvent.Done); ch.close()
    advanceUntilIdle()
    assertEquals(listOf("c1|A title|answer"), env.finished)
  }

  @Test fun aStoppedAnswerDoesNotNotify() = runTest(UnconfinedTestDispatcher()) {
    val env = Env(FakeEnv())
    val be = FakeBackend()
    val ch = be.newStream()
    val s = ChatSession(be, backgroundScope, env)
    assertTrue(s.submit("hi"))
    ch.send(SendEvent.Chat("c1", "T")); ch.send(SendEvent.Stopped); ch.close()
    advanceUntilIdle()
    assertTrue(env.finished.isEmpty())
  }

  @Test fun stopOnAComputerSaysSoAndKeepsTheSameChat() = runTest(UnconfinedTestDispatcher()) { // LINK-10, LINK-13
    val base = FakeEnv().also { it.run = RunOn("d1") }
    val be = FakeBackend()
    val ch = be.newStream()
    val s = ChatSession(be, backgroundScope, base)
    assertTrue(s.submit("run it"))
    ch.send(SendEvent.Stream(7, "desktop", "Laptop")); ch.send(SendEvent.Chat("c9", "T"))
    s.stop(); advanceUntilIdle()
    assertEquals(listOf("stop 7"), be.replies)
    assertTrue(base.toasts.contains("Stop sent to your computer."))
    ch.close(); advanceUntilIdle()
    assertTrue(s.submit("and again")) // same chat id on the next turn
    assertEquals("c9", be.requests.last().chat); assertEquals("desktop", be.requests.last().target)
  }
}
