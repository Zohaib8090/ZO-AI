package org.zohara.agent.core.net

import java.util.concurrent.TimeUnit
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response

/** Gives the Firebase ID token. Blocking on purpose: OkHttp interceptors run on a network thread. */
fun interface TokenSource {
  /** The ID token, or null when nobody is signed in. [forceRefresh] asks Firebase for a new one (after a 401). */
  fun idToken(forceRefresh: Boolean): String?
}

/**
 * Adds `Authorization: Bearer <Firebase ID token>` to every call except the public pages (`/healthz`, `/privacy`, `/terms`).
 * On a 401 it asks for a fresh token ONCE and repeats the call; a second 401 is returned as is (NET-09, API.md section 9).
 * [localTestMode] (debug builds only) adds `X-ZO-Local: 1` for a test server started with ZO_SERVER_NO_AUTH=1.
 */
class AuthInterceptor(
  private val tokens: TokenSource,
  private val localTestMode: () -> Boolean = { false },
) : Interceptor {
  override fun intercept(chain: Interceptor.Chain): Response {
    val req = chain.request()
    val path = req.url.encodedPath
    if (path in PUBLIC_PATHS || req.header("Authorization") != null) return chain.proceed(req)
    fun signed(token: String?) = req.newBuilder().apply {
      if (token != null) header("Authorization", "Bearer $token")
      if (localTestMode()) header("X-ZO-Local", "1")
    }.build()

    val first = chain.proceed(signed(runCatching { tokens.idToken(false) }.getOrNull()))
    if (first.code != 401) return first
    val fresh = runCatching { tokens.idToken(true) }.getOrNull() ?: return first
    first.close()
    return chain.proceed(signed(fresh))
  }

  companion object {
    val PUBLIC_PATHS = setOf("/healthz", "/privacy", "/terms")
  }
}

object ZoHttp {
  const val CONNECT_SECONDS = 15L
  /** `/api/op` waits up to 180 s for a model, so the read timeout is a little more (NET-08). */
  const val OP_READ_SECONDS = 190L
  /** silence on a cloud `/api/send` stream: the cloud sends no `ping`, so 90 s without a byte is a lost connection */
  const val STREAM_SILENCE_SECONDS = 90L

  fun newClient(tokens: TokenSource, localTestMode: () -> Boolean = { false }): OkHttpClient =
    OkHttpClient.Builder()
      .connectTimeout(CONNECT_SECONDS, TimeUnit.SECONDS)
      .readTimeout(OP_READ_SECONDS, TimeUnit.SECONDS)
      .writeTimeout(60, TimeUnit.SECONDS)
      .addInterceptor(AuthInterceptor(tokens, localTestMode))
      .build()

  /** True for an address the app accepts as the server: `https://`, or plain `http://` only for localhost and the emulator host when [allowLocalCleartext]. */
  fun isAcceptableServer(url: String, allowLocalCleartext: Boolean): Boolean {
    val u = url.trim().lowercase()
    if (u.startsWith("https://")) return u.length > "https://".length && !u.contains(' ')
    if (allowLocalCleartext && u.startsWith("http://")) {
      val host = u.removePrefix("http://").substringBefore('/').substringBefore(':')
      return host == "localhost" || host == "127.0.0.1" || host == "10.0.2.2" || host == "[::1]"
    }
    return false
  }

  fun normalizeServer(url: String): String = url.trim().trimEnd('/')
}
