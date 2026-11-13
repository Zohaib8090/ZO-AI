package org.zohara.chat.core.design

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * The icon set of the web app (index.html `P`, copied as is: same 24x24 shapes, stroke only), plus a few more drawn the same way.
 * Each icon is an SVG fragment; [svgFragmentToPath] turns it into path data and [icon] into an [ImageVector] (cached).
 */
object ZoIcons {
  // ---- the web's own set (index.html:455-466) ----
  private val fragments: Map<String, String> = mapOf(
    "plus" to """<path d="M12 5v14M5 12h14"/>""",
    "chat" to """<path d="M5 5h14a1 1 0 011 1v9a1 1 0 01-1 1h-7l-4 3.5V16H5a1 1 0 01-1-1V6a1 1 0 011-1z"/>""",
    "search" to """<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4 4"/>""",
    "grid" to """<rect x="4" y="4" width="7" height="7" rx="1.2"/><rect x="13" y="4" width="7" height="7" rx="1.2"/><rect x="4" y="13" width="7" height="7" rx="1.2"/><rect x="13" y="13" width="7" height="7" rx="1.2"/>""",
    "list" to """<path d="M9 7h11M9 12h11M9 17h11"/><circle cx="5" cy="7" r=".8"/><circle cx="5" cy="12" r=".8"/><circle cx="5" cy="17" r=".8"/>""",
    "alarm" to """<circle cx="12" cy="13" r="7"/><path d="M12 9v4l2.5 1.5M5 4L3 6M19 4l2 2"/>""",
    "refresh" to """<path d="M21 12a9 9 0 00-15.5-6.4L3 8M3 3v5h5M3 12a9 9 0 0015.5 6.4L21 16M21 21v-5h-5"/>""",
    "context" to """<circle cx="12" cy="12" r="9"/><path d="M12 3v9h9"/>""",
    "activity" to """<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>""",
    "terminal" to """<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>""",
    "sidebar" to """<rect x="4" y="5" width="16" height="14" rx="2"/><path d="M10 5v14"/>""",
    "gear" to """<circle cx="12" cy="12" r="3"/><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8"/>""",
    "up" to """<path d="M6 15l6-6 6 6"/>""",
    "chev" to """<path d="M9 6l6 6-6 6"/>""",
    "down" to """<path d="M6 9l6 6 6-6"/>""",
    "copy" to """<rect x="8" y="8" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 012-2h9"/>""",
    "folder" to """<path d="M4 7a1 1 0 011-1h4l2 2h8a1 1 0 011 1v8a1 1 0 01-1 1H5a1 1 0 01-1-1V7z"/>""",
    "edit" to """<path d="M4 20l4-1 11-11-3-3L5 16l-1 4z"/>""",
    "trash" to """<path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12"/>""",
    "star" to """<path d="M12 4l2.5 5 5.5.8-4 3.9.9 5.5-4.9-2.6-4.9 2.6.9-5.5-4-3.9 5.5-.8z"/>""",
    "mic" to """<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3"/>""",
    "speaker" to """<path d="M4 9v6h4l5 4V5L8 9H4z"/><path d="M16 9a4 4 0 010 6M18.5 6.5a8 8 0 010 11"/>""",
    "out" to """<path d="M14 5h4a1 1 0 011 1v12a1 1 0 01-1 1h-4M10 8l-4 4 4 4M6 12h10"/>""",
    "stop" to """<rect x="7" y="7" width="10" height="10" rx="2"/>""",
    "check" to """<path d="M5 12l5 5 9-10"/>""",
    "pin" to """<path d="M9 4h6l-1 6 3 3H7l3-3-1-6zM12 13v7"/>""",
    // ---- extra icons drawn in the same style ----
    "x" to """<path d="M18 6L6 18M6 6l12 12"/>""",
    "left" to """<path d="M15 6l-6 6 6 6"/>""",
    "attach" to """<path d="M20 11.5l-7.8 7.8a4.5 4.5 0 01-6.4-6.4l8-8a3 3 0 014.3 4.3l-8 8a1.5 1.5 0 01-2.1-2.1l7.3-7.3"/>""",
    "image" to """<rect x="4" y="5" width="16" height="14" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M4 17l5-4.5 4 3.5 3-2.5 4 3.5"/>""",
    "camera" to """<path d="M4 8a1 1 0 011-1h2.5l1.5-2h6l1.5 2H19a1 1 0 011 1v10a1 1 0 01-1 1H5a1 1 0 01-1-1V8z"/><circle cx="12" cy="13" r="3.5"/>""",
    "more" to """<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>""",
    "eye" to """<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>""",
    "eyeoff" to """<path d="M3 3l18 18M10.6 5.7A9 9 0 0112 5.5c6 0 9.5 6.5 9.5 6.5a16 16 0 01-3 3.6M6.5 7.5A16 16 0 002.5 12S6 18.5 12 18.5a9 9 0 004-.9"/>""",
    "link" to """<path d="M10 14a4 4 0 005.7 0l3-3a4 4 0 00-5.7-5.7l-1 1M14 10a4 4 0 00-5.7 0l-3 3a4 4 0 005.7 5.7l1-1"/>""",
    "external" to """<path d="M14 4h6v6M20 4l-9 9M18 14v4a1 1 0 01-1 1H6a1 1 0 01-1-1V7a1 1 0 011-1h4"/>""",
    "info" to """<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.8v.4"/>""",
    "warning" to """<path d="M12 4L3 19h18L12 4zM12 10v4.5M12 16.8v.4"/>""",
    "send" to """<path d="M12 19V6M6 11l6-6 6 6"/>""",
    "bell" to """<path d="M6 16V11a6 6 0 0112 0v5l1.5 2h-15L6 16zM10 20.5h4"/>""",
    "user" to """<circle cx="12" cy="9" r="3.5"/><path d="M5 20a7 7 0 0114 0"/>""",
    "computer" to """<rect x="3.5" y="5" width="17" height="11" rx="1.5"/><path d="M8 20h8M12 16v4"/>""",
    "globe" to """<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 010 18M12 3a14 14 0 000 18"/>""",
    "key" to """<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3"/>""",
    "download" to """<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>""",
    "file" to """<path d="M7 3h7l5 5v12a1 1 0 01-1 1H7a1 1 0 01-1-1V4a1 1 0 011-1zM14 3v5h5"/>""",
    "moon" to """<path d="M20 14.5A8 8 0 019.5 4 8 8 0 1020 14.5z"/>""",
    "bolt" to """<path d="M13 3L5 13.5h6L10 21l8-10.5h-6L13 3z"/>""",
  )

  val names: Set<String> get() = fragments.keys

  private val cache = HashMap<String, ImageVector>()

  /** The icon called [name] (a key of the web's icon table), drawn black; tint it with `Icon(tint = ...)`. */
  fun icon(name: String): ImageVector = synchronized(cache) {
    cache.getOrPut(name) { build(name, fragments[name] ?: error("no icon $name")) }
  }

  val Plus get() = icon("plus")
  val Chat get() = icon("chat")
  val Search get() = icon("search")
  val Grid get() = icon("grid")
  val List get() = icon("list")
  val Alarm get() = icon("alarm")
  val Refresh get() = icon("refresh")
  val Context get() = icon("context")
  val Activity get() = icon("activity")
  val Terminal get() = icon("terminal")
  val Sidebar get() = icon("sidebar")
  val Gear get() = icon("gear")
  val Up get() = icon("up")
  val Chevron get() = icon("chev")
  val Down get() = icon("down")
  val Copy get() = icon("copy")
  val Folder get() = icon("folder")
  val Edit get() = icon("edit")
  val Trash get() = icon("trash")
  val Star get() = icon("star")
  val Mic get() = icon("mic")
  val Speaker get() = icon("speaker")
  val SignOut get() = icon("out")
  val Stop get() = icon("stop")
  val Check get() = icon("check")
  val Pin get() = icon("pin")
  val Close get() = icon("x")
  val Back get() = icon("left")
  val Attach get() = icon("attach")
  val Image get() = icon("image")
  val Camera get() = icon("camera")
  val More get() = icon("more")
  val Eye get() = icon("eye")
  val EyeOff get() = icon("eyeoff")
  val Link get() = icon("link")
  val External get() = icon("external")
  val Info get() = icon("info")
  val Warning get() = icon("warning")
  val Send get() = icon("send")
  val Bell get() = icon("bell")
  val User get() = icon("user")
  val Computer get() = icon("computer")
  val Globe get() = icon("globe")
  val Key get() = icon("key")
  val Download get() = icon("download")
  val File get() = icon("file")
  val Moon get() = icon("moon")
  val Bolt get() = icon("bolt")

  private fun build(name: String, fragment: String): ImageVector {
    val b = ImageVector.Builder(name = "zo_$name", defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f)
    for (d in svgFragmentToPaths(fragment)) {
      b.addPath(
        pathData = PathParser().parsePathString(d).toNodes(),
        stroke = SolidColor(Color.Black),
        strokeLineWidth = 1.8f,
        strokeLineCap = StrokeCap.Round,
        strokeLineJoin = StrokeJoin.Round,
      )
    }
    return b.build()
  }

  private val tag = Regex("""<(path|circle|rect|polyline|line)\b([^>]*?)/?>""")
  private val attr = Regex("""([a-z0-9]+)="([^"]*)"""")

  private val number = Regex("""[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?""")

  /**
   * SVG lets the two flags of an arc be written without separators ("a7 7 0 0014 0"); Compose's path parser reads that wrongly and drops the arc.
   * This writes every number of the path with a space, flags as single digits, so the arcs of the web's icons (mic, link, chat bubble...) are drawn.
   */
  fun expandArcFlags(d: String): String {
    val out = StringBuilder()
    var i = 0
    var cmd = ' '
    var idx = 0
    while (i < d.length) {
      val ch = d[i]
      when {
        ch.isWhitespace() || ch == ',' -> i++
        ch.isLetter() -> { cmd = ch; idx = 0; out.append(ch); i++ }
        (cmd == 'a' || cmd == 'A') && idx % 7 in 3..4 && (ch == '0' || ch == '1') -> { out.append(' ').append(ch); i++; idx++ }
        else -> {
          val m = number.matchAt(d, i) ?: return d // not understood: leave it alone
          out.append(' ').append(m.value); i += m.value.length; idx++
        }
      }
    }
    return out.toString()
  }

  /** Turns the elements of an SVG fragment (path, circle, rect, polyline, line) into SVG path data strings. */
  fun svgFragmentToPaths(fragment: String): List<String> = tag.findAll(fragment).map { m ->
    val a = attr.findAll(m.groupValues[2]).associate { it.groupValues[1] to it.groupValues[2] }
    fun f(k: String, d: Float = 0f) = a[k]?.toFloatOrNull() ?: d
    when (m.groupValues[1]) {
      "path" -> expandArcFlags(a["d"].orEmpty())
      "circle" -> {
        val cx = f("cx"); val cy = f("cy"); val r = f("r")
        "M${cx - r} ${cy}a$r $r 0 1 0 ${2 * r} 0a$r $r 0 1 0 ${-2 * r} 0z"
      }
      "rect" -> {
        val x = f("x"); val y = f("y"); val w = f("width"); val h = f("height"); val r = f("rx")
        if (r <= 0f) "M$x ${y}h${w}v${h}h${-w}z"
        else "M${x + r} ${y}h${w - 2 * r}a$r $r 0 0 1 $r ${r}v${h - 2 * r}a$r $r 0 0 1 ${-r} ${r}h${-(w - 2 * r)}a$r $r 0 0 1 ${-r} ${-r}v${-(h - 2 * r)}a$r $r 0 0 1 $r ${-r}z"
      }
      "polyline" -> {
        val pts = a["points"].orEmpty().trim().split(Regex("[ ,]+")).mapNotNull { it.toFloatOrNull() }
        pts.chunked(2).filter { it.size == 2 }.mapIndexed { i, p -> (if (i == 0) "M" else "L") + "${p[0]} ${p[1]}" }.joinToString("")
      }
      "line" -> "M${f("x1")} ${f("y1")}L${f("x2")} ${f("y2")}"
      else -> ""
    }
  }.filter { it.isNotBlank() }.toList()
}
