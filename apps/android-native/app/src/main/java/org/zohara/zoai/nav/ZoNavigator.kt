package org.zohara.zoai.nav

import androidx.compose.runtime.Stable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.navigation.NavHostController

/** The drawer's state, shared with screens that need to open or close it. */
@Stable
interface DrawerControl {
  val isOpen: Boolean
  fun open()
  fun close()
}

/**
 * Navigation for screens: `LocalNav.current.openChat(id)`. Screens never touch the NavController themselves, so the back behaviour
 * (NAV-21) stays in one place.
 */
@Stable
class ZoNavigator(private val controller: NavHostController, val drawer: DrawerControl) {
  private fun go(route: String) {
    drawer.close()
    controller.navigate(route) { launchSingleTop = true }
  }

  /** the home screen = a new chat (NAV-02 "New") */
  fun home() {
    drawer.close()
    if (!controller.popBackStack(Routes.HOME, inclusive = false)) controller.navigate(Routes.HOME) { launchSingleTop = true }
  }
  fun openChat(id: String) = go(Routes.chat(id))
  fun openToolOutput(chatId: String, index: Int) = go(Routes.toolOutput(chatId, index))
  fun chats() = go(Routes.CHATS)
  fun search() = go(Routes.SEARCH)
  fun projects() = go(Routes.PROJECTS)
  fun artifacts() = go(Routes.ARTIFACTS)
  fun scheduled() = go(Routes.SCHEDULED)
  fun activity() = go(Routes.ACTIVITY)
  fun terminal() = go(Routes.TERMINAL)
  /** [page] is one of the `Routes.SETTINGS_*` routes, or null for the Settings home */
  fun settings(page: String? = null) = go(page ?: Routes.SETTINGS)
  /** NAV-23: in the two-pane layout a page chosen in the list replaces the page on the right (back still leaves Settings in one step) */
  fun settingsInPane(page: String) {
    controller.navigate(page) { popUpTo(Routes.SETTINGS) { inclusive = false }; launchSingleTop = true }
  }
  fun camera() = go(Routes.CAMERA)
  fun picture(index: Int) = go(Routes.picture(index))
  /** any route of [Routes] (already encoded) */
  fun navigate(route: String) = go(route)
  fun back(): Boolean = controller.popBackStack()
  val currentRoute: String? get() = controller.currentBackStackEntry?.destination?.route
}

val LocalNav = staticCompositionLocalOf<ZoNavigator> { error("no navigator") }
