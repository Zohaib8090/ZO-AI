package org.zohara.zoai.pages

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.zoai.core.net.ChatSummary

@OptIn(ExperimentalCoroutinesApi::class)
class ChatActionsTest {
  private val chats = listOf(ChatSummary("a", false, "Alpha", 3), ChatSummary("b", true, "Beta", 2))

  private class Rig(scope: kotlinx.coroutines.CoroutineScope, active: String?, val api: FakeApi) {
    val holder = ChatsHolder(api, scope)
    var home = 0
    val toasts = ArrayList<String>()
    val shared = ArrayList<Pair<String, String>>()
    val actions = ChatActions(holder, scope, active, { home++ }, { toasts += it }, { t, x -> shared += t to x })
  }

  @Test fun menuOffersPinOrUnpinShareDelete() = runTest {
    val rig = Rig(backgroundScope, null, FakeApi().apply { chatList = chats })
    assertEquals(listOf("Pin this chat", "Share conversation", "Delete this chat"), rig.actions.menu(chats[0]).map { it.label })
    assertEquals("Unpin", rig.actions.menu(chats[1]).first().label)
    assertTrue(rig.actions.menu(chats[0]).last().danger)
  }

  @Test fun deleteAsksFirstThenGoesHomeOnlyForTheOpenChat() = runTest {
    val rig = Rig(backgroundScope, "a", FakeApi().apply { chatList = chats })
    rig.holder.refresh(); runCurrent()
    rig.actions.menu(chats[1]).last().onClick()
    assertEquals("b", rig.actions.pendingDelete?.id)
    assertTrue(rig.api.calls.none { it.startsWith("delete_chat") })
    rig.actions.confirmDelete(); runCurrent()
    assertEquals(0, rig.home)
    rig.actions.menu(chats[0]).last().onClick()
    rig.actions.confirmDelete(); runCurrent()
    assertEquals(1, rig.home)
    assertNull(rig.actions.pendingDelete)
  }

  @Test fun dismissingTheConfirmDeletesNothing() = runTest {
    val rig = Rig(backgroundScope, null, FakeApi().apply { chatList = chats })
    rig.actions.menu(chats[0]).last().onClick()
    rig.actions.dismissDelete(); runCurrent()
    assertNull(rig.actions.pendingDelete)
    assertTrue(rig.api.calls.isEmpty())
  }

  @Test fun failedDeleteToastsTheSentenceAndStaysHere() = runTest {
    val rig = Rig(backgroundScope, "a", FakeApi().apply { chatList = chats; failWith = "The server is busy. Try again in a moment."; failOps = mutableSetOf("delete_chat") })
    rig.holder.refresh(); runCurrent()
    rig.api.failOps = mutableSetOf("delete_chat")
    rig.actions.menu(chats[0]).last().onClick()
    rig.actions.confirmDelete(); runCurrent()
    assertEquals(listOf("The server is busy. Try again in a moment."), rig.toasts)
    assertEquals(0, rig.home)
  }

  @Test fun pinTogglesAndShareSendsTheTranscript() = runTest {
    val rig = Rig(backgroundScope, null, FakeApi().apply { chatList = chats })
    rig.holder.refresh(); runCurrent()
    rig.actions.menu(chats[0]).first().onClick(); runCurrent()
    assertTrue(rig.api.calls.contains("pin_chat a true"))
    rig.actions.menu(chats[1]).first().onClick(); runCurrent()
    assertTrue(rig.api.calls.contains("pin_chat b false"))
    rig.actions.menu(chats[0])[1].onClick(); runCurrent()
    assertEquals("Alpha" to "t\n\nYou: hi\n\nZO-AI: hello", rig.shared.single())
  }
}
