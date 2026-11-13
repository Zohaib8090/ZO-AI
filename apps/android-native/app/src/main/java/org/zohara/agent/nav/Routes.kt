package org.zohara.agent.nav

import android.net.Uri

/**
 * The route of every screen of the app (docs/ANDROID-NATIVE-PARITY.md). Other modules never invent a route: they fill in the stub screen
 * that owns it (see [org.zohara.agent.nav.ScreenRegistry]). Argument routes have a builder function that encodes the argument.
 */
object Routes {
  // -- main --
  const val HOME = "home"
  const val ARG_ID = "id"
  const val CHAT = "chat/{id}"
  /** full-screen viewer for the output of a tool run (CHAT-34) */
  const val TOOL_OUTPUT = "chat/{id}/tool/{index}"
  const val ARG_INDEX = "index"
  const val CHATS = "chats"
  const val SEARCH = "search"
  const val PROJECTS = "projects"
  const val ARTIFACTS = "artifacts"
  const val SCHEDULED = "scheduled"
  const val ACTIVITY = "activity"
  const val TERMINAL = "terminal"

  // -- settings --
  const val SETTINGS = "settings"
  const val SETTINGS_MODEL = "settings/model"
  const val SETTINGS_PERSONALITY = "settings/personality"
  const val SETTINGS_MEMORY = "settings/memory"
  const val SETTINGS_GENERAL = "settings/general"
  const val SETTINGS_APPS = "settings/apps"
  const val SETTINGS_MACHINES = "settings/machines"
  const val SETTINGS_SYNC = "settings/sync"
  const val SETTINGS_COMPUTERS = "settings/computers"
  const val SETTINGS_NOTIFICATIONS = "settings/notifications"
  const val SETTINGS_ACCOUNT = "settings/account"
  const val SETTINGS_ADVANCED = "settings/advanced"
  const val SETTINGS_DATA = "settings/data"
  const val SETTINGS_ABOUT = "settings/about"
  const val SETTINGS_APPROVE = "settings/approve"

  // -- media --
  const val CAMERA = "media/camera"
  const val PICTURE = "media/picture/{index}"

  fun chat(id: String) = "chat/${Uri.encode(id)}"
  fun toolOutput(id: String, index: Int) = "chat/${Uri.encode(id)}/tool/$index"
  fun picture(index: Int) = "media/picture/$index"

  /** Which module fills the screen in. */
  enum class Owner { Chat, Pages, Settings, Media, Link }

  class Def(val pattern: String, val title: String, val owner: Owner, val drawerItem: Boolean = false)

  /** Every screen, in one list. The navigation graph is built from it, and a unit test checks that each one has a screen. */
  val all: List<Def> = listOf(
    Def(HOME, "", Owner.Chat, drawerItem = true),
    Def(CHAT, "", Owner.Chat),
    Def(TOOL_OUTPUT, "Output", Owner.Chat),
    Def(CHATS, "Chats", Owner.Pages, drawerItem = true),
    Def(SEARCH, "Search", Owner.Pages, drawerItem = true),
    Def(PROJECTS, "Projects", Owner.Pages, drawerItem = true),
    Def(ARTIFACTS, "Artifacts", Owner.Pages, drawerItem = true),
    Def(SCHEDULED, "Scheduled", Owner.Pages, drawerItem = true),
    Def(ACTIVITY, "Activity", Owner.Pages, drawerItem = true),
    Def(TERMINAL, "Terminal", Owner.Pages),
    Def(SETTINGS, "Settings", Owner.Settings),
    Def(SETTINGS_MODEL, "Model and profiles", Owner.Settings),
    Def(SETTINGS_PERSONALITY, "Personality", Owner.Settings),
    Def(SETTINGS_MEMORY, "Memory", Owner.Settings),
    Def(SETTINGS_GENERAL, "Appearance and permissions", Owner.Settings),
    Def(SETTINGS_APPS, "Connected apps", Owner.Settings),
    Def(SETTINGS_MACHINES, "Machines", Owner.Settings),
    Def(SETTINGS_SYNC, "Desktop sync", Owner.Settings),
    Def(SETTINGS_COMPUTERS, "My computers", Owner.Link),
    Def(SETTINGS_NOTIFICATIONS, "Notifications", Owner.Settings),
    Def(SETTINGS_ACCOUNT, "Account", Owner.Settings),
    Def(SETTINGS_ADVANCED, "Advanced", Owner.Settings),
    Def(SETTINGS_DATA, "Data", Owner.Settings),
    Def(SETTINGS_ABOUT, "About", Owner.Settings),
    Def(SETTINGS_APPROVE, "Approve a sign-in", Owner.Settings),
    Def(CAMERA, "Camera", Owner.Media),
    Def(PICTURE, "Picture", Owner.Media),
  )

  val byPattern: Map<String, Def> = all.associateBy { it.pattern }

  /** The title of the top bar for a back stack route pattern (NAV-16); a chat shows its own title through `SetTopBar`. */
  fun titleFor(pattern: String?): String = pattern?.let { byPattern[it]?.title }.orEmpty()
}
