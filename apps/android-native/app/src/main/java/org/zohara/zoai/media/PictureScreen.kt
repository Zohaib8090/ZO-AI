package org.zohara.zoai.media

import androidx.compose.runtime.Composable
import org.zohara.zoai.nav.StubScreen

/** Full-screen preview of an attached picture. Parity rows: ATT-*. */
@Composable
fun PictureScreen(index: Int) {
  StubScreen("Picture", "ATT-*")
}
