package org.zohara.agent.chat

import android.content.Context
import androidx.compose.ui.platform.UriHandler
import org.zohara.agent.core.markdown.Block
import org.zohara.agent.core.markdown.Inline
import org.zohara.agent.core.markdown.MarkdownParser
import org.zohara.agent.core.util.openInBrowser

/** Links in answers open in a Custom Tab, never inside the app (CHAT-22). Only http and https addresses are opened. */
class ChatUriHandler(private val context: Context) : UriHandler {
  override fun openUri(uri: String) = openInBrowser(context, uri)
}

/** The web addresses of the links in an answer, in order, without repeats: the long-press menu offers "Copy link" for them (CHAT-22). */
object AnswerLinks {
  fun of(text: String, max: Int = 3): List<String> {
    val out = LinkedHashSet<String>()
    fun walk(nodes: List<Inline>) {
      for (n in nodes) when (n) {
        is Inline.Link -> { out.add(n.url); walk(n.label) }
        is Inline.Bold -> walk(n.children)
        is Inline.Italic -> walk(n.children)
        else -> Unit
      }
    }
    for (b in MarkdownParser.parse(text)) when (b) {
      is Block.Paragraph -> walk(b.inlines)
      is Block.Heading -> walk(b.inlines)
      is Block.Item -> walk(b.inlines)
      is Block.Quote -> walk(b.inlines)
      else -> Unit
    }
    return out.take(max)
  }
}
