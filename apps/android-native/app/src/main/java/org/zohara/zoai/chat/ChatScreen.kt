package org.zohara.zoai.chat

import androidx.compose.runtime.Composable
import org.zohara.zoai.nav.StubScreen

/** A chat: thread, streaming, composer. Parity rows: CHAT-*, COMP-*, INTER-*, CTX-*, PERM-*. */
@Composable
fun ChatScreen(chatId: String) {
  StubScreen("Chat", "CHAT-*, COMP-*, INTER-*, CTX-*, PERM-*")
}
