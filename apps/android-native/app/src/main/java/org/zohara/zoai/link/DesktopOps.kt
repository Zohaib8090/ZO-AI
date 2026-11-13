package org.zohara.zoai.link

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.decodeFromJsonElement
import kotlinx.serialization.json.put
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ModelsResp
import org.zohara.zoai.core.net.ProfileUseResp
import org.zohara.zoai.core.net.ProfilesResp
import org.zohara.zoai.core.net.ProvidersResp
import org.zohara.zoai.core.net.SelectResp
import org.zohara.zoai.core.net.SendEventParser
import org.zohara.zoai.core.net.StatusInfo
import org.zohara.zoai.core.net.ZoJson

/** The relay `desktop_op` of the API client: a settings question for the linked computer. */
fun interface DesktopRelay { suspend fun call(device: String, inner: JsonObject): JsonObject }

/** An error of a computer op with the sentence to show. */
class DesktopOpException(message: String, val offline: Boolean = false) : Exception(message)

/**
 * The model and mode controls on My computer (MODEL-32, LINK-07): `desktop_op` with `status providers profiles profile_use models select mode` only; Bypass is refused;
 * API keys are never set here. An offline computer gives the OFFLINE message and calls [onOffline] (the screens refresh the link state).
 */
class DesktopOps(private val relay: DesktopRelay, private val device: () -> String?, private val onOffline: () -> Unit = {}) {
  companion object {
    val ALLOWED = setOf("status", "providers", "profiles", "profile_use", "models", "select", "mode")

    /** The sentence for an error that came back from `desktop_op` (503 desktop_offline / desktop_timeout / desktop_lost / desktop_error, 400 not_allowed, 429 limit). */
    fun explain(e: ApiException): DesktopOpException {
      val t = e.text
      return when {
        t.contains("desktop_offline") || t.contains("not reachable", true) || t.contains("offline", true) -> DesktopOpException(LinkText.OFFLINE, offline = true)
        t.contains("desktop_timeout") -> DesktopOpException(SendEventParser.friendlyCode("desktop_timeout"))
        t.contains("desktop_lost") -> DesktopOpException(SendEventParser.friendlyCode("desktop_lost"))
        t.contains("not_allowed") -> DesktopOpException("That is not allowed from the web.")
        t.contains("desktop_error") -> DesktopOpException("Your computer could not do that.")
        else -> DesktopOpException(t)
      }
    }
  }

  private suspend fun call(op: String, fields: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}): JsonObject {
    require(op in ALLOWED) { "not a computer op: $op" }
    val id = device() ?: throw DesktopOpException("Run on My computer first.")
    return try {
      relay.call(id, buildJsonObject { put("op", op); fields() })
    } catch (e: ApiException) {
      val x = explain(e)
      if (x.offline) onOffline()
      throw x
    }
  }

  private inline fun <reified T> JsonObject.decode(): T = ZoJson.decodeFromJsonElement<T>(this as JsonElement)

  suspend fun status(): StatusInfo = call("status").decode()
  suspend fun providers(): ProvidersResp = call("providers").decode()
  suspend fun profiles(): ProfilesResp = call("profiles").decode()
  suspend fun profileUse(id: String): ProfileUseResp = call("profile_use") { put("id", id) }.decode()
  suspend fun models(provider: String): ModelsResp = call("models") { put("provider", provider) }.decode()
  suspend fun select(provider: String, model: String): SelectResp = call("select") { put("provider", provider); put("model", model) }.decode()

  /** the computer's mode ("Manual", "Auto"); with [set] it changes it. Bypass is refused (LINK-11). */
  suspend fun mode(set: String? = null): String {
    if (set != null && set.equals("bypass", ignoreCase = true)) throw DesktopOpException(LinkText.MODE_NOTE)
    val r = call("mode") { if (set != null) put("set", set.lowercase()) }
    return (r["mode"] as? kotlinx.serialization.json.JsonPrimitive)?.content.orEmpty()
  }
}
