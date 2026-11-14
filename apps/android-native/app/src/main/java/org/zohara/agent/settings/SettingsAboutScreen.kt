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
import org.zohara.agent.core.design.SettingsRow
import org.zohara.agent.core.design.ZoSwitch
import org.zohara.agent.core.design.ZoBottomSheet
import org.zohara.agent.core.design.ZoTheme
import org.zohara.agent.core.util.openInBrowser
import org.zohara.agent.update.CheckState
import org.zohara.agent.update.InstallSource
import org.zohara.agent.update.UpdateOutcome
import org.zohara.agent.update.UpdateOutcomes

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

/** What the About page shows (the screen fills it from the container; tests and screenshots build it directly). */
data class AboutUi(
  val versionName: String,
  val versionCode: Int,
  val serverVersion: String?,
  val check: CheckState,
  val auto: Boolean,
  val fromPlay: Boolean,
)

class AboutActions(
  val onCheck: () -> Unit = {},
  val onAuto: (Boolean) -> Unit = {},
  val onDownload: () -> Unit = {},
  val onPrivacy: () -> Unit = {},
  val onTerms: () -> Unit = {},
  val onLicenses: () -> Unit = {},
)

/** About. Parity rows: SET-ABOUT-01; the update check (docs/UPDATES.md): version line, "Check for updates", the result, Download, the automatic switch. */
@Composable
fun SettingsAboutScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val loader = rememberLoader { container.api.status() }
  val load by loader.state.collectAsState()
  val check by container.updates.state.collectAsState()
  val auto by container.prefs.updateAuto.collectAsState(initial = true)
  var licenses by remember { mutableStateOf(false) }
  val fromPlay = remember { InstallSource.fromPlay(context) }
  val serverVersion = ((check as? CheckState.Done)?.outcome?.serverVersion) ?: (load as? Load.Ready)?.value?.version?.takeIf { it.isNotBlank() }
  SettingsPage("About") {
    AboutContent(
      AboutUi(BuildConfig.VERSION_NAME, BuildConfig.VERSION_CODE, serverVersion, check, auto, fromPlay),
      AboutActions(
        onCheck = container.updates::check,
        onAuto = container.updates::setAuto,
        onDownload = {
          val latest = ((check as? CheckState.Done)?.outcome as? UpdateOutcome.Available)?.latest
          InstallSource.open(context, InstallSource.targetFor(InstallSource.installerOf(context), context.packageName, latest))
        },
        onPrivacy = { openInBrowser(context, legalUrl(container.serverUrl, "privacy")) },
        onTerms = { openInBrowser(context, legalUrl(container.serverUrl, "terms")) },
        onLicenses = { licenses = true },
      ),
    )
  }
  if (licenses) LicensesSheet { licenses = false }
}

@Composable
fun AboutContent(ui: AboutUi, a: AboutActions) {
  SectionHeader("About")
  SettingsCard {
    ActionRow(
      "Zohara Agent",
      "App version ${ui.versionName} (build ${ui.versionCode})" + (ui.serverVersion?.let { " · server version $it" } ?: "") + " · the AI built into Zohara OS",
    )
    ActionRow(
      "Legal and licences", "What is stored, where, and who can see it. The open-source parts of this app.", divider = false,
      actions = {
        PillButton("Privacy", a.onPrivacy)
        PillButton("Terms", a.onTerms)
        PillButton("Open-source licences", a.onLicenses)
      },
    )
  }
  SectionHeader("Updates")
  SettingsCard {
    val checking = ui.check is CheckState.Checking
    ActionRow(
      "Check for updates",
      when (val c = ui.check) {
        CheckState.Idle -> "Looks for a newer version of Zohara Agent."
        CheckState.Checking -> "Checking..."
        is CheckState.Failed -> c.message
        is CheckState.Done -> UpdateOutcomes.summary(c.outcome)
      },
      extra = {
        val available = (ui.check as? CheckState.Done)?.outcome as? UpdateOutcome.Available
        if (available != null) {
          if (available.required) Text("This version is no longer supported. Please update.", style = ZoTheme.type.cardMeta, color = ZoTheme.colors.danger, modifier = Modifier.padding(top = 6.dp))
          if (available.latest.notes.isNotBlank()) Text(available.latest.notes, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.text, modifier = Modifier.padding(top = 8.dp))
        }
      },
      actions = {
        PillButton("Check for updates", a.onCheck, enabled = !checking)
        if (((ui.check as? CheckState.Done)?.outcome) is UpdateOutcome.Available) {
          PillButton(if (ui.fromPlay) "Open Google Play" else "Download", a.onDownload, accent = true)
        }
      },
    )
    SettingsRow(
      "Check for updates automatically",
      subtitle = "Looks about every 12 hours when the phone has a network and the battery is not low, and tells you once per new version. Nothing is downloaded or installed without you.",
      divider = false,
      control = { ZoSwitch(ui.auto, a.onAuto, label = "Check for updates automatically") },
    )
  }
  Note("The check asks the Zohara Agent server for the newest release. Nothing about you or this phone is sent, and GitHub never sees your address.")
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
