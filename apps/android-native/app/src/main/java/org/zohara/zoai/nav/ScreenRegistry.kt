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

/**
 * Route pattern -> the screen composable. The navigation graph is built from this map; [Routes.all] lists every screen and a unit test checks
 * that both have the same keys, so a screen can not be added to one and forgotten in the other.
 */
object ScreenRegistry {
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
    Routes.SETTINGS to { _ -> SettingsHomeScreen() },
    Routes.SETTINGS_MODEL to { _ -> SettingsModelScreen() },
    Routes.SETTINGS_PERSONALITY to { _ -> SettingsPersonalityScreen() },
    Routes.SETTINGS_MEMORY to { _ -> SettingsMemoryScreen() },
    Routes.SETTINGS_GENERAL to { _ -> SettingsGeneralScreen() },
    Routes.SETTINGS_APPS to { _ -> SettingsAppsScreen() },
    Routes.SETTINGS_MACHINES to { _ -> SettingsMachinesScreen() },
    Routes.SETTINGS_SYNC to { _ -> SettingsSyncScreen() },
    Routes.SETTINGS_COMPUTERS to { _ -> MyComputersScreen() },
    Routes.SETTINGS_NOTIFICATIONS to { _ -> SettingsNotificationsScreen() },
    Routes.SETTINGS_ACCOUNT to { _ -> SettingsAccountScreen() },
    Routes.SETTINGS_ADVANCED to { _ -> SettingsAdvancedScreen() },
    Routes.SETTINGS_DATA to { _ -> SettingsDataScreen() },
    Routes.SETTINGS_ABOUT to { _ -> SettingsAboutScreen() },
    Routes.SETTINGS_APPROVE to { _ -> SettingsApproveScreen() },
    Routes.CAMERA to { _ -> CameraScreen() },
    Routes.PICTURE to { e -> PictureScreen(e.arguments?.getInt(Routes.ARG_INDEX) ?: 0) },
  )
}
