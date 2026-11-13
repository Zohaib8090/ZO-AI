package org.zohara.zoai.core.design

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/**
 * Every colour token of the desktop and web look (crates/zo-ai/src/style.css, theme.rs, web index.html `:root`).
 * Names follow the web's CSS variables. The values are copied exactly; the light set is what theme.rs swaps in.
 */
@Immutable
data class ZoColors(
  val isDark: Boolean,
  val bg: Color,
  val card: Color,
  val card2: Color,
  val raise: Color,
  val composer: Color,
  val user: Color,
  val input: Color,
  val popover: Color,
  val sheet: Color,
  val text: Color,
  val text2: Color,
  val text3: Color,
  val dim: Color,
  val dimmer: Color,
  val icon: Color,
  val accent: Color,
  /** accent as the colour of link text: the light accent is too pale for small text (A11Y-03) */
  val link: Color,
  val accent2: Color,
  val onAccent: Color,
  val check: Color,
  val danger: Color,
  /** --line: borders, 7 percent */
  val line: Color,
  /** --line4 */
  val line4: Color,
  /** --line8 */
  val line8: Color,
  /** --line2: strong border, 12 percent */
  val line12: Color,
  val hover: Color,
  val active: Color,
  val scrim: Color,
  val btn: Color,
  val btnFg: Color,
  val btnHover: Color,
  val code: Color,
  val codeFg: Color,
  val shadow: Color,
  /** solid borders of the desktop stylesheet (#2B2B2B dark) */
  val border: Color,
  /** focused border (#3A3A3A dark) */
  val borderFocus: Color,
  // the same in both themes
  val ok: Color,
  val okChip: Color,
  val mic: Color,
  val warn: Color,
  val blue: Color,
  val gold: Color,
)

private val Same = object {
  val ok = Color(0xFF22C55E)
  val okChip = Color(0xFF3FB950)
  val mic = Color(0xFFE5484D)
  val warn = Color(0xFFEAB308)
  val blue = Color(0xFF3B82F6)
  val gold = Color(0xFFF2B84B)
}

val DarkZoColors = ZoColors(
  isDark = true,
  bg = Color(0xFF191919), card = Color(0xFF202020), card2 = Color(0xFF242424), raise = Color(0xFF2E2E2E),
  composer = Color(0xFF303030), user = Color(0xFF2B2B2B), input = Color(0xFF171717), popover = Color(0xFF262626), sheet = Color(0xFF202020),
  text = Color(0xFFECECEC), text2 = Color(0xFFD0D0D0), text3 = Color(0xFFBDBDBD), dim = Color(0xFF8C8C8C), dimmer = Color(0xFF858585), icon = Color(0xFF9A9A9A),
  accent = Color(0xFF5FD4D4), link = Color(0xFF5FD4D4), accent2 = Color(0xFF7EE0E0), onAccent = Color(0xFF0B0E14), check = Color(0xFF4C9AFF), danger = Color(0xFFE5736A),
  line = Color.White.copy(alpha = 0.07f), line4 = Color.White.copy(alpha = 0.04f), line8 = Color.White.copy(alpha = 0.08f), line12 = Color.White.copy(alpha = 0.12f),
  hover = Color.White.copy(alpha = 0.06f), active = Color.White.copy(alpha = 0.09f), scrim = Color.Black.copy(alpha = 0.55f),
  btn = Color(0xFFECECEC), btnFg = Color(0xFF191919), btnHover = Color(0xFFFFFFFF),
  code = Color(0xFF202020), codeFg = Color(0xFFE4E4E4), shadow = Color.Black.copy(alpha = 0.28f),
  border = Color(0xFF2B2B2B), borderFocus = Color(0xFF3A3A3A),
  ok = Same.ok, okChip = Same.okChip, mic = Same.mic, warn = Same.warn, blue = Same.blue, gold = Same.gold,
)

val LightZoColors = ZoColors(
  isDark = false,
  bg = Color(0xFFFAF9F5), card = Color(0xFFFFFFFF), card2 = Color(0xFFF3F1EA), raise = Color(0xFFEDEAE0),
  composer = Color(0xFFFFFFFF), user = Color(0xFFE9E6DC), input = Color(0xFFF1EFE8), popover = Color(0xFFFFFFFF), sheet = Color(0xFFFFFFFF),
  text = Color(0xFF1F1E1D), text2 = Color(0xFF3D3C39), text3 = Color(0xFF55534E), dim = Color(0xFF6F6C65), dimmer = Color(0xFF737068), icon = Color(0xFF6B6963),
  accent = Color(0xFF1FA5A5), link = Color(0xFF137C7C), accent2 = Color(0xFF2BB8B8), onAccent = Color(0xFFFFFFFF), check = Color(0xFF4C9AFF), danger = Color(0xFFE5736A),
  line = Color.Black.copy(alpha = 0.07f), line4 = Color.Black.copy(alpha = 0.04f), line8 = Color.Black.copy(alpha = 0.08f), line12 = Color.Black.copy(alpha = 0.12f),
  hover = Color.Black.copy(alpha = 0.06f), active = Color.Black.copy(alpha = 0.09f), scrim = Color.Black.copy(alpha = 0.35f),
  btn = Color(0xFF1F1E1D), btnFg = Color(0xFFFAF9F5), btnHover = Color(0xFF000000),
  code = Color(0xFFFFFFFF), codeFg = Color(0xFF2A2926), shadow = Color.Black.copy(alpha = 0.12f),
  border = Color(0xFFE9E6DC), borderFocus = Color(0xFFD3CFC1),
  ok = Same.ok, okChip = Same.okChip, mic = Same.mic, warn = Same.warn, blue = Same.blue, gold = Same.gold,
)

val LocalZoColors = staticCompositionLocalOf { DarkZoColors }

/** Where the colours of the current theme are read: `ZoTheme.colors.accent`. */
object ZoTheme {
  val colors: ZoColors
    @Composable @ReadOnlyComposable get() = LocalZoColors.current
  val shapes: ZoShapes
    @Composable @ReadOnlyComposable get() = LocalZoShapes.current
  val type: ZoType
    @Composable @ReadOnlyComposable get() = LocalZoType.current
}

/** The theme choice of Settings > General: follow the system, dark, or light. */
enum class ThemeChoice(val key: String) {
  System("system"), Dark("dark"), Light("light");

  companion object {
    fun from(key: String?): ThemeChoice = entries.firstOrNull { it.key == key } ?: System
  }
}

@Composable
fun isDarkFor(choice: ThemeChoice): Boolean = when (choice) {
  ThemeChoice.System -> isSystemInDarkTheme()
  ThemeChoice.Dark -> true
  ThemeChoice.Light -> false
}

fun materialScheme(c: ZoColors): ColorScheme {
  val base = if (c.isDark) darkColorScheme() else lightColorScheme()
  return base.copy(
    primary = c.accent, onPrimary = c.onAccent, primaryContainer = c.card2, onPrimaryContainer = c.text,
    secondary = c.accent2, onSecondary = c.onAccent, secondaryContainer = c.raise, onSecondaryContainer = c.text,
    tertiary = c.gold, onTertiary = c.onAccent,
    background = c.bg, onBackground = c.text,
    surface = c.bg, onSurface = c.text, surfaceVariant = c.card2, onSurfaceVariant = c.text3,
    surfaceTint = c.accent, inverseSurface = c.btn, inverseOnSurface = c.btnFg, inversePrimary = c.accent2,
    surfaceContainerLowest = c.input, surfaceContainerLow = c.bg, surfaceContainer = c.card, surfaceContainerHigh = c.card2, surfaceContainerHighest = c.raise,
    surfaceBright = c.card2, surfaceDim = c.bg,
    error = c.danger, onError = c.onAccent, errorContainer = c.card2, onErrorContainer = c.danger,
    outline = c.line12, outlineVariant = c.line, scrim = Color.Black,
  )
}

/** The whole look. Wrap every screen (and every screenshot test) in this. */
@Composable
fun ZoAiTheme(dark: Boolean = true, content: @Composable () -> Unit) {
  val colors = if (dark) DarkZoColors else LightZoColors
  val type = rememberZoType()
  CompositionLocalProvider(
    LocalZoColors provides colors,
    LocalZoShapes provides DefaultZoShapes,
    LocalZoType provides type,
  ) {
    MaterialTheme(colorScheme = materialScheme(colors), typography = type.material, content = content)
  }
}
