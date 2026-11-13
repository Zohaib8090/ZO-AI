package org.zohara.zoai.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.design.SheetContent
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.design.ZoTheme

/** CTX-01: the context sheet has the "Auto-compact buffer" row (the desktop's `ctx-buffer`). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h900dp-xhdpi")
class GapCloseChatTest {
  @get:Rule val rule = createComposeRule()

  private val info = Transcript.context(listOf(UserItem(1, "x".repeat(4000), 0), AiItem(2, "y".repeat(8000))), "claude-opus-5-5")

  @Test fun theContextSheetListsTheBufferWithTokensAndPercent() {
    rule.setContent { ZoAiTheme(true) { Box(Modifier.background(ZoTheme.colors.sheet)) { SheetContent("Context", {}) { ContextSheetContent(ContextView(info, Usage(10, 5)), {}, {}) } } } }
    rule.onNodeWithText("Auto-compact buffer").assertExists()
    rule.onNodeWithText("Free capacity").assertExists()
    rule.onNodeWithText("System instructions").assertExists()
    // 20 percent of 200 000 = 40 000 tokens
    assertEquals(40_000, info.buffer)
    assertTrue(rule.onAllNodesWithText("≈ 40,000 tokens · 20.0%").fetchSemanticsNodes().size == 1)
  }

  @Test fun theBufferAndTheFreeSpaceAddUpToTheWindow() {
    assertEquals(info.window, info.used + info.buffer + info.free)
    // a window that is nearly full: the buffer shrinks to what is left, free is zero
    val full = Transcript.context(listOf(AiItem(1, "z".repeat((200_000 * 3.8f).toInt()))), "claude-opus-5-5")
    assertEquals(0, full.buffer)
    assertEquals(0, full.free)
  }
}
