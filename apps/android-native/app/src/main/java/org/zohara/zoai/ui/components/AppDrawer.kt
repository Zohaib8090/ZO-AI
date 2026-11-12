package org.zohara.zoai.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import org.zohara.zoai.model.ChatSummary
import org.zohara.zoai.theme.*

@Composable
fun AppDrawer(
    chats: List<ChatSummary>,
    currentChatId: String,
    onSelectChat: (String) -> Unit,
    onNewChat: () -> Unit,
    onNavigate: (String) -> Unit,
    isAdmin: Boolean = false,
    modifier: Modifier = Modifier
) {
    val pinned = chats.filter { it.pinned }
    val recents = chats.filter { !it.pinned }

    ModalDrawerSheet(
        drawerContainerColor = BgDark,
        drawerContentColor = TextDark,
        modifier = modifier.width(300.dp)
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(16.dp)
        ) {
            // App Header
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(vertical = 8.dp)
            ) {
                Text(
                    text = "ZO-AI",
                    style = MaterialTheme.typography.titleLarge,
                    color = TextDark
                )
            }

            Spacer(modifier = Modifier.height(8.dp))

            // New chat button
            Button(
                onClick = onNewChat,
                colors = ButtonDefaults.buttonColors(containerColor = Card2Dark, contentColor = TextDark),
                shape = RoundedCornerShape(10.dp),
                modifier = Modifier.fillMaxWidth()
            ) {
                Icon(Icons.Default.Add, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(8.dp))
                Text("New chat")
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Chat lists
            LazyColumn(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp)
            ) {
                if (pinned.isNotEmpty()) {
                    item {
                        Text(
                            text = "PINNED",
                            style = MaterialTheme.typography.labelSmall,
                            color = DimDark,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                        )
                    }
                    items(pinned) { chat ->
                        DrawerChatItem(
                            chat = chat,
                            isSelected = chat.id == currentChatId,
                            onClick = { onSelectChat(chat.id) }
                        )
                    }
                }

                if (recents.isNotEmpty()) {
                    item {
                        Text(
                            text = "CHATS",
                            style = MaterialTheme.typography.labelSmall,
                            color = DimDark,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp)
                        )
                    }
                    items(recents) { chat ->
                        DrawerChatItem(
                            chat = chat,
                            isSelected = chat.id == currentChatId,
                            onClick = { onSelectChat(chat.id) }
                        )
                    }
                }
            }

            Divider(color = LineDark, modifier = Modifier.padding(vertical = 8.dp))

            // Navigation items
            DrawerNavItem(
                icon = Icons.Default.Tune,
                label = "Settings",
                onClick = { onNavigate("settings") }
            )
            DrawerNavItem(
                icon = Icons.Default.Timeline,
                label = "Activity",
                onClick = { onNavigate("activity") }
            )
            if (isAdmin) {
                DrawerNavItem(
                    icon = Icons.Default.Terminal,
                    label = "Terminal",
                    onClick = { onNavigate("terminal") }
                )
            }
            DrawerNavItem(
                icon = Icons.Default.QrCodeScanner,
                label = "Approve sign-in",
                onClick = { onNavigate("device") }
            )
        }
    }
}

@Composable
private fun DrawerChatItem(
    chat: ChatSummary,
    isSelected: Boolean,
    onClick: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .background(if (isSelected) Card2Dark else androidx.compose.ui.graphics.Color.Transparent)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 9.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(
            imageVector = if (chat.pinned) Icons.Default.PushPin else Icons.Default.ChatBubbleOutline,
            contentDescription = null,
            tint = if (isSelected) Accent else DimDark,
            modifier = Modifier.size(16.dp)
        )
        Spacer(modifier = Modifier.width(10.dp))
        Text(
            text = chat.title,
            style = MaterialTheme.typography.bodyMedium,
            color = if (isSelected) TextDark else Text2Dark,
            maxLines = 1,
            modifier = Modifier.weight(1f)
        )
    }
}

@Composable
private fun DrawerNavItem(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    onClick: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(8.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(imageVector = icon, contentDescription = null, tint = DimDark, modifier = Modifier.size(18.dp))
        Spacer(modifier = Modifier.width(12.dp))
        Text(text = label, style = MaterialTheme.typography.bodyMedium, color = TextDark)
    }
}
