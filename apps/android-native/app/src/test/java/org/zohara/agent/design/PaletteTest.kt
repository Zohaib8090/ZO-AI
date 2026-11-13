package org.zohara.agent.core.design

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

/** The tokens are the desktop's (crates/zohara-agent/src/theme.rs dark/light pairs). */
class PaletteTest {
  @Test fun darkPalette() {
    assertEquals(Color(0xFF191919), DarkZoColors.bg)
    assertEquals(Color(0xFF202020), DarkZoColors.card)
    assertEquals(Color(0xFFECECEC), DarkZoColors.text)
    assertEquals(Color(0xFF5FD4D4), DarkZoColors.accent)
    assertEquals(Color(0xFF303030), DarkZoColors.composer)
    assertEquals(Color(0xFFE5736A), DarkZoColors.danger)
  }

  @Test fun lightPalette() {
    assertEquals(Color(0xFFFAF9F5), LightZoColors.bg)
    assertEquals(Color(0xFFFFFFFF), LightZoColors.card)
    assertEquals(Color(0xFF1F1E1D), LightZoColors.text)
    assertEquals(Color(0xFF1FA5A5), LightZoColors.accent)
    assertEquals(Color(0xFFFFFFFF), LightZoColors.composer)
  }

  @Test fun materialSchemeUsesTheTokens() {
    assertEquals(DarkZoColors.bg, materialScheme(DarkZoColors).background)
    assertEquals(LightZoColors.accent, materialScheme(LightZoColors).primary)
  }

  @Test fun themeChoiceParsing() {
    assertEquals(ThemeChoice.Dark, ThemeChoice.from("dark"))
    assertEquals(ThemeChoice.System, ThemeChoice.from("junk"))
    assertEquals(ThemeChoice.System, ThemeChoice.from(null))
  }
}
