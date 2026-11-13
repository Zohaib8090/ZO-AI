package org.zohara.zoai.core.design

import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp

/** Corner radii of the desktop stylesheet (THEME-09). */
@Immutable
data class ZoShapes(
  val composer: Shape = RoundedCornerShape(16.dp),
  val sheet: Shape = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp),
  val sheetCard: Shape = RoundedCornerShape(18.dp),
  val card: Shape = RoundedCornerShape(12.dp),
  val field: Shape = RoundedCornerShape(10.dp),
  val button: Shape = RoundedCornerShape(9.dp),
  val pill: Shape = RoundedCornerShape(999.dp),
  val bubble: Shape = RoundedCornerShape(14.dp),
  val popover: Shape = RoundedCornerShape(12.dp),
  val code: Shape = RoundedCornerShape(10.dp),
  val navItem: Shape = RoundedCornerShape(8.dp),
  val avatar: Shape = CircleShape,
)

val DefaultZoShapes = ZoShapes()
val LocalZoShapes = staticCompositionLocalOf { DefaultZoShapes }
