package org.zohara.zoai.media

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/** What the dictation controller needs from a speech recogniser. The real one wraps Android's `SpeechRecognizer`; tests use a fake. All calls are on the main thread. */
interface SpeechEngine {
  interface Listener {
    fun onPartial(text: String)
    /** one utterance is over; [text] is what was heard (may be empty) */
    fun onFinal(text: String)
    fun onError(error: VoiceError)
  }
  val available: Boolean
  fun start(language: String, listener: Listener)
  /** ends listening and delivers what was heard so far */
  fun stop()
  /** ends listening and delivers nothing */
  fun cancel()
  fun destroy()
}

/**
 * Voice input (VOICE-01 to VOICE-09). The text box gets the words as they come (interim results appended to what was typed, no duplication);
 * Android's recogniser hears one utterance at a time, so unless auto-send is on it listens again after each one (the web's "continuous").
 * Cancel puts the text back as it was before listening; typing while listening stops it and the typed text wins.
 */
class VoiceController(
  private val engine: SpeechEngine,
  private val getText: () -> String,
  private val setText: (String) -> Unit,
  private val language: () -> String,
  private val autoSend: () -> Boolean,
  private val send: () -> Unit,
  private val toast: (String) -> Unit,
  /** runs [block] after [ms] milliseconds on the main thread; used so a stop always ends even if the recogniser says nothing more */
  private val later: ((Long, () -> Unit) -> Unit)? = null,
) {
  private val _listening = MutableStateFlow(false)
  val listening: StateFlow<Boolean> = _listening.asStateFlow()

  private val _announce = MutableSharedFlow<String>(extraBufferCapacity = 4)
  /** "Listening" and "Stopped" for screen readers (VOICE-09) */
  val announcements: SharedFlow<String> = _announce.asSharedFlow()

  private var original = ""
  private var base = ""
  private var accumulated = ""
  private var heard = false
  private var manual = false
  private var error: VoiceError? = null
  private var session = 0

  val available: Boolean get() = engine.available

  fun toggle() { if (_listening.value) stop() else start() }

  fun start() {
    if (_listening.value) return
    if (!engine.available) { toast(VoiceError.ServiceNotAllowed.message); return }
    original = getText()
    base = VoiceText.baseOf(original)
    accumulated = ""
    heard = false; manual = false; error = null
    session++
    _listening.value = true
    _announce.tryEmit("Listening")
    listen()
  }

  private fun listen() {
    val mine = session
    engine.start(language(), object : SpeechEngine.Listener {
      override fun onPartial(text: String) { if (mine == session && _listening.value) heardText(text) }
      override fun onFinal(text: String) { if (mine == session && _listening.value) finalText(text) }
      override fun onError(error: VoiceError) { if (mine == session && _listening.value) failed(error) }
    })
  }

  private fun heardText(s: String) {
    if (s.isNotBlank()) heard = true
    setText(VoiceText.fill(base, join(accumulated, s)))
  }

  private fun join(a: String, b: String) = if (a.isEmpty()) b else if (b.isEmpty()) a else "$a $b"

  private fun finalText(s: String) {
    if (s.isNotBlank()) { heard = true; accumulated = join(accumulated, s.trim()); setText(VoiceText.fill(base, accumulated)) }
    if (autoSend() || manual) { finish() } else listen() // continuous: listen again until the person stops it
  }

  private fun failed(e: VoiceError) {
    // "no speech" after something was heard is just the end of the dictation
    if (e == VoiceError.NoSpeech && (heard || manual)) { finish(); return }
    error = e
    toast(e.message)
    finish()
  }

  /** the end of a session: auto-send when something was heard, nothing went wrong and the person did not press stop */
  private fun finish() {
    if (!_listening.value) return
    val doSend = autoSend() && heard && error == null && !manual
    detach()
    _announce.tryEmit("Stopped")
    if (doSend && getText().isNotBlank()) send()
  }

  private fun detach() { session++; _listening.value = false }

  /** Press stop: ends the utterance now, the words heard so far stay (and are not sent by auto-send). */
  fun stop() {
    if (!_listening.value) return
    manual = true
    engine.stop()
    // the recogniser answers with a final result or an error; if it does not, the session ends anyway
    val mine = session
    later?.invoke(1500) { if (mine == session) finish() }
  }

  /** Stop and drop: nothing more is heard or sent; the text typed before stays as it is now (pause, a new view, busy). */
  fun cancel() {
    if (!_listening.value) return
    detach()
    runCatching { engine.cancel() }
    _announce.tryEmit("Stopped")
  }

  /** Esc or Back while listening: cancel and put the text back as it was before (VOICE-05). */
  fun cancelAndRestore() {
    if (!_listening.value) return
    cancel()
    setText(original)
  }

  /** The person typed: the typed text wins, listening ends (VOICE-05). */
  fun onTyped() { if (_listening.value) cancel() }

  fun release() { cancel(); engine.destroy() }
}

/** The real recogniser. Partial results on; the language tag is the voice setting. */
class AndroidSpeechEngine(private val context: Context) : SpeechEngine {
  private var rec: SpeechRecognizer? = null
  private var listener: SpeechEngine.Listener? = null

  override val available: Boolean get() = SpeechRecognizer.isRecognitionAvailable(context)

  override fun start(language: String, listener: SpeechEngine.Listener) {
    this.listener = listener
    val r = rec ?: SpeechRecognizer.createSpeechRecognizer(context).also { rec = it }
    r.setRecognitionListener(object : RecognitionListener {
      override fun onReadyForSpeech(params: Bundle?) {}
      override fun onBeginningOfSpeech() {}
      override fun onRmsChanged(rmsdB: Float) {}
      override fun onBufferReceived(buffer: ByteArray?) {}
      override fun onEndOfSpeech() {}
      override fun onEvent(eventType: Int, params: Bundle?) {}
      override fun onError(error: Int) { this@AndroidSpeechEngine.listener?.onError(VoiceError.fromAndroid(error)) }
      override fun onPartialResults(partialResults: Bundle?) { first(partialResults)?.let { this@AndroidSpeechEngine.listener?.onPartial(it) } }
      override fun onResults(results: Bundle?) { this@AndroidSpeechEngine.listener?.onFinal(first(results).orEmpty()) }
    })
    val i = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
      putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
      putExtra(RecognizerIntent.EXTRA_LANGUAGE, language)
      putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
      putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 1)
      putExtra(RecognizerIntent.EXTRA_CALLING_PACKAGE, context.packageName)
    }
    r.startListening(i)
  }

  private fun first(b: Bundle?): String? = b?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull()

  override fun stop() { runCatching { rec?.stopListening() } }
  override fun cancel() { runCatching { rec?.cancel() } }
  override fun destroy() { runCatching { rec?.destroy() }; rec = null }
}
