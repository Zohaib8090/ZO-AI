package org.zohara.chat.pages

import java.util.Locale
import java.util.TimeZone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.chat.core.net.ActivityItem
import org.zohara.chat.core.net.ArtifactItem
import org.zohara.chat.core.net.ChatEntry
import org.zohara.chat.core.net.ChatSummary
import org.zohara.chat.core.net.ClientSeen
import org.zohara.chat.core.net.ScheduledTask

class PagesFormatTest {
  private val utc = TimeZone.getTimeZone("UTC")
  private val now = 1_794_509_131_000L // 2026-11-13

  // ---- relative time (the desktop's model::relative)
  @Test fun relativeWords() {
    assertEquals("just now", PagesFormat.relative(now - 30_000, now, utc))
    assertEquals("5 min ago", PagesFormat.relative(now - 5 * 60_000, now, utc))
    assertEquals("1 hour ago", PagesFormat.relative(now - 3_600_000, now, utc))
    assertEquals("3 hours ago", PagesFormat.relative(now - 3 * 3_600_000, now, utc))
    assertEquals("1 day ago", PagesFormat.relative(now - 86_400_000, now, utc))
    assertEquals("12 days ago", PagesFormat.relative(now - 12 * 86_400_000L, now, utc))
    assertEquals("Oct 4", PagesFormat.relative(1_759_536_000_000L, now, utc)) // 2025-10-04
    assertEquals("", PagesFormat.relative(0, now, utc))
  }

  @Test fun whenTextIsLocaleDateAndTime() {
    assertEquals("Nov 12, 2026, 3:05 PM", PagesFormat.whenText(1_794_495_900_000L, Locale.US, utc).replace(' ', ' '))
    assertEquals("", PagesFormat.whenText(0, Locale.US, utc))
  }

  @Test fun clockIsTheDesktopsClock() {
    assertEquals("12 s", PagesFormat.clock(12_900))
    assertEquals("3 min 5 s", PagesFormat.clock(185_000))
    assertEquals("1 h 2 min", PagesFormat.clock(3_720_000))
    assertEquals("0 s", PagesFormat.clock(-5))
  }

  // ---- chats
  @Test fun sectionsPinnedAllRecentsEightNewestUnpinned() {
    val chats = (1..12).map { ChatSummary("c$it", pinned = it % 6 == 0, title = "t$it", updated = it * 1000L) }
    val s = PagesFormat.sections(chats)
    assertEquals(listOf("c12", "c6"), s.pinned.map { it.id })
    assertEquals(8, s.recents.size)
    assertEquals("c11", s.recents.first().id)
    assertTrue(s.recents.none { it.pinned })
    assertEquals("c3", s.recents.last().id)
  }

  @Test fun sectionsEmptyHidesBothLabels() {
    val s = PagesFormat.sections(emptyList())
    assertTrue(s.pinned.isEmpty() && s.recents.isEmpty())
  }

  @Test fun untitledChat() {
    assertEquals("Untitled", PagesFormat.title(ChatSummary("a", title = "")))
    assertEquals("Hi", PagesFormat.title(ChatSummary("a", title = "Hi")))
  }

  @Test fun transcriptKeepsTheConversationAndDropsToolNotes() {
    val t = PagesFormat.transcript(
      "run it",
      listOf(ChatEntry("msg-user", "run it"), ChatEntry("msg-tool", "▸ zohara_shell  cmd: echo hi"), ChatEntry("msg-tool-out", "✓ hi"), ChatEntry("msg-ai", "Done."), ChatEntry("msg-error", "Boom")),
    )
    assertEquals("run it\n\nYou: run it\n\nZohara Chat: Done.\n\nError: Boom", t)
  }

  // ---- projects: the live line from API.md
  @Test fun projectsLiveLine() {
    val p = PagesFormat.parseProjects("p1794509093697 | Demo | g | 0 memory notes")
    assertEquals(1, p.size)
    assertEquals("p1794509093697", p[0].id)
    assertEquals("Demo", p[0].name)
    assertEquals("g", p[0].goal)
    assertEquals("0 memory notes", p[0].memoryNotes)
    assertEquals("g · 0 memory notes", p[0].meta)
  }

  @Test fun projectsNone() {
    assertTrue(PagesFormat.parseProjects("There are no projects.").isEmpty())
    assertTrue(PagesFormat.parseProjects("").isEmpty())
  }

  @Test fun projectsManyLinesAndShortLines() {
    val p = PagesFormat.parseProjects("p1 | A | | 2 memory notes\nstray line\np2 | B | do things | 1 memory note | extra")
    assertEquals(listOf("A", "B"), p.map { it.name })
    assertEquals("2 memory notes", p[0].meta)
    assertEquals("do things · 1 memory note · extra", p[1].meta)
  }

  // ---- artifacts
  @Test fun artifactSublines() {
    val code = ArtifactItem("a", "code", "t", path = "", text = "a\nb\nc", created = now - 120_000)
    assertEquals("3 lines of code · 2 min ago", PagesFormat.artifactSubline(code, now, utc))
    val file = ArtifactItem("b", "file", "t", path = "/srv/x.txt", text = null, created = now - 10_000)
    assertEquals("/srv/x.txt · just now", PagesFormat.artifactSubline(file, now, utc))
    assertEquals("", PagesFormat.artifactSubline(ArtifactItem("c"), now, utc))
    assertEquals(3, PagesFormat.lineCount("a\nb\nc\n"))
    assertEquals(3, PagesFormat.lineCount("a\nb\nc"))
    assertEquals(1, PagesFormat.lineCount("one"))
  }

  @Test fun artifactBodyPrefersText() {
    assertEquals("code", PagesFormat.artifactBody(ArtifactItem("a", text = "code", path = "/p")))
    assertEquals("/p", PagesFormat.artifactBody(ArtifactItem("a", text = null, path = "/p")))
    assertEquals("/p", PagesFormat.artifactBody(ArtifactItem("a", text = "", path = "/p")))
  }

  // ---- scheduled
  @Test fun taskLines() {
    val on = ScheduledTask("t", "Disk", "check disk", nextRun = 1_794_495_900_000L, repeat = "daily", enabled = true)
    assertEquals("Next: Nov 12, 2026, 3:05 PM · daily", PagesFormat.taskWhen(on, Locale.US, utc).replace(' ', ' '))
    assertEquals("Off · once", PagesFormat.taskWhen(on.copy(enabled = false, repeat = "once")))
    assertEquals("Disk", PagesFormat.taskTitle(on))
    assertEquals("check disk", PagesFormat.taskTitle(on.copy(title = "")))
  }

  @Test fun ownIntervalWinsOverTheChoice() {
    assertEquals("every 3 days", PagesFormat.repeatValue("daily", " every 3 days "))
    assertEquals("weekly", PagesFormat.repeatValue("weekly", "  "))
  }

  // ---- activity
  @Test fun kindAndSourceWords() {
    assertEquals("Chat", PagesFormat.kindWord("answer"))
    assertEquals("Claude Code", PagesFormat.kindWord("claude_code"))
    assertEquals("Scheduled task", PagesFormat.kindWord("scheduled"))
    assertEquals("Memory", PagesFormat.kindWord("memory_job"))
    assertEquals("Command", PagesFormat.kindWord("command"))
    assertEquals("Terminal", PagesFormat.kindWord("terminal"))
    assertEquals("Sync", PagesFormat.kindWord("sync"))
    assertEquals("Client", PagesFormat.kindWord("client"))
    assertEquals("the web app", PagesFormat.sourceWord("web"))
    assertEquals("the phone", PagesFormat.sourceWord("phone"))
    assertEquals("this app", PagesFormat.sourceWord("desktop"))
    assertEquals("a web session", PagesFormat.sourceWord("web-link"))
    assertEquals("the command line", PagesFormat.sourceWord("cli"))
    assertEquals("the scheduler", PagesFormat.sourceWord("scheduler"))
    assertEquals("in the background", PagesFormat.sourceWord("background"))
    assertEquals("odd", PagesFormat.sourceWord("odd"))
  }

  @Test fun activitySubLine() {
    val i = ActivityItem(1, "answer", "Hello", detail = "m1", source = "web")
    assertEquals("Chat from the web app · m1", PagesFormat.activitySub(i))
    assertEquals("Memory", PagesFormat.activitySub(ActivityItem(2, "memory_job", "x")))
  }

  @Test fun clientsAreNotRunning() {
    val items = listOf(ActivityItem(1, "answer", "a"), ActivityItem(2, "client", "c"), ActivityItem(3, "client", "d"))
    assertEquals(1, PagesFormat.running(items).size)
    assertEquals(2, PagesFormat.clients(items).size)
    assertEquals("2 connections open", PagesFormat.connectionsLine(2))
    assertEquals("1 connection open", PagesFormat.connectionsLine(1))
    assertEquals("1 thing is running", PagesFormat.runningTitle(1))
    assertEquals("3 things are running", PagesFormat.runningTitle(3))
  }

  @Test fun elapsedUsesTheServersClockAndCountsOnLocally() {
    val i = ActivityItem(1, "answer", "a", startedMs = 100_000)
    // the server says it has run 12 s; the phone clock is an hour off but only the difference counts
    assertEquals(12_000, PagesFormat.elapsedMs(i, serverNow = 112_000, fetchedAt = 9_000_000, localNow = 9_000_000))
    assertEquals(15_000, PagesFormat.elapsedMs(i, serverNow = 112_000, fetchedAt = 9_000_000, localNow = 9_003_000))
    // no server time: fall back to the phone's own clock
    assertEquals(20_000, PagesFormat.elapsedMs(i, serverNow = 0, fetchedAt = 120_000, localNow = 120_000))
    assertEquals(0, PagesFormat.elapsedMs(ActivityItem(2, "answer", "b", startedMs = 0), 5, 5, 5))
  }

  @Test fun clientsSeenTexts() {
    assertEquals("Not connected yet", PagesFormat.clientsSeenText(emptyList(), 1_000_000))
    assertEquals("Connected, last request 7 s ago", PagesFormat.clientsSeenText(listOf(ClientSeen(993_000, "web")), 1_000_000))
    assertEquals("Last seen 5 min ago", PagesFormat.clientsSeenText(listOf(ClientSeen(700_000, "cli")), 1_000_000))
  }

  @Test fun wordingIsTheDesktopsAndTheWebs() {
    assertEquals("No chats yet", PagesFormat.CHATS_EMPTY_TITLE)
    assertEquals("Nothing is running", PagesFormat.ACTIVITY_EMPTY_TITLE)
    assertEquals("Delete this chat?", PagesFormat.DELETE_CHAT_TITLE)
    assertEquals("It will not run again.", PagesFormat.CANCEL_TASK_TEXT)
    assertEquals("Older chats can be found with Search", PagesFormat.OLDER_CHATS_NOTE)
  }
}
