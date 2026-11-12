package org.zohara.zoai.settings

import androidx.compose.runtime.Composable
import org.zohara.zoai.nav.StubScreen

/** Account: data, devices, export, delete. Parity rows: SET-ACC-*, AUTH-21, SRV-*. */
@Composable
fun SettingsAccountScreen() {
  StubScreen("SettingsAccount", "SET-ACC-*, AUTH-21, SRV-*")
}
