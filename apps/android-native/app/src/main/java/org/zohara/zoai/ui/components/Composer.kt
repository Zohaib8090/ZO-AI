package org.zohara.zoai.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.AttachFile
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.theme.*

@Composable
fun Composer(
    text: String,
    onTextChange: (String) -> Unit,
    isBusy: Boolean,
    enabled: Boolean,
    onSend: () -> Unit,
    onStop: () -> Unit,
    onAttach: () -> Unit = {},
    onMic: () -> Unit = {},
    selectedModel: String = "claude-3-5-sonnet",
    permissionMode: String = "Auto",
    modifier: Modifier = Modifier
) {
    Card(
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = ComposerBg),
        modifier = modifier
            .fillMaxWidth()
            .border(1.dp, LineDark, RoundedCornerShape(16.dp))
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)
        ) {
            TextField(
                value = text,
                onValueChange = onTextChange,
                enabled = enabled,
                placeholder = {
                    Text(
                        if (enabled) "Ask anything..." else "Answer the card above...",
                        color = DimDark,
                        fontSize = 15.sp
                    )
                },
                colors = TextFieldDefaults.colors(
                    focusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                    unfocusedContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                    disabledContainerColor = androidx.compose.ui.graphics.Color.Transparent,
                    focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    disabledIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
                    focusedTextColor = TextDark,
                    unfocusedTextColor = TextDark,
                    disabledTextColor = DimDark
                ),
                maxLines = 6,
                modifier = Modifier.fillMaxWidth()
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically
            ) {
                IconButton(
                    onClick = onAttach,
                    enabled = enabled && !isBusy,
                    modifier = Modifier.size(32.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.AttachFile,
                        contentDescription = "Attach",
                        tint = DimDark
                    )
                }

                IconButton(
                    onClick = onMic,
                    enabled = enabled && !isBusy,
                    modifier = Modifier.size(32.dp)
                ) {
                    Icon(
                        imageVector = Icons.Default.Mic,
                        contentDescription = "Voice input",
                        tint = DimDark
                    )
                }

                Spacer(modifier = Modifier.weight(1f))

                // Mode chip
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(6.dp))
                        .background(Card2Dark)
                        .padding(horizontal = 6.dp, vertical = 3.dp)
                ) {
                    Text(
                        text = permissionMode,
                        style = MaterialTheme.typography.labelSmall,
                        color = Text2Dark
                    )
                }

                Spacer(modifier = Modifier.width(8.dp))

                // Send or Stop button
                if (isBusy) {
                    IconButton(
                        onClick = onStop,
                        modifier = Modifier
                            .size(32.dp)
                            .clip(CircleShape)
                            .background(AccentRed)
                    ) {
                        Icon(
                            imageVector = Icons.Default.Stop,
                            contentDescription = "Stop",
                            tint = TextDark,
                            modifier = Modifier.size(16.dp)
                        )
                    }
                } else {
                    IconButton(
                        onClick = onSend,
                        enabled = enabled && text.trim().isNotEmpty(),
                        modifier = Modifier
                            .size(32.dp)
                            .clip(CircleShape)
                            .background(if (enabled && text.trim().isNotEmpty()) Accent else Card2Dark)
                    ) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.Send,
                            contentDescription = "Send",
                            tint = if (enabled && text.trim().isNotEmpty()) BgDark else DimDark,
                            modifier = Modifier.size(16.dp)
                        )
                    }
                }
            }
        }
    }
}
