package org.zohara.agent.settings

import androidx.compose.foundation.layout.height
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.PrimaryButton
import org.zohara.agent.core.design.SectionHeader
import org.zohara.agent.core.design.SettingsCard
import org.zohara.agent.core.net.ApiException
import org.zohara.agent.nav.LocalNav
import org.zohara.agent.nav.Routes

/** What the server stores for the person (SET-SYNC-01) and whether it syncs outward itself (SET-SYNC-03). */
data class SyncInfo(val chats: Int, val memory: Int, val personalities: Int, val newestMs: Long, val outward: OutwardSync?)
data class OutwardSync(val url: String, val lastOkMs: Long, val error: String)

object SyncLogic {
  private fun plural(n: Int, one: String, many: String) = "$n ${if (n == 1) one else many}"

  /** Counts the keys of each kind of a `POST /api/sync/manifest` answer and takes the newest time. */
  fun parse(manifest: JsonObject, status: JsonObject?): SyncInfo {
    fun kind(k: String): Map<String, Long> = (manifest[k] as? JsonObject)?.mapValues { (_, v) -> (v as? JsonPrimitive)?.let { it.longOrNull ?: it.doubleOrNull?.toLong() } ?: 0L }.orEmpty()
    val c = kind("chats"); val m = kind("memory"); val p = kind("personalities")
    val newest = listOf(c, m, p).flatMap { it.values }.maxOrNull() ?: 0L
    val out = if ((status?.get("enabled") as? JsonPrimitive)?.booleanOrNull == true) {
      OutwardSync(
        (status["url"] as? JsonPrimitive)?.contentOrNull.orEmpty(), (status["last_ok"] as? JsonPrimitive)?.longOrNull ?: 0, (status["error"] as? JsonPrimitive)?.contentOrNull.orEmpty(),
      )
    } else null
    return SyncInfo(c.size, m.size, p.size, newest, out)
  }

  fun storedText(i: SyncInfo, now: Long = System.currentTimeMillis()): String =
    "${plural(i.chats, "chat", "chats")}, ${plural(i.memory, "memory item", "memory items")} and ${plural(i.personalities, "personality", "personalities")}. Last change: ${formatAgo(i.newestMs, now)}."

  fun outwardText(o: OutwardSync, now: Long = System.currentTimeMillis()): String =
    "To ${o.url}. Last good sync: ${formatAgo(toMillis(o.lastOkMs), now)}." + if (o.error.isNotBlank()) " Problem: ${o.error}" else ""
}

/** Desktop sync: information only (D4). Parity rows: SET-SYNC-01..03. */
@Composable
fun SettingsSyncScreen() {
  val container = LocalAppContainer.current
  val nav = LocalNav.current
  val loader = rememberLoader {
    coroutineScope {
      val m = async { container.api.syncManifest() }
      val s = async { try { container.api.syncStatus() } catch (_: ApiException) { null } }
      SyncLogic.parse(m.await(), s.await())
    }
  }
  val load by loader.state.collectAsState()
  SettingsPage("Desktop sync") {
    LoadBox(load, { loader.load() }) { info -> SyncContent(info) { nav.settings(Routes.SETTINGS_ACCOUNT) } }
  }
}

@Composable
fun SyncContent(info: SyncInfo, onOpenAccount: () -> Unit) {
  SectionHeader("Desktop sync")
  SettingsCard {
    ActionRow("Stored for you", SyncLogic.storedText(info))
    ActionRow(
      "Turn it on in the desktop app",
      "Sign in on the desktop app to sync. Nothing is synced until you do. Use the same account you use here: Google, or the same e-mail and password. Then chats, memory and personalities are the same here and on the computer.",
    )
    ActionRow("Signed-in devices", "See which computers are signed in, and sign one out, on the Account page.", divider = false, actions = { PrimaryButton("Open Account", onOpenAccount) })
  }
  info.outward?.let {
    androidx.compose.foundation.layout.Spacer(androidx.compose.ui.Modifier.height(12.dp))
    SettingsCard { ActionRow("This server also syncs out", SyncLogic.outwardText(it), divider = false) }
  }
}
