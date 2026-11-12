package org.zohara.zoai.pages

import androidx.compose.runtime.Composable
import org.zohara.zoai.nav.StubScreen

/** Terminal command box (admin, cloud only; D1). Parity rows: TERM-*. */
@Composable
fun TerminalScreen() {
  StubScreen("Terminal", "TERM-*")
}
