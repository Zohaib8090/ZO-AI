package org.zohara.zoai.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.zoai.data.ZoaiPreferences
import org.zohara.zoai.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    prefs: ZoaiPreferences,
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val scope = rememberCoroutineScope()
    val serverUrl by prefs.serverUrl.collectAsState(initial = ZoaiPreferences.DEFAULT_SERVER)
    val selectedModel by prefs.selectedModel.collectAsState(initial = "claude-3-5-sonnet")
    val permissionMode by prefs.permissionMode.collectAsState(initial = "Auto")
    val userEmail by prefs.userEmail.collectAsState(initial = null)

    var editServer by remember(serverUrl) { mutableStateOf(serverUrl) }
    var editModel by remember(selectedModel) { mutableStateOf(selectedModel) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings", style = MaterialTheme.typography.titleMedium, color = TextDark) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back", tint = TextDark)
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = BgDark)
            )
        },
        containerColor = BgDark,
        modifier = modifier.fillMaxSize()
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            item {
                Text("MODEL & PERMISSIONS", style = MaterialTheme.typography.labelSmall, color = DimDark)
                Spacer(modifier = Modifier.height(8.dp))
                Card(
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = CardDark),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text("Default Model", style = MaterialTheme.typography.bodyMedium, color = TextDark)
                        OutlinedTextField(
                            value = editModel,
                            onValueChange = {
                                editModel = it
                                scope.launch { prefs.setModel(it, "anthropic") }
                            },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = Accent,
                                unfocusedBorderColor = LineDark,
                                focusedTextColor = TextDark,
                                unfocusedTextColor = TextDark
                            ),
                            modifier = Modifier.fillMaxWidth()
                        )

                        Text("Permission Mode", style = MaterialTheme.typography.bodyMedium, color = TextDark)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            listOf("Auto", "Manual", "Bypass").forEach { mode ->
                                val isSelected = permissionMode == mode
                                Button(
                                    onClick = { scope.launch { prefs.setPermissionMode(mode) } },
                                    colors = ButtonDefaults.buttonColors(
                                        containerColor = if (isSelected) Accent else Card2Dark,
                                        contentColor = if (isSelected) BgDark else TextDark
                                    ),
                                    shape = RoundedCornerShape(8.dp)
                                ) {
                                    Text(mode)
                                }
                            }
                        }
                    }
                }
            }

            item {
                Text("SERVER ADDRESS", style = MaterialTheme.typography.labelSmall, color = DimDark)
                Spacer(modifier = Modifier.height(8.dp))
                Card(
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = CardDark),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        OutlinedTextField(
                            value = editServer,
                            onValueChange = {
                                editServer = it
                                scope.launch { prefs.setServerUrl(it) }
                            },
                            colors = OutlinedTextFieldDefaults.colors(
                                focusedBorderColor = Accent,
                                unfocusedBorderColor = LineDark,
                                focusedTextColor = TextDark,
                                unfocusedTextColor = TextDark
                            ),
                            modifier = Modifier.fillMaxWidth()
                        )
                        Text(
                            "Default: https://zo-ai-server.onrender.com",
                            style = MaterialTheme.typography.bodySmall,
                            color = DimDark
                        )
                    }
                }
            }

            item {
                Text("ACCOUNT", style = MaterialTheme.typography.labelSmall, color = DimDark)
                Spacer(modifier = Modifier.height(8.dp))
                Card(
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = CardDark),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        Text(
                            text = if (userEmail != null) "Signed in as $userEmail" else "Signed out",
                            style = MaterialTheme.typography.bodyMedium,
                            color = TextDark
                        )
                        OutlinedButton(
                            onClick = { scope.launch { prefs.setAuth(null, null) } },
                            colors = ButtonDefaults.outlinedButtonColors(contentColor = AccentRed),
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text("Sign out")
                        }
                    }
                }
            }
        }
    }
}
