package org.zohara.chat.media

import android.content.Context
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import java.util.Locale
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** What read-aloud needs from a speech synthesiser. The real one wraps `TextToSpeech`; tests use a fake. Callbacks may come from any thread. */
interface TtsEngine {
  interface Callbacks { fun onDone(utteranceId: String); fun onError(utteranceId: String) }
  /** [then] is called with true when the engine is ready */
  fun init(then: (Boolean) -> Unit)
  fun setLanguage(tag: String)
  fun setRate(rate: Float)
  fun speak(text: String, utteranceId: String, callbacks: Callbacks)
  fun stop()
  fun shutdown()
}

/**
 * Read answers aloud (VOICE-10, VOICE-12): plain words only (no code, links or markdown marks), chunks of at most 200 characters, one reading at a time,
 * Stop on the same button. [speaking] is the id of the message being read, or null.
 */
class ReadAloud(
  private val engine: TtsEngine,
  private val language: () -> String,
  private val rate: () -> Float,
  private val toast: (String) -> Unit,
) {
  private val _speaking = MutableStateFlow<String?>(null)
  val speaking: StateFlow<String?> = _speaking.asStateFlow()

  private var ready: Boolean? = null
  @Volatile private var run = 0
  private var pendingStart: (() -> Unit)? = null

  /** Reads [markdown] as message [id]; reading the same message again stops it (the button toggles). */
  fun toggle(id: String, markdown: String) {
    if (_speaking.value == id) { stop(); return }
    speak(id, markdown)
  }

  fun speak(id: String, markdown: String) {
    val plain = VoiceText.plain(markdown)
    if (plain.isEmpty()) { toast("Nothing to read aloud in this answer."); return }
    stop()
    val chunks = VoiceText.chunks(plain)
    val mine = ++run
    _speaking.value = id
    val go = {
      if (mine == run) {
        engine.setLanguage(VoiceLanguages.effective(language()))
        engine.setRate(rate())
        next(mine, chunks, 0)
      }
    }
    when (ready) {
      true -> go()
      false -> { _speaking.value = null; toast("This phone has no speech voice to read with.") }
      null -> {
        pendingStart = go
        engine.init { ok ->
          ready = ok
          val p = pendingStart; pendingStart = null
          if (ok) p?.invoke() else if (mine == run) { _speaking.value = null; toast("This phone has no speech voice to read with.") }
        }
      }
    }
  }

  private fun next(mine: Int, chunks: List<String>, i: Int) {
    if (mine != run) return
    if (i >= chunks.size) { _speaking.value = null; return }
    engine.speak(chunks[i], "zo-$mine-$i", object : TtsEngine.Callbacks {
      override fun onDone(utteranceId: String) { next(mine, chunks, i + 1) }
      override fun onError(utteranceId: String) { if (mine == run) { _speaking.value = null } }
    })
  }

  /** Reading stops on navigation, a new message, pause (VOICE-12). */
  fun stop() {
    run++
    if (_speaking.value != null) { _speaking.value = null; runCatching { engine.stop() } }
  }

  fun release() { stop(); engine.shutdown() }
}

class AndroidTtsEngine(private val context: Context) : TtsEngine {
  private var tts: TextToSpeech? = null
  private var callbacks: TtsEngine.Callbacks? = null

  override fun init(then: (Boolean) -> Unit) {
    tts = TextToSpeech(context.applicationContext) { status ->
      val t = tts
      if (status == TextToSpeech.SUCCESS && t != null) {
        t.setOnUtteranceProgressListener(object : UtteranceProgressListener() {
          override fun onStart(utteranceId: String?) {}
          override fun onDone(utteranceId: String?) { callbacks?.onDone(utteranceId.orEmpty()) }
          @Deprecated("Deprecated in Java") override fun onError(utteranceId: String?) { callbacks?.onError(utteranceId.orEmpty()) }
          override fun onError(utteranceId: String?, errorCode: Int) { callbacks?.onError(utteranceId.orEmpty()) }
        })
        then(true)
      } else then(false)
    }
  }

  override fun setLanguage(tag: String) {
    val t = tts ?: return
    val r = t.setLanguage(Locale.forLanguageTag(tag))
    if (r == TextToSpeech.LANG_MISSING_DATA || r == TextToSpeech.LANG_NOT_SUPPORTED) t.language = Locale.getDefault()
  }

  override fun setRate(rate: Float) { tts?.setSpeechRate(rate) }

  override fun speak(text: String, utteranceId: String, callbacks: TtsEngine.Callbacks) {
    this.callbacks = callbacks
    tts?.speak(text, TextToSpeech.QUEUE_FLUSH, null, utteranceId)
  }

  override fun stop() { tts?.stop() }
  override fun shutdown() { runCatching { tts?.shutdown() }; tts = null }
}
