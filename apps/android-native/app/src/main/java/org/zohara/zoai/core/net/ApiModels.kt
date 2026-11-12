package org.zohara.zoai.core.net

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNamingStrategy

/**
 * Response shapes of docs API.md. Property names are camelCase; the JSON is snake_case ([ZoJson] maps them).
 * Every field has a default so a server that leaves one out (older server, restricted account) never breaks the app.
 */
@OptIn(ExperimentalSerializationApi::class)
val ZoJson: Json = Json {
  ignoreUnknownKeys = true
  isLenient = true
  coerceInputValues = true
  explicitNulls = false
  encodeDefaults = false
  namingStrategy = JsonNamingStrategy.SnakeCase
}

// ---- status, chats ----
@Serializable data class StatusInfo(val mode: String = "", val model: String = "", val provider: String = "", val ready: Boolean = false, val version: String = "")
@Serializable data class ChatSummary(val id: String, val pinned: Boolean = false, val title: String = "", val updated: Long = 0)
@Serializable data class ChatsResp(val chats: List<ChatSummary> = emptyList())
@Serializable data class AttachInfo(val kind: String = "", val name: String = "", val size: Long = 0, val thumb: String? = null)
@Serializable data class ChatEntry(val kind: String = "", val text: String = "", val attach: List<AttachInfo> = emptyList())
@Serializable data class TakeInfo(val count: Int = 1, val current: Int = 0, val nth: Int = 0)
@Serializable data class ChatDetail(val id: String = "", val title: String = "", val entries: List<ChatEntry> = emptyList(), val takes: List<TakeInfo> = emptyList())
@Serializable data class SearchHit(val id: String, val snippet: String = "", val title: String = "", val updated: Long = 0)
@Serializable data class SearchResp(val hits: List<SearchHit> = emptyList())
@Serializable data class LinkPreview(val title: String = "", val description: String = "", val host: String = "")
@Serializable data class ModeResp(val mode: String = "")
@Serializable data class ActivityItem(
  val id: Long = 0, val kind: String = "", val title: String = "", val detail: String = "", val source: String = "",
  val chatId: String? = null, val startedMs: Long = 0, val canStop: Boolean = false,
)
@Serializable data class ClientSeen(val lastMs: Long = 0, val source: String = "")
@Serializable data class ActivityResp(val nowMs: Long = 0, val clientsSeen: List<ClientSeen> = emptyList(), val items: List<ActivityItem> = emptyList())
@Serializable data class DeletedCount(val deleted: Int = 0)
@Serializable data class TextResp(val output: String = "", val message: String = "", val text: String = "")

// ---- models ----
@Serializable data class ProviderInfo(
  val id: String, val name: String = "", val local: Boolean = false, val hasKey: Boolean = false, val needsUrl: Boolean = false,
  val defaultUrl: String = "", val baseUrl: String = "", val hidden: Boolean = false,
)
@Serializable data class ProvidersResp(val current: String = "", val hiddenProviders: List<String> = emptyList(), val providers: List<ProviderInfo> = emptyList())
@Serializable data class ModelsResp(
  val provider: String = "", val models: List<String> = emptyList(), val current: String = "", val efforts: List<String> = emptyList(),
  val effort: String? = null, val baseUrl: String = "", val error: String? = null,
)
@Serializable data class ModelTestResp(val works: Boolean = false, val message: String = "")
@Serializable data class SelectResp(val provider: String = "", val model: String = "", val effort: String? = null, val baseUrl: String = "")
@Serializable data class Profile(
  val id: String, val name: String = "", val provider: String = "", val model: String = "", val effort: String? = null,
  val pinned: List<String> = emptyList(), val baseUrl: String = "", val vision: Boolean? = null, val updated: Long = 0,
)
@Serializable data class ProfilesResp(val active: String = "", val profiles: List<Profile> = emptyList())
@Serializable data class IdResp(val id: String = "")
@Serializable data class ProfileUseResp(val provider: String = "", val model: String = "", val effort: String? = null, val baseUrl: String = "")
@Serializable data class ProviderPref(val order: Int? = null)
@Serializable data class SettingsResp(val hiddenProviders: List<String> = emptyList(), val providerPrefs: Map<String, ProviderPref> = emptyMap(), val updated: Long = 0)

// ---- memory, personalities ----
@Serializable data class MemoryItem(val id: String, val kind: String = "fact", val text: String = "", val title: String = "", val chat: String = "", val pinned: Boolean = false, val updated: Long = 0)
@Serializable data class MemoryResp(val enabled: Boolean = true, val every: Int = 3, val items: List<MemoryItem> = emptyList())
@Serializable data class Personality(val id: String, val name: String = "", val prompt: String = "", val generated: Boolean = false, val builtin: Boolean = false)
@Serializable data class PersonalitiesResp(val active: String = "", val items: List<Personality> = emptyList())
@Serializable data class PersonalityGenerated(val id: String = "", val name: String = "", val prompt: String = "")

// ---- projects, artifacts, schedules, machines, MCP ----
@Serializable data class ProjectsResp(val text: String = "")
@Serializable data class ArtifactItem(val id: String = "", val kind: String = "", val title: String = "", val path: String = "", val text: String? = null, val chat: String = "", val created: Long = 0)
@Serializable data class ArtifactsResp(val items: List<ArtifactItem> = emptyList())
@Serializable data class ScheduledTask(val id: String, val title: String = "", val prompt: String = "", val nextRun: Long = 0, val repeat: String = "once", val enabled: Boolean = true)
@Serializable data class ScheduledResp(val tasks: List<ScheduledTask> = emptyList())
@Serializable data class Machine(val name: String, val host: String = "", val user: String = "", val port: Int = 22, val note: String = "", val hasKey: Boolean = false)
@Serializable data class MachinesResp(val machines: List<Machine> = emptyList())
@Serializable data class McpServer(val name: String, val command: String = "", val enabled: Boolean = true)
@Serializable data class McpResp(val servers: List<McpServer> = emptyList())

// ---- account ----
@Serializable data class TermsState(val version: String = "", val accepted: Boolean = false)
@Serializable data class Limits(val requestsPerMin: Int = 0, val concurrentSends: Int = 0, val messageChars: Int? = null, val dataBytes: Long? = null, val toolSteps: Int? = null, val answerSecs: Int? = null)
@Serializable data class Whoami(
  val admin: Boolean = false, val restricted: Boolean = true, val email: String = "", val tenant: String = "",
  val limits: Limits = Limits(), val dataBytes: Long = 0, val terms: TermsState = TermsState(),
)
@Serializable data class VersionResp(val version: String = "")
@Serializable data class AccountDevice(val id: String, val name: String = "", val kind: String = "", val scope: String = "", val created: Long = 0, val lastUsed: Long = 0)
@Serializable data class DevicesResp(val devices: List<AccountDevice> = emptyList())
@Serializable data class DeleteAccountResp(val loginRemoved: Boolean = true)
@Serializable data class PushConfig(val vapidPublicKey: String = "", val fcm: Boolean = false)
@Serializable data class PushSubscription(val id: String, val kind: String = "", val label: String = "", val created: Long = 0)
@Serializable data class PushListResp(val subscriptions: List<PushSubscription> = emptyList())
@Serializable data class ClaudeStatus(val present: Boolean = false, val source: String = "none", val setAt: Long = 0, val admin: Boolean = false)
@Serializable data class DeviceLookup(val name: String = "", val kind: String = "", val requestedAt: Long = 0, val ip: String = "", val sameNetwork: Boolean = true, val email: String = "")
@Serializable data class LinkCaps(val claudeCode: Boolean = false, val approvalsFromWeb: Boolean = false, val version: String = "", val os: String = "")
@Serializable data class LinkDevice(val id: String, val name: String = "", val online: Boolean = false, val lastSeen: Long = 0, val caps: LinkCaps = LinkCaps())
@Serializable data class LinkStatusResp(val devices: List<LinkDevice> = emptyList())
