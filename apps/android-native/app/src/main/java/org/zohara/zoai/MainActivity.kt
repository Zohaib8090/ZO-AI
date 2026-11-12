package org.zohara.zoai

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import kotlinx.coroutines.launch
import org.zohara.zoai.data.ApiClient
import org.zohara.zoai.data.ZoaiPreferences
import org.zohara.zoai.model.ChatSummary
import org.zohara.zoai.notification.NotificationHelper
import org.zohara.zoai.theme.ZoaiTheme
import org.zohara.zoai.ui.components.AppDrawer
import org.zohara.zoai.ui.screens.*

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Create high-priority notification channels per INTERACTION.md
        NotificationHelper.createChannels(this)

        val prefs = ZoaiPreferences(this)
        val apiClient = ApiClient()

        setContent {
            ZoaiTheme {
                val scope = rememberCoroutineScope()
                val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)

                val serverUrl by prefs.serverUrl.collectAsState(initial = ZoaiPreferences.DEFAULT_SERVER)
                val authToken by prefs.authToken.collectAsState(initial = null)
                val selectedModel by prefs.selectedModel.collectAsState(initial = "claude-3-5-sonnet")
                val selectedProvider by prefs.selectedProvider.collectAsState(initial = "anthropic")
                val permissionMode by prefs.permissionMode.collectAsState(initial = "Auto")

                LaunchedEffect(serverUrl, authToken) {
                    apiClient.setBaseUrl(serverUrl)
                    apiClient.setTokenProvider { authToken }
                }

                var currentRoute by remember { mutableStateOf("chat") }
                var currentChatId by remember { mutableStateOf("new") }
                var currentChatTitle by remember { mutableStateOf("New chat") }
                var chatList by remember { mutableStateOf<List<ChatSummary>>(emptyList()) }
                var isAdmin by remember { mutableStateOf(false) }

                LaunchedEffect(Unit) {
                    apiClient.whoami().onSuccess { w ->
                        isAdmin = w.admin
                    }
                    apiClient.getChats().onSuccess { list ->
                        chatList = list
                    }
                }

                ModalNavigationDrawer(
                    drawerState = drawerState,
                    drawerContent = {
                        AppDrawer(
                            chats = chatList,
                            currentChatId = currentChatId,
                            isAdmin = isAdmin,
                            onSelectChat = { id ->
                                currentChatId = id
                                currentChatTitle = chatList.firstOrNull { it.id == id }?.title ?: "Chat"
                                currentRoute = "chat"
                                scope.launch { drawerState.close() }
                            },
                            onNewChat = {
                                currentChatId = "new"
                                currentChatTitle = "New chat"
                                currentRoute = "chat"
                                scope.launch { drawerState.close() }
                            },
                            onNavigate = { route ->
                                currentRoute = route
                                scope.launch { drawerState.close() }
                            }
                        )
                    }
                ) {
                    Surface(modifier = Modifier.fillMaxSize()) {
                        when (currentRoute) {
                            "chat" -> {
                                ChatScreen(
                                    chatId = currentChatId,
                                    title = currentChatTitle,
                                    apiClient = apiClient,
                                    selectedModel = selectedModel,
                                    selectedProvider = selectedProvider,
                                    permissionMode = permissionMode,
                                    onOpenDrawer = { scope.launch { drawerState.open() } },
                                    onNewChat = {
                                        currentChatId = "new"
                                        currentChatTitle = "New chat"
                                    }
                                )
                            }
                            "settings" -> {
                                SettingsScreen(
                                    prefs = prefs,
                                    onBack = { currentRoute = "chat" }
                                )
                            }
                            "terminal" -> {
                                TerminalScreen(
                                    apiClient = apiClient,
                                    onBack = { currentRoute = "chat" }
                                )
                            }
                            "device" -> {
                                DeviceApprovalScreen(
                                    apiClient = apiClient,
                                    onBack = { currentRoute = "chat" }
                                )
                            }
                            "activity" -> {
                                SettingsScreen(
                                    prefs = prefs,
                                    onBack = { currentRoute = "chat" }
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
