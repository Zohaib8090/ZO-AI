package org.zohara.agent.update

import android.Manifest
import android.app.Application
import android.app.NotificationManager
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.test.core.app.ApplicationProvider
import androidx.work.ListenableWorker
import androidx.work.NetworkType
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.testing.TestListenableWorkerBuilder
import androidx.work.testing.WorkManagerTestInitHelper
import java.io.File
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.zohara.agent.core.store.ZoPrefs

/** The periodic check: what the worker does, and the schedule (every ~12 h, network, battery not low, one unique job, stopped by the switch). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = Application::class)
class UpdateWorkerTest {
  @get:Rule val tmp = TemporaryFolder()
  private val context get() = ApplicationProvider.getApplicationContext<Application>()
  private val server = MockWebServer()
  private lateinit var prefs: ZoPrefs
  private val nm get() = context.getSystemService(NotificationManager::class.java)

  private val release = """{"ok":true,"server_version":"0.1.0","latest":{"version":"0.2.0","notes":"Update checks.","release_url":"https://github.com/o/r/releases/tag/v0.2.0"}}"""

  @Before fun setUp() {
    server.start()
    prefs = ZoPrefs(PreferenceDataStoreFactory.create(scope = CoroutineScope(Dispatchers.IO)) { File(tmp.root, "p${System.nanoTime()}.preferences_pb") }, "https://x.example")
    shadowOf(context).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
    val base = server.url("/").toString().trimEnd('/')
    UpdateWorkerEnv.provider = { UpdateDeps(prefs, UpdateChecker(UpdateApi(serverUrl = { base }), "0.1.0")) }
  }

  @After fun tearDown() {
    runCatching { server.shutdown() }
    UpdateWorkerEnv.provider = { null }
  }

  private fun run(attempt: Int = 0): ListenableWorker.Result = runBlocking {
    TestListenableWorkerBuilder<UpdateWorker>(context).setRunAttemptCount(attempt).build().doWork()
  }

  @Test fun aNewReleasePostsOneNotificationAndNothingElse() {
    server.enqueue(MockResponse().setBody(release))
    assertEquals(ListenableWorker.Result.success(), run())
    assertEquals(1, shadowOf(nm).allNotifications.size)
    assertEquals(1, server.requestCount)
    // the next period finds the same release: no second notification
    server.enqueue(MockResponse().setBody(release))
    run()
    assertEquals(1, shadowOf(nm).allNotifications.size)
  }

  @Test fun whenSwitchedOffTheWorkerDoesNotEvenAsk() = runBlocking {
    prefs.setUpdateAuto(false)
    assertEquals(ListenableWorker.Result.success(), run())
    assertEquals(0, server.requestCount)
    assertTrue(shadowOf(nm).allNotifications.isEmpty())
  }

  @Test fun aFailedCheckIsRetriedTwiceThenWaitsForTheNextPeriod() {
    server.enqueue(MockResponse().setResponseCode(500))
    assertEquals(ListenableWorker.Result.retry(), run(attempt = 0))
    server.enqueue(MockResponse().setResponseCode(500))
    assertEquals(ListenableWorker.Result.retry(), run(attempt = 1))
    server.enqueue(MockResponse().setResponseCode(500))
    assertEquals(ListenableWorker.Result.success(), run(attempt = 2))
    assertTrue(shadowOf(nm).allNotifications.isEmpty())
  }

  @Test fun noRelease_noNotification() {
    server.enqueue(MockResponse().setBody("""{"ok":true,"latest":null,"error":"no release has been published yet"}"""))
    assertEquals(ListenableWorker.Result.success(), run())
    assertTrue(shadowOf(nm).allNotifications.isEmpty())
  }

  @Test fun theScheduleIsTwelveHoursNetworkAndBatteryFriendly() {
    val spec = UpdateScheduler.request().workSpec
    assertEquals(TimeUnit.HOURS.toMillis(12), spec.intervalDuration)
    assertEquals(TimeUnit.HOURS.toMillis(3), spec.flexDuration)
    assertEquals(NetworkType.CONNECTED, spec.constraints.requiredNetworkType)
    assertTrue(spec.constraints.requiresBatteryNotLow())
    assertFalse("no charging requirement: it would never run for most people", spec.constraints.requiresCharging())
  }

  @Test fun theSwitchStartsAndStopsOneUniqueJob() {
    WorkManagerTestInitHelper.initializeTestWorkManager(context)
    val wm = WorkManager.getInstance(context)
    UpdateScheduler.apply(context, true)
    val first = wm.getWorkInfosForUniqueWork(UpdateScheduler.UNIQUE_NAME).get().single()
    assertEquals(WorkInfo.State.ENQUEUED, first.state)
    // started again at the next app start: the same job, not a second one and not pushed back
    UpdateScheduler.apply(context, true)
    assertEquals(listOf(first.id), wm.getWorkInfosForUniqueWork(UpdateScheduler.UNIQUE_NAME).get().map { it.id })
    UpdateScheduler.apply(context, false)
    assertEquals(WorkInfo.State.CANCELLED, wm.getWorkInfosForUniqueWork(UpdateScheduler.UNIQUE_NAME).get().single().state)
  }

  @Test fun theScheduleFollowsTheSavedSwitch() = runBlocking {
    val seen = java.util.concurrent.CopyOnWriteArrayList<Boolean>()
    val scope = CoroutineScope(Dispatchers.IO)
    watchUpdateSchedule(scope, context, prefs) { _, on -> seen.add(on) }
    waitFor { seen.size == 1 }
    assertEquals(listOf(true), seen.toList()) // on by default
    prefs.setUpdateAuto(false)
    waitFor { seen.size == 2 }
    assertEquals(listOf(true, false), seen.toList())
    assertFalse(prefs.updateAuto.first())
  }

  private fun waitFor(f: () -> Boolean) {
    val end = System.currentTimeMillis() + 5000
    while (!f() && System.currentTimeMillis() < end) Thread.sleep(20)
    assertTrue("timed out", f())
  }
}
