package org.zohara.zoai.core.markdown

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.em
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.zohara.zoai.core.design.MonoFamily
import org.zohara.zoai.core.design.ZoIcons
import org.zohara.zoai.core.design.ZoIconButton
import org.zohara.zoai.core.design.ZoTheme

/** Builds the styled text of inline nodes: code gets the code colours, links the accent colour and an underline. */
@Composable
fun rememberInlineText(nodes: List<Inline>): AnnotatedString {
  val c = ZoTheme.colors
  return remember(nodes, c) {
    buildAnnotatedString { appendInline(nodes, c.accent, c.input, c.codeFg) }
  }
}

private fun AnnotatedString.Builder.appendInline(nodes: List<Inline>, link: androidx.compose.ui.graphics.Color, codeBg: androidx.compose.ui.graphics.Color, codeFg: androidx.compose.ui.graphics.Color) {
  for (n in nodes) when (n) {
    is Inline.Text -> append(n.text)
    is Inline.Bold -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { appendInline(n.children, link, codeBg, codeFg) }
    is Inline.Italic -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { appendInline(n.children, link, codeBg, codeFg) }
    is Inline.Code -> withStyle(SpanStyle(fontFamily = MonoFamily, fontSize = 0.92.em, background = codeBg, color = codeFg)) { append(n.text) }
    is Inline.Link -> withLink(LinkAnnotation.Url(n.url, TextLinkStyles(SpanStyle(color = link, textDecoration = TextDecoration.Underline)))) { appendInline(n.label, link, codeBg, codeFg) }
  }
}

/**
 * Renders markdown the way the web's `md()` does: paragraphs, headings (all one size, like the web's h3), bullet and numbered lists, quotes,
 * fenced code blocks with the language label and a copy button. No WebView.
 */
@Composable
fun MarkdownText(text: String, modifier: Modifier = Modifier, style: TextStyle = ZoTheme.type.message, color: androidx.compose.ui.graphics.Color = ZoTheme.colors.text) {
  val blocks = remember(text) { MarkdownParser.parse(text) }
  MarkdownBlocks(blocks, modifier, style, color)
}

@Composable
fun MarkdownBlocks(blocks: List<Block>, modifier: Modifier = Modifier, style: TextStyle = ZoTheme.type.message, color: androidx.compose.ui.graphics.Color = ZoTheme.colors.text) {
  val c = ZoTheme.colors
  Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
    for (b in blocks) when (b) {
      is Block.Paragraph -> Text(rememberInlineText(b.inlines), style = style, color = color)
      is Block.Heading -> Text(rememberInlineText(b.inlines), style = style.copy(fontWeight = FontWeight.SemiBold, fontSize = style.fontSize * 1.1f), color = color, modifier = Modifier.padding(top = 4.dp))
      is Block.Item -> Row(Modifier.padding(start = (12 + 18 * b.indent).dp)) {
        Text(if (b.number == null) "•" else "${b.number}.", style = style, color = c.dim, modifier = Modifier.width(22.dp))
        Text(rememberInlineText(b.inlines), style = style, color = color, modifier = Modifier.weight(1f))
      }
      is Block.Quote -> Row(Modifier.padding(start = 2.dp).height(IntrinsicSize.Min)) {
        Box(Modifier.width(3.dp).fillMaxHeight().background(c.line12))
        Text(rememberInlineText(b.inlines), style = style, color = c.text3, modifier = Modifier.padding(start = 10.dp))
      }
      is Block.Code -> CodeBlock(b.language, b.code)
      Block.Rule -> Box(Modifier.fillMaxWidth().height(1.dp).background(c.line12))
    }
  }
}

/** A fenced code block: the language (or "code") at the top left, a copy button at the top right, monospace text that scrolls sideways. */
@Composable
fun CodeBlock(language: String, code: String, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  val clipboard = LocalClipboardManager.current
  Column(modifier.fillMaxWidth().background(c.code, ZoTheme.shapes.code).border(1.dp, c.line, ZoTheme.shapes.code)) {
    Row(Modifier.fillMaxWidth().padding(start = 12.dp, end = 4.dp, top = 2.dp), verticalAlignment = Alignment.CenterVertically) {
      Text(language.ifBlank { "code" }, style = ZoTheme.type.fieldHint, color = c.dim, modifier = Modifier.weight(1f))
      ZoIconButton(ZoIcons.Copy, "Copy code", { clipboard.setText(AnnotatedString(code)) }, size = 36.dp, iconSize = 16.dp)
    }
    Text(
      code,
      style = TextStyle(fontFamily = MonoFamily, fontSize = 13.sp, lineHeight = 19.sp),
      color = c.codeFg,
      softWrap = false,
      modifier = Modifier.horizontalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, bottom = 12.dp, top = 2.dp),
    )
  }
}
