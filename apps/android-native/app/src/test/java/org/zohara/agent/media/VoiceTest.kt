package org.zohara.agent.media

import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** VOICE-01 to VOICE-10, VOICE-12: dictation and read aloud, against fake engines. */
@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class VoiceTest {
  // ---------------------------------------------------------------- text helpers

  @Test fun spokenWordsAreAddedAfterWhatWasTyped() {
    assertEquals("hello world", VoiceText.fill("hello ", "  world"))
    assertEquals("hello world", VoiceText.fill(VoiceText.baseOf("hello"), "world"))
    assertEquals("a b c", VoiceText.fill("", "  a   b\n c"))
    assertEquals("line\nthen", VoiceText.fill("line\n", " then"))
  }

  @Test fun markdownBecomesPlainWords() {
    assertEquals("Title. Some bold and italic text with a link.", VoiceText.plain("# Title\nSome **bold** and *italic* text with a [link](https://x.example.com)."))
    assertEquals("Before. After.", VoiceText.plain("Before.\n\n```rust\nfn main() { println!(\"hi\"); }\n```\n\nAfter."))
    assertEquals("one. two.", VoiceText.plain("- one\n- two"))
    assertEquals("see here", VoiceText.plain("see https://example.com/a/b here").replace("  ", " "))
    assertEquals("", VoiceText.plain("```\ncode only\n```"))
    assertEquals("quoted text", VoiceText.plain("> quoted text"))
    assertEquals("a, b", VoiceText.plain("a | b"))
    assertEquals("snake_case stays", VoiceText.plain("snake_case stays"))
  }

  @Test fun chunksAreAtMost200CharactersAndKeepAllWords() {
    val long = (1..120).joinToString(" ") { "word$it" } + "."
    val chunks = VoiceText.chunks(long)
    assertTrue(chunks.size > 2)
    assertTrue(chunks.all { it.length <= 200 })
    assertEquals(long.split(" "), chunks.joinToString(" ").split(" "))
    assertEquals(listOf("One. Two. Three."), VoiceText.chunks("One. Two. Three."))
  }

  @Test fun androidErrorsMapToTheWebSentences() {
    assertEquals(VoiceError.NotAllowed, VoiceError.fromAndroid(9))
    assertEquals(VoiceError.Network, VoiceError.fromAndroid(2))
    assertEquals(VoiceError.NoSpeech, VoiceError.fromAndroid(7))
    assertEquals(VoiceError.NoSpeech, VoiceError.fromAndroid(6))
    assertEquals(VoiceError.AudioCapture, VoiceError.fromAndroid(3))
    assertEquals(VoiceError.LanguageNotSupported, VoiceError.fromAndroid(12))
    assertEquals(VoiceError.Busy, VoiceError.fromAndroid(8))
    assertEquals("The microphone is blocked. Allow it for Zohara Agent in the Android app settings.", VoiceError.NotAllowed.message)
    assertEquals("No microphone found.", VoiceError.AudioCapture.message)
    assertEquals("The speech service could not be reached. Check your connection and try again.", VoiceError.Network.message)
    assertEquals("I did not hear anything. Try again closer to the microphone.", VoiceError.NoSpeech.message)
    assertTrue(VoiceError.LanguageNotSupported.message.endsWith("Pick another one in Settings > Appearance and permissions."))
  }

  @Test fun languagesListHasTheWebChoices() {
    assertEquals("", VoiceLanguages.all.first().first)
    assertEquals(18, VoiceLanguages.all.size)
    assertTrue(VoiceLanguages.all.any { it == "hi-IN" to "Hindi" } && VoiceLanguages.all.any { it == "ur-PK" to "Urdu" })
    assertEquals("ur-PK", VoiceLanguages.effective("ur-PK"))
    assertEquals("de-DE", VoiceLanguages.effective("", "de-DE"))
  }

  // ---------------------------------------------------------------- dictation

  private class FakeEngine(override var available: Boolean = true) : SpeechEngine {
    var listener: SpeechEngine.Listener? = null
    var starts = 0; var stops = 0; var cancels = 0; var lang = ""
    override fun start(language: String, listener: SpeechEngine.Listener) { this.listener = listener; starts++; lang = language }
    override fun stop() { stops++ }
    override fun cancel() { cancels++ }
    var destroyed = false
    override fun destroy() { destroyed = true }
  }

  private class Rig(auto: Boolean = false, typed: String = "") {
    val engine = FakeEngine()
    var text = typed
    var sent = 0
    val toasts = ArrayList<String>()
    var autoSend = auto
    val voice = VoiceController(engine, { text }, { text = it }, { "en-US" }, { autoSend }, { sent++ }, { toasts.add(it) })
  }

  @Test fun interimWordsFillTheBoxWithoutDuplicates() {
    val r = Rig(typed = "hello")
    r.voice.start()
    assertTrue(r.voice.listening.value)
    assertEquals("en-US", r.engine.lang)
    r.engine.listener!!.onPartial("how")
    assertEquals("hello how", r.text)
    r.engine.listener!!.onPartial("how are")
    assertEquals("hello how are", r.text)
    r.engine.listener!!.onPartial("how are you")
    assertEquals("hello how are you", r.text)
    r.engine.listener!!.onFinal("how are you")
    assertEquals("hello how are you", r.text)
  }

  @Test fun continuousListeningStartsAgainAndKeepsEarlierPhrases() {
    val r = Rig()
    r.voice.start()
    r.engine.listener!!.onFinal("first phrase")
    assertEquals(2, r.engine.starts) // listens again
    assertTrue(r.voice.listening.value)
    r.engine.listener!!.onPartial("second")
    assertEquals("first phrase second", r.text)
    r.engine.listener!!.onFinal("second one")
    assertEquals("first phrase second one", r.text)
    r.voice.stop()
    assertEquals(1, r.engine.stops)
  }

  @Test fun leavingTheViewEndsListeningAndFreesTheRecogniser() { // VOICE-16, VOICE-06
    val r = Rig(typed = "keep")
    r.voice.start()
    r.engine.listener!!.onPartial("hello")
    r.voice.release()
    assertFalse(r.voice.listening.value)
    assertEquals(1, r.engine.cancels)
    assertTrue(r.engine.destroyed)
    r.engine.listener!!.onPartial("late words") // nothing more is taken after that
    assertEquals("keep hello", r.text)
  }

  @Test fun pressingStopWaitsForTheLastWordsThenEndsWithoutSending() {
    val r = Rig(auto = true)
    r.voice.start()
    r.engine.listener!!.onPartial("almost")
    r.voice.stop()
    r.engine.listener!!.onFinal("almost done")
    assertFalse(r.voice.listening.value)
    assertEquals("almost done", r.text)
    assertEquals("stopped by hand: auto-send does not fire", 0, r.sent)
  }

  @Test fun autoSendSendsWhenTheUtteranceEndsAfterSomethingWasHeard() {
    val r = Rig(auto = true)
    r.voice.start()
    r.engine.listener!!.onFinal("send this")
    assertEquals(1, r.engine.starts) // not continuous
    assertFalse(r.voice.listening.value)
    assertEquals("send this", r.text)
    assertEquals(1, r.sent)
  }

  @Test fun autoSendNeedsSomethingHeardAndNoError() {
    val a = Rig(auto = true)
    a.voice.start(); a.engine.listener!!.onFinal("")
    assertEquals(0, a.sent)
    val b = Rig(auto = true)
    b.voice.start(); b.engine.listener!!.onPartial("hi"); b.engine.listener!!.onError(VoiceError.Network)
    assertEquals(0, b.sent)
    assertEquals(listOf(VoiceError.Network.message), b.toasts)
  }

  @Test fun noSpeechAfterWordsIsJustTheEnd() {
    val r = Rig()
    r.voice.start()
    r.engine.listener!!.onPartial("hello")
    r.engine.listener!!.onError(VoiceError.NoSpeech)
    assertFalse(r.voice.listening.value)
    assertTrue(r.toasts.isEmpty())
    val q = Rig()
    q.voice.start(); q.engine.listener!!.onError(VoiceError.NoSpeech)
    assertEquals(listOf(VoiceError.NoSpeech.message), q.toasts)
  }

  @Test fun cancelPutsTheTextBackAsItWas() {
    val r = Rig(typed = "keep me")
    r.voice.start()
    r.engine.listener!!.onPartial("noise")
    assertEquals("keep me noise", r.text)
    r.voice.cancelAndRestore()
    assertEquals("keep me", r.text)
    assertFalse(r.voice.listening.value)
    assertEquals(1, r.engine.cancels)
    r.engine.listener!!.onFinal("late result is ignored")
    assertEquals("keep me", r.text)
  }

  @Test fun typingWhileListeningStopsItAndTheTypedTextWins() {
    val r = Rig(typed = "a")
    r.voice.start()
    r.engine.listener!!.onPartial("spoken")
    r.text = "a typed"
    r.voice.onTyped()
    assertFalse(r.voice.listening.value)
    assertEquals("a typed", r.text)
    r.engine.listener!!.onPartial("more")
    assertEquals("a typed", r.text)
  }

  @Test fun anUnavailableRecogniserSaysSo() {
    val r = Rig()
    r.engine.available = false
    r.voice.start()
    assertFalse(r.voice.listening.value)
    assertEquals(listOf(VoiceError.ServiceNotAllowed.message), r.toasts)
  }

  @Test fun announcementsForScreenReaders() = runTest(UnconfinedTestDispatcher()) {
    val r = Rig()
    val seen = ArrayList<String>()
    val job = launch(UnconfinedTestDispatcher(testScheduler)) { r.voice.announcements.collect { seen.add(it) } }
    r.voice.start()
    r.voice.cancel()
    assertEquals(listOf("Listening", "Stopped"), seen)
    job.cancel()
  }

  // ---------------------------------------------------------------- read aloud

  private class FakeTts(val ready: Boolean = true) : TtsEngine {
    val spoken = ArrayList<String>(); var langs = ArrayList<String>(); var stops = 0
    var cb: TtsEngine.Callbacks? = null
    var initThen: ((Boolean) -> Unit)? = null
    override fun init(then: (Boolean) -> Unit) { then(ready) }
    override fun setLanguage(tag: String) { langs.add(tag) }
    override fun setRate(rate: Float) {}
    override fun speak(text: String, utteranceId: String, callbacks: TtsEngine.Callbacks) { spoken.add(text); cb = callbacks; lastId = utteranceId }
    var lastId = ""
    override fun stop() { stops++ }
    override fun shutdown() {}
    fun finishOne() { cb!!.onDone(lastId) }
  }

  private fun reader(tts: FakeTts, toasts: MutableList<String> = ArrayList()) = ReadAloud(tts, { "ur-PK" }, { 1f }, { toasts.add(it) })

  @Test fun readsPlainWordsChunkByChunkThenStops() {
    val tts = FakeTts()
    val r = reader(tts)
    r.speak("a1", "# Heading\n" + (1..80).joinToString(" ") { "word$it" } + ".\n\n```x```\n")
    assertEquals("a1", r.speaking.value)
    assertEquals(listOf("ur-PK"), tts.langs)
    assertTrue(tts.spoken[0].startsWith("Heading."))
    val total = tts.spoken.size
    while (r.speaking.value != null) { tts.finishOne(); if (tts.spoken.size > 50) break }
    assertTrue(tts.spoken.size > total - 1 && tts.spoken.size >= 2)
    assertTrue(tts.spoken.none { it.contains("```") })
    assertEquals(null, r.speaking.value)
  }

  @Test fun theButtonToggles() {
    val tts = FakeTts()
    val r = reader(tts)
    r.toggle("m1", "Hello there.")
    assertEquals("m1", r.speaking.value)
    r.toggle("m1", "Hello there.")
    assertEquals(null, r.speaking.value)
    assertEquals(1, tts.stops)
  }

  @Test fun onlyOneReadingAtATime() {
    val tts = FakeTts()
    val r = reader(tts)
    r.speak("m1", "First answer.")
    r.speak("m2", "Second answer.")
    assertEquals("m2", r.speaking.value)
    tts.cb!!.onDone("old") // the first one's callback arrives late and is ignored for m2's run
    assertEquals(listOf("First answer.", "Second answer."), tts.spoken.take(2))
  }

  @Test fun codeOnlyAnswersSayNothingToRead() {
    val toasts = ArrayList<String>()
    val r = reader(FakeTts(), toasts)
    r.speak("m", "```\nonly code\n```")
    assertEquals(null, r.speaking.value)
    assertEquals(listOf("Nothing to read aloud in this answer."), toasts)
  }

  @Test fun noVoiceOnThePhoneSaysSo() {
    val toasts = ArrayList<String>()
    val r = reader(FakeTts(ready = false), toasts)
    r.speak("m", "Hello.")
    assertEquals(null, r.speaking.value)
    assertEquals(listOf("This phone has no speech voice to read with."), toasts)
  }

  @Test fun stopCancelsTheSpeech() {
    val tts = FakeTts()
    val r = reader(tts)
    r.speak("m", "Hello. World.")
    r.stop()
    assertEquals(null, r.speaking.value)
    assertEquals(1, tts.stops)
  }
}
