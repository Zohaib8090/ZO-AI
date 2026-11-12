package org.zohara.zoai.ui.screens

import android.app.Activity
import android.content.Intent
import android.speech.RecognizerIntent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.zoai.data.ApiClient
import org.zohara.zoai.model.*
import org.zohara.zoai.theme.*
import org.zohara.zoai.ui.components.*

sealed class ChatUiItem {
    data class UserMessage(val text: String) : ChatUiItem()
    data class AssistantMessage(val text: String) : ChatUiItem()
    data class ToolRun(val name: String, val summary: String, val output: String = "", val isRunning: Boolean = false) : ChatUiItem()
    data class ApprovalItem(val id: Long, val tool: String, val summary: String) : ChatUiItem()
    data class QuestionItem(val id: Long, val questions: List<QuestionData>) : ChatUiItem()
    data class ErrorItem(val text: String) : ChatUiItem()
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(
    chatId: String,
    title: String,
    apiClient: ApiClient,
    selectedModel: String,
    selectedProvider: String,
    permissionMode: String,
    onOpenDrawer: () -> Unit,
    onNewChat: () -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val listState = rememberLazyListState()

    var inputText by remember { mutableStateOf("") }
    var isBusy by remember { mutableStateOf(false) }
    var currentStreamId by remember { mutableStateOf<Long?>(null) }
    var hasActiveQuestion by remember { mutableStateOf(false) }

    val messages = remember { mutableStateListOf<ChatUiItem>() }

    // Voice launcher
    val voiceLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) {
            val spoken = result.data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
            if (!spoken.isNullOrBlank()) {
                inputText = if (inputText.isBlank()) spoken else "$inputText $spoken"
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = title.ifEmpty { "ZO-AI" },
                        style = MaterialTheme.typography.titleMedium,
                        color = TextDark,
                        maxLines = 1
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onOpenDrawer) {
                        Icon(Icons.Default.Menu, contentDescription = "Menu", tint = TextDark)
                    }
                },
                actions = {
                    IconButton(onClick = onNewChat) {
                        Icon(Icons.Default.Add, contentDescription = "New chat", tint = TextDark)
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
                .padding(horizontal = 12.dp)
        ) {
            LazyColumn(
                state = listState,
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(10.dp),
                contentPadding = PaddingValues(vertical = 12.dp)
            ) {
                items(messages) { item ->
                    when (item) {
                        is ChatUiItem.UserMessage -> {
                            Box(modifier = Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                                Box(
                                    modifier = Modifier
                                        .clip(RoundedCornerShape(12.dp))
                                        .background(Card2Dark)
                                        .padding(12.dp)
                                ) {
                                    Text(text = item.text, style = MaterialTheme.typography.bodyLarge, color = TextDark)
                                }
                            }
                        }
                        is ChatUiItem.AssistantMessage -> {
                            Box(modifier = Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart) {
                                Box(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .clip(RoundedCornerShape(12.dp))
                                        .background(CardDark)
                                        .border(1.dp, LineDark, RoundedCornerShape(12.dp))
                                        .padding(14.dp)
                                ) {
                                    Text(text = item.text, style = MaterialTheme.typography.bodyLarge, color = TextDark)
                                }
                            }
                        }
                        is ChatUiItem.ToolRun -> {
                            ToolGroupItem(
                                title = item.summary.ifEmpty { item.name },
                                output = item.output,
                                isRunning = item.isRunning
                            )
                        }
                        is ChatUiItem.ApprovalItem -> {
                            ApprovalCard(
                                tool = item.tool,
                                summary = item.summary,
                                onAllow = {
                                    currentStreamId?.let { sId ->
                                        scope.launch { apiClient.replyApproval(sId, item.id, true) }
                                    }
                                },
                                onDeny = {
                                    currentStreamId?.let { sId ->
                                        scope.launch { apiClient.replyApproval(sId, item.id, false) }
                                    }
                                }
                            )
                        }
                        is ChatUiItem.QuestionItem -> {
                            QuestionCard(
                                questions = item.questions,
                                onAnswer = { answers ->
                                    hasActiveQuestion = false
                                    currentStreamId?.let { sId ->
                                        scope.launch { apiClient.replyAnswer(sId, item.id, answers) }
                                    }
                                },
                                onSkip = {
                                    hasActiveQuestion = false
                                    currentStreamId?.let { sId ->
                                        scope.launch { apiClient.replyAnswer(sId, item.id, null, cancelled = true) }
                                    }
                                }
                            )
                        }
                        is ChatUiItem.ErrorItem -> {
                            Text(
                                text = item.text,
                                style = MaterialTheme.typography.bodyMedium,
                                color = AccentRed,
                                modifier = Modifier.padding(horizontal = 4.dp)
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(6.dp))

            // Composer
            Composer(
                text = inputText,
                onTextChange = { inputText = it },
                isBusy = isBusy,
                enabled = !hasActiveQuestion,
                selectedModel = selectedModel,
                permissionMode = permissionMode,
                onSend = {
                    val msg = inputText.trim()
                    if (msg.isEmpty() || isBusy) return@Composer
                    inputText = ""
                    messages.add(ChatUiItem.UserMessage(msg))
                    isBusy = true

                    scope.launch {
                        listState.animateScrollToItem(messages.size - 1)
                        var currentAssistantText = ""
                        var assistantIndex: Int? = null

                        apiClient.send(
                            chatId = chatId,
                            text = msg,
                            model = selectedModel,
                            provider = selectedProvider,
                            mode = permissionMode
                        ).collect { event ->
                            when (event) {
                                is StreamEvent.Stream -> currentStreamId = event.id
                                is StreamEvent.Delta -> {
                                    currentAssistantText += event.text
                                    if (assistantIndex == null) {
                                        messages.add(ChatUiItem.AssistantMessage(currentAssistantText))
                                        assistantIndex = messages.size - 1
                                    } else {
                                        messages[assistantIndex!!] = ChatUiItem.AssistantMessage(currentAssistantText)
                                    }
                                }
                                is StreamEvent.ToolStart -> {
                                    messages.add(ChatUiItem.ToolRun(event.name, event.summary, isRunning = true))
                                    listState.animateScrollToItem(messages.size - 1)
                                }
                                is StreamEvent.ToolDone -> {
                                    val idx = messages.indexOfLast { it is ChatUiItem.ToolRun && it.name == event.name }
                                    if (idx != -1) {
                                        val old = messages[idx] as ChatUiItem.ToolRun
                                        messages[idx] = old.copy(output = event.output, isRunning = false)
                                    }
                                }
                                is StreamEvent.Approval -> {
                                    messages.add(ChatUiItem.ApprovalItem(event.id, event.tool, event.summary))
                                    listState.animateScrollToItem(messages.size - 1)
                                }
                                is StreamEvent.Question -> {
                                    hasActiveQuestion = true
                                    messages.add(ChatUiItem.QuestionItem(event.id, event.questions))
                                    listState.animateScrollToItem(messages.size - 1)
                                }
                                is StreamEvent.Done -> isBusy = false
                                is StreamEvent.Stopped -> isBusy = false
                                is StreamEvent.Error -> {
                                    isBusy = false
                                    messages.add(ChatUiItem.ErrorItem(event.message ?: event.error))
                                }
                                else -> {}
                            }
                        }
                        isBusy = false
                    }
                },
                onStop = {
                    currentStreamId?.let { sId ->
                        scope.launch { apiClient.stopStream(sId) }
                    }
                    isBusy = false
                },
                onMic = {
                    val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
                        putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
                        putExtra(RecognizerIntent.EXTRA_PROMPT, "Speak to ZO-AI")
                    }
                    try {
                        voiceLauncher.launch(intent)
                    } catch (e: Exception) {
                        // voice not available
                    }
                },
                modifier = Modifier.padding(bottom = 12.dp)
            )
        }
    }
}
