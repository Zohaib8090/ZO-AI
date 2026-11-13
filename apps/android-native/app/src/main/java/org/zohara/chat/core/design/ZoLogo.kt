package org.zohara.chat.core.design

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlin.math.acos
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * The Zohara Chat mark (crates/zohara-chat/data/zohara-chat-mark.svg): a ring with a round gap at the upper right, a four-point star and a small gold star.
 * The 120 x 120 geometry is exactly the SVG's; the ring and star take the accent colours of the current theme.
 */
object ZoMark {
  const val VIEW = 120f
  private const val CX = 60f
  private const val CY = 60f
  private const val R = 48f
  private const val GAP_X = 94f
  private const val GAP_Y = 26f
  private const val GAP_R = 16f

  /** Ring start and end angle (degrees, screen coordinates, clockwise) so the arc stops where the gap circle cuts it. */
  fun ringArc(): Pair<Double, Double> {
    val dx = GAP_X - CX
    val dy = GAP_Y - CY
    val d = sqrt((dx * dx + dy * dy).toDouble())
    val centre = Math.toDegrees(kotlin.math.atan2(dy.toDouble(), dx.toDouble()))
    val half = Math.toDegrees(acos((R * R + d * d - GAP_R * GAP_R) / (2 * R * d)))
    return (centre + half) to (centre - half + 360.0)
  }

  /** The ring as a path in the 120 x 120 box. */
  fun ringPath(): Path {
    val (a0, a1) = ringArc()
    val p = Path()
    val steps = 96
    for (i in 0..steps) {
      val a = Math.toRadians(a0 + (a1 - a0) * i / steps)
      val x = (CX + R * cos(a)).toFloat()
      val y = (CY + R * sin(a)).toFloat()
      if (i == 0) p.moveTo(x, y) else p.lineTo(x, y)
    }
    return p
  }

  fun starPath(): Path = Path().apply {
    moveTo(60f, 30f); quadraticTo(60f, 60f, 90f, 60f); quadraticTo(60f, 60f, 60f, 90f)
    quadraticTo(60f, 60f, 30f, 60f); quadraticTo(60f, 60f, 60f, 30f); close()
  }

  fun goldPath(): Path = Path().apply {
    moveTo(94f, 14f); quadraticTo(94f, 26f, 106f, 26f); quadraticTo(94f, 26f, 94f, 38f)
    quadraticTo(94f, 26f, 82f, 26f); quadraticTo(94f, 26f, 94f, 14f); close()
  }
}

@Composable
fun ZoLogo(
  size: Dp = 26.dp,
  ring: Color = ZoTheme.colors.accent,
  star: Color = ZoTheme.colors.accent2,
  gold: Color = ZoTheme.colors.gold,
  modifier: Modifier = Modifier,
) {
  Canvas(modifier.size(size).semantics { contentDescription = "Zohara Chat logo" }) {
    val s = this.size.minDimension / ZoMark.VIEW
    scale(s, s, pivot = Offset.Zero) {
      drawPath(ZoMark.ringPath(), ring, style = Stroke(width = 8f, cap = StrokeCap.Butt))
      drawPath(ZoMark.starPath(), star)
      drawPath(ZoMark.goldPath(), gold)
    }
  }
}
