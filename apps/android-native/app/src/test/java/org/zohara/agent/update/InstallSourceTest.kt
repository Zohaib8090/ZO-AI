package org.zohara.agent.update

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** What "Download" opens depends on where the app came from (docs/UPDATES.md). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class InstallSourceTest {
  private val latest = LatestRelease("0.2.0", releaseUrl = "https://github.com/o/r/releases/tag/v0.2.0")

  @Test fun onlyGooglePlayCountsAsPlay() {
    assertTrue(InstallSource.isPlayInstaller("com.android.vending"))
    for (other in listOf(null, "", "com.google.android.packageinstaller", "com.android.shell", "org.fdroid.fdroid", "com.amazon.venezia")) assertFalse("$other", InstallSource.isPlayInstaller(other))
  }

  @Test fun aPlayInstallOpensThePlayListing() {
    assertEquals(InstallSource.Target.Play("org.zohara.agent"), InstallSource.targetFor("com.android.vending", "org.zohara.agent", latest))
    assertEquals("market://details?id=org.zohara.agent", InstallSource.playMarketUri("org.zohara.agent").toString())
    assertEquals("https://play.google.com/store/apps/details?id=org.zohara.agent", InstallSource.playWebUrl("org.zohara.agent"))
  }

  @Test fun aSideloadedInstallOpensTheReleasePage() {
    assertEquals(InstallSource.Target.ReleasePage("https://github.com/o/r/releases/tag/v0.2.0"), InstallSource.targetFor(null, "org.zohara.agent", latest))
    assertEquals(InstallSource.Target.ReleasePage(UpdateOutcomes.DEFAULT_RELEASES), InstallSource.targetFor("com.android.shell", "org.zohara.agent", null))
  }
}
