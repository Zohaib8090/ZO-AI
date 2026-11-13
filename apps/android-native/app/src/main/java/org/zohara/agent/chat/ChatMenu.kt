package org.zohara.agent.chat

import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import org.zohara.agent.core.design.ZoTheme

/** What the chat's "More" menu can do (NAV-17): everything the desktop and web put in the top bar of a chat. */
class ChatMenuActions(
  val onContext: () -> Unit,
  val onCopyAll: () -> Unit,
  val onPin: () -> Unit,
  val onTerminal: (() -> Unit)?,
  val onDelete: () -> Unit,
)

/** Context, Copy conversation, Pin, Terminal (admin accounts only, and the browser is not offered: D1), Delete. */
@Composable
fun ChatMenu(expanded: Boolean, onDismiss: () -> Unit, pinned: Boolean?, a: ChatMenuActions) {
  val c = ZoTheme.colors
  DropdownMenu(expanded, onDismiss, containerColor = c.popover) {
    DropdownMenuItem(text = { Text("Context", color = c.text) }, onClick = { onDismiss(); a.onContext() })
    DropdownMenuItem(text = { Text("Copy conversation", color = c.text) }, onClick = { onDismiss(); a.onCopyAll() })
    DropdownMenuItem(text = { Text(if (pinned == true) "Unpin chat" else "Pin chat", color = c.text) }, onClick = { onDismiss(); a.onPin() })
    a.onTerminal?.let { t -> DropdownMenuItem(text = { Text("Terminal", color = c.text) }, onClick = { onDismiss(); t() }) }
    DropdownMenuItem(text = { Text("Delete chat", color = c.danger) }, onClick = { onDismiss(); a.onDelete() })
  }
}
