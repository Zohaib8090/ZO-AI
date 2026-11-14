package org.zohara.agent.update

import android.Manifest
import android.app.Application
import android.app.NotificationManager
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.test.core.app.ApplicationProvider
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.zohara.agent.MainActivity
import org.zohara.agent.core.store.ZoPrefs
import org.zohara.agent.core.util.NotificationChannels

/** ONE notification per new version, on its own channel, tapping opens About, and nothing is downloaded (docs/UPDATES.md). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class UpdateNotifierTest {
  @get:Rule val tmp = TemporaryFolder()
  private val context get() = ApplicationProvider.getApplicationContext<Application>()
  private lateinit var prefs: ZoPrefs
  private val nm get() = context.getSystemService(NotificationManager::class.java)

  @Before fun setUp() {
    prefs = ZoPrefs(PreferenceDataStoreFactory.create(scope = CoroutineScope(Dispatchers.IO)) { File(tmp.root, "p${System.nanoTime()}.preferences_pb") }, "https://x.example")
    shadowOf(context).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
  }

  private fun available(current: String = "0.1.0", latest: String = "0.2.0", notes: String = "Update checks.\nMore.", required: Boolean = false) =
    UpdateOutcome.Available(current, LatestRelease(latest, notes = notes, releaseUrl = "https://github.com/o/r/releases/tag/v$latest"), required, "0.1.0")

  private fun shown() = shadowOf(nm).allNotifications

  @Test fun shouldNotifyOnlyForANewerVersionNotAnnouncedYet() {
    assertTrue(UpdateNotifier.shouldNotify("0.1.0", "0.2.0", null))
    assertTrue(UpdateNotifier.shouldNotify("0.1.0", "0.2.0", ""))
    assertFalse(UpdateNotifier.shouldNotify("0.1.0", "0.2.0", "0.2.0"))
    assertFalse(UpdateNotifier.shouldNotify("0.1.0", "0.2.0", "0.3.0"))
    assertTrue("an even newer one is announced again", UpdateNotifier.shouldNotify("0.1.0", "0.3.0", "0.2.0"))
    assertFalse(UpdateNotifier.shouldNotify("0.2.0", "0.2.0", null))
    assertFalse(UpdateNotifier.shouldNotify("0.3.0", "0.2.0", null))
    assertFalse(UpdateNotifier.shouldNotify("0.1.0", "junk", null))
  }

  @Test fun aNewVersionPostsOneNotificationOnTheUpdatesChannelAndIsRemembered() = runBlocking {
    assertTrue(UpdateNotifier.announce(context, prefs, available()))
    val n = shown().single()
    assertEquals(NotificationChannels.UPDATES, n.channelId)
    assertEquals("Zohara Agent update available", n.extras.getString(android.app.Notification.EXTRA_TITLE))
    assertEquals("Version 0.2.0 is available (you have 0.1.0). Update checks.", n.extras.getCharSequence(android.app.Notification.EXTRA_TEXT).toString())
    assertEquals("0.2.0", prefs.updateNotified.first())
    // the channel is quiet (low importance), with its own name
    val ch = nm.getNotificationChannel(NotificationChannels.UPDATES)
    assertEquals(NotificationManager.IMPORTANCE_LOW, ch.importance)
    assertEquals("App updates", ch.name.toString())
  }

  @Test fun theSameVersionIsNeverAnnouncedTwiceButANewerOneIs() = runBlocking {
    assertTrue(UpdateNotifier.announce(context, prefs, available()))
    assertFalse(UpdateNotifier.announce(context, prefs, available()))
    assertFalse(UpdateNotifier.announce(context, prefs, available()))
    assertEquals(1, shown().size)
    assertTrue(UpdateNotifier.announce(context, prefs, available(latest = "0.3.0")))
    // the second one replaces the first (same id): still one on screen
    assertEquals(1, shown().size)
    assertEquals("0.3.0", prefs.updateNotified.first())
  }

  @Test fun tappingItOpensTheAboutPage() = runBlocking {
    UpdateNotifier.announce(context, prefs, available())
    val intent = shadowOf(shown().single().contentIntent).savedIntent
    assertEquals("zohara-agent://about", intent.data.toString())
    assertEquals(MainActivity::class.java.name, intent.component?.className)
    assertEquals(org.zohara.agent.nav.DeepLinks.ABOUT, org.zohara.agent.nav.DeepLinks.pendingFrom(intent))
  }

  @Test fun anUnsupportedVersionSaysRequired() = runBlocking {
    UpdateNotifier.announce(context, prefs, available(required = true))
    assertEquals("Zohara Agent update required", shown().single().extras.getString(android.app.Notification.EXTRA_TITLE))
  }

  @Test fun upToDateOrUnknownPostsNothing() = runBlocking {
    assertFalse(UpdateNotifier.announce(context, prefs, UpdateOutcome.UpToDate("0.2.0", LatestRelease("0.2.0"), null)))
    assertFalse(UpdateNotifier.announce(context, prefs, UpdateOutcome.Unknown("no release", null)))
    assertTrue(shown().isEmpty())
  }

  @Test fun withoutThePermissionNothingIsPostedAndTheVersionIsNotUsedUp() = runBlocking {
    shadowOf(context).denyPermissions(Manifest.permission.POST_NOTIFICATIONS)
    assertFalse(UpdateNotifier.announce(context, prefs, available()))
    assertTrue(shown().isEmpty())
    assertEquals("", prefs.updateNotified.first())
    shadowOf(context).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
    assertTrue("announced once it is allowed", UpdateNotifier.announce(context, prefs, available()))
    assertNotNull(shown().singleOrNull())
  }
}
