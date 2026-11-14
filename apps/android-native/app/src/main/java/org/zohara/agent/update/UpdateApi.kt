package org.zohara.agent.update

import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import org.zohara.agent.core.net.ZoJson

/**
 * Asks the Zohara Agent server for the newest release (`GET /api/update/latest`). The call is public: no sign-in token and no cookie is sent
 * (this is a separate client with no interceptors), and the app never talks to GitHub itself, so GitHub never learns the phone's address.
 */
class UpdateApi(
  private val client: OkHttpClient = plainClient(),
  private val serverUrl: () -> String,
) {
  suspend fun latest(): UpdateResponse = withContext(Dispatchers.IO) {
    val request = Request.Builder().url(serverUrl().trimEnd('/') + "/api/update/latest").header("Accept", "application/json").build()
    try {
      client.newCall(request).execute().use { r ->
        if (!r.isSuccessful) throw UpdateCheckException("the server answered ${r.code}")
        val body = r.body?.source()?.also { it.request(MAX_BYTES + 1) }?.buffer?.snapshot()?.utf8().orEmpty()
        if (body.length > MAX_BYTES) throw UpdateCheckException("the server's answer is too big")
        val parsed = try { ZoJson.decodeFromString(UpdateResponse.serializer(), body) } catch (_: Exception) { throw UpdateCheckException("the server's answer is not understood") }
        if (!parsed.ok) throw UpdateCheckException("the server does not support update checks")
        parsed
      }
    } catch (e: IOException) {
      throw UpdateCheckException("the server could not be reached")
    }
  }

  companion object {
    private const val MAX_BYTES = 256L * 1024

    fun plainClient(): OkHttpClient = OkHttpClient.Builder()
      .connectTimeout(15, TimeUnit.SECONDS)
      .readTimeout(20, TimeUnit.SECONDS)
      .callTimeout(30, TimeUnit.SECONDS)
      .build()
  }
}

/** Runs a check and turns the answer into an [UpdateOutcome]. */
class UpdateChecker(private val api: UpdateApi, private val currentVersion: String) {
  /** Throws [UpdateCheckException] when the check could not be done at all. */
  suspend fun check(): UpdateOutcome = UpdateOutcomes.from(currentVersion, api.latest())
}
