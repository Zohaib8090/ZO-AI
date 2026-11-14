package org.zohara.agent.update

import kotlinx.serialization.Serializable

/** The files of a release (names, not addresses) and their checksums. */
@Serializable
data class UpdateAssets(
  val apk: String? = null,
  val pkg: String? = null,
  val aab: String? = null,
  val sha256s: Map<String, String> = emptyMap(),
)

/** The newest release as the server sends it (`GET /api/update/latest`, the `latest.json` of the GitHub release, cleaned by the server). */
@Serializable
data class LatestRelease(
  val version: String,
  val date: String = "",
  val notes: String = "",
  val minSupported: String? = null,
  val assets: UpdateAssets = UpdateAssets(),
  val releaseUrl: String = "",
)

/** `GET /api/update/latest`: public, cached for an hour on the server; `latest` is null when the server could not find out (then `error` says why). */
@Serializable
data class UpdateResponse(
  val ok: Boolean = false,
  val serverVersion: String? = null,
  val repo: String? = null,
  val latest: LatestRelease? = null,
  val checkedAt: Long? = null,
  val stale: Boolean = false,
  val error: String? = null,
)

/** What a check came to. */
sealed interface UpdateOutcome {
  val serverVersion: String?

  data class UpToDate(val current: String, val latest: LatestRelease, override val serverVersion: String?) : UpdateOutcome

  /** [required]: this version is older than the release's `min_supported`. */
  data class Available(val current: String, val latest: LatestRelease, val required: Boolean, override val serverVersion: String?) : UpdateOutcome

  /** The server could not find out (no release yet, GitHub unreachable). */
  data class Unknown(val reason: String?, override val serverVersion: String?) : UpdateOutcome
}

/** The check itself failed (no network, a server that has no update check, an answer that is not understood). */
class UpdateCheckException(message: String) : Exception(message)

object UpdateOutcomes {
  const val DEFAULT_RELEASES = "https://github.com/Zohaib8090/zohara-agent/releases"

  /** The release page to open: the server's address when it is a GitHub address, else the list of releases. */
  fun releasePage(latest: LatestRelease?): String = latest?.releaseUrl?.takeIf { it.startsWith("https://github.com/") && !it.any { c -> c.isWhitespace() } } ?: DEFAULT_RELEASES

  fun from(current: String, r: UpdateResponse): UpdateOutcome {
    val latest = r.latest?.takeIf { AppVersion.parse(it.version) != null }
      ?: return UpdateOutcome.Unknown(r.error?.take(200), r.serverVersion)
    val required = latest.minSupported?.let { AppVersion.isNewer(current, it) } ?: false
    return if (AppVersion.isNewer(current, latest.version)) UpdateOutcome.Available(current, latest, required, r.serverVersion)
    else UpdateOutcome.UpToDate(current, latest, r.serverVersion)
  }

  /** One line for the About page. */
  fun summary(o: UpdateOutcome): String = when (o) {
    is UpdateOutcome.Available -> "Version ${o.latest.version} is available (you have ${o.current})."
    is UpdateOutcome.UpToDate -> "You are up to date (${o.current})."
    is UpdateOutcome.Unknown -> "Could not check for updates" + (o.reason?.let { ": ${it.trimEnd('.')}" } ?: "") + "."
  }
}
