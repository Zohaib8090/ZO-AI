package org.zohara.agent.core.design

import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import org.zohara.agent.R

/** Inter for the interface, Source Serif 4 for titles (both OFL, bundled as variable fonts in res/font). */
@OptIn(ExperimentalTextApi::class)
private fun variable(res: Int, weight: Int) = Font(
  resId = res,
  weight = FontWeight(weight),
  variationSettings = FontVariation.Settings(FontVariation.weight(weight)),
)

val InterFamily = FontFamily(
  variable(R.font.inter, 400), variable(R.font.inter, 500), variable(R.font.inter, 600), variable(R.font.inter, 700),
)
val SerifFamily = FontFamily(variable(R.font.source_serif_4, 400), variable(R.font.source_serif_4, 600))
val MonoFamily = FontFamily.Monospace

/** The type scale of the desktop stylesheet (THEME-08): body 14, messages and composer 15, hero 32 serif, page title 30 serif, sheet title 19. */
@Immutable
class ZoType(
  val body: TextStyle,
  val message: TextStyle,
  val hero: TextStyle,
  val pageTitle: TextStyle,
  val wordmark: TextStyle,
  val sheetTitle: TextStyle,
  val cardName: TextStyle,
  val cardMeta: TextStyle,
  val fieldLabel: TextStyle,
  val fieldHint: TextStyle,
  val popTitle: TextStyle,
  val popSub: TextStyle,
  val sideItem: TextStyle,
  val navItem: TextStyle,
  val button: TextStyle,
  val code: TextStyle,
  val sectionHeader: TextStyle,
  val material: Typography,
)

private fun st(size: Float, weight: Int = 400, family: FontFamily = InterFamily, lh: Float = 1.45f, ls: TextUnit = 0.em) =
  TextStyle(fontFamily = family, fontSize = size.sp, fontWeight = FontWeight(weight), lineHeight = (size * lh).sp, letterSpacing = ls)

fun buildZoType(): ZoType {
  val body = st(14f)
  val message = st(15f, lh = 1.55f)
  val hero = st(32f, family = SerifFamily, lh = 1.2f, ls = (-0.01).em)
  val page = st(30f, family = SerifFamily, lh = 1.2f, ls = (-0.01).em)
  val wordmark = st(21f, family = SerifFamily, lh = 1.2f, ls = (-0.01).em)
  val sheet = st(19f, family = SerifFamily, lh = 1.25f, ls = (-0.01).em)
  val material = Typography(
    displayLarge = hero, displayMedium = page, displaySmall = sheet,
    headlineLarge = hero, headlineMedium = page, headlineSmall = sheet,
    titleLarge = st(17f, 600), titleMedium = st(15f, 500), titleSmall = st(14f, 500),
    bodyLarge = message, bodyMedium = body, bodySmall = st(12.5f),
    labelLarge = st(13.5f, 500), labelMedium = st(12f, 500), labelSmall = st(11f, 500),
  )
  return ZoType(
    body = body, message = message, hero = hero, pageTitle = page, wordmark = wordmark, sheetTitle = sheet,
    cardName = st(15f, 500), cardMeta = st(13f), fieldLabel = st(13f), fieldHint = st(12f),
    popTitle = st(14f, 500), popSub = st(12f), sideItem = st(13.5f), navItem = st(14f), button = st(13.5f, 500),
    code = st(13f, family = MonoFamily, lh = 1.5f), sectionHeader = st(12f, 500),
    material = material,
  )
}

val LocalZoType = staticCompositionLocalOf { buildZoType() }

@Composable
fun rememberZoType(): ZoType = remember { buildZoType() }
