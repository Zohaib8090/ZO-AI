package org.zohara.zoai.settings

import java.net.URI
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.net.Profile
import org.zohara.zoai.core.net.ProfilesResp
import org.zohara.zoai.core.net.ProviderInfo

/** The pure rules of the model menu, the model sheet and the Model page (MODEL-*). No Android, no network: unit tested. */
object ModelLogic {
  private val LOCAL_IDS = setOf("ollama", "lmstudio", "llamacpp", "vllm", "sglang", "claude-code")
  private val MAJOR_IDS = setOf("anthropic", "openai", "gemini")

  fun isLocal(p: ProviderInfo): Boolean = p.local || p.id in LOCAL_IDS

  /**
   * MODEL-10: the providers the person may be offered. Restricted accounts get no local providers and no Claude Code; hidden ones are left
   * out except the one in use ([keep]).
   */
  fun shownProviders(all: List<ProviderInfo>, hidden: Collection<String>, keep: String?, restricted: Boolean): List<ProviderInfo> =
    all.filter { !(restricted && isLocal(it)) && (it.id == keep || it.id !in hidden) }

  /** The list of the sheet: also no Claude Code unless this admin's server has the sign-in ([claudeReady], MODEL-10). */
  fun sheetProviders(all: List<ProviderInfo>, hidden: Collection<String>, keep: String?, restricted: Boolean, claudeReady: Boolean): List<ProviderInfo> =
    shownProviders(all, hidden, keep, restricted).filter { restricted || claudeReady || it.id != "claude-code" }

  /** "N providers are hidden. Settings > Model and profiles has a switch for each." or null when none is hidden. */
  fun hiddenHint(all: List<ProviderInfo>, hidden: Collection<String>, keep: String?, restricted: Boolean): String? {
    val n = all.count { !(restricted && isLocal(it)) && it.id != keep && it.id in hidden }
    if (n == 0) return null
    return (if (n == 1) "1 provider is hidden." else "$n providers are hidden.") + " Settings > Model and profiles has a switch for each."
  }

  data class Group(val label: String, val items: List<ProviderInfo>)

  /** MODEL-09: Popular Providers, Cloud & Open-Weight, Local Models (This Computer), Custom Endpoint. Empty groups are left out. */
  fun groups(list: List<ProviderInfo>): List<Group> = listOf(
    Group("Popular Providers", list.filter { it.id in MAJOR_IDS }),
    Group("Cloud & Open-Weight", list.filter { it.id !in MAJOR_IDS && it.id != "custom" && !isLocal(it) }),
    Group("Local Models (This Computer)", list.filter { it.id !in MAJOR_IDS && it.id != "custom" && isLocal(it) }),
    Group("Custom Endpoint", list.filter { it.id == "custom" }),
  ).filter { it.items.isNotEmpty() }

  /** The name of a provider in the picker: a tick after the ones that have a key (or need none). */
  fun providerLabel(p: ProviderInfo): String = p.name.ifBlank { p.id } + if (p.hasKey || isLocal(p)) " ✓" else ""

  fun hasKeyOrNone(p: ProviderInfo): Boolean = p.hasKey || isLocal(p)

  /** MODEL-12: the address the person sees prefilled: the saved one, only when it is not the normal one. */
  fun savedAddress(p: ProviderInfo?, currentProvider: String): String =
    if (p != null && p.id == currentProvider && p.baseUrl.isNotBlank() && p.baseUrl != p.defaultUrl) p.baseUrl else ""

  /** MODEL-12: the hint under the address field. */
  fun addressHint(p: ProviderInfo?, restricted: Boolean): String = when {
    p == null -> ""
    p.id == "custom" -> "The address of any OpenAI-compatible server, for example http://192.168.1.20:8000/v1 or https://my-host.example/v1." +
      if (restricted) " Only public https addresses work here." else ""
    isLocal(p) || p.needsUrl -> "Where the server runs. The default is right unless you changed its port."
    else -> "Only change this for a proxy or a regional endpoint." + if (p.defaultUrl.isNotBlank()) " Normal address: ${p.defaultUrl}" else ""
  }

  /** MODEL-12/MODEL-25: "" when the address is fine, else the sentence to show. Restricted accounts: a public https address only. */
  fun validateAddress(address: String, needs: Boolean, restricted: Boolean): String {
    val u = address.trim()
    if (u.isEmpty()) return if (needs) "Enter the address of the endpoint, for example https://api.example.com/v1" else ""
    val uri = try { URI(u) } catch (_: Exception) { null }
    val scheme = uri?.scheme?.lowercase()
    val host = uri?.host?.lowercase()
    if (uri == null || scheme == null || host.isNullOrEmpty()) return "That does not look like a web address. It should start with https://"
    if (restricted) {
      if (scheme != "https") return "Only https addresses work here."
      if (isPrivateHost(host)) return "Use a public address. Local and private addresses are not allowed on this server."
    } else if (scheme != "http" && scheme != "https") return "The address must start with http:// or https://"
    return ""
  }

  private val PRIVATE = Regex("^(0\\.|10\\.|127\\.|169\\.254\\.|192\\.168\\.|172\\.(1[6-9]|2\\d|3[01])\\.)")

  fun isPrivateHost(host: String): Boolean =
    host == "localhost" || host.startsWith("[") || !host.contains('.') || PRIVATE.containsMatchIn(host) ||
      host.endsWith(".local") || host.endsWith(".internal") || host.endsWith(".lan")

  /** MODEL-25 validation of the add-endpoint sheet. */
  fun validateNewEndpoint(address: String, restricted: Boolean): String {
    val u = address.trim()
    if (u.isEmpty()) return "Type the address first."
    if (!u.startsWith("http://", true) && !u.startsWith("https://", true)) return "The address should start with http:// or https://"
    return validateAddress(u, true, restricted)
  }

  /** MODEL-14: the models matching [filter], the pinned ones first. */
  fun orderModels(models: List<String>, pins: Set<String>, filter: String): List<String> {
    val q = filter.trim().lowercase()
    val hit = models.filter { q.isEmpty() || it.lowercase().contains(q) }
    return hit.filter { it in pins } + hit.filter { it !in pins }
  }

  /** MODEL-14/17: typed text that is not in the list is offered as "Use “x” as the model name". */
  fun customOffer(models: List<String>, filter: String): String {
    val q = filter.trim()
    return if (q.isNotEmpty() && models.none { it.equals(q, ignoreCase = true) }) q else ""
  }

  /** The pinned models of the profile in use. */
  fun activePins(prof: ProfilesResp): List<String> = prof.profiles.firstOrNull { it.id == prof.active }?.pinned.orEmpty()

  /** MODEL-07: the label of an effort level. */
  fun effortLabel(level: String?): String = when (level?.lowercase()) {
    null, "" -> "Default"
    "low" -> "Low"
    "medium" -> "Medium"
    "high" -> "High"
    "xhigh" -> "Extra high"
    "max" -> "Max"
    else -> level.replaceFirstChar { it.uppercase() }
  }

  /** MODEL-37: "<provider> · <model or 'no model yet'>[ · N pinned]". */
  fun profileLine(p: Profile, providerName: String): String {
    val base = providerName.ifBlank { p.provider } + " · " + p.model.ifBlank { "no model yet" }
    return if (p.pinned.isNotEmpty()) base + " · ${p.pinned.size} pinned" else base
  }

  /** MODEL-18: the read-only picture setting of a profile, or null when the server did not send it. */
  fun visionLine(p: Profile): String? = when (p.vision) {
    true -> "Pictures: this model can see pictures (set on the desktop)"
    false -> "Pictures: this model cannot see pictures (set on the desktop)"
    null -> null
  }

  /** MODEL-29: the sub line of a provider switch. */
  fun providerSub(p: ProviderInfo, pins: Int, inUse: Boolean, claudeSignedIn: Boolean = true): String {
    val base = when {
      p.id == "claude-code" -> "Uses your Claude Code sign-in"
      isLocal(p) -> "Runs on this computer, no key"
      p.hasKey -> "Key saved"
      else -> "No key yet"
    }
    return base + (if (pins > 0) " · $pins pinned" else "") + (if (inUse) " · in use, always shown" else "")
  }

  /** The pins of every profile, for MODEL-28. */
  data class PinRow(val profileId: String, val profileName: String, val provider: String, val model: String)

  fun pinRows(prof: ProfilesResp, providerName: (String) -> String): List<PinRow> =
    prof.profiles.flatMap { p -> p.pinned.map { PinRow(p.id, p.name, providerName(p.provider), it) } }

  /** MODEL-01/06: the groups of the model menu: one per profile, with its pinned models (or its one model). */
  data class MenuGroup(val profileId: String, val header: String, val provider: String, val models: List<String>)

  fun menuGroups(prof: ProfilesResp, providerName: (String) -> String): List<MenuGroup> = prof.profiles.mapNotNull { p ->
    val ms = if (p.pinned.isNotEmpty()) p.pinned else if (p.model.isNotBlank()) listOf(p.model) else emptyList()
    if (ms.isEmpty()) null else MenuGroup(p.id, p.name + " · " + providerName(p.provider).ifBlank { p.provider }, p.provider, ms)
  }

  /** MODEL-02: true when the current model is not one of the models the menu lists, so "In use now" is shown. */
  fun needsInUseNow(groups: List<MenuGroup>, activeProfile: String, currentModel: String): Boolean =
    currentModel.isNotBlank() && groups.none { it.profileId == activeProfile && currentModel in it.models }

  fun cacheKey(provider: String, baseUrl: String) = provider + "|" + baseUrl.trim()

  /** Did the answer of `models` carry a list we can use (an empty list with no error means the provider returned none). */
  fun listMessage(r: ModelsResp): String = when {
    !r.error.isNullOrBlank() -> r.error
    r.models.isEmpty() -> "The provider returned no models. You can still type a model name above."
    else -> ""
  }
}
