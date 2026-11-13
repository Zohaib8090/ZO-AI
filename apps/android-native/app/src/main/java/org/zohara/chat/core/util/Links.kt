package org.zohara.chat.core.util

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent

/** Opens a web page in a Custom Tab (the legal pages, links in answers). Only http(s) addresses are opened. */
fun openInBrowser(context: Context, url: String) {
  val uri = Uri.parse(url)
  if (uri.scheme != "https" && uri.scheme != "http") return
  try {
    CustomTabsIntent.Builder().build().launchUrl(context, uri)
  } catch (_: ActivityNotFoundException) {
    try { context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } catch (_: ActivityNotFoundException) { }
  }
}
