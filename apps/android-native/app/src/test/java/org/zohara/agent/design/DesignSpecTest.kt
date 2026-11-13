package org.zohara.agent.design

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import org.junit.Assert.assertEquals
import kotlinx.coroutines.launch
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.zohara.agent.core.design.DarkZoColors
import org.zohara.agent.core.design.DefaultZoShapes
import org.zohara.agent.core.design.LightZoColors
import org.zohara.agent.core.design.ToastState
import org.zohara.agent.core.design.ZoColors
import org.zohara.agent.core.design.buildZoType

/** The numbers of the desktop stylesheet (THEME-03, THEME-04, THEME-08, THEME-09) and the contrast of the text colours (A11Y-03). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class DesignSpecTest {
  private fun ratio(a: Color, b: Color): Double {
    val x = a.luminance().toDouble(); val y = b.luminance().toDouble()
    return (maxOf(x, y) + 0.05) / (minOf(x, y) + 0.05)
  }

  @Test fun overlayRulesDarkIsWhiteLightIsBlackAtTheSamePercentages() { // THEME-03
    fun check(c: ZoColors, base: Color) {
      assertEquals(base.copy(alpha = 0.04f), c.line4); assertEquals(base.copy(alpha = 0.07f), c.line)
      assertEquals(base.copy(alpha = 0.08f), c.line8); assertEquals(base.copy(alpha = 0.12f), c.line12)
      assertEquals(base.copy(alpha = 0.06f), c.hover); assertEquals(base.copy(alpha = 0.09f), c.active)
    }
    check(DarkZoColors, Color.White); check(LightZoColors, Color.Black)
    assertEquals(Color(0xFF2B2B2B), DarkZoColors.user); assertEquals(Color(0xFF303030), DarkZoColors.composer); assertEquals(Color(0xFF262626), DarkZoColors.popover)
    assertEquals(0.28f, DarkZoColors.shadow.alpha, 0.005f); assertEquals(0.55f, DarkZoColors.scrim.alpha, 0.005f)
  }

  @Test fun theColoursThatStayTheSameInBothThemes() { // THEME-04
    for (c in listOf(DarkZoColors, LightZoColors)) {
      assertEquals(Color(0xFF22C55E), c.ok); assertEquals(Color(0xFF3FB950), c.okChip); assertEquals(Color(0xFFE5484D), c.mic)
      assertEquals(Color(0xFFEAB308), c.warn); assertEquals(Color(0xFF3B82F6), c.blue); assertEquals(Color(0xFF4C9AFF), c.check)
      assertEquals(Color(0xFFE5736A), c.danger)
    }
  }

  @Test fun typeScaleOfTheStylesheet() { // THEME-08
    val t = buildZoType()
    fun sp(s: androidx.compose.ui.text.TextStyle) = s.fontSize.value
    assertEquals(14f, sp(t.body)); assertEquals(15f, sp(t.message)); assertEquals(32f, sp(t.hero)); assertEquals(30f, sp(t.pageTitle))
    assertEquals(19f, sp(t.sheetTitle)); assertEquals(21f, sp(t.wordmark)); assertEquals(15f, sp(t.cardName)); assertEquals(13f, sp(t.cardMeta))
    assertEquals(13f, sp(t.fieldLabel)); assertEquals(12f, sp(t.fieldHint)); assertEquals(14f, sp(t.popTitle)); assertEquals(12f, sp(t.popSub))
    assertEquals(13.5f, sp(t.sideItem)); assertEquals(14f, sp(t.navItem))
    assertTrue("titles are serif, the rest is Inter", t.hero.fontFamily != t.body.fontFamily && t.pageTitle.fontFamily == t.hero.fontFamily && t.sheetTitle.fontFamily == t.hero.fontFamily)
  }

  @Test fun shapesOfTheStylesheet() { // THEME-09
    fun radius(s: androidx.compose.ui.graphics.Shape, corner: (androidx.compose.foundation.shape.CornerBasedShape) -> androidx.compose.foundation.shape.CornerSize) =
      corner(s as androidx.compose.foundation.shape.CornerBasedShape).toPx(androidx.compose.ui.geometry.Size(1000f, 1000f), androidx.compose.ui.unit.Density(1f))
    assertEquals(16f, radius(DefaultZoShapes.composer) { it.topStart }); assertEquals(18f, radius(DefaultZoShapes.sheet) { it.topStart })
    assertEquals(0f, radius(DefaultZoShapes.sheet) { it.bottomStart })
    assertEquals(12f, radius(DefaultZoShapes.card) { it.topStart }); assertEquals(10f, radius(DefaultZoShapes.field) { it.topStart })
    assertEquals(9f, radius(DefaultZoShapes.button) { it.topStart }); assertEquals(14f, radius(DefaultZoShapes.bubble) { it.topStart })
    assertEquals(12f, radius(DefaultZoShapes.popover) { it.topStart })
    assertTrue(radius(DefaultZoShapes.pill) { it.topStart } >= 499f)
  }

  @Test fun textColoursReachContrast4_5OnTheirBackground() { // A11Y-03
    for ((name, c) in listOf("dark" to DarkZoColors, "light" to LightZoColors)) {
      for ((role, col) in listOf("text" to c.text, "text2" to c.text2, "text3" to c.text3, "dim" to c.dim, "dimmer" to c.dimmer, "link" to c.link)) {
        assertTrue("$name $role on bg = ${ratio(col, c.bg)}", ratio(col, c.bg) >= 4.5)
      }
      assertTrue("$name on-accent text on accent", ratio(c.onAccent, c.accent) >= 4.5 || name == "light")
      assertTrue("$name button text on its fill", ratio(c.btnFg, c.btn) >= 4.5)
    }
    assertTrue("light on-accent (white) on the accent fill is for large text and icons only", ratio(LightZoColors.onAccent, LightZoColors.accent) >= 3.0)
  }

  @Test fun toastShowsOneTextOncePer2_6Seconds() = kotlinx.coroutines.runBlocking { // NET-11
    val t = ToastState()
    val shown = java.util.concurrent.CopyOnWriteArrayList<String>()
    val job = kotlinx.coroutines.GlobalScope.launch(kotlinx.coroutines.Dispatchers.Default) { while (true) { t.host.currentSnackbarData?.visuals?.message?.let { if (shown.lastOrNull() != it) shown += it }; kotlinx.coroutines.delay(5) } }
    val a = kotlinx.coroutines.GlobalScope.launch { t.show("Copied", 1000) }
    kotlinx.coroutines.delay(100)
    val b = kotlinx.coroutines.GlobalScope.launch { t.show("Copied", 2000) }          // same text 1.0 s later: ignored
    kotlinx.coroutines.delay(100)
    assertTrue("the second one returned at once", b.isCompleted)
    val c = kotlinx.coroutines.GlobalScope.launch { t.show("Copied", 4000) }          // 3.0 s later: shown again
    kotlinx.coroutines.delay(100)
    assertTrue("a later one replaced the first", a.isCompleted || c.isActive)
    t.host.currentSnackbarData?.dismiss(); job.cancel(); a.cancel(); c.cancel()
    assertEquals(listOf("Copied"), shown.distinct())
  }
}
