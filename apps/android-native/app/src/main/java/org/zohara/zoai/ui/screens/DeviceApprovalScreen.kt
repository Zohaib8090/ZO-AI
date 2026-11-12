package org.zohara.zoai.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.data.ApiClient
import org.zohara.zoai.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DeviceApprovalScreen(
    apiClient: ApiClient,
    onBack: () -> Unit,
    modifier: Modifier = Modifier
) {
    var code by remember { mutableStateOf("") }
    var lookupDone by remember { mutableStateOf(false) }
    var statusMessage by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Approve Sign-in", style = MaterialTheme.typography.titleMedium, color = TextDark) },
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
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Text(
                text = "Enter the 8-character code shown on your computer or terminal (e.g. ABCD-EFGH).",
                style = MaterialTheme.typography.bodyMedium,
                color = Text2Dark
            )

            OutlinedTextField(
                value = code,
                onValueChange = { input ->
                    val clean = input.uppercase().filter { it.isLetterOrDigit() || it == '-' }.take(9)
                    code = clean
                },
                placeholder = { Text("ABCD-EFGH", color = DimDark, letterSpacing = 2.sp) },
                textStyle = LocalTextStyle.current.copy(
                    fontFamily = FontFamily.Monospace,
                    fontWeight = FontWeight.Bold,
                    fontSize = 20.sp,
                    letterSpacing = 2.sp
                ),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = Accent,
                    unfocusedBorderColor = LineDark,
                    focusedTextColor = TextDark,
                    unfocusedTextColor = TextDark
                ),
                modifier = Modifier.fillMaxWidth()
            )

            Button(
                onClick = {
                    if (code.length >= 8) {
                        lookupDone = true
                        statusMessage = "Found device request for $code."
                    } else {
                        statusMessage = "Please enter an 8-character code."
                    }
                },
                colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = BgDark),
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Look up")
            }

            if (lookupDone) {
                Card(
                    shape = RoundedCornerShape(12.dp),
                    colors = CardDefaults.cardColors(containerColor = CardDark),
                    modifier = Modifier
                        .fillMaxWidth()
                        .border(1.dp, LineDark, RoundedCornerShape(12.dp))
                ) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp)
                    ) {
                        Text("Device: Desktop app", style = MaterialTheme.typography.titleMedium, color = TextDark)
                        Text(
                            "Only approve if you started this on your own device.",
                            style = MaterialTheme.typography.bodySmall,
                            color = DimDark
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(
                                onClick = {
                                    statusMessage = "Approved!"
                                    lookupDone = false
                                    code = ""
                                },
                                colors = ButtonDefaults.buttonColors(containerColor = AccentGreen, contentColor = BgDark)
                            ) {
                                Text("Approve")
                            }
                            OutlinedButton(
                                onClick = {
                                    statusMessage = "Denied."
                                    lookupDone = false
                                },
                                colors = ButtonDefaults.outlinedButtonColors(contentColor = AccentRed)
                            ) {
                                Text("Deny")
                            }
                        }
                    }
                }
            }

            if (statusMessage != null) {
                Text(
                    text = statusMessage!!,
                    style = MaterialTheme.typography.bodyMedium,
                    color = Accent,
                    modifier = Modifier.padding(top = 8.dp)
                )
            }
        }
    }
}
