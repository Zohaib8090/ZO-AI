package org.zohara.agent.core.net

import java.io.IOException
import java.io.InterruptedIOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import javax.net.ssl.SSLException
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

/** What went wrong, in terms the screens act on (docs API.md sections 1 and 9). */
enum class ErrorKind {
  /** no network or the server did not answer: banner "Offline. Retrying." */
  Offline,
  /** the answer took too long */
  Timeout,
  /** 401: refresh once, then "Signed out or not allowed." and the sign-in screen */
  Unauthorized,
  /** 403 terms_required: open the consent screen with [ApiException.termsVersion] (never a toast) */
  TermsRequired,
  /** 403 other: the account is blocked; sign out with the message */
  Blocked,
  /** 410: the account was deleted; sign out with the message */
  Deleted,
  /** 429 */
  RateLimited,
  /** 503 and 502: server busy or full */
  Busy,
  /** 413 and 408 on a send: restore the draft */
  TooLarge,
  /** `unknown request: <op>`: the server is older than the app */
  UnknownOp,
  /** 404 and other statuses */
  NotFound,
  /** HTTP 200 with ok:false (or a 4xx with a sentence): show [ApiException.message] as is */
  Rejected,
  /** anything else */
  Server,
  /** the person has not got the Firebase sign-in in this build */
  NotSignedIn,
}

class ApiException(
  val kind: ErrorKind,
  message: String,
  val status: Int = 0,
  val retryAfterSec: Int? = null,
  val termsVersion: String? = null,
  cause: Throwable? = null,
) : Exception(message, cause) {
  /** the text to show to the person (same as [message]) */
  val text: String get() = message ?: ""
}

object ApiErrors {
  const val SIGNED_OUT = "Signed out or not allowed."
  const val OFFLINE = "Offline. Retrying."
  const val OLD_SERVER = "This server is too old for that."
  const val TOO_BIG = "The request is too big."
  const val TOO_SLOW = "The request took too long to arrive."
  const val BUSY = "The server is busy. Try again in a moment."
  const val FULL = "Zohara Agent has as many people as it can take right now. Try again later."
  const val DELETED = "This account was deleted."
  const val BLOCKED = "This account is blocked."
  const val TIMEOUT = "Zohara Agent took too long to answer. Try again."
  const val GENERIC = "Something went wrong. Please try again."

  /** NET-04: "You are going a little fast. Please wait N second(s) and try again." */
  fun rateLimitMessage(retryAfterSec: Int?): String {
    val n = retryAfterSec?.takeIf { it > 0 }
    return if (n == null) "You are going a little fast. Please wait a moment and try again."
    else "You are going a little fast. Please wait $n second${if (n == 1) "" else "s"} and try again."
  }

  /** The error of a failed HTTP call: [status], the raw [body] (may be JSON or plain text) and the `Retry-After` header. */
  fun fromHttp(status: Int, body: String, retryAfterHeader: String?): ApiException {
    val obj = parseObject(body)
    val serverText = obj?.get("error")?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }
    val retry = retryAfterHeader?.trim()?.toIntOrNull() ?: obj?.get("retry_after")?.jsonPrimitive?.intOrNull
    return when {
      status == 401 -> ApiException(ErrorKind.Unauthorized, SIGNED_OUT, status)
      status == 403 && obj?.get("terms_required")?.jsonPrimitive?.booleanOrNull == true ->
        ApiException(ErrorKind.TermsRequired, serverText ?: "Accept the privacy notice and the terms first.", status, termsVersion = obj["version"]?.jsonPrimitive?.contentOrNull)
      status == 403 -> ApiException(ErrorKind.Blocked, serverText ?: BLOCKED, status)
      status == 410 -> ApiException(ErrorKind.Deleted, serverText ?: DELETED, status)
      status == 429 -> ApiException(ErrorKind.RateLimited, rateLimitMessage(retry), status, retryAfterSec = retry)
      status == 413 -> ApiException(ErrorKind.TooLarge, serverText ?: TOO_BIG, status)
      status == 408 -> ApiException(ErrorKind.TooLarge, serverText ?: TOO_SLOW, status)
      status == 503 -> ApiException(ErrorKind.Busy, serverText ?: BUSY, status, retryAfterSec = retry)
      status == 502 -> ApiException(ErrorKind.Busy, serverText ?: "Zohara Agent gave no answer.", status, retryAfterSec = retry)
      status == 404 -> ApiException(ErrorKind.NotFound, serverText ?: "Not found.", status)
      serverText != null -> ApiException(ErrorKind.Rejected, serverText, status)
      else -> ApiException(ErrorKind.Server, "Error $status", status)
    }
  }

  /** An op that reached the service but failed: HTTP 200 with `{"ok":false,"error":"..."}`. */
  fun fromOpFailure(error: String): ApiException {
    val e = error.trim()
    return when {
      e.startsWith("unknown request") -> ApiException(ErrorKind.UnknownOp, OLD_SERVER)
      e.isEmpty() -> ApiException(ErrorKind.Rejected, GENERIC)
      else -> ApiException(ErrorKind.Rejected, e)
    }
  }

  /** A network exception before or during a call. */
  fun fromThrowable(t: Throwable): ApiException = when (t) {
    is ApiException -> t
    is SocketTimeoutException, is InterruptedIOException -> if (t.message?.contains("timeout", ignoreCase = true) == true || t is SocketTimeoutException) ApiException(ErrorKind.Timeout, TIMEOUT, cause = t) else ApiException(ErrorKind.Offline, OFFLINE, cause = t)
    is UnknownHostException, is ConnectException, is SSLException -> ApiException(ErrorKind.Offline, OFFLINE, cause = t)
    is IOException -> ApiException(ErrorKind.Offline, OFFLINE, cause = t)
    else -> ApiException(ErrorKind.Server, t.message?.takeIf { it.isNotBlank() } ?: GENERIC, cause = t)
  }

  private fun parseObject(body: String): JsonObject? = try {
    val el = ZoJson.parseToJsonElement(body)
    el as? JsonObject
  } catch (_: Exception) {
    null
  }
}
