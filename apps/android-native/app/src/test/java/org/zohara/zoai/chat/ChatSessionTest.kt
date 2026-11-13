package org.zohara.zoai.chat

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.net.QuestionAnswer
import org.zohara.zoai.core.net.QuestionOption
import org.zohara.zoai.core.net.QuestionSpec
import org.zohara.zoai.core.net.SendEvent
import org.zohara.zoai.core.net.TakeInfo
import org.zohara.zoai.core.store.RunOn

@OptIn(ExperimentalCoroutinesApi::class)
class ChatSessionTest {
  private fun TestScope.make(be: FakeBackend = FakeBackend(), env: FakeEnv = FakeEnv()) = Triple(ChatSession(be, backgroundScope, env), be, env)
  private fun ChatSession.types() = state.value.items.map { it::class.simpleName }

  private suspend fun TestScope.play(s: ChatSession, be: FakeBackend, text: String, vararg events: SendEvent, close: Boolean = true) {
    val ch = be.newStream()
    assertTrue(s.submit(text))
    for (e in events) ch.send(e)
    if (close) ch.close()
    advanceUntilIdle()
  }

  @Test fun aPlainAnswerStreamsIntoOneMessageAndIgnoresTheFinalText() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    be.chats["c1"] = detail("c1", "hi there", entry("msg-user", "hi there"), entry("msg-ai", "Hello from the fake model."))
    val ch = be.newStream()
    assertTrue(s.submit("  hi there  "))
    assertTrue(s.state.value.busy)
    assertEquals(listOf("UserItem", "ThinkingItem"), s.types())
    ch.send(SendEvent.Stream(1)); ch.send(SendEvent.Chat("c1", "hi there"))
    assertEquals("c1", s.state.value.chatId); assertEquals("hi there", s.state.value.title)
    ch.send(SendEvent.Delta("Hello ")); ch.send(SendEvent.Delta("from the "))
    assertEquals(listOf("UserItem", "AiItem"), s.types())
    assertTrue((s.state.value.items[1] as AiItem).live)
    ch.send(SendEvent.Delta("fake model.")); ch.send(SendEvent.Usage(12, 4)); ch.send(SendEvent.Text("Hello from the fake model."))
    assertEquals("Hello from the fake model.", (s.state.value.items[1] as AiItem).text)
    assertFalse((s.state.value.items[1] as AiItem).live)
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    val st = s.state.value
    assertFalse(st.busy)
    assertEquals(listOf("UserItem", "AiItem"), s.types())
    assertEquals(Usage(12, 4), st.usage)
    assertEquals("hi there", be.requests.single().text)
    assertNull(be.requests.single().chat)
    assertTrue(env.changed.isNotEmpty())
  }

  @Test fun aTextEventAloneBecomesTheAnswerWhenNothingStreamed() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    play(s, be, "q", SendEvent.Stream(1), SendEvent.Chat("c", "q"), SendEvent.Text("All at once."), SendEvent.Done)
    assertEquals("All at once.", (s.state.value.items.last() as AiItem).text)
  }

  @Test fun toolsFormOneGroupThatEndsWhenTheAnswerArrives() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    val ch = be.newStream()
    s.submit("run it")
    ch.send(SendEvent.Stream(2)); ch.send(SendEvent.Chat("c1", "run it"))
    ch.send(SendEvent.ToolStart("zohara_shell", "zohara_shell\ncmd: echo hi"))
    var g = s.state.value.items.filterIsInstance<ToolGroupItem>().single()
    assertTrue(g.active); assertEquals("Running a task in the terminal…", g.head)
    ch.send(SendEvent.ToolDone("zohara_shell", true, "hi"))
    ch.send(SendEvent.ToolStart("read_file", "read_file\npath: a")); ch.send(SendEvent.ToolDone("read_file", false, "no such file"))
    g = s.state.value.items.filterIsInstance<ToolGroupItem>().single()
    assertEquals(listOf(LineKind.Start, LineKind.Ok, LineKind.Start, LineKind.Fail), g.lines.map { it.kind })
    assertEquals("zohara_shell  cmd: echo hi", g.lines[0].text)
    assertEquals("hi", g.lines[1].text)
    ch.send(SendEvent.Delta("Done"))
    g = s.state.value.items.filterIsInstance<ToolGroupItem>().single()
    assertFalse(g.active); assertEquals("Ran 2 commands", g.head)
    assertEquals(listOf("UserItem", "ToolGroupItem", "AiItem"), s.types())
    // a second round of tools starts a new group after the answer
    ch.send(SendEvent.Text("Done")); ch.send(SendEvent.ToolStart("x", "x")); 
    assertEquals(2, s.state.value.items.count { it is ToolGroupItem })
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    assertTrue(s.state.value.items.filterIsInstance<ToolGroupItem>().none { it.active })
  }

  @Test fun visionNoticeIsInlineAndTheAnswerStillComes() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    play(s, be, "look", SendEvent.Stream(1), SendEvent.Chat("c", "look"), SendEvent.ToolDone("vision", false, "The model m1 cannot look at pictures."), SendEvent.Delta("ok"), SendEvent.Done)
    assertEquals(listOf("UserItem", "NoticeItem", "AiItem"), s.types())
  }

  @Test fun approvalCardAllowAndDeny() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    s.submit("run it")
    ch.send(SendEvent.Stream(5)); ch.send(SendEvent.Chat("c1", "run it"))
    ch.send(SendEvent.ToolStart("zohara_shell", "zohara_shell\ncmd: rm x"))
    ch.send(SendEvent.Approval(1, "zohara_shell", "zohara_shell\ncmd: rm x"))
    val card = s.state.value.items.filterIsInstance<ApprovalItem>().single()
    assertEquals(ApprovalState.Pending, card.state)
    assertTrue(env.needs.single().contains("Approval c1 5 1"))
    s.approve(card.key, true)
    assertEquals("approve 5 1 true", be.replies.single())
    val done = s.state.value.items.filterIsInstance<ApprovalItem>().single()
    assertEquals(ApprovalState.Allowed, done.state)
    assertEquals("Allowed: zohara_shell cmd: rm x", done.result)
    assertTrue(env.done.single().contains("Approval c1 1"))
    // the tool still reports into its group, which sits above the card
    ch.send(SendEvent.ToolDone("zohara_shell", true, "")); 
    assertEquals(LineKind.Ok, s.state.value.items.filterIsInstance<ToolGroupItem>().single().lines.last().kind)
    ch.send(SendEvent.Approval(2, "zohara_shell", "zohara_shell\ncmd: ls"))
    val k2 = s.state.value.items.filterIsInstance<ApprovalItem>().last().key
    s.approve(k2, false)
    assertEquals(ApprovalState.Denied, s.state.value.items.filterIsInstance<ApprovalItem>().last().state)
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
  }

  @Test fun approvalReplyIsRetriedOnceAndKeptOpenOnFailure() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    s.submit("x"); ch.send(SendEvent.Stream(3)); ch.send(SendEvent.Chat("c", "x")); ch.send(SendEvent.Approval(1, "t", "t"))
    be.replyError = ApiException(ErrorKind.Offline, "Offline. Retrying."); be.replyErrorTimes = 1
    val k = s.state.value.items.filterIsInstance<ApprovalItem>().single().key
    s.approve(k, true); advanceTimeBy(1000)
    assertEquals(2, be.replies.size) // one failure, one retry that worked
    assertEquals(ApprovalState.Allowed, s.state.value.items.filterIsInstance<ApprovalItem>().single().state)
    ch.send(SendEvent.Approval(2, "t", "t"))
    val k2 = s.state.value.items.filterIsInstance<ApprovalItem>().last().key
    be.replyError = ApiException(ErrorKind.Offline, "Offline. Retrying."); be.replyErrorTimes = 5
    s.approve(k2, true); advanceTimeBy(2000)
    val card = s.state.value.items.filterIsInstance<ApprovalItem>().last()
    assertEquals(ApprovalState.Pending, card.state); assertFalse(card.sending)
    assertTrue(env.toasts.contains("Could not send your answer"))
    ch.close()
  }

  @Test fun approvalOnTheComputerTurnsIntoTheWaitingRow() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make(env = FakeEnv().also { it.run = RunOn("d1") })
    val ch = be.newStream()
    s.submit("x"); ch.send(SendEvent.Stream(3, "desktop", "Laptop")); ch.send(SendEvent.Chat("c", "x"))
    assertEquals("Running on your computer (Laptop)", (s.state.value.items.filterIsInstance<NoticeItem>().single()).text)
    ch.send(SendEvent.Approval(1, "zohara_shell", "zohara_shell\ncmd: ls", local = true))
    assertEquals(1, s.state.value.items.count { it is WaitingItem })
    assertEquals(0, s.state.value.items.count { it is ApprovalItem })
    ch.send(SendEvent.ToolDone("zohara_shell", true, "a"))
    assertEquals(0, s.state.value.items.count { it is WaitingItem }) // cleared on the next event
    ch.send(SendEvent.Approval(2, "zohara_shell", "zohara_shell\ncmd: ls"))
    assertTrue((s.state.value.items.filterIsInstance<ApprovalItem>().single()).onComputer)
    be.replyError = ApiException(ErrorKind.Rejected, "approvals_on_computer", status = 409); be.replyErrorTimes = 5
    s.approve(s.state.value.items.filterIsInstance<ApprovalItem>().single().key, true); advanceUntilIdle()
    assertEquals(1, s.state.value.items.count { it is WaitingItem })
    assertEquals("desktop", be.requests.single().target); assertEquals("d1", be.requests.single().device)
    ch.close()
  }

  @Test fun restrictedAccountsNeverGetApprovalCards() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make(env = FakeEnv().also { it.restricted = true })
    play(s, be, "x", SendEvent.Stream(1), SendEvent.Chat("c", "x"), SendEvent.Approval(1, "t", "t"), SendEvent.Done)
    assertEquals(0, s.state.value.items.count { it is ApprovalItem })
  }

  @Test fun questionCardAnswerSkipAndComposerLock() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    val spec = QuestionSpec("Which colour?", "Colour", false, listOf(QuestionOption("Red"), QuestionOption("Blue")))
    s.submit("ask me"); ch.send(SendEvent.Stream(9)); ch.send(SendEvent.Chat("c", "ask me")); ch.send(SendEvent.Question(7, listOf(spec)))
    assertTrue(s.state.value.questionOpen)
    assertTrue(env.needs.single().contains("Question c 9 7 Which colour?"))
    assertFalse(s.submit("another")) // COMP-16: the composer is off, nothing is sent
    val k = s.state.value.items.filterIsInstance<QuestionItem>().single().key
    s.answer(k, listOf(QuestionAnswer(listOf("Red"), null))); advanceUntilIdle()
    assertEquals("answer 9 7 [([Red], null)]", be.replies.single())
    assertEquals("You answered: Red", s.state.value.items.filterIsInstance<QuestionItem>().single().result)
    assertFalse(s.state.value.questionOpen)
    ch.send(SendEvent.Question(8, listOf(spec)))
    s.skip(s.state.value.items.filterIsInstance<QuestionItem>().last().key)
    assertEquals("skip 9 8", be.replies.last())
    assertEquals("Skipped", s.state.value.items.filterIsInstance<QuestionItem>().last().result)
    ch.send(SendEvent.Question(10, listOf(spec)))
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    assertEquals("No answer was given", s.state.value.items.filterIsInstance<QuestionItem>().last().result) // timed out with the stream
    assertFalse(s.state.value.busy)
  }

  @Test fun errorEventShowsAnErrorRowAndEndsTheTurn() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    play(s, be, "x", SendEvent.Stream(1), SendEvent.Error("desktop_offline", "Your computer is not reachable."), SendEvent.Done)
    val e = s.state.value.items.last() as ErrorItem
    assertEquals("Your computer is not reachable.", e.text)
    assertEquals(1, env.offline)
    assertFalse(s.state.value.busy)
  }

  @Test fun noModelErrorOffersChooseAModel() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    play(s, be, "x", SendEvent.Error("", "No model is chosen on the computer yet. Open ZO-AI there and choose one."), SendEvent.Done)
    assertTrue((s.state.value.items.last() as ErrorItem).chooseModel)
  }

  @Test fun stoppedEventAddsTheStoppedRow() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    play(s, be, "x", SendEvent.Stream(1), SendEvent.Chat("c", "x"), SendEvent.Delta("par"), SendEvent.Stopped, SendEvent.Done)
    assertEquals(listOf("UserItem", "AiItem", "StoppedItem"), s.types())
    assertFalse((s.state.value.items[1] as AiItem).live)
  }

  @Test fun stopSendsTheReplyAndBeforeTheStreamExistsCancelsTheSend() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    val ch = be.newStream()
    s.submit("x")
    s.stop(); advanceUntilIdle() // no stream id yet
    assertFalse(s.state.value.busy)
    assertTrue(s.state.value.items.last() is StoppedItem)
    be.newStream().also { s.submit("y") }
    be.stream.send(SendEvent.Stream(4)); s.stop(); advanceUntilIdle()
    assertEquals("stop 4", be.replies.single())
    be.stream.close()
  }

  @Test fun connectionLostShowsTheSentenceAndReloadsTheChat() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "x"), entry("msg-ai", "saved answer"))
    play(s, be, "x", SendEvent.Stream(1), SendEvent.Chat("c1", "t"), SendEvent.ConnectionLost("lost"))
    val st = s.state.value
    assertFalse(st.busy)
    assertEquals(listOf("UserItem", "AiItem"), s.types()) // reloaded from the server
    assertEquals("saved answer", (st.items[1] as AiItem).text)
  }

  @Test fun connectionLostTextForAComputer() {
    assertTrue(ChatSession.CONNECTION_LOST_TEXT(true).startsWith("The connection to your computer was lost."))
    assertTrue(ChatSession.CONNECTION_LOST_TEXT(false).contains("open the chat again in a moment"))
  }

  @Test fun aRefusedSendRestoresTheDraftAndAttachments() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend().also { it.sendError = ApiException(ErrorKind.TooLarge, "The request is too big.", 413) }
    val env = FakeEnv()
    val s = ChatSession(be, backgroundScope, env)
    assertTrue(s.submit("a long one")); advanceUntilIdle()
    val st = s.state.value
    assertFalse(st.busy)
    assertEquals(listOf("ErrorItem"), s.types()) // the optimistic bubble is gone
    assertEquals("The request is too big.", (st.items.single() as ErrorItem).text)
    assertEquals("a long one", s.draft.value)
    assertEquals(1, env.restored)
  }

  @Test fun tooLongMessagesAreRefusedLocally() = runTest(UnconfinedTestDispatcher()) {
    val env = FakeEnv().also { it.restricted = true; it.messageLimit = 20_000 }
    val (s, be, _) = make(env = env)
    assertFalse(s.submit("x".repeat(20_001)))
    assertEquals("That message is too long (at most 20000 characters).", env.toasts.single())
    assertEquals(1, env.restored)
    assertTrue(be.requests.isEmpty())
    // a computer allows up to 100,000
    val env2 = FakeEnv().also { it.run = RunOn("d"); it.messageLimit = null }
    val s2 = ChatSession(be, backgroundScope, env2)
    be.newStream()
    assertTrue(s2.submit("x".repeat(50_000)))
    assertFalse(s2.submit("y")) // busy
    assertEquals("Wait for the current answer to finish.", env2.toasts.last())
  }

  @Test fun emptyMessagesAreNotSentButAttachmentsAloneAre() = runTest(UnconfinedTestDispatcher()) {
    val env = FakeEnv()
    val (s, be, _) = make(env = env)
    assertFalse(s.submit("   "))
    assertTrue(be.requests.isEmpty())
    env.attachmentCount.value = 1
    env.taken = Taken("", listOf(org.zohara.zoai.core.net.Attachments.textFile("n.txt", "hello")), listOf(org.zohara.zoai.core.net.AttachInfo("text", "n.txt", 5)), {})
    be.newStream()
    assertTrue(s.submit(""))
    assertEquals(1, be.requests.single().attachments.size)
    assertEquals("n.txt", (s.state.value.items.first() as UserItem).attach.single().name)
  }

  @Test fun editSendsANewTakeFromThatMessage() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "one"), entry("msg-ai", "A1"), entry("msg-user", "two"), entry("msg-ai", "A2"))
    s.open("c1"); advanceUntilIdle()
    assertEquals(4, s.state.value.items.size)
    be.newStream()
    s.edit(0, "  uno  ")
    assertEquals(0, be.requests.single().edit); assertEquals("uno", be.requests.single().text); assertEquals("c1", be.requests.single().chat)
    assertEquals(listOf("UserItem", "ThinkingItem"), s.types())
    assertEquals("uno", (s.state.value.items[0] as UserItem).text)
    be.stream.close()
  }

  @Test fun retryAsksTheLastMessageAgainWithEmptyTextAndKeepsTheBubble() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "one"), entry("msg-ai", "A1"), entry("msg-user", "two"), entry("msg-ai", "A2"))
    s.open("c1"); advanceUntilIdle()
    be.newStream()
    s.retry()
    assertEquals(1, be.requests.single().edit); assertEquals("", be.requests.single().text)
    assertEquals(listOf("UserItem", "AiItem", "UserItem", "ThinkingItem"), s.types())
    s.retry()
    assertEquals("Wait for the current answer to finish.", env.toasts.last())
    be.stream.close(); advanceUntilIdle()
    val empty = ChatSession(FakeBackend(), backgroundScope, env)
    empty.retry()
    assertEquals("Nothing to retry.", env.toasts.last())
  }

  @Test fun retryOnAComputerResendsTheTextBecauseEditIsNotForwarded() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make(env = FakeEnv().also { it.run = RunOn("d1") })
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "one"), entry("msg-ai", "A1"))
    s.open("c1"); advanceUntilIdle()
    be.newStream()
    s.retry()
    assertNull(be.requests.single().edit); assertEquals("one", be.requests.single().text)
    be.stream.close()
  }

  @Test fun takesArrowsReloadTheChatThroughSwitch() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "new"), entry("msg-ai", "A"), takes = listOf(TakeInfo(count = 2, current = 1, nth = 0)))
    s.open("c1"); advanceUntilIdle()
    assertEquals(1, (s.state.value.items[0] as UserItem).take?.current)
    be.switchResult = detail("c1", "t", entry("msg-user", "old"), entry("msg-ai", "B"), takes = listOf(TakeInfo(count = 2, current = 0, nth = 0)))
    s.switchTake(0, -1); advanceUntilIdle()
    assertEquals("switch c1 0 -1", be.calls.single())
    assertEquals("old", (s.state.value.items[0] as UserItem).text)
    be.switchError = ApiException(ErrorKind.Rejected, "There is no such take.")
    s.switchTake(0, -1); advanceUntilIdle()
    assertEquals("There is no such take.", env.toasts.last())
  }

  @Test fun openingALoadsAndAMissingChatIsAnErrorRow() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    s.open("nope"); advanceUntilIdle()
    assertEquals("that chat is gone", (s.state.value.items.single() as ErrorItem).text)
    assertFalse(s.state.value.loading)
  }

  @Test fun reopeningARunningChatKeepsItsRows() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "x"))
    val ch = be.newStream()
    s.submit("x"); ch.send(SendEvent.Chat("c1", "t")); ch.send(SendEvent.Delta("partial"))
    s.open("c1"); advanceUntilIdle()
    assertEquals("partial", (s.state.value.items.last() as AiItem).text)
    s.refreshIfIdle()
    assertEquals("partial", (s.state.value.items.last() as AiItem).text)
    ch.close()
  }

  @Test fun longChatsShowTheLastRowsAndShowEarlierAddsMore() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    val rows = (0 until 150).flatMap { listOf(entry("msg-user", "q$it"), entry("msg-ai", "a$it")) }.toTypedArray()
    be.chats["big"] = detail("big", "t", *rows)
    s.open("big"); advanceUntilIdle()
    assertEquals(300, s.state.value.items.size)
    assertEquals(240, s.state.value.hiddenCount)
    s.showEarlier()
    assertEquals(180, s.state.value.hiddenCount)
    repeat(10) { s.showEarlier() }
    assertEquals(0, s.state.value.hiddenCount)
  }

  @Test fun compactReportsTheServersSentences() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    var result: String? = "unset"
    s.compact { result = it }
    assertEquals("No active chat to compact.", result)
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "x"), entry("msg-ai", "y"))
    s.open("c1"); advanceUntilIdle()
    be.compactError = ApiException(ErrorKind.Rejected, "The conversation is too short to compact.")
    s.compact { result = it }; advanceUntilIdle()
    assertEquals("The conversation is too short to compact.", result)
    be.compactError = null
    s.compact { result = it }; advanceUntilIdle()
    assertNull(result); assertEquals(listOf("c1"), be.compacted)
    val ch = be.newStream()
    s.submit("more")
    s.compact { result = it }
    assertEquals("Wait until the answer is finished.", result)
    ch.close()
  }

  @Test fun deleteAndPinCallTheServerAndRefreshTheLists() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, env) = make()
    be.chats["c1"] = detail("c1", "t", entry("msg-user", "x"))
    s.open("c1"); advanceUntilIdle()
    s.pin(true); advanceUntilIdle()
    assertEquals(listOf("c1" to true), be.pinned)
    var ok = false
    s.delete { ok = it }; advanceUntilIdle()
    assertTrue(ok); assertEquals(listOf("c1"), be.deleted)
    assertTrue(env.changed.size >= 2)
  }

  @Test fun keepRunningAndTheComputerGoIntoTheRequest() = runTest(UnconfinedTestDispatcher()) {
    val env = FakeEnv().also { it.keep = true }
    val (s, be, _) = make(env = env)
    be.newStream(); s.submit("x")
    assertTrue(be.requests.single().keepRunning)
    be.stream.close()
  }

  @Test fun theHomeSessionMovesToTheListWhenItGetsAChatId() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend(); val env = FakeEnv()
    val hub = ChatHub({ ChatSession(be, backgroundScope, env) })
    val home = hub.home()
    val ch = be.newStream()
    home.submit("hello"); ch.send(SendEvent.Chat("c9", "hello")); ch.send(SendEvent.Delta("hi"))
    val moved = hub.promote()
    assertTrue(moved === home)
    assertTrue(hub.home() !== home)
    assertTrue(hub.forId("c9") === home) // the answer keeps streaming into the same session
    ch.send(SendEvent.Delta(" there"))
    assertEquals("hi there", (home.state.value.items.last() as AiItem).text)
    ch.close()
    hub.remove("c9")
  }

  @Test fun pingAndUnknownEventsAreIgnored() = runTest(UnconfinedTestDispatcher()) {
    val (s, be, _) = make()
    val ch = be.newStream()
    s.submit("x")
    val before = s.state.value.items
    ch.send(SendEvent.Ping); ch.send(SendEvent.Unknown("approval_wait2")); ch.send(SendEvent.Stream(1))
    assertEquals(before, s.state.value.items)
    assertTrue(s.state.value.busy)
    ch.send(SendEvent.Done); ch.close(); advanceUntilIdle()
    assertFalse(s.state.value.busy)
  }

  @Test fun aComputerTurnRefreshesTheListsAgainAfterSixSeconds() = runTest(UnconfinedTestDispatcher()) {
    val env = FakeEnv().also { it.run = RunOn("d1") }
    val (s, be, _) = make(env = env)
    play(s, be, "x", SendEvent.Stream(1, "desktop", "Laptop"), SendEvent.Chat("c", "x"), SendEvent.Done)
    assertTrue(env.changed.contains(0L)); assertTrue(env.changed.contains(6000L))
  }

  @Test fun theCounterShowsFromNinetyPercentOfTheLimit() {
    assertFalse(showCounter(17_999, 20_000)); assertTrue(showCounter(18_000, 20_000)); assertFalse(showCounter(50_000, null))
  }

  @Test fun contextMessagesForClaudeCodeAndNoProvider() {
    assertTrue(Transcript.contextMessage("claude-code", true)!!.startsWith("In Claude Code mode"))
    assertEquals("Choose a provider first.", Transcript.contextMessage("", false))
    assertNull(Transcript.contextMessage("openai", true))
  }
}
