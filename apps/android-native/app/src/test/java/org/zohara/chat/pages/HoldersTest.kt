package org.zohara.chat.pages

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.chat.core.net.ActivityItem
import org.zohara.chat.core.net.ActivityResp
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.ArtifactItem
import org.zohara.chat.core.net.ChatDetail
import org.zohara.chat.core.net.ChatEntry
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.core.net.ErrorKind
import org.zohara.chat.core.net.ScheduledTask
import org.zohara.chat.core.net.SearchHit

/** A fake server: every call is recorded, every answer can be set, any call can be made to fail. */
class FakeApi : PagesApi {
  val calls = ArrayList<String>()
  var chatList = listOf<ChatSummary>()
  var hits = mapOf<String, List<SearchHit>>()
  var projectsText = "There are no projects."
  var artifactList = listOf<ArtifactItem>()
  var tasks = listOf<ScheduledTask>()
  var activityResp = ActivityResp()
  var detail = ChatDetail("c1", "t", listOf(ChatEntry("msg-user", "hi"), ChatEntry("msg-ai", "hello")))
  var terminalOut = "hi"
  var failWith: String? = null
  var failOps = mutableSetOf<String>()

  private fun hit(op: String, vararg args: Any?) {
    calls += (listOf(op) + args.toList()).joinToString(" ")
    if (failWith != null && (failOps.isEmpty() || op in failOps)) throw ApiException(ErrorKind.Rejected, failWith!!)
  }

  override suspend fun chats() = run { hit("chats"); chatList }
  override suspend fun chat(id: String) = run { hit("chat", id); detail }
  override suspend fun deleteChat(id: String) { hit("delete_chat", id); chatList = chatList.filter { it.id != id } }
  override suspend fun pinChat(id: String, on: Boolean) { hit("pin_chat", id, on); chatList = chatList.map { if (it.id == id) it.copy(pinned = on) else it } }
  override suspend fun search(query: String) = run { hit("search", query); hits[query].orEmpty() }
  override suspend fun projects() = run { hit("projects"); projectsText }
  override suspend fun projectCreate(name: String, goal: String) = run { hit("project_create", name, goal); projectsText = "p1 | $name | $goal | 0 memory notes"; "Created" }
  override suspend fun artifacts() = run { hit("artifacts"); artifactList }
  override suspend fun scheduled() = run { hit("scheduled"); tasks }
  override suspend fun scheduleAdd(task: String, whenText: String, repeat: String?) = run { hit("schedule_add", task, whenText, repeat); "Scheduled" }
  override suspend fun scheduleCancel(id: String) { hit("schedule_cancel", id); tasks = tasks.filter { it.id != id } }
  override suspend fun activity() = run { hit("activity"); activityResp }
  override suspend fun activityStop(id: Long) { hit("activity_stop", id) }
  override suspend fun terminalRun(cmd: String) = run { hit("terminal_run", cmd); terminalOut }
}

@OptIn(ExperimentalCoroutinesApi::class)
class HoldersTest {
  private val chats = listOf(
    ChatSummary("a", false, "Alpha", 3000),
    ChatSummary("b", true, "Beta", 2000),
    ChatSummary("c", false, "Gamma", 1000),
  )

  // ---------------------------------------------------------------- chats

  @Test fun chatsLoadThenShow() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    assertTrue(h.state.value.isFirstLoad)
    h.refresh(); runCurrent()
    assertEquals(3, h.state.value.value?.size)
    assertFalse(h.state.value.loading)
    assertNull(h.state.value.error)
  }

  @Test fun failedFirstLoadShowsTheServersSentence() = runTest {
    val api = FakeApi().apply { failWith = "Projects and scheduled tasks are not available to this account." }
    val h = ProjectsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    assertNull(h.state.value.value)
    assertEquals("Projects and scheduled tasks are not available to this account.", h.state.value.error)
  }

  @Test fun failedReloadKeepsTheOldList() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    api.failWith = "Offline. Retrying."
    h.refresh(); runCurrent()
    assertEquals(3, h.state.value.value?.size)
    assertEquals("Offline. Retrying.", h.state.value.error)
  }

  @Test fun overlappingRefreshIsDropped() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    h.refresh(); h.refresh(); h.refresh(); runCurrent()
    assertEquals(1, api.calls.count { it == "chats" })
  }

  @Test fun deleteLeavesTheListAtOnceAndReloads() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    val err = h.delete("a"); runCurrent()
    assertNull(err)
    assertTrue(api.calls.contains("delete_chat a"))
    assertEquals(listOf("b", "c"), h.state.value.value?.map { it.id })
  }

  @Test fun failedDeleteBringsTheChatBack() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    api.failWith = "The server is busy. Try again in a moment."; api.failOps = mutableSetOf("delete_chat")
    val err = h.delete("a")
    assertEquals("The server is busy. Try again in a moment.", err)
    assertEquals(3, h.state.value.value?.size)
  }

  @Test fun pinAndUnpin() = runTest {
    val api = FakeApi().apply { chatList = chats }
    val h = ChatsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    assertNull(h.pin("a", true)); runCurrent()
    assertTrue(api.calls.contains("pin_chat a true"))
    assertTrue(h.state.value.value!!.first { it.id == "a" }.pinned)
    assertNull(h.pin("b", false)); runCurrent()
    assertFalse(h.state.value.value!!.first { it.id == "b" }.pinned)
  }

  @Test fun transcriptOfAChat() = runTest {
    val h = ChatsHolder(FakeApi(), backgroundScope)
    assertEquals("t\n\nYou: hi\n\nZohara Chat: hello", h.transcript("c1").getOrThrow())
  }

  // ---------------------------------------------------------------- search

  @Test fun searchWaitsTwoHundredMsAfterTheLastKey() = runTest {
    val api = FakeApi().apply { hits = mapOf("fake" to listOf(SearchHit("c1", "Hello from the fake model.", "run it", 5))) }
    val h = SearchHolder(api, backgroundScope)
    h.setQuery("f"); advanceTimeBy(100)
    h.setQuery("fa"); advanceTimeBy(100)
    h.setQuery("fake"); advanceTimeBy(199)
    assertTrue(api.calls.none { it.startsWith("search") })
    assertTrue(h.ui.value.loading)
    advanceTimeBy(2); runCurrent()
    assertEquals(listOf("search fake"), api.calls.filter { it.startsWith("search") })
    assertEquals("c1", h.ui.value.hits?.single()?.id)
    assertFalse(h.ui.value.loading)
  }

  @Test fun emptyQueryShowsRecentsAndSendsNothing() = runTest {
    val api = FakeApi()
    val h = SearchHolder(api, backgroundScope)
    h.setQuery("abc"); advanceTimeBy(300); runCurrent()
    h.setQuery("  "); advanceUntilIdle()
    assertNull(h.ui.value.hits)
    assertFalse(h.ui.value.loading)
    assertEquals(1, api.calls.count { it.startsWith("search") })
  }

  @Test fun anOldSearchNeverOverwritesANewer() = runTest {
    val api = FakeApi().apply { hits = mapOf("one" to listOf(SearchHit("1")), "two" to listOf(SearchHit("2"))) }
    val h = SearchHolder(api, backgroundScope)
    h.setQuery("one"); advanceTimeBy(250); runCurrent()
    h.setQuery("two"); advanceTimeBy(250); runCurrent()
    assertEquals("2", h.ui.value.hits?.single()?.id)
    assertEquals("two", h.ui.value.query)
  }

  @Test fun searchErrorIsShown() = runTest {
    val api = FakeApi().apply { failWith = "You are going a little fast. Please wait 3 seconds and try again." }
    val h = SearchHolder(api, backgroundScope)
    h.setQuery("x"); advanceTimeBy(250); runCurrent()
    assertEquals("You are going a little fast. Please wait 3 seconds and try again.", h.ui.value.error)
  }

  // ---------------------------------------------------------------- projects

  @Test fun projectsParseAndCreate() = runTest {
    val api = FakeApi()
    val h = ProjectsHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    assertEquals(emptyList<PagesFormat.Project>(), h.state.value.value)
    assertNull(h.create("  Demo ", " g "))
    assertTrue(api.calls.contains("project_create Demo g"))
    runCurrent()
    assertEquals("Demo", h.state.value.value?.single()?.name)
  }

  @Test fun projectCreateErrorComesBack() = runTest {
    val api = FakeApi().apply { failWith = "That project already exists."; failOps = mutableSetOf("project_create") }
    val h = ProjectsHolder(api, backgroundScope)
    assertEquals("That project already exists.", h.create("Demo", ""))
  }

  // ---------------------------------------------------------------- scheduled

  @Test fun cancelTask() = runTest {
    val api = FakeApi().apply { tasks = listOf(ScheduledTask("t1", "A", "a"), ScheduledTask("t2", "B", "b")) }
    val h = ScheduledHolder(api, backgroundScope)
    h.refresh(); runCurrent()
    assertNull(h.cancel("t1")); runCurrent()
    assertTrue(api.calls.contains("schedule_cancel t1"))
    assertEquals(listOf("t2"), h.state.value.value?.map { it.id })
  }

  @Test fun addTaskSendsTheChoiceOrTheOwnInterval() = runTest {
    val api = FakeApi()
    val h = ScheduledHolder(api, backgroundScope)
    h.add(TaskForm("check disk", "tomorrow 9am", "daily", "")).getOrThrow()
    h.add(TaskForm("check disk", "in 2 hours", "daily", "every 90 minutes")).getOrThrow()
    h.add(TaskForm("once only", "2026-11-20 18:30", "once", "")).getOrThrow()
    assertEquals(
      listOf("schedule_add check disk tomorrow 9am daily", "schedule_add check disk in 2 hours every 90 minutes", "schedule_add once only 2026-11-20 18:30 null"),
      api.calls.filter { it.startsWith("schedule_add") },
    )
  }

  @Test fun taskFormNeedsTextAndTime() {
    assertFalse(TaskForm("", "tomorrow").valid)
    assertFalse(TaskForm("x", " ").valid)
    assertTrue(TaskForm("x", "tomorrow").valid)
  }

  @Test fun addTaskErrorIsTheServersSentence() = runTest {
    val api = FakeApi().apply { failWith = "I couldn't read \"soon\" as a time."; failOps = mutableSetOf("schedule_add") }
    val r = ScheduledHolder(api, backgroundScope).add(TaskForm("x", "soon"))
    assertEquals("I couldn't read \"soon\" as a time.", r.exceptionOrNull()?.sentence())
  }

  // ---------------------------------------------------------------- activity

  private fun running(vararg i: ActivityItem) = ActivityResp(nowMs = 112_000, items = i.toList())

  @Test fun activityCountsRunningAndNotClients() = runTest {
    val api = FakeApi().apply {
      activityResp = running(ActivityItem(1, "answer", "a", startedMs = 100_000, canStop = true), ActivityItem(2, "client", "c"), ActivityItem(3, "memory_job", "m"))
    }
    var count = -1
    val h = ActivityHolder(api, backgroundScope, { 5L }, { count = it })
    h.fetch()
    assertEquals(2, count)
    assertEquals(2, h.ui.value.running.size)
    assertEquals(1, h.ui.value.clients.size)
    assertTrue(h.ui.value.loaded)
    assertEquals(5L, h.ui.value.fetchedAt)
  }

  @Test fun stopSendsTheIdAndMarksTheRow() = runTest {
    val api = FakeApi().apply { activityResp = running(ActivityItem(7, "answer", "a", canStop = true)) }
    val h = ActivityHolder(api, backgroundScope)
    h.fetch()
    assertNull(h.stop(7))
    assertTrue(api.calls.contains("activity_stop 7"))
    assertTrue(7L in h.ui.value.stopSent)
    // the item is gone on the next read: the mark goes with it
    api.activityResp = running()
    h.fetch()
    assertTrue(h.ui.value.stopSent.isEmpty())
  }

  @Test fun stopFailureReturnsASentence() = runTest {
    val api = FakeApi().apply { failWith = "That is not running any more."; failOps = mutableSetOf("activity_stop") }
    val h = ActivityHolder(api, backgroundScope)
    assertEquals("That is not running any more.", h.stop(1))
    assertTrue(h.ui.value.stopSent.isEmpty())
  }

  @Test fun activityPollReadsEveryThreeSeconds() = runTest {
    val api = FakeApi()
    val h = ActivityHolder(api, backgroundScope)
    val j = backgroundScope.launch { h.poll(3000) }
    runCurrent(); advanceTimeBy(3001); runCurrent(); advanceTimeBy(3000); runCurrent()
    assertEquals(3, api.calls.count { it == "activity" })
    j.cancel()
  }

  @Test fun activityErrorKeepsTheLastItems() = runTest {
    val api = FakeApi().apply { activityResp = running(ActivityItem(1, "answer", "a")) }
    val h = ActivityHolder(api, backgroundScope)
    h.fetch()
    api.failWith = "Offline. Retrying."
    h.fetch()
    assertEquals(1, h.ui.value.items.size)
    assertEquals("Offline. Retrying.", h.ui.value.error)
  }

  // ---------------------------------------------------------------- terminal

  @Test fun terminalRunsAndKeepsHistory() = runTest {
    val api = FakeApi().apply { terminalOut = "hi\n" }
    val h = TerminalHolder(api, backgroundScope)
    assertTrue(h.ui.value.log.startsWith("Zohara Chat Terminal\nConnected. Type a shell command below"))
    h.run("echo hi", onComputer = false); runCurrent()
    assertTrue(h.ui.value.log.endsWith("\$ echo hi\nhi\n"))
    h.run("pwd", onComputer = false); runCurrent()
    assertEquals("pwd", h.historyUp())
    assertEquals("echo hi", h.historyUp())
    assertEquals("echo hi", h.historyUp())
    assertEquals("pwd", h.historyDown())
    assertEquals("", h.historyDown())
  }

  @Test fun terminalErrorsAreErrorLines() = runTest {
    val api = FakeApi().apply { failWith = "Terminal command execution on the cloud server requires an admin account." }
    val h = TerminalHolder(api, backgroundScope)
    h.run("ls", false); runCurrent()
    assertTrue(h.ui.value.log.endsWith("\$ ls\nError: Terminal command execution on the cloud server requires an admin account.\n"))
    assertFalse(h.ui.value.running)
  }

  @Test fun terminalOnMyComputerSendsNothing() = runTest {
    val api = FakeApi()
    val h = TerminalHolder(api, backgroundScope)
    h.run("rm -rf x", onComputer = true); runCurrent()
    assertTrue(api.calls.isEmpty())
    assertTrue(h.ui.value.log.contains("Error: Commands on your own computer need your approval there"))
  }

  @Test fun terminalIgnoresEmptyCommandsAndClears() = runTest {
    val api = FakeApi()
    val h = TerminalHolder(api, backgroundScope)
    h.run("   ", false); runCurrent()
    assertTrue(api.calls.isEmpty())
    h.clear()
    assertEquals("", h.ui.value.log)
  }

  // ---------------------------------------------------------------- what the search page shows

  @Test fun searchDisplayRules() {
    val recent = listOf(ChatSummary("a", title = "A"))
    assertTrue(SearchDisplay.of(SearchUi(), null) is SearchDisplay.Recents)
    assertEquals(false, (SearchDisplay.of(SearchUi(), null) as SearchDisplay.Recents).loaded)
    assertEquals(true, (SearchDisplay.of(SearchUi(), emptyList()) as SearchDisplay.Recents).loaded)
    assertEquals(recent, (SearchDisplay.of(SearchUi(), recent) as SearchDisplay.Recents).chats)
    assertEquals(SearchDisplay.Searching, SearchDisplay.of(SearchUi("x", loading = true), recent))
    assertEquals(SearchDisplay.NoHits, SearchDisplay.of(SearchUi("x", hits = emptyList()), recent))
    assertEquals(SearchDisplay.Searching, SearchDisplay.of(SearchUi("x", loading = true, hits = emptyList()), recent))
    assertTrue(SearchDisplay.of(SearchUi("x", loading = true, hits = listOf(SearchHit("1"))), recent) is SearchDisplay.Hits)
    assertEquals(SearchDisplay.Failed("nope"), SearchDisplay.of(SearchUi("x", error = "nope"), recent))
  }
}
