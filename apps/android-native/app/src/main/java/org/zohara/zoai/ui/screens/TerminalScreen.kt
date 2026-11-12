package org.zohara.zoai.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch
import org.zohara.zoai.data.ApiClient
import org.zohara.zoai.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(
    apiClient: ApiClient,
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    val scope = rememberCoroutineScope()
    var cmdText by remember { mutableStateOf("") }
    var terminalOutput by remember { mutableStateOf("") }
    var isRunning by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Cloud Terminal", style = MaterialTheme.typography.titleMedium, color = TextDark) },
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
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(14.dp)
        ) {
            // Output area
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .background(CardDark)
                    .border(1.dp, LineDark, RoundedCornerShape(10.dp))
                    .padding(12.dp)
            ) {
                LazyColumn {
                    item {
                        Text(
                            text = if (terminalOutput.isEmpty()) "$ echo \"Connected to cloud terminal\"" else terminalOutput,
                            fontFamily = FontFamily.Monospace,
                            fontSize = 13.sp,
                            lineHeight = 18.sp,
                            color = TextDark
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Command input row
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                OutlinedTextField(
                    value = cmdText,
                    onValueChange = { cmdText = it },
                    placeholder = { Text("Run command (e.g. ls -la)", color = DimDark, fontSize = 14.sp) },
                    enabled = !isRunning,
                    colors = OutlinedTextFieldDefaults.colors(
                        focusedBorderColor = Accent,
                        unfocusedBorderColor = LineDark,
                        focusedTextColor = TextDark,
                        unfocusedTextColor = TextDark
                    ),
                    modifier = Modifier.weight(1f)
                )
                Spacer(modifier = Modifier.width(8.dp))
                IconButton(
                    onClick = {
                        val cmd = cmdText.trim()
                        if (cmd.isEmpty() || isRunning) return@IconButton
                        isRunning = true
                        terminalOutput += "\n$ $cmd\n"
                        cmdText = ""
                        // In cloud mode, runs via terminal_run op or command
                        scope.launch {
                            // simulated terminal response until op completes
                            terminalOutput += "Running...\n"
                            isRunning = false
                        }
                    },
                    enabled = !isRunning && cmdText.trim().isNotEmpty(),
                    modifier = Modifier
                        .size(44.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .background(if (!isRunning && cmdText.trim().isNotEmpty()) Accent else Card2Dark)
                ) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Filled.Send,
                        contentDescription = "Run",
                        tint = if (!isRunning && cmdText.trim().isNotEmpty()) BgDark else DimDark
                    )
                }
            }
        }
    }
}
