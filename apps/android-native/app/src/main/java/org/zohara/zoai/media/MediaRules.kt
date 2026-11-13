package org.zohara.zoai.media

import java.net.URL
import java.util.Locale

/** The limits of the web page (`A_LIM`, index.html:996). The server enforces its own copy (`zo_core::attach::parse`). */
object Lim {
  const val IMAGES = 4
  const val TEXTS = 6
  const val EACH = 200 * 1024
  const val TOTAL = 400 * 1024
  const val SIDE = 1600
  const val THUMB_SIDE = 160
  const val THUMB_CHARS = 20_000
  const val IMG_BYTES = 3 * 1024 * 1024
  const val IMG_TOTAL = 9 * 1024 * 1024
  const val IMG_FILE = 40L * 1024 * 1024
  const val PDF_FILE = 30L * 1024 * 1024
  const val ZIP_FILE = 25L * 1024 * 1024
  const val ZIP_ENTRIES = 200
  const val ZIP_TOTAL = 2 * 1024 * 1024
  const val ZIP_ENTRY = 100 * 1024
  const val LINKS = 5
  const val PDF_PAGES = 500
}

/** What a file is (ATT-06). */
enum class FileKind { Pdf, Zip, Image, Text }

/** A problem with one file, as a sentence for the person (a toast). */
class AttachProblem(message: String) : Exception(message)

object MediaRules {
  private val TEXT_EXT = ("txt md markdown rst json jsonl ndjson csv tsv log xml yaml yml toml ini cfg conf properties env html htm xhtml css scss sass less js mjs cjs jsx ts tsx vue svelte py pyi rs go java kt kts scala c h cc cpp cxx hpp hh cs rb php pl pm sh bash zsh fish ps1 bat cmd sql lua swift m mm r jl dart ex exs erl hs clj ml fs tex bib srt vtt diff patch gradle cmake make mk dockerfile gitignore editorconfig lock svg gql graphql proto tf hcl nix").split(" ").toSet()
  private val TEXT_NAMES = setOf("dockerfile", "makefile", "readme", "license", "changelog", "notice", "authors", "cmakelists.txt", "gemfile", "rakefile", "procfile")
  private val IMAGE_MIME = Regex("""^image/(png|jpe?g|webp|gif|bmp|avif|heic|heif)$""")
  private val IMAGE_EXT = Regex("""^(png|jpe?g|webp|gif|bmp|avif|heic|heif)$""")
  private val TEXT_APP_MIME = Regex("""^application/(json|xml|x-yaml|yaml|toml|x-sh|javascript|x-javascript|sql)$""")
  private val ZIP_MIME = Regex("""^application/(x-)?zip(-compressed)?$""")
  val ARCHIVE_NAME = Regex("""\.(zip|jar|apk|7z|rar|tar|gz|tgz|bz2|xz|docx|xlsx|pptx|odt|ods|epub|whl)$""", RegexOption.IGNORE_CASE)

  /** the lower case extension (1 to 12 letters or digits) of [name], or "" */
  fun ext(name: String?): String = Regex("""\.([A-Za-z0-9]{1,12})$""").find(name.orEmpty())?.groupValues?.get(1)?.lowercase(Locale.ROOT).orEmpty()

  fun isTextName(name: String): Boolean {
    val n = name.lowercase(Locale.ROOT)
    return ext(name) in TEXT_EXT || n in TEXT_NAMES
  }

  /** A file name for the chip and the payload: control characters and `/ \ " < > & backtick` become spaces, cut to 120 (`aName`). */
  fun cleanName(raw: String?): String {
    val s = (raw ?: "file").map { if (it.isISOControl() || it in "/\\\"<>&`") ' ' else it }.joinToString("").replace(Regex("""\s+"""), " ").trim().take(120)
    return s.ifEmpty { "file" }
  }

  /** The kind from the mime type and the name; null means it can not be attached (`aKind`). `application/octet-stream` counts as no type. */
  fun kindOf(mime: String?, name: String?): FileKind? {
    val t0 = mime.orEmpty().lowercase(Locale.ROOT).substringBefore(';').trim()
    val t = if (t0 == "application/octet-stream" || t0 == "binary/octet-stream") "" else t0
    val e = ext(name)
    val n = name.orEmpty().lowercase(Locale.ROOT)
    return when {
      t == "application/pdf" || e == "pdf" -> FileKind.Pdf
      ZIP_MIME.matches(t) || e == "zip" -> FileKind.Zip
      IMAGE_MIME.matches(t) || (t.isEmpty() && IMAGE_EXT.matches(e)) -> FileKind.Image
      t.startsWith("text/") || TEXT_APP_MIME.matches(t) || e in TEXT_EXT || n in TEXT_NAMES -> FileKind.Text
      else -> null
    }
  }

  /** The refusal sentence of `aRefusal` (ATT-09). */
  fun refusal(mime: String?, name: String?): String {
    val t = mime.orEmpty().lowercase(Locale.ROOT)
    val e = ext(name)
    val nm = cleanName(name)
    var what = if (e.isNotEmpty()) ".$e" else t.ifEmpty { "this kind of" }
    when {
      Regex("""^(mp4|mov|mkv|avi|webm)$""").matches(e) || t.startsWith("video/") -> what = "video"
      Regex("""^(mp3|wav|ogg|m4a|flac)$""").matches(e) || t.startsWith("audio/") -> what = "audio"
      Regex("""^(docx?|xlsx?|pptx?|odt|ods|odp)$""").matches(e) -> what = ".$e (Office)"
      Regex("""^(exe|msi|dll|so|bin|apk|dmg|iso|deb|rpm)$""").matches(e) -> what = "program"
      Regex("""^(7z|rar|tar|gz|tgz|bz2|xz)$""").matches(e) -> what = ".$e archive"
    }
    return "\"$nm\" cannot be attached: $what files are not supported. You can attach pictures, PDF, ZIP and text or code files."
  }

  fun heicRefusal(name: String?) = "\"${cleanName(name)}\" is a HEIC picture that this phone cannot read. Save it as JPEG or PNG and add it again."

  /** `aKB`: "12 B", "3 KB", "1.5 MB" */
  fun kb(n: Long): String = when {
    n >= 1_048_576 -> String.format(Locale.US, "%.1f MB", n / 1_048_576.0)
    n >= 1024 -> "${Math.round(n / 1024.0)} KB"
    else -> "$n B"
  }
  fun kb(n: Int) = kb(n.toLong())

  /** A thumbnail is only drawn when it is a small base64 data URI of a png, jpeg or webp (`aSafeThumb`, ATT-23, ATT-27). */
  fun safeThumb(u: String?): Boolean = u != null && u.length <= Lim.THUMB_CHARS && Regex("""^data:image/(png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$""").matches(u)

  // ---------------------------------------------------------------- text

  fun utf8Len(s: String): Int = s.toByteArray(Charsets.UTF_8).size

  /** replaces a half emoji by U+FFFD (`aWellFormed`) */
  fun wellFormed(s: String): String {
    val sb = StringBuilder(s.length)
    var i = 0
    while (i < s.length) {
      val c = s[i]
      when {
        Character.isHighSurrogate(c) && i + 1 < s.length && Character.isLowSurrogate(s[i + 1]) -> { sb.append(c).append(s[i + 1]); i += 2; continue }
        Character.isSurrogate(c) -> sb.append('�')
        else -> sb.append(c)
      }
      i++
    }
    return sb.toString()
  }

  /** control characters other than tab and line breaks become spaces: the server refuses them (`aClean`) */
  fun clean(s: String): String {
    val sb = StringBuilder(s.length)
    for (c in s) sb.append(if ((c.code <= 8) || c.code == 0x0B || c.code == 0x0C || c.code in 0x0E..0x1F || c.code in 0x7F..0x9F) ' ' else c)
    return wellFormed(sb.toString())
  }

  /** cuts [s] so that it is at most [max] UTF-8 bytes, never inside a character (`aCut`) */
  fun cut(s: String, max: Int): String {
    if (utf8Len(s) <= max) return s
    var bytes = 0
    var i = 0
    while (i < s.length) {
      val cp = s.codePointAt(i)
      val n = when { cp < 0x80 -> 1; cp < 0x800 -> 2; cp < 0x10000 -> 3; else -> 4 }
      if (bytes + n > max) break
      bytes += n
      i += Character.charCount(cp)
    }
    return wellFormed(s.substring(0, i))
  }

  // ---------------------------------------------------------------- links

  private val schemeRe = Regex("""^[a-z][a-z0-9+.-]*:""", RegexOption.IGNORE_CASE)
  private val bareHostRe = Regex("""^[^/?#:]+\.[a-z]{2,}([/?#:]|$)""", RegexOption.IGNORE_CASE)
  private val hostOkRe = Regex("""\.[a-z]{2,}$|^\d+\.\d+\.\d+\.\d+$|^\[""", RegexOption.IGNORE_CASE)

  /** A typed web address to its normal form, or null (ATT-18, `aUrl`): http(s) or a bare host (https assumed); no spaces, no user:pass, no single-label hosts. */
  fun normalizeUrl(raw: String?): String? {
    var s = raw.orEmpty().trim()
    if (s.isEmpty() || s.any { it.isWhitespace() } || s.length > 2000) return null
    if (!schemeRe.containsMatchIn(s)) {
      if (!bareHostRe.containsMatchIn(s)) return null
      s = "https://$s"
    }
    val u = try { URL(s) } catch (_: Exception) { return null }
    val proto = u.protocol.lowercase(Locale.ROOT)
    if (proto != "http" && proto != "https") return null
    if (u.userInfo != null) return null
    val host = u.host.lowercase(Locale.ROOT)
    if (host.isEmpty() || !hostOkRe.containsMatchIn(host)) return null
    val defPort = if (proto == "https") 443 else 80
    val port = if (u.port == -1 || u.port == defPort) "" else ":${u.port}"
    return "$proto://$host$port${u.path.ifEmpty { "/" }}${u.query?.let { "?$it" }.orEmpty()}${u.ref?.let { "#$it" }.orEmpty()}"
  }

  fun hostOf(url: String): String = try { URL(url).host.removePrefix("www.") } catch (_: Exception) { url }

  private val urlInText = Regex("""https?://[^\s<>"')\]]+""", RegexOption.IGNORE_CASE)
  private val trailing = Regex("""[.,;:!?]+$""")

  /** Web addresses typed or pasted into the message (ATT-20): normalised, not in [known], at most [Lim.LINKS]. */
  fun detectLinks(text: String, known: Set<String>): List<String> {
    val seen = HashSet(known)
    val out = ArrayList<String>()
    for (m in urlInText.findAll(text)) {
      val url = normalizeUrl(m.value.replace(trailing, "")) ?: continue
      if (!seen.add(url)) continue
      out.add(url)
      if (out.size >= Lim.LINKS) break
    }
    return out
  }

  /** Takes the chip's address out of the text (the chip's x on a detected link). */
  fun removeFromText(text: String, url: String): String =
    urlInText.replace(text) { m -> val bare = m.value.replace(trailing, ""); if (normalizeUrl(bare) == url) m.value.substring(bare.length) else m.value }.replace(Regex("""[ \t]{2,}"""), " ").trim()

  /** "Links to read (use your web tool):\n- url" (COMP-21) */
  fun linkText(links: List<String>): String = if (links.isEmpty()) "" else "Links to read (use your web tool):\n" + links.joinToString("\n") { "- $it" }
}
