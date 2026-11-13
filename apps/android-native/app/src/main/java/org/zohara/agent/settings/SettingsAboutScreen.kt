package org.zohara.agent.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import org.zohara.agent.BuildConfig
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.design.PillButton
import org.zohara.agent.core.design.SectionHeader
import org.zohara.agent.core.design.SecondaryButton
import org.zohara.agent.core.design.SettingsCard
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.util.openInBrowser

/** The open-source parts of the app and their licences (SET-ABOUT-01). */
object Licenses {
  data class Entry(val name: String, val license: String, val note: String = "")

  val all: List<Entry> = listOf(
    Entry("Kotlin and kotlinx (coroutines, serialization)", "Apache License 2.0"),
    Entry("AndroidX: Compose, Navigation, Lifecycle, DataStore, Activity, Core, Browser, Credentials", "Apache License 2.0"),
    Entry("Material components for Compose", "Apache License 2.0"),
    Entry("OkHttp and Okio", "Apache License 2.0"),
    Entry("Firebase Authentication and Cloud Messaging; Google Play services; Google Identity", "Apache License 2.0 and the Google APIs terms"),
    Entry("Inter (interface font)", "SIL Open Font License 1.1", "Copyright The Inter Project Authors"),
    Entry("Source Serif 4 (title font)", "SIL Open Font License 1.1", "Copyright Adobe"),
    Entry("Zohara Agent", "The Zohara Agent project licence", "The AI built into Zohara OS"),
  )
}

fun legalUrl(server: String, page: String) = server.trimEnd('/') + "/" + page

/** About. Parity rows: SET-ABOUT-01. */
@Composable
fun SettingsAboutScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val loader = rememberLoader { container.api.status() }
  val load by loader.state.collectAsState()
  var licenses by remember { mutableStateOf(false) }
  SettingsPage("About") {
    SectionHeader("About")
    SettingsCard {
      ActionRow(
        "Zohara Agent",
        "App version ${BuildConfig.VERSION_NAME} (build ${BuildConfig.VERSION_CODE})" +
          ((load as? Load.Ready)?.value?.version?.takeIf { it.isNotBlank() }?.let { " · server version $it" } ?: "") + " · the AI built into Zohara OS",
      )
      ActionRow(
        "Legal and licences", "What is stored, where, and who can see it. The open-source parts of this app.", divider = false,
        actions = {
          PillButton("Privacy", { openInBrowser(context, legalUrl(container.serverUrl, "privacy")) })
          PillButton("Terms", { openInBrowser(context, legalUrl(container.serverUrl, "terms")) })
          PillButton("Open-source licences", { licenses = true })
        },
      )
    }
  }
  if (licenses) LicensesSheet { licenses = false }
}

@Composable
fun LicensesSheet(onDismiss: () -> Unit) {
  ZoBottomSheet("Open-source licences", onDismiss, footer = { SecondaryButton("Close", onDismiss) }) {
    Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState())) {
      for (e in Licenses.all) {
        Text(e.name, style = ZoTheme.type.cardName, color = ZoTheme.colors.text, modifier = Modifier.padding(top = 10.dp))
        Text(e.license + if (e.note.isNotBlank()) " · ${e.note}" else "", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.dim)
      }
    }
  }
}
