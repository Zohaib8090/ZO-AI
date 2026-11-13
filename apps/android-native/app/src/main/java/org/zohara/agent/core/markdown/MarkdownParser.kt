package org.zohara.agent.core.markdown

/** Inline pieces of a line (the web's `inline()`: code, bold, italic, links). */
sealed interface Inline {
  data class Text(val text: String) : Inline
  data class Bold(val children: List<Inline>) : Inline
  data class Italic(val children: List<Inline>) : Inline
  data class Code(val text: String) : Inline
  data class Link(val label: List<Inline>, val url: String) : Inline
}

/** Blocks of a message (the web's `md()`): paragraphs, headings, lists, quotes, fenced code. */
sealed interface Block {
  data class Paragraph(val inlines: List<Inline>) : Block
  data class Heading(val level: Int, val inlines: List<Inline>) : Block
  /** a list item: [indent] is the nesting level (2 spaces = 1), [number] is null for a bullet */
  data class Item(val indent: Int, val number: Int?, val inlines: List<Inline>) : Block
  data class Quote(val inlines: List<Inline>) : Block
  data class Code(val language: String, val code: String) : Block
  data object Rule : Block
}

object MarkdownParser {
  private val bullet = Regex("""^(\s*)[-*+]\s+(.*)""")
  private val numbered = Regex("""^(\s*)(\d+)[.)]\s+(.*)""")
  private val heading = Regex("""^#{1,6}\s+(.*)""")
  private val quote = Regex("""^>\s?(.*)""")
  private val rule = Regex("""^\s*([-*_])(\s*\1){2,}\s*$""")

  fun parse(src: String): List<Block> {
    val out = ArrayList<Block>()
    val parts = src.split("```")
    parts.forEachIndexed { i, part ->
      if (i % 2 == 1) {
        // a fenced block: the first line is the language; an unclosed fence (a message still streaming) is code too
        val nl = part.indexOf('\n')
        val lang = (if (nl >= 0) part.substring(0, nl) else "").trim()
        val code = (if (nl >= 0) part.substring(nl + 1) else part).removeSuffix("\n")
        out.add(Block.Code(lang, code))
        return@forEachIndexed
      }
      for (line in part.split("\n")) {
        val l = line.trimEnd('\r')
        when {
          l.isBlank() -> Unit
          rule.matches(l) && !bullet.matches(l) -> out.add(Block.Rule)
          bullet.matchEntire(l) != null -> bullet.matchEntire(l)!!.let { m -> out.add(Block.Item(m.groupValues[1].length / 2, null, parseInline(m.groupValues[2]))) }
          numbered.matchEntire(l) != null -> numbered.matchEntire(l)!!.let { m -> out.add(Block.Item(m.groupValues[1].length / 2, m.groupValues[2].toIntOrNull() ?: 1, parseInline(m.groupValues[3]))) }
          heading.matchEntire(l) != null -> {
            val level = l.takeWhile { it == '#' }.length
            out.add(Block.Heading(level, parseInline(heading.matchEntire(l)!!.groupValues[1])))
          }
          quote.matchEntire(l) != null -> out.add(Block.Quote(parseInline(quote.matchEntire(l)!!.groupValues[1])))
          else -> out.add(Block.Paragraph(parseInline(l)))
        }
      }
    }
    return out
  }

  /** `code`, **bold**, *italic* (also _italic_), [label](https://url). Anything else is plain text. */
  fun parseInline(s: String): List<Inline> {
    val out = ArrayList<Inline>()
    val plain = StringBuilder()
    fun flush() { if (plain.isNotEmpty()) { out.add(Inline.Text(plain.toString())); plain.clear() } }
    var i = 0
    while (i < s.length) {
      val c = s[i]
      when {
        c == '`' -> {
          val end = s.indexOf('`', i + 1)
          if (end > i + 1 && s.indexOf('\n', i + 1).let { it < 0 || it > end }) {
            flush(); out.add(Inline.Code(s.substring(i + 1, end))); i = end + 1
          } else { plain.append(c); i++ }
        }
        c == '*' && s.startsWith("**", i) -> {
          var end = s.indexOf("**", i + 2)
          // "**bold *x***": the closing marker is the LAST two stars of a run
          while (end > i + 2 && s.getOrNull(end + 2) == '*') end++
          if (end > i + 2) {
            flush(); out.add(Inline.Bold(parseInline(s.substring(i + 2, end)))); i = end + 2
          } else { plain.append("**"); i += 2 }
        }
        (c == '*' || c == '_') && openerOk(s, i) -> {
          val end = closer(s, i + 1, c)
          if (end > i + 1) {
            flush(); out.add(Inline.Italic(parseInline(s.substring(i + 1, end)))); i = end + 1
          } else { plain.append(c); i++ }
        }
        c == '[' -> {
          val m = linkAt(s, i)
          if (m != null) {
            flush(); out.add(Inline.Link(parseInline(m.label), m.url)); i = m.end
          } else { plain.append(c); i++ }
        }
        else -> { plain.append(c); i++ }
      }
    }
    flush()
    return out
  }

  private fun openerOk(s: String, i: Int): Boolean {
    val before = if (i == 0) ' ' else s[i - 1]
    val after = s.getOrNull(i + 1) ?: return false
    return (before.isWhitespace() || before == '(') && !after.isWhitespace() && after != s[i]
  }

  private fun closer(s: String, from: Int, ch: Char): Int {
    var j = from
    while (j < s.length) {
      if (s[j] == '\n') return -1
      if (s[j] == ch && j > from && !s[j - 1].isWhitespace()) {
        val next = s.getOrNull(j + 1)
        if (next == null || next.isWhitespace() || next in ").,;:!?") return j
      }
      j++
    }
    return -1
  }

  private class LinkMatch(val label: String, val url: String, val end: Int)

  private fun linkAt(s: String, i: Int): LinkMatch? {
    val close = s.indexOf(']', i + 1)
    if (close < 0 || close == i + 1 || s.substring(i + 1, close).contains('\n')) return null
    if (s.getOrNull(close + 1) != '(') return null
    val end = s.indexOf(')', close + 2)
    if (end < 0) return null
    val url = s.substring(close + 2, end)
    if (!(url.startsWith("http://") || url.startsWith("https://")) || url.any { it.isWhitespace() }) return null
    return LinkMatch(s.substring(i + 1, close), url, end + 1)
  }

  /** Plain text of inline nodes (copying, tests, accessibility). */
  fun plainText(list: List<Inline>): String = buildString {
    for (n in list) when (n) {
      is Inline.Text -> append(n.text)
      is Inline.Bold -> append(plainText(n.children))
      is Inline.Italic -> append(plainText(n.children))
      is Inline.Code -> append(n.text)
      is Inline.Link -> append(plainText(n.label))
    }
  }
}
