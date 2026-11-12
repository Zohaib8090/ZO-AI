package org.zohara.zoai.core.util

import android.util.Log

/** Logging that never prints secrets: bearer tokens, API keys (sk-...), long base64 and private keys are replaced before anything is written. */
object ZoLog {
  @Volatile var enabled: Boolean = true

  private val rules = listOf(
    Regex("""(?i)bearer\s+[A-Za-z0-9._~+/=-]{8,}""") to "Bearer ***",
    Regex("""(?i)(authorization\s*[:=]\s*)\S+""") to "$1***",
    Regex("""\bsk-[A-Za-z0-9_-]{8,}""") to "sk-***",
    Regex("""\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{5,}""") to "***jwt***",
    Regex("""-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(-----END [A-Z ]*PRIVATE KEY-----|$)""") to "***private key***",
    Regex("""(?i)("?(?:key|token|password|data_b64|api_key)"?\s*[:=]\s*"?)[^"\s,}]{6,}""") to "$1***",
    Regex("""[A-Za-z0-9+/]{200,}={0,2}""") to "***data***",
  )

  /** [s] with every secret replaced (public for the unit test). */
  fun redact(s: String): String = rules.fold(s) { acc, (re, to) -> re.replace(acc, to) }

  fun d(tag: String, msg: String) { if (enabled) runCatching { Log.d("ZO-$tag", redact(msg)) } }
  fun w(tag: String, msg: String, t: Throwable? = null) { if (enabled) runCatching { Log.w("ZO-$tag", redact(msg), t) } }
  fun e(tag: String, msg: String, t: Throwable? = null) { if (enabled) runCatching { Log.e("ZO-$tag", redact(msg), t) } }
}
