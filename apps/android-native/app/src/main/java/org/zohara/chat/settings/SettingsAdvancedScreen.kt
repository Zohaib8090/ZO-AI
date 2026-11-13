package org.zohara.chat.settings

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import org.zohara.chat.BuildConfig
import org.zohara.chat.LocalAppContainer
import org.zohara.chat.core.design.PillButton
import org.zohara.chat.core.design.PrimaryButton
import org.zohara.chat.core.design.SectionHeader
import org.zohara.chat.core.design.SettingsCard
import org.zohara.chat.core.design.ZoTextField
import org.zohara.chat.core.net.ZoHttp

object AdvancedLogic {
  const val BAD = "Use an https address, for example https://chat.example.com. Plain http is only for a server on this phone's own network while testing."

  /** AUTH-28: "" when [url] may be the server address. */
  fun validate(url: String, debug: Boolean): String = if (ZoHttp.isAcceptableServer(url.trim(), debug)) "" else BAD
}

/** Advanced: the server address (AUTH-28). Changing it signs the person out, as on the desktop. */
@Composable
fun SettingsAdvancedScreen() {
  val container = LocalAppContainer.current
  val scope = rememberCoroutineScope()
  val saved by container.prefs.serverUrl.collectAsState(BuildConfig.DEFAULT_SERVER_URL)
  var text by remember(saved) { mutableStateOf(saved) }
  var error by remember { mutableStateOf("") }
  SettingsPage("Advanced") {
    SectionHeader("Server address")
    SettingsCard {
      ActionRow(
        "Zohara Chat server", "The default is ${BuildConfig.DEFAULT_SERVER_URL}. Change it only to use your own server. You sign in again after a change.", divider = false,
        extra = {
          ZoTextField(text, { text = it; error = "" }, placeholder = BuildConfig.DEFAULT_SERVER_URL, hint = error.ifBlank { null }, isError = error.isNotBlank(), keyboardType = androidx.compose.ui.text.input.KeyboardType.Uri, modifier = Modifier.fillMaxWidth().padding(top = 10.dp))
        },
        actions = {
          PrimaryButton("Save and sign out", {
            val bad = AdvancedLogic.validate(text, BuildConfig.DEBUG)
            if (bad.isNotEmpty()) error = bad
            else scope.launch { container.prefs.setServerUrl(ZoHttp.normalizeServer(text)); container.session.signOut() }
          }, enabled = text.trim().trimEnd('/') != saved)
          PillButton("Use the default", { text = BuildConfig.DEFAULT_SERVER_URL; error = "" })
        },
      )
    }
  }
}
