package org.zohara.agent.settings

import android.content.Context
import android.content.Intent
import android.os.Build
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.SectionHeader
import org.zohara.agent.core.design.SettingsCard
import org.zohara.agent.core.design.SettingsRow
import org.zohara.agent.core.design.ThemeChoice
import org.zohara.agent.core.design.ZoIcon
import org.zohara.agent.core.design.ZoIcons
import org.zohara.agent.core.design.ZoSwitch
import org.zohara.agent.core.design.ZoTheme

/** A button that opens a menu of choices (the `select` of the web). [options] are value to label. */
@Composable
fun ChoiceButton(options: List<Pair<String, String>>, value: String, onChange: (String) -> Unit, description: String, modifier: Modifier = Modifier, width: androidx.compose.ui.unit.Dp = 150.dp) {
  val c = ZoTheme.colors
  var open by remember { mutableStateOf(false) }
  Box(modifier) {
    Row(
      Modifier.width(width).clip(ZoTheme.shapes.field).background(c.input).border(1.dp, c.border, ZoTheme.shapes.field)
        .clickable(role = Role.DropdownList) { open = true }.semantics { contentDescription = description }.padding(horizontal = 12.dp, vertical = 10.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      Text(options.firstOrNull { it.first == value }?.second ?: value, style = ZoTheme.type.body, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
      ZoIcon(ZoIcons.Down, tint = c.dim, size = 16.dp)
    }
    DropdownMenu(open, { open = false }, modifier = Modifier.background(c.popover).heightIn(max = 420.dp)) {
      for ((v, label) in options) {
        DropdownMenuItem(
          text = { Text(label, style = ZoTheme.type.body, color = c.text) },
          trailingIcon = if (v == value) ({ ZoIcon(ZoIcons.Check, tint = c.accent, size = 16.dp) }) else null,
          onClick = { open = false; onChange(v) },
        )
      }
    }
  }
}

val THEME_OPTIONS = listOf(ThemeChoice.System.key to "Follow the system", ThemeChoice.Dark.key to "Dark", ThemeChoice.Light.key to "Light")

/** What the phone can do for voice (the web disables the switches when the browser cannot). Unknown counts as available. */
data class VoiceSupport(val recognition: Boolean = true, val speech: Boolean = true) {
  companion object {
    fun detect(context: Context): VoiceSupport {
      val rec = try { SpeechRecognizer.isRecognitionAvailable(context) } catch (_: Exception) { false }
      val tts = try { context.packageManager.queryIntentServices(Intent(TextToSpeech.Engine.INTENT_ACTION_TTS_SERVICE), 0).isNotEmpty() } catch (_: Exception) { false }
      // from Android 11 an app only sees the services its manifest asks for; a "no" from there is not proof, so it counts as "available"
      val blind = Build.VERSION.SDK_INT >= 30
      return VoiceSupport(rec || blind, tts || blind)
    }
  }
}

/** Appearance and permissions, with Voice. Parity rows: SET-GEN-01..03, PERM-05, VOICE-03, 11, 13. */
@Composable
fun SettingsGeneralScreen() {
  val container = LocalAppContainer.current
  val account = rememberAccount()
  val restricted = account?.restricted ?: true
  val scope = rememberCoroutineScope()
  val doer = rememberDoer()
  val context = LocalContext.current
  val theme by container.prefs.theme.collectAsState(ThemeChoice.System)
  val lang by container.prefs.voiceLanguage.collectAsState("")
  val auto by container.prefs.voiceAutoSend.collectAsState(false)
  val read by container.prefs.readAloudAuto.collectAsState(false)
  val voice = remember { VoiceSupport.detect(context) }
  val loader = rememberLoader { container.api.mode() }
  val mode by loader.state.collectAsState()
  var shownMode by remember { mutableStateOf<String?>(null) }
  SettingsPage("Appearance and permissions") {
    GeneralContent(
      theme = theme.key, onTheme = { k -> scope.launch { container.prefs.setTheme(ThemeChoice.from(k)) } },
      restricted = restricted,
      mode = shownMode ?: (mode as? Load.Ready)?.value,
      onMode = { m -> val before = shownMode; shownMode = m; doer.run(onFail = { shownMode = before; doer.say(it) }) { container.api.mode(m.lowercase()) } },
      lang = lang, onLang = { scope.launch { container.prefs.setVoiceLanguage(it) } },
      autoSend = auto, onAutoSend = { scope.launch { container.prefs.setVoiceAutoSend(it) } },
      readAloud = read, onReadAloud = { scope.launch { container.prefs.setReadAloudAuto(it) } },
      voice = voice,
    )
  }
}

@Composable
fun GeneralContent(
  theme: String, onTheme: (String) -> Unit, restricted: Boolean, mode: String?, onMode: (String) -> Unit,
  lang: String, onLang: (String) -> Unit, autoSend: Boolean, onAutoSend: (Boolean) -> Unit, readAloud: Boolean, onReadAloud: (Boolean) -> Unit,
  voice: VoiceSupport,
) {
  SectionHeader("Appearance")
  SettingsCard {
    SettingsRow("Theme", subtitle = "Light or dark, or the same as the rest of the system.", divider = false, control = { ChoiceButton(THEME_OPTIONS, theme, onTheme, "Theme") })
  }
  SectionHeader("Permissions")
  SettingsCard {
    SettingsRow(
      "Permission mode", subtitle = permissionModeText(restricted), divider = false,
      control = {
        if (mode == null) Text("…", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
        else ChoiceButton(permissionModes(restricted).map { it to it }, mode, onMode, "Permission mode", width = 120.dp)
      },
    )
  }
  SectionHeader("Voice")
  SettingsCard {
    SettingsRow(
      "Voice language", subtitle = "The language you speak to the microphone, and the voice that reads answers. Hindi and Urdu can be mixed with English words.",
      control = { ChoiceButton(VOICE_LANGS, lang, onLang, "Voice language", width = 160.dp) },
    )
    SettingsRow(
      "Send automatically when I stop talking",
      subtitle = if (voice.recognition) "Off: you check the text and press Send." else "This phone has no speech recognition.",
      control = { ZoSwitch(autoSend && voice.recognition, onAutoSend, enabled = voice.recognition) },
    )
    SettingsRow(
      "Read answers aloud",
      subtitle = if (voice.speech) "Reads each new answer. Code is never read." else "This phone cannot read aloud.", divider = false,
      control = { ZoSwitch(readAloud && voice.speech, onReadAloud, enabled = voice.speech) },
    )
  }
  Text(
    "Your voice goes to the speech service of your phone (Google), not to Zohara Agent's server. Zohara Agent only receives the text you send.",
    style = ZoTheme.type.fieldHint, color = ZoTheme.colors.dimmer, modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
  )
  Spacer(Modifier.width(1.dp))
}
