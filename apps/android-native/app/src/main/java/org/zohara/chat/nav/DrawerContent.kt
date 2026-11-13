package org.zohara.chat.nav

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.zohara.chat.core.design.Avatar
import org.zohara.chat.core.design.SheetContent
import org.zohara.chat.core.design.ZoBottomSheet
import org.zohara.chat.core.design.ZoDivider
import org.zohara.chat.core.design.ZoIconButton
import org.zohara.chat.core.design.ZoIcons
import org.zohara.chat.core.design.ZoLogo
import org.zohara.chat.core.design.ZoTheme
import org.zohara.chat.pages.RunningNowSection
import org.zohara.chat.pages.SidebarChatLists

/** One entry of the sidebar: icon and label, highlighted when it is the current screen (NAV-03). */
data class DrawerItem(val route: String, val label: String, val icon: ImageVector, val needsAdmin: Boolean = false)

object DrawerItems {
  /** NAV-02: this order. */
  val all = listOf(
    DrawerItem(Routes.HOME, "New", ZoIcons.Plus),
    DrawerItem(Routes.CHATS, "Chats", ZoIcons.Chat),
    DrawerItem(Routes.SEARCH, "Search", ZoIcons.Search),
    DrawerItem(Routes.PROJECTS, "Projects", ZoIcons.Grid, needsAdmin = true),
    DrawerItem(Routes.ARTIFACTS, "Artifacts", ZoIcons.Folder, needsAdmin = true),
    DrawerItem(Routes.SCHEDULED, "Scheduled", ZoIcons.Alarm, needsAdmin = true),
    DrawerItem(Routes.ACTIVITY, "Activity", ZoIcons.Activity),
  )

  /** NAV-04: restricted accounts do not see Projects, Artifacts and Scheduled. */
  fun visible(restricted: Boolean): List<DrawerItem> = all.filter { !(restricted && it.needsAdmin) }
}

@Composable
fun DrawerContent(
  currentRoute: String?,
  restricted: Boolean,
  userLabel: String,
  userInitial: String,
  runningCount: Int,
  onNavigate: (String) -> Unit,
  onSettings: () -> Unit,
  onSignOut: () -> Unit,
  onClose: (() -> Unit)?,
  activeChatId: String? = null,
  onOpenChat: (String) -> Unit = {},
  modifier: Modifier = Modifier,
) {
  val c = ZoTheme.colors
  var menu by remember { mutableStateOf(false) }
  var confirmSignOut by remember { mutableStateOf(false) }
  Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().navigationBarsPadding()) {
    // NAV-01 brand row
    Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 8.dp, top = 12.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
      ZoLogo(size = 26.dp)
      Text("Zohara Chat", style = ZoTheme.type.wordmark, color = c.text, modifier = Modifier.padding(start = 10.dp).weight(1f))
      if (onClose != null) ZoIconButton(ZoIcons.Sidebar, "Hide the sidebar", onClose)
    }
    Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 10.dp)) {
      for (item in DrawerItems.visible(restricted)) {
        val active = item.route == currentRoute || (item.route == Routes.HOME && currentRoute == Routes.CHAT)
        Row(
          Modifier.fillMaxWidth().clip(ZoTheme.shapes.navItem).background(if (active) c.card2 else androidx.compose.ui.graphics.Color.Transparent)
            .clickable(role = Role.Button) { onNavigate(item.route) }.padding(horizontal = 12.dp, vertical = 11.dp),
          verticalAlignment = Alignment.CenterVertically,
        ) {
          Icon(item.icon, null, Modifier.width(20.dp).height(20.dp), tint = if (active) c.text else c.icon)
          Text(item.label, style = ZoTheme.type.navItem, color = c.text, modifier = Modifier.padding(start = 12.dp))
        }
      }
      Spacer(Modifier.height(8.dp))
      // NAV-09, NAV-10: Pinned and Recents (the pages module fills these in)
      SidebarChatLists(activeChatId = activeChatId, onOpenChat = onOpenChat)
    }
    ZoDivider()
    // NAV-13 footer
    Row(Modifier.fillMaxWidth().clickable(role = Role.Button) { menu = true }.padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
      Avatar(userInitial, badge = runningCount)
      Text(userLabel, style = ZoTheme.type.sideItem, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 12.dp).weight(1f))
      ZoIconButton(ZoIcons.Gear, "Settings", onSettings)
      ZoIconButton(ZoIcons.SignOut, "Sign out", { confirmSignOut = true })
    }
  }
  if (menu) UserMenuSheet(userLabel, onDismiss = { menu = false }, onSettings = { menu = false; onSettings() }, onSignOut = { menu = false; confirmSignOut = true })
  // SET-ACC-09: signing out is asked once, the default is to stay
  if (confirmSignOut) org.zohara.chat.settings.ConfirmSheet(
    "Sign out of Zohara Chat on this phone?", "Your chats stay on the server.", "Sign out",
    onConfirm = { confirmSignOut = false; onSignOut() }, onDismiss = { confirmSignOut = false },
  )
}

/** NAV-14: the footer menu as a bottom sheet: "Running now" (pages module), Settings, Sign out. */
@Composable
fun UserMenuSheet(userLabel: String, onDismiss: () -> Unit, onSettings: () -> Unit, onSignOut: () -> Unit) {
  val c = ZoTheme.colors
  ZoBottomSheet(title = userLabel, onDismiss = onDismiss) {
    RunningNowSection()
    SheetRow(ZoIcons.Gear, "Settings", onSettings)
    SheetRow(ZoIcons.SignOut, "Sign out", onSignOut)
  }
}

@Composable
private fun SheetRow(icon: ImageVector, label: String, onClick: () -> Unit) {
  val c = ZoTheme.colors
  Row(Modifier.fillMaxWidth().clip(ZoTheme.shapes.navItem).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 6.dp, vertical = 13.dp), verticalAlignment = Alignment.CenterVertically) {
    Icon(icon, null, Modifier.width(20.dp).height(20.dp), tint = c.icon)
    Text(label, style = ZoTheme.type.navItem, color = c.text, modifier = Modifier.padding(start = 14.dp))
  }
}
