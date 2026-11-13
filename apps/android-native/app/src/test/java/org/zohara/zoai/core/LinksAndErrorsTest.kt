package org.zohara.zoai.core

import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.zohara.zoai.core.net.ApiErrors
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.util.openInBrowser

/** External links open in a Custom Tab (PHN-10, AUTH-15, SRV-05) and the messages of the busy and full server (NET-05, SRV-02). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class LinksAndErrorsTest {
  private val app get() = ApplicationProvider.getApplicationContext<android.app.Application>()

  @Test fun anHttpsLinkOpensInACustomTab() {
    val activity = org.robolectric.Robolectric.buildActivity(androidx.activity.ComponentActivity::class.java).setup().get()
    openInBrowser(activity, "https://zo-ai-server.onrender.com/privacy")
    val i = shadowOf(activity).nextStartedActivity
    assertNotNull(i)
    assertEquals(android.content.Intent.ACTION_VIEW, i.action)
    assertEquals("https://zo-ai-server.onrender.com/privacy", i.data.toString())
    assertEquals("it is a Custom Tab, not a plain browser intent", true, i.extras!!.containsKey(androidx.browser.customtabs.CustomTabsIntent.EXTRA_SESSION))
  }

  @Test fun otherSchemesAreNeverOpened() {
    val activity = org.robolectric.Robolectric.buildActivity(androidx.activity.ComponentActivity::class.java).setup().get()
    openInBrowser(activity, "javascript:alert(1)"); openInBrowser(activity, "file:///etc/passwd"); openInBrowser(activity, "intent://x#Intent;end")
    assertNull(shadowOf(activity).nextStartedActivity)
  }

  @Test fun theBusyServerSaysSoAndAFullServerShowsItsOwnSentence() {
    val busy = ApiErrors.fromHttp(503, """{"error":"ZO-AI has as many people as it can take right now. Try again later."}""", "7")
    assertEquals(ErrorKind.Busy, busy.kind); assertEquals(7, busy.retryAfterSec)
    assertEquals("ZO-AI has as many people as it can take right now. Try again later.", busy.text)
    assertEquals(ApiErrors.BUSY, ApiErrors.fromHttp(503, "", null).text)
    val full = ApiErrors.fromHttp(413, """{"error":"Your storage is full (25 MB). Delete something first."}""", null)
    assertEquals("Your storage is full (25 MB). Delete something first.", full.text)
    assertEquals("an op that failed with a sentence shows it as is", "Your storage is full (25 MB). Delete something first.", ApiErrors.fromOpFailure("Your storage is full (25 MB). Delete something first.").text)
  }
}
