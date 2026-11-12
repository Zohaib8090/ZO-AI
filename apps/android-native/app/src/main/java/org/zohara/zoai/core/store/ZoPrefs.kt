package org.zohara.zoai.core.store

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.floatPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import org.zohara.zoai.core.design.ThemeChoice

/** Where a message runs (Run on, LINK module): the cloud server, or a linked computer. */
data class RunOn(val device: String?) {
  val isCloud: Boolean get() = device == null
  companion object { val Cloud = RunOn(null) }
}

/**
 * The phone's own settings, in DataStore preferences (core.store). Nothing here is secret: no keys, no tokens.
 * Defaults: theme follows the system, server https://zo-ai-server.onrender.com, both notification channels on, details in notifications off.
 */
class ZoPrefs(private val store: DataStore<Preferences>, private val defaultServer: String) {
  private object K {
    val theme = stringPreferencesKey("theme")
    val language = stringPreferencesKey("language")
    val voiceLanguage = stringPreferencesKey("voice_language")
    val voiceAutoSend = booleanPreferencesKey("voice_auto_send")
    val readAloudAuto = booleanPreferencesKey("read_aloud_auto")
    val speechRate = floatPreferencesKey("speech_rate")
    val notifAnswers = booleanPreferencesKey("notif_answers")
    val notifNeedsYou = booleanPreferencesKey("notif_needs_you")
    val notifDetails = booleanPreferencesKey("notif_details")
    val serverUrl = stringPreferencesKey("server_url")
    val runOn = stringPreferencesKey("run_on")
    val pushId = stringPreferencesKey("push_registration_id")
    val consentVersion = stringPreferencesKey("consent_version")
  }

  val all: Flow<Preferences> get() = store.data

  // ---- look ----
  val theme: Flow<ThemeChoice> = store.data.map { ThemeChoice.from(it[K.theme]) }
  suspend fun setTheme(c: ThemeChoice) { store.edit { it[K.theme] = c.key } }

  /** app language: `""` follows the system */
  val language: Flow<String> = store.data.map { it[K.language].orEmpty() }
  suspend fun setLanguage(tag: String) { store.edit { it[K.language] = tag } }

  // ---- voice ----
  val voiceLanguage: Flow<String> = store.data.map { it[K.voiceLanguage].orEmpty() }
  suspend fun setVoiceLanguage(tag: String) { store.edit { it[K.voiceLanguage] = tag } }
  val voiceAutoSend: Flow<Boolean> = store.data.map { it[K.voiceAutoSend] ?: false }
  suspend fun setVoiceAutoSend(on: Boolean) { store.edit { it[K.voiceAutoSend] = on } }
  val readAloudAuto: Flow<Boolean> = store.data.map { it[K.readAloudAuto] ?: false }
  suspend fun setReadAloudAuto(on: Boolean) { store.edit { it[K.readAloudAuto] = on } }
  val speechRate: Flow<Float> = store.data.map { it[K.speechRate] ?: 1f }
  suspend fun setSpeechRate(rate: Float) { store.edit { it[K.speechRate] = rate.coerceIn(0.5f, 2f) } }

  // ---- notifications (NOTIF module; D10) ----
  val notifAnswers: Flow<Boolean> = store.data.map { it[K.notifAnswers] ?: true }
  suspend fun setNotifAnswers(on: Boolean) { store.edit { it[K.notifAnswers] = on } }
  val notifNeedsYou: Flow<Boolean> = store.data.map { it[K.notifNeedsYou] ?: true }
  suspend fun setNotifNeedsYou(on: Boolean) { store.edit { it[K.notifNeedsYou] = on } }
  /** "Show details in notifications": off by default (INTERACTION.md section 3) */
  val notifDetails: Flow<Boolean> = store.data.map { it[K.notifDetails] ?: false }
  suspend fun setNotifDetails(on: Boolean) { store.edit { it[K.notifDetails] = on } }

  // ---- server ----
  val serverUrl: Flow<String> = store.data.map { it[K.serverUrl]?.takeIf { s -> s.isNotBlank() } ?: defaultServer }
  suspend fun setServerUrl(url: String) { store.edit { if (url.isBlank() || url == defaultServer) it.remove(K.serverUrl) else it[K.serverUrl] = url } }
  suspend fun serverUrlNow(): String = serverUrl.first()

  // ---- run on ----
  val runOn: Flow<RunOn> = store.data.map { p -> RunOn(p[K.runOn]?.takeIf { it.isNotBlank() }) }
  suspend fun setRunOn(r: RunOn) { store.edit { if (r.device == null) it.remove(K.runOn) else it[K.runOn] = r.device } }

  // ---- push and consent bookkeeping ----
  val pushRegistrationId: Flow<String> = store.data.map { it[K.pushId].orEmpty() }
  suspend fun setPushRegistrationId(id: String) { store.edit { if (id.isEmpty()) it.remove(K.pushId) else it[K.pushId] = id } }
  /** the consent version this phone last sent to the server; only a hint, the record lives on the server */
  val consentVersion: Flow<String> = store.data.map { it[K.consentVersion].orEmpty() }
  suspend fun setConsentVersion(v: String) { store.edit { it[K.consentVersion] = v } }

  suspend fun clearAccountData() {
    store.edit { it.remove(K.runOn); it.remove(K.pushId); it.remove(K.consentVersion) }
  }
}
