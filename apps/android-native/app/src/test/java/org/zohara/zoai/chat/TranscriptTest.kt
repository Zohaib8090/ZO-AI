package org.zohara.zoai.chat

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.zoai.core.net.AttachInfo
import org.zohara.zoai.core.net.TakeInfo

class TranscriptTest {
  private var k = 0L
  private val keys = { ++k }

  @Test fun savedChatGroupsConsecutiveToolLinesAndEndsGroupsAtAnswers() {
    val items = Transcript.fromSaved(
      listOf(
        entry("msg-user", "run it"),
        entry("msg-tool", "▸ zohara_shell  cmd: echo hi"),
        entry("msg-tool-out", "✓ hi"),
        entry("msg-tool", "▸ read_file  a.txt"),
        entry("msg-error", "✗ no such file"),
        entry("msg-ai", "Done."),
        entry("msg-tool", "▸ zohara_shell  cmd: ls"),
        entry("msg-tool-out", "✓ a"),
      ),
      emptyList(), keys,
    )
    assertEquals(listOf("UserItem", "ToolGroupItem", "AiItem", "ToolGroupItem"), items.map { it::class.simpleName })
    val g1 = items[1] as ToolGroupItem
    assertEquals(2, g1.count)
    assertEquals("Ran 2 commands", g1.head)
    assertEquals(LineKind.Fail, g1.lines.last().kind)
    assertEquals("no such file", g1.lines.last().text)
    assertEquals("Ran 1 command", (items[3] as ToolGroupItem).head)
    assertFalse(g1.active)
  }

  @Test fun activeGroupHeadsMentionTheTerminalForShellRuns() {
    val shell = ToolGroupItem(1, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: ls")), active = true)
    val other = ToolGroupItem(2, listOf(ToolLine(LineKind.Start, "read_file  a")), active = true)
    assertEquals("Running a task in the terminal…", shell.head)
    assertEquals("Working on your computer…", other.head)
  }

  @Test fun userMessagesAreNumberedAndCarryTheirTakes() {
    val items = Transcript.fromSaved(
      listOf(entry("msg-user", "a", listOf(AttachInfo("image", "p.png", 70, "data:image/png;base64,AA"))), entry("msg-ai", "x"), entry("msg-user", "b"), entry("msg-ai", "y")),
      listOf(TakeInfo(count = 2, current = 1, nth = 0)), keys,
    )
    val u0 = items[0] as UserItem
    val u1 = items[2] as UserItem
    assertEquals(0, u0.nth); assertEquals(1, u1.nth)
    assertEquals(2, u0.take?.count)
    assertNull(u1.take)
    assertEquals("p.png", u0.attach.single().name)
  }

  @Test fun visionNoticeAndSummaryRowsAreInlineNotices() {
    val items = Transcript.fromSaved(
      listOf(entry("msg-user", "look"), entry("msg-error", "✗ The model m1 cannot look at pictures, so the picture was left out."), entry("msg-tool", "Earlier messages were summarized."), entry("msg-error", "Something broke")),
      emptyList(), keys,
    )
    assertEquals(listOf("UserItem", "NoticeItem", "NoticeItem", "ErrorItem"), items.map { it::class.simpleName })
    assertEquals("The model m1 cannot look at pictures, so the picture was left out.", (items[1] as NoticeItem).text)
  }

  @Test fun toolLineFromSummaryUsesNameAndSecondLine() {
    assertEquals("zohara_shell  cmd: echo hi", Transcript.toolLineFromSummary("zohara_shell", "zohara_shell\ncmd: echo hi"))
    assertEquals("web_search", Transcript.toolLineFromSummary("web_search", "web_search"))
    assertEquals("x", Transcript.toolLineFromSummary("x", ""))
  }

  @Test fun copyAllHasOnlyYouAndZoAiBlocks() {
    val items = listOf(UserItem(1, "hi", 0), ToolGroupItem(2, emptyList(), false), AiItem(3, "hello"), ErrorItem(4, "boom"))
    assertEquals("You: hi\n\nZO-AI: hello", Transcript.copyAll(items))
  }

  @Test fun greetingUsesDesktopHoursAndFirstName() {
    assertEquals("Good morning, Zohaib", Transcript.greeting(5, "Zohaib Baig", "z@x.org"))
    assertEquals("Good morning, Zohaib", Transcript.greeting(11, "Zohaib Baig", ""))
    assertEquals("Good afternoon, Ali", Transcript.greeting(12, "", "ali@x.org"))
    assertEquals("Good afternoon, Ali", Transcript.greeting(17, "  ", "ali@x.org"))
    assertEquals("Good evening, Ali", Transcript.greeting(18, "Ali", ""))
    assertEquals("Good evening, Ali", Transcript.greeting(4, "Ali", ""))
    assertEquals("Good evening", Transcript.greeting(23, "", ""))
  }

  @Test fun modelNamesAndPillText() {
    assertEquals("Opus 5.5", Transcript.niceName("claude-opus-5-5"))
    assertEquals("Haiku 4.5", Transcript.niceName("claude-haiku-4-5-20251001"))
    assertEquals("Sonnet", Transcript.niceName("sonnet"))
    assertEquals("gpt-4o", Transcript.niceName("gpt-4o"))
    assertEquals("Choose a model", Transcript.pillText(false, "", ""))
    assertEquals("Claude", Transcript.pillText(true, "", "claude-code"))
    assertEquals("Opus 5.5", Transcript.pillText(true, "claude-opus-5-5", "claude-code"))
    assertEquals("ollama", Transcript.pillText(true, "", "ollama"))
  }

  @Test fun contextWindowAndEstimatesFollowTheWebRule() {
    assertEquals(128_000, Transcript.windowFor("llama3"))
    assertEquals(200_000, Transcript.windowFor("claude-opus-5-5"))
    assertEquals(200_000, Transcript.windowFor("GPT-4o"))
    assertEquals(1_000_000, Transcript.windowFor("gemini-2.5-pro"))
    val items = listOf(UserItem(1, "x".repeat(380), 0), AiItem(2, "y".repeat(379)))
    val info = Transcript.context(items, "llama3")
    assertEquals(200, info.history) // 760 chars / 3.8
    assertEquals(200 + 2400 + 3200, info.used)
    assertEquals(128_000 - info.used, info.free)
    assertEquals(4.5, info.percent, 0.05)
    assertTrue(Transcript.untilCompact(info) > 0)
    assertEquals(1.0, Transcript.context(emptyList(), "gemini-2.0").percent, 0.0)
  }

  @Test fun titles() {
    assertEquals("", Transcript.displayTitle("", false))
    assertEquals("New chat", Transcript.displayTitle("", true))
    assertEquals("hi", Transcript.displayTitle("hi", true))
  }
}
