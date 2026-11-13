package org.zohara.agent.chat

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.agent.core.design.ZoAgentTheme
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.net.AttachInfo
import org.zohara.agent.core.net.Profile
import org.zohara.agent.core.net.QuestionOption
import org.zohara.agent.core.net.QuestionSpec
import org.zohara.agent.core.net.TakeInfo
import org.zohara.agent.nav.ZoTopBar

/**
 * JVM screenshot tests of the chat screens (home, thread, streaming, cards, errors, sheets) in both themes and on a small and a large phone.
 * `./gradlew recordRoborazziDebug` writes app/src/test/screenshots/chat_*.png; the PNGs are looked at by a person, and `verifyRoborazziDebug`
 * keeps them from changing by accident.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w420dp-h900dp-xhdpi", application = android.app.Application::class)
class ChatScreenshotTest {
  @get:Rule val rule = createComposeRule()

  /** stand-ins for what the media and link modules plug in, so the pictures show the slots (the real ones need an AppContainer) */
  @org.junit.Before fun slots() {
    ChatHooks.attachButton = { Pill("+", "Add", {}) }
    ChatHooks.runOnPill = { Pill("Cloud", "Run on", {}, chevron = true) }
  }
  @org.junit.After fun clearSlots() { ChatHooks.attachButton = null; ChatHooks.runOnPill = null }

  private val noop = MessageActions(true, {}, {}, {}, {}, { _, _ -> }, {}, null)
  private val cb = ThreadCallbacks(noop, {}, { _, _ -> }, { _, _ -> }, { _, _ -> }, {})
  private fun model(busy: Boolean = false, question: Boolean = false, computer: String? = null, context: Boolean = true, home: Boolean = false) =
    ComposerModel(busy, question, "Opus 5.5", PermMode.Manual, computer, false, null, context, home, {}, {}, {}, {}, {})

  private val code = "Here is the fix. The loop never ended because `i` was not increased:\n\n```rust\nfn main() {\n    let mut i = 0;\n    while i < 3 {\n        println!(\"{i}\");\n        i += 1;\n    }\n}\n```\n\nTwo things changed:\n- the counter now grows\n- the loop stops at **3**\n\nRun it again and tell me what you see."

  private fun thread(): List<ChatItem> = listOf(
    UserItem(1, "My program prints 0 forever. Can you fix it?", 0),
    ToolGroupItem(2, listOf(
      ToolLine(LineKind.Start, "read_file  path: src/main.rs"), ToolLine(LineKind.Ok, "fn main() { let i = 0; while i < 3 { println!(\"{i}\"); } }"),
      ToolLine(LineKind.Start, "zohara_shell  cmd: cargo run"), ToolLine(LineKind.Fail, "error: process timed out after 10s"),
    ), active = false),
    AiItem(3, code),
  )

  private fun shot(name: String, dark: Boolean, w: Dp, h: Dp, content: @Composable () -> Unit) {
    rule.setContent {
      ZoAgentTheme(dark) {
        Column(Modifier.width(w).height(h).background(ZoTheme.colors.bg)) {
          ZoTopBar("", showMenuButton = true, onMenu = {})
          Box(Modifier.weight(1f)) { content() }
        }
      }
    }
    rule.onRoot().captureRoboImage("src/test/screenshots/chat_${name}_${if (dark) "dark" else "light"}_${if (w < 380.dp) "small" else "large"}.png")
  }

  private fun both(name: String, w: Dp = 411.dp, h: Dp = 860.dp, content: @Composable () -> Unit) { shot(name, true, w, h, content); }

  // ---- home ----
  @Composable private fun Home() = ChatContent(ChatUiState(), cb, model(context = false, home = true), "", {}, "Good morning, Zohaib", false, false)
  @Test fun homeDarkLarge() = shot("home", true, 411.dp, 860.dp) { Home() }
  @Test fun homeLightLarge() = shot("home", false, 411.dp, 860.dp) { Home() }
  @Test fun homeDarkSmall() = shot("home", true, 320.dp, 620.dp) { Home() }
  @Test fun homeLightSmall() = shot("home", false, 320.dp, 620.dp) { Home() }
  @Test fun homeRestrictedComputer() = shot("home_restricted", true, 411.dp, 860.dp) {
    ChatContent(ChatUiState(), cb, model(context = false), "Draft text that is long enough to show how the box grows", {}, "Good evening, Ali", true, false)
  }

  // ---- a chat with a tool group and a code block ----
  @Composable private fun Chat(items: List<ChatItem>, busy: Boolean = false, draft: String = "", m: ComposerModel = model(busy)) =
    ChatContent(ChatUiState(chatId = "c1", title = "Fix my loop", items = items, busy = busy), cb, m, draft, {}, null, false, false)

  @Test fun chatDarkLarge() = shot("chat", true, 411.dp, 860.dp) { Chat(thread()) }
  @Test fun chatLightLarge() = shot("chat", false, 411.dp, 860.dp) { Chat(thread()) }
  @Test fun chatDarkSmall() = shot("chat", true, 320.dp, 620.dp) { Chat(thread()) }
  @Test fun chatLightSmall() = shot("chat", false, 320.dp, 620.dp) { Chat(thread()) }

  @Test fun toolGroupOpen() = shot("toolgroup_open", true, 411.dp, 600.dp) {
    androidx.compose.foundation.layout.Column(Modifier.padding(16.dp)) {
      ToolGroupView(ToolGroupItem(2, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: ls -la"), ToolLine(LineKind.Ok, "total 8\ndrwxr-xr-x 2 me me 4096 .\n-rw-r--r-- 1 me me  120 main.rs"), ToolLine(LineKind.Start, "read_file  path: x"), ToolLine(LineKind.Fail, "no such file")), false), {}, startExpanded = true)
      ToolGroupView(ToolGroupItem(3, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: cargo build")), true), {}, startExpanded = false)
      ToolGroupView(ToolGroupItem(4, listOf(ToolLine(LineKind.Start, "web_search  query: weather")), false), {}, startExpanded = false)
    }
  }

  // ---- streaming ----
  @Test fun streamingDark() = shot("streaming", true, 411.dp, 860.dp) {
    Chat(listOf(UserItem(1, "Explain how a hash map works", 0), AiItem(2, "A hash map stores pairs in **buckets**. The key is run through a hash function, and the result picks the bucket:\n\n1. hash the key\n2. find the bucket", live = true)), busy = true)
  }
  @Test fun streamingLight() = shot("streaming", false, 411.dp, 860.dp) {
    Chat(listOf(UserItem(1, "Explain how a hash map works", 0), AiItem(2, "A hash map stores pairs in **buckets**. The key is run through a hash function, and the result picks the bucket:\n\n1. hash the key\n2. find the bucket", live = true)), busy = true)
  }
  @Test fun workingDark() = shot("working", true, 411.dp, 860.dp) {
    Chat(listOf(UserItem(1, "list my files", 0), ToolGroupItem(2, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: ls")), true), ThinkingItem(3)), busy = true)
  }

  // ---- approval ----
  private val approval = ApprovalItem(5, 1, "zohara_shell", "zohara_shell\ncmd: rm -rf build/ && cargo build --release", state = ApprovalState.Pending)
  private fun approvalThread(a: ApprovalItem, computer: Boolean = false) = listOf(
    UserItem(1, "clean the build and rebuild", 0),
    ToolGroupItem(2, listOf(ToolLine(LineKind.Start, "zohara_shell  cmd: rm -rf build/ && cargo build --release")), true),
    a.copy(onComputer = computer),
  )
  @Test fun approvalDark() = shot("approval", true, 411.dp, 860.dp) { Chat(approvalThread(approval), busy = true) }
  @Test fun approvalLight() = shot("approval", false, 411.dp, 860.dp) { Chat(approvalThread(approval, computer = true), busy = true, m = model(true, computer = "Laptop")) }
  @Test fun approvalAnsweredAndWaiting() = shot("approval_done", true, 411.dp, 640.dp) {
    Chat(listOf(approval.copy(state = ApprovalState.Allowed), approval.copy(key = 6, state = ApprovalState.Denied), approval.copy(key = 7, state = ApprovalState.NoAnswer), WaitingItem(8, "zohara_shell cmd: ls"), NoticeItem(9, "Running on your computer (Laptop)")), busy = true)
  }

  // ---- questions ----
  private val single = QuestionSpec("Which database should the project use?", "Database", false, listOf(QuestionOption("SQLite", "One file, no server."), QuestionOption("PostgreSQL", "A real server, better for many users."), QuestionOption("Keep both", "")))
  private val multi = QuestionSpec("Which platforms must it support?", "Platforms", true, listOf(QuestionOption("Android", "Phones and tablets"), QuestionOption("Linux", "The desktop app"), QuestionOption("Web", "The browser version")))
  private fun qThread(q: QuestionItem) = listOf(UserItem(1, "set up storage for my app", 0), q)

  @Test fun questionSingleDark() = shot("question_single", true, 411.dp, 860.dp) { Chat(qThread(QuestionItem(2, 1, listOf(single))), busy = true, m = model(true, question = true)) }
  @Test fun questionSingleLightSmall() = shot("question_single", false, 320.dp, 640.dp) { Chat(qThread(QuestionItem(2, 1, listOf(single))), busy = true, m = model(true, question = true)) }
  @Test fun questionMultiLight() = shot("question_multi", false, 411.dp, 860.dp) {
    QuestionCardPreview(QuestionItem(2, 1, listOf(multi)), QuestionDraft(listOf(multi)).toggle(0, "Android").toggle(0, "Web"))
  }
  @Test fun questionOtherDark() = shot("question_other", true, 411.dp, 860.dp) {
    QuestionCardPreview(QuestionItem(2, 1, listOf(single)), QuestionDraft(listOf(single)).setOther(0, "MariaDB, because the host already runs it"))
  }
  @Test fun questionSeveralPageTwoDark() = shot("question_page2", true, 411.dp, 860.dp) {
    QuestionCardPreview(QuestionItem(2, 1, listOf(single, multi)), QuestionDraft(listOf(single, multi)).toggle(0, "SQLite").next().toggle(1, "Linux"))
  }
  @Test fun questionResults() = shot("question_results", true, 411.dp, 500.dp) {
    Chat(listOf(
      QuestionItem(2, 1, listOf(single), QuestionState.Answered("SQLite")), QuestionItem(3, 2, listOf(multi), QuestionState.Answered("Android, Web - Other: Plan 9")),
      QuestionItem(4, 3, listOf(single), QuestionState.Skipped), QuestionItem(5, 4, listOf(single), QuestionState.Expired),
    ), busy = false)
  }

  @Composable private fun QuestionCardPreview(q: QuestionItem, d: QuestionDraft) {
    androidx.compose.foundation.layout.Column(Modifier.padding(16.dp)) { QuestionCard(q, {}, {}, initial = d) }
  }

  // ---- errors and states ----
  private fun errorItems() = listOf(
    UserItem(1, "what is the weather", 0),
    ErrorItem(2, "You are going a little fast. Please wait 12 seconds and try again.", "RateLimited"),
    NoticeItem(3, "The model m1 cannot look at pictures, so the picture was left out. Choose a model that can see images (for example Claude, Gemini or GPT-4o) in the model settings."),
    StoppedItem(4),
    ErrorItem(5, "The connection was lost. The answer may still be finishing on the server: open the chat again in a moment.", "lost", retry = false),
    ErrorItem(6, "No model is chosen on the computer yet. Open Zohara Agent there and choose one.", chooseModel = true),
  )
  @Test fun errorsDark() = shot("errors", true, 411.dp, 860.dp) { Chat(errorItems()) }
  @Test fun errorsLight() = shot("errors", false, 411.dp, 860.dp) { Chat(errorItems()) }
  @Test fun offlineSendKeepsDraft() = shot("send_failed", true, 411.dp, 640.dp) {
    Chat(listOf(ErrorItem(1, "Offline. Retrying.", "Offline")), draft = "a message that did not go out and is back in the box")
  }
  @Test fun takesAndAttachments() = shot("takes", false, 411.dp, 640.dp) {
    Chat(listOf(
      UserItem(1, "describe this and this file", 0, listOf(AttachInfo("image", "photo.png", 70_000), AttachInfo("text", "notes.txt", 2_048)), TakeInfo(count = 3, current = 1, nth = 0)),
      AiItem(2, "I see **a photo** and your notes.")))
  }

  // ---- sheets ----
  @Test fun modelSheet() = shot("sheet_model", true, 411.dp, 640.dp) {
    Box(Modifier.background(ZoTheme.colors.sheet)) {
      org.zohara.agent.core.design.SheetContent("Model", {}) {
        ModelSheetContent(
          PillsUi("Opus 5.5", true, "claude-opus-5-5", "claude-code", PermMode.Auto, listOf(
            Profile("p1", "Work", "claude-code", "claude-opus-5-5", pinned = listOf("claude-opus-5-5", "claude-sonnet-5-5")),
            Profile("p2", "Home", "ollama", "qwen", pinned = emptyList()),
          ), "p1", false), { _, _ -> }, {}, {},
        )
      }
    }
  }
  @Test fun modeSheetLight() = shot("sheet_mode", false, 411.dp, 560.dp) {
    Box(Modifier.background(ZoTheme.colors.sheet)) { org.zohara.agent.core.design.SheetContent("What Zohara Agent may do", {}) { ModeSheetContent(PermMode.Auto, true) {} } }
  }
  @Test fun contextSheet() = shot("sheet_context", true, 411.dp, 720.dp) {
    Box(Modifier.background(ZoTheme.colors.sheet)) {
      org.zohara.agent.core.design.SheetContent("Context", {}) {
        ContextSheetContent(ContextView(Transcript.context(thread(), "claude-opus-5-5"), Usage(5200, 310), result = "Session compacted."), {}, {})
      }
    }
  }
  @Test fun contextSheetLightSmall() = shot("sheet_context", false, 320.dp, 700.dp) {
    Box(Modifier.background(ZoTheme.colors.sheet)) {
      org.zohara.agent.core.design.SheetContent("Context", {}) { ContextSheetContent(ContextView(Transcript.context(thread(), "llama3"), null, compacting = true), {}, {}) }
    }
  }
  @Test fun toolOutputViewer() = shot("tool_output", true, 411.dp, 420.dp) {
    ToolOutputBody(
      ToolGroupItem(1, listOf(
        ToolLine(LineKind.Start, "zohara_shell  cmd: ls"), ToolLine(LineKind.Ok, "a.txt\nb.txt\nsome very long file name that goes on and on and on.rs…"),
        ToolLine(LineKind.Start, "screenshot  of the desktop"), ToolLine(LineKind.Ok, "/home/me/shot-1.png"), ToolLine(LineKind.Fail, "permission denied"),
      ), false), "Laptop",
    )
  }

  // ---- loading, a link answer, a tablet with the pinned panel ----
  @Test fun loadingDark() = shot("loading", true, 411.dp, 640.dp) { ThreadView(ChatUiState(chatId = "c1", loading = true), cb) }
  @Test fun loadingLight() = shot("loading", false, 411.dp, 640.dp) { ThreadView(ChatUiState(chatId = "c1", loading = true), cb) }
  @Config(qualifiers = "w1000dp-h700dp-xhdpi")
  @Test fun tabletWithThePinnedContextPanel() = shot("tablet_panel", true, 1000.dp, 700.dp) {
    androidx.compose.foundation.layout.Row(Modifier.fillMaxSize()) {
      Box(Modifier.weight(1f)) { Chat(thread()) }
      ContextPanel(ContextView(Transcript.context(thread(), "claude-opus-5-5"), Usage(5200, 310)), {}, {}, {})
    }
  }
}
