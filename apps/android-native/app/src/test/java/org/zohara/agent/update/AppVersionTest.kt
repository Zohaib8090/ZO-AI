package org.zohara.agent.update

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The same vectors as `zo_core::update` (Rust) and the web page, so every client agrees on what is newer. */
class AppVersionTest {
  private fun v(s: String) = AppVersion.parse(s) ?: error(s)

  @Test fun numbersAreComparedAsNumbers() {
    assertTrue(v("0.10.0") > v("0.9.9"))
    assertTrue(v("1.0.0") > v("0.99.99"))
    assertTrue(v("1.2.10") > v("1.2.9"))
    assertEquals(v("0.1.0"), v("v0.1.0"))
    assertEquals(v("0.1"), v("0.1.0"))
    assertEquals(v("1"), v("1.0.0"))
    // the desktop package has a fourth number (a build time)
    assertTrue(v("0.1.0.202611141300") > v("0.1.0.202611141200"))
    assertTrue(v("0.1.1") > v("0.1.0.202699999"))
  }

  @Test fun preReleasesFollowTheSemverOrder() {
    val order = listOf("1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0")
    for ((a, b) in order.zipWithNext()) assertTrue("$a < $b", v(a) < v(b))
    assertTrue(v("1.0.0-rc.1").isPrerelease)
    assertFalse(v("1.0.0").isPrerelease)
    assertEquals(v("1.0.0+abc"), v("1.0.0+xyz"))
    assertEquals(v("1.0.0+abc"), v("1.0.0"))
  }

  @Test fun nonsenseDoesNotParse() {
    for (bad in listOf("", "v", "a.b.c", "1.2.3.4.5", "1..2", "1.2.x", "-1.0.0", "1.0.0-", "1.0.0-a..b", "99999999999999999999.0.0", "1.0.0 beta", "latest")) {
      assertNull("'$bad'", AppVersion.parse(bad))
    }
    assertNull(AppVersion.parse(null))
    assertNotNull(AppVersion.parse(" v1.2.3 "))
  }

  @Test fun isNewerNeverRaisesAFalseAlarm() {
    assertTrue(AppVersion.isNewer("0.1.0", "0.2.0"))
    assertFalse(AppVersion.isNewer("0.2.0", "0.2.0"))
    assertFalse(AppVersion.isNewer("0.3.0", "0.2.0"))
    assertFalse(AppVersion.isNewer("0.1.0", "garbage"))
    assertFalse(AppVersion.isNewer("garbage", "9.9.9"))
    assertFalse(AppVersion.isNewer(null, "9.9.9"))
    assertTrue(AppVersion.isNewer("1.0.0-rc.1", "1.0.0"))
    assertFalse(AppVersion.isNewer("1.0.0", "1.0.0-rc.1"))
  }
}
