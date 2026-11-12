package org.zohara.zoai.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
data class QuestionOptionData(
    val label: String,
    val description: String = ""
)

@Serializable
data class QuestionData(
    val question: String,
    val header: String = "",
    @SerialName("multi_select") val multiSelect: Boolean = false,
    val options: List<QuestionOptionData> = emptyList()
)

@Serializable
data class QuestionAnswerData(
    val selected: List<String> = emptyList(),
    val other: String? = null
)

@Serializable
data class ChatSummary(
    val id: String,
    val title: String = "Untitled",
    val updated: Long = 0,
    val pinned: Boolean = false
)

@Serializable
data class ChatEntry(
    val kind: String,
    val text: String,
    val attach: List<String> = emptyList()
)

@Serializable
data class Whoami(
    val email: String = "",
    val admin: Boolean = false,
    val restricted: Boolean = true
)

@Serializable
data class ModelInfo(
    val id: String,
    val name: String,
    val provider: String = ""
)

sealed class StreamEvent {
    data class Stream(val id: Long, val device: String? = null) : StreamEvent()
    data class Chat(val id: String, val title: String) : StreamEvent()
    data class Delta(val text: String) : StreamEvent()
    data class Text(val text: String) : StreamEvent()
    data class ToolStart(val name: String, val summary: String) : StreamEvent()
    data class ToolDone(val name: String, val ok: Boolean, val output: String) : StreamEvent()
    data class Approval(val id: Long, val tool: String, val summary: String) : StreamEvent()
    data class Question(val id: Long, val questions: List<QuestionData>) : StreamEvent()
    object Done : StreamEvent()
    object Stopped : StreamEvent()
    data class Error(val error: String, val message: String? = null) : StreamEvent()
}
