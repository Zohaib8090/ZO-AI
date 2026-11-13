package org.zohara.chat.settings

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.DangerButton
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.nav.LocalNav

object DataLogic {
  fun savedText(n: Int) = "$n saved chat${if (n == 1) "" else "s"}, kept on the server."
  fun deletedToast(n: Int) = if (n == 1) "1 chat deleted" else "$n chats deleted"
  const val DELETE_ALL_TITLE = "Delete all saved chats?"
  const val DELETE_ALL_TEXT = "Every saved chat is deleted. This cannot be undone. If your computer or phone syncs with this server, they are deleted there too. Projects and scheduled tasks stay."
}

/** Data. Parity rows: SET-DATA-01, 02, LIST-10. */
@Composable
fun SettingsDataScreen() {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val doer = rememberDoer()
  val loader = rememberLoader { container.api.chats() }
  val load by loader.state.collectAsState()
  var confirm by remember { mutableStateOf(false) }
  SettingsPage("Data") {
    LoadBox(load, { loader.load() }) { chats ->
      SectionHeader("Data")
      SettingsCard {
        ActionRow("Saved chats, projects and tasks", DataLogic.savedText(chats.size), actions = { PillButton("View chats", { nav.chats() }) })
        ActionRow(
          "Delete all chats", "Removes the saved conversations here, and on every computer that syncs with this server.", divider = false,
          actions = { DangerButton("Delete all chats", { confirm = true }, outlined = true) },
        )
      }
    }
  }
  if (confirm) ConfirmSheet(
    DataLogic.DELETE_ALL_TITLE, DataLogic.DELETE_ALL_TEXT, "Delete",
    { doer.run(onDone = { loader.load() }) { doer.say(DataLogic.deletedToast(container.api.deleteAllChats())) } }, { confirm = false },
  )
}
