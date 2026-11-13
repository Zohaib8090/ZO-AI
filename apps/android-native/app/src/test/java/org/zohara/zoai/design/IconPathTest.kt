package org.zohara.zoai.core.design

import androidx.compose.ui.graphics.vector.PathNode
import androidx.compose.ui.graphics.vector.PathParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** The arcs of the web's icons must survive the path parser (they were dropped when the flags had no separator). */
class IconPathTest {
  private fun arcs(d: String) = PathParser().parsePathString(ZoIcons.expandArcFlags(d)).toNodes().filterIsInstance<PathNode.RelativeArcTo>()

  @Test fun compressedFlagsAreExpanded() {
    assertEquals("M 5 11a 7 7 0 0 0 14 0", ZoIcons.expandArcFlags("M5 11a7 7 0 0014 0"))
    val a = arcs("M5 11a7 7 0 0014 0").single()
    assertEquals(14f, a.arcStartDx, 0f)
    assertTrue(!a.isPositiveArc && !a.isMoreThanHalf)
  }

  @Test fun theLinkIconKeepsItsFourArcs() {
    assertEquals(4, arcs("M10 14a4 4 0 005.7 0l3-3a4 4 0 00-5.7-5.7l-1 1M14 10a4 4 0 00-5.7 0l-3 3a4 4 0 005.7 5.7l1-1").size)
  }

  @Test fun flagsWithSpacesStayAndExpandingTwiceChangesNothing() {
    val once = ZoIcons.expandArcFlags("M5 11a7 7 0 0 0 14 0")
    assertEquals(once, ZoIcons.expandArcFlags(once))
    assertEquals(1, arcs("M5 11a7 7 0 0 1 14 0").size)
    assertEquals("M 4 8a 1 1 0 0 1 1 -1h 2.5", ZoIcons.expandArcFlags("M4 8a1 1 0 011-1h2.5"))
  }

  @Test fun everyIconKeepsAllItsArcs() {
    val sources = ZoIcons.javaClass.getDeclaredField("fragments").also { it.isAccessible = true }.get(ZoIcons) as Map<*, *>
    for ((name, frag) in sources) {
      for (d in ZoIcons.svgFragmentToPaths(frag as String)) {
        val declared = Regex("""[aA]""").findAll(d).count()
        val got = PathParser().parsePathString(d).toNodes().count { it is PathNode.ArcTo || it is PathNode.RelativeArcTo }
        assertTrue("$name: $declared arc commands, $got arcs in $d", got >= declared)
      }
    }
  }
}
