package org.zohara.chat.core.markdown

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MarkdownParserTest {
  private fun blocks(s: String) = MarkdownParser.parse(s)

  @Test fun headingsAndParagraphs() {
    val b = blocks("# Title\n\n### Small\nplain text")
    assertEquals(3, b.size)
    assertEquals(1, (b[0] as Block.Heading).level)
    assertEquals(3, (b[1] as Block.Heading).level)
    assertEquals("plain text", MarkdownParser.plainText((b[2] as Block.Paragraph).inlines))
  }

  @Test fun inlineStyles() {
    val i = MarkdownParser.parseInline("a **bold** and *it* and `code` and [site](https://x.org/p?q=1) end")
    assertTrue(i.any { it is Inline.Bold })
    assertTrue(i.any { it is Inline.Italic })
    assertEquals("code", (i.first { it is Inline.Code } as Inline.Code).text)
    assertEquals("https://x.org/p?q=1", (i.first { it is Inline.Link } as Inline.Link).url)
    assertEquals("a bold and it and code and site end", MarkdownParser.plainText(i))
  }

  @Test fun nestedBoldItalic() {
    val i = MarkdownParser.parseInline("**bold *and italic***")
    assertTrue(i.single() is Inline.Bold)
  }

  @Test fun listsBulletAndNumbered() {
    val b = blocks("- one\n- two\n  - nested\n1. first\n2) second")
    val items = b.filterIsInstance<Block.Item>()
    assertEquals(5, items.size)
    assertEquals(null, items[0].number)
    assertEquals(1, items[2].indent)
    assertEquals(1, items[3].number)
    assertEquals(2, items[4].number)
  }

  @Test fun quoteAndRule() {
    val b = blocks("> quoted *text*\n---")
    assertTrue(b[0] is Block.Quote)
    assertEquals(Block.Rule, b[1])
  }

  @Test fun fencedCodeWithLanguage() {
    val b = blocks("before\n```kotlin\nval x = 1\nprintln(x)\n```\nafter")
    val code = b.filterIsInstance<Block.Code>().single()
    assertEquals("kotlin", code.language)
    assertEquals("val x = 1\nprintln(x)", code.code)
    assertEquals(3, b.size)
  }

  @Test fun unclosedFenceWhileStreamingIsCode() {
    val code = blocks("text\n```py\nprint(1)").filterIsInstance<Block.Code>().single()
    assertEquals("py", code.language)
    assertEquals("print(1)", code.code)
  }

  @Test fun markdownInsideCodeIsNotParsed() {
    val code = blocks("```\n# not a heading **x**\n```").single() as Block.Code
    assertEquals("# not a heading **x**", code.code)
  }

  @Test fun unsafeLinksStayPlain() {
    val i = MarkdownParser.parseInline("[x](javascript:alert(1)) and [y](https://a.b)")
    assertEquals(1, i.filterIsInstance<Inline.Link>().size)
    assertEquals("https://a.b", i.filterIsInstance<Inline.Link>()[0].url)
  }

  @Test fun unbalancedMarkersAreText() {
    assertEquals("2 * 3 = 6 and a_b_c", MarkdownParser.plainText(MarkdownParser.parseInline("2 * 3 = 6 and a_b_c")))
    assertEquals("**open", MarkdownParser.plainText(MarkdownParser.parseInline("**open")))
  }

  @Test fun unicodeSurvives() {
    assertEquals("नमस्ते **दुनिया**".replace("**", ""), MarkdownParser.plainText(MarkdownParser.parseInline("नमस्ते **दुनिया**")))
  }
}
