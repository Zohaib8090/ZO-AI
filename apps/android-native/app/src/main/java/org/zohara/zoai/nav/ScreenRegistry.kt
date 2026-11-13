package org.zohara.zoai.nav

import androidx.compose.runtime.Composable
import androidx.navigation.NavBackStackEntry
import org.zohara.zoai.chat.ChatScreen
import org.zohara.zoai.chat.HomeScreen
import org.zohara.zoai.chat.ToolOutputScreen
import org.zohara.zoai.link.MyComputersScreen
import org.zohara.zoai.media.CameraScreen
import org.zohara.zoai.media.PictureScreen
import org.zohara.zoai.pages.ActivityScreen
import org.zohara.zoai.pages.ArtifactsScreen
import org.zohara.zoai.pages.ChatsScreen
import org.zohara.zoai.pages.ProjectsScreen
import org.zohara.zoai.pages.ScheduledScreen
import org.zohara.zoai.pages.SearchScreen
import org.zohara.zoai.pages.TerminalScreen
import org.zohara.zoai.settings.SettingsAboutScreen
import org.zohara.zoai.settings.SettingsAccountScreen
import org.zohara.zoai.settings.SettingsAdvancedScreen
import org.zohara.zoai.settings.SettingsApproveScreen
import org.zohara.zoai.settings.SettingsAppsScreen
import org.zohara.zoai.settings.SettingsDataScreen
import org.zohara.zoai.settings.SettingsGeneralScreen
import org.zohara.zoai.settings.SettingsHomeScreen
import org.zohara.zoai.settings.SettingsMachinesScreen
import org.zohara.zoai.settings.SettingsMemoryScreen
import org.zohara.zoai.settings.SettingsModelScreen
import org.zohara.zoai.settings.SettingsNotificationsScreen
import org.zohara.zoai.settings.SettingsPersonalityScreen
import org.zohara.zoai.settings.SettingsSyncScreen
import org.zohara.zoai.settings.SettingsTwoPane

/**
 * Route pattern -> the screen composable. The navigation graph is built from this map; [Routes.all] lists every screen and a unit test checks
 * that both have the same keys, so a screen can not be added to one and forgotten in the other.
 */
object ScreenRegistry {
  /** the page screens of Settings by route, for the right pane of the two-pane layout (NAV-23) */
  private val settingsPages: Map<String, @Composable () -> Unit> = mapOf(
    Routes.SETTINGS_MODEL to { SettingsModelScreen() },
    Routes.SETTINGS_PERSONALITY to { SettingsPersonalityScreen() },
    Routes.SETTINGS_MEMORY to { SettingsMemoryScreen() },
    Routes.SETTINGS_GENERAL to { SettingsGeneralScreen() },
    Routes.SETTINGS_APPS to { SettingsAppsScreen() },
    Routes.SETTINGS_MACHINES to { SettingsMachinesScreen() },
    Routes.SETTINGS_SYNC to { SettingsSyncScreen() },
    Routes.SETTINGS_COMPUTERS to { MyComputersScreen() },
    Routes.SETTINGS_NOTIFICATIONS to { SettingsNotificationsScreen() },
    Routes.SETTINGS_ACCOUNT to { SettingsAccountScreen() },
    Routes.SETTINGS_ADVANCED to { SettingsAdvancedScreen() },
    Routes.SETTINGS_DATA to { SettingsDataScreen() },
    Routes.SETTINGS_ABOUT to { SettingsAboutScreen() },
  )

  /** a Settings route: a single screen on a phone, the list and the page together from 840 dp (NAV-23) */
  private fun settings(route: String?, page: @Composable () -> Unit): @Composable (NavBackStackEntry) -> Unit = { _ ->
    if (isTwoPane()) SettingsTwoPane(route) { r -> (settingsPages[r] ?: page)() } else page()
  }

  val screens: Map<String, @Composable (NavBackStackEntry) -> Unit> = mapOf(
    Routes.HOME to { _ -> HomeScreen() },
    Routes.CHAT to { e -> ChatScreen(e.arguments?.getString(Routes.ARG_ID).orEmpty()) },
    Routes.TOOL_OUTPUT to { e -> ToolOutputScreen(e.arguments?.getString(Routes.ARG_ID).orEmpty(), e.arguments?.getInt(Routes.ARG_INDEX) ?: 0) },
    Routes.CHATS to { _ -> ChatsScreen() },
    Routes.SEARCH to { _ -> SearchScreen() },
    Routes.PROJECTS to { _ -> ProjectsScreen() },
    Routes.ARTIFACTS to { _ -> ArtifactsScreen() },
    Routes.SCHEDULED to { _ -> ScheduledScreen() },
    Routes.ACTIVITY to { _ -> ActivityScreen() },
    Routes.TERMINAL to { _ -> TerminalScreen() },
    Routes.SETTINGS to settings(null) { SettingsHomeScreen() },
    Routes.SETTINGS_MODEL to settings(Routes.SETTINGS_MODEL) { SettingsModelScreen() },
    Routes.SETTINGS_PERSONALITY to settings(Routes.SETTINGS_PERSONALITY) { SettingsPersonalityScreen() },
    Routes.SETTINGS_MEMORY to settings(Routes.SETTINGS_MEMORY) { SettingsMemoryScreen() },
    Routes.SETTINGS_GENERAL to settings(Routes.SETTINGS_GENERAL) { SettingsGeneralScreen() },
    Routes.SETTINGS_APPS to settings(Routes.SETTINGS_APPS) { SettingsAppsScreen() },
    Routes.SETTINGS_MACHINES to settings(Routes.SETTINGS_MACHINES) { SettingsMachinesScreen() },
    Routes.SETTINGS_SYNC to settings(Routes.SETTINGS_SYNC) { SettingsSyncScreen() },
    Routes.SETTINGS_COMPUTERS to settings(Routes.SETTINGS_COMPUTERS) { MyComputersScreen() },
    Routes.SETTINGS_NOTIFICATIONS to settings(Routes.SETTINGS_NOTIFICATIONS) { SettingsNotificationsScreen() },
    Routes.SETTINGS_ACCOUNT to settings(Routes.SETTINGS_ACCOUNT) { SettingsAccountScreen() },
    Routes.SETTINGS_ADVANCED to settings(Routes.SETTINGS_ADVANCED) { SettingsAdvancedScreen() },
    Routes.SETTINGS_DATA to settings(Routes.SETTINGS_DATA) { SettingsDataScreen() },
    Routes.SETTINGS_ABOUT to settings(Routes.SETTINGS_ABOUT) { SettingsAboutScreen() },
    Routes.SETTINGS_APPROVE to { _ -> SettingsApproveScreen() },
    Routes.CAMERA to { _ -> CameraScreen() },
    Routes.PICTURE to { e -> PictureScreen(e.arguments?.getInt(Routes.ARG_INDEX) ?: 0) },
  )
}
