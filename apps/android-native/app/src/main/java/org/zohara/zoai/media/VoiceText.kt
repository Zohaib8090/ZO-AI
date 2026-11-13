package org.zohara.zoai.media

/** The languages of Settings > Appearance and permissions (VOICE-03): "" follows the phone. */
object VoiceLanguages {
  val all: List<Pair<String, String>> = listOf(
    "" to "Phone default", "en-US" to "English (US)", "en-GB" to "English (UK)", "ur-PK" to "Urdu", "hi-IN" to "Hindi", "ar-SA" to "Arabic",
    "bn-BD" to "Bengali", "es-ES" to "Spanish", "fr-FR" to "French", "de-DE" to "German", "it-IT" to "Italian", "pt-BR" to "Portuguese (Brazil)",
    "tr-TR" to "Turkish", "ru-RU" to "Russian", "id-ID" to "Indonesian", "zh-CN" to "Chinese (Mandarin)", "ja-JP" to "Japanese", "ko-KR" to "Korean",
  )

  /** the language tag to use: the choice, or the phone's */
  fun effective(choice: String, deviceTag: String = java.util.Locale.getDefault().toLanguageTag()): String = choice.ifBlank { deviceTag.ifBlank { "en-US" } }
}

/** Why listening failed, in the web's vocabulary (VOICE-07). */
enum class VoiceError(val message: String) {
  NotAllowed("The microphone is blocked. Allow it for ZO-AI in the Android app settings."),
  ServiceNotAllowed("This phone does not let apps use speech recognition. Turn on voice typing in the Android settings and try again."),
  AudioCapture("No microphone found."),
  Network("The speech service could not be reached. Check your connection and try again."),
  NoSpeech("I did not hear anything. Try again closer to the microphone."),
  LanguageNotSupported("This recogniser cannot recognise that language. Pick another one in Settings > Appearance and permissions."),
  Busy("The speech service is busy. Try again in a moment."),
  Other("Voice input stopped. Try again."),
  ;

  companion object {
    /** the codes of `android.speech.SpeechRecognizer.ERROR_*` */
    fun fromAndroid(code: Int): VoiceError = when (code) {
      1, 2, 4, 11 -> Network          // network timeout, network, server, server disconnected
      3 -> AudioCapture               // audio
      5 -> Other                      // client
      6, 7 -> NoSpeech                // speech timeout, no match
      8, 10 -> Busy                   // recognizer busy, too many requests
      9 -> NotAllowed                 // insufficient permissions
      12, 13 -> LanguageNotSupported  // language not supported, language unavailable
      else -> Other
    }
  }
}

/** Text helpers for dictation and for reading answers aloud. Pure: ported from the web page's `voiceFill` and `voicePlain`. */
object VoiceText {
  /** The composer text while listening: [base] (what was typed before) plus [spoken], whitespace collapsed, no double space (VOICE-02, `voiceFill`). */
  fun fill(base: String, spoken: String): String {
    var s = spoken.replace(Regex("""\s+"""), " ")
    if (base.isEmpty() || base.last().isWhitespace()) s = s.trimStart()
    return base + s
  }

  /** the text a session starts from: what is typed, with one space after it (`vBase`) */
  fun baseOf(typed: String): String = if (typed.isNotEmpty() && !typed.last().isWhitespace()) "$typed " else typed

  private fun rx(p: String, vararg o: RegexOption) = Regex(p, o.toSet())
  private val md = listOf(
    rx("""</?[a-zA-Z][^>\n]*>""") to { _: MatchResult -> " " },
    rx("""!\[([^\]]*)]\([^)]*\)""") to { m: MatchResult -> m.groupValues[1] },
    rx("""\[([^\]]+)]\([^)]*\)""") to { m: MatchResult -> m.groupValues[1] },
    rx("""https?://\S+""") to { _: MatchResult -> "" },
    rx("""`+""") to { _: MatchResult -> "" },
    rx("""^\s{0,3}#{1,6}\s+(.*?)[ \t]*$""", RegexOption.MULTILINE) to { m: MatchResult -> m.groupValues[1] + (if (Regex("""[.!?:]$""").containsMatchIn(m.groupValues[1])) "" else ".") },
    rx("""^\s*>\s?""", RegexOption.MULTILINE) to { _: MatchResult -> "" },
    rx("""^\s*([-*+]|\d+[.)])\s+(.*?)[ \t]*$""", RegexOption.MULTILINE) to { m: MatchResult -> m.groupValues[2] + (if (Regex("""[.!?:;,]$""").containsMatchIn(m.groupValues[2])) "" else ".") },
    rx("""^\s*([-*_]\s*){3,}$""", RegexOption.MULTILINE) to { _: MatchResult -> "" },
    rx("""^\s*\|?[\s:|-]+\|[\s:|-]*$""", RegexOption.MULTILINE) to { _: MatchResult -> "" },
    rx("""^[ \t]*\|""", RegexOption.MULTILINE) to { _: MatchResult -> "" },
    rx("""\|[ \t]*$""", RegexOption.MULTILINE) to { _: MatchResult -> "" },
    rx("""[ \t]*\|[ \t]*""") to { _: MatchResult -> ", " },
    rx("""(\*\*|\*|~~)(?=\S)([^\n]*?\S)\1""") to { m: MatchResult -> m.groupValues[2] },
    rx("""(^|[^\w])(__|_)(?=\S)([^\n]*?\S)\2(?![\w])""") to { m: MatchResult -> m.groupValues[1] + m.groupValues[3] },
    rx("""\*{1,3}|~~""") to { _: MatchResult -> "" },
    rx("""[ \t]+""") to { _: MatchResult -> " " },
    rx("""\n{2,}""") to { _: MatchResult -> ". " },
    rx("""\n""") to { _: MatchResult -> " " },
    rx("""\.\s*\.""") to { _: MatchResult -> "." },
    rx("""\s+""") to { _: MatchResult -> " " },
  )

  /** An answer as plain words: no code blocks, links, URLs or markdown marks; headings and list items get a full stop (VOICE-10). */
  fun plain(markdown: String): String {
    var s = markdown.split("```").filterIndexed { i, _ -> i % 2 == 0 }.joinToString("\n")
    for ((re, f) in md) s = re.replace(s, f)
    return s.trim()
  }

  private val sentence = Regex("""[^.!?؟۔।]+[.!?؟۔।]*\s*""")
  private val piece = Regex("""[\s\S]{1,200}(?:\s+|$)|[\s\S]{1,200}""")

  /** Pieces of at most 200 characters, cut at sentence ends (`voiceSpeak`). */
  fun chunks(plain: String): List<String> {
    val parts = sentence.findAll(plain).map { it.value }.toList().ifEmpty { listOf(plain) }
    val out = ArrayList<String>()
    var cur = ""
    for (p in parts.flatMap { s -> piece.findAll(s).map { it.value }.toList().ifEmpty { listOf(s) } }) {
      if ((cur + p).length > 200 && cur.isNotEmpty()) { out.add(cur); cur = "" }
      cur += p
    }
    if (cur.isNotEmpty()) out.add(cur)
    return out.map { it.trim() }.filter { it.isNotEmpty() }
  }
}
