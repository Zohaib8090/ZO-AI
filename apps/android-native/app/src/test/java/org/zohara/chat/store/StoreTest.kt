package org.zohara.chat.core.store

import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.zohara.chat.core.design.ThemeChoice

class StoreTest {
  @get:Rule val tmp = TemporaryFolder()

  private fun prefs(): ZoPrefs {
    val ds = PreferenceDataStoreFactory.create(scope = CoroutineScope(Dispatchers.IO)) { File(tmp.root, "p${System.nanoTime()}.preferences_pb") }
    return ZoPrefs(ds, "https://default.example")
  }

  @Test fun defaults() = runBlocking {
    val p = prefs()
    assertEquals(ThemeChoice.System, p.theme.first())
    assertEquals("https://default.example", p.serverUrl.first())
    assertEquals(true, p.notifAnswers.first())
    assertEquals(true, p.notifNeedsYou.first())
    assertEquals(false, p.notifDetails.first())
    assertEquals(true, p.runOn.first().isCloud)
    assertEquals(1f, p.speechRate.first())
  }

  @Test fun valuesRoundTripAndReset() = runBlocking {
    val p = prefs()
    p.setTheme(ThemeChoice.Light); assertEquals(ThemeChoice.Light, p.theme.first())
    p.setServerUrl("https://mine.example"); assertEquals("https://mine.example", p.serverUrl.first())
    p.setServerUrl(""); assertEquals("https://default.example", p.serverUrl.first())
    p.setRunOn(RunOn("dev1")); assertEquals("dev1", p.runOn.first().device)
    p.setRunOn(RunOn.Cloud); assertNull(p.runOn.first().device)
    p.setSpeechRate(9f); assertEquals(2f, p.speechRate.first())
    p.setNotifDetails(true); assertEquals(true, p.notifDetails.first())
    p.setPushRegistrationId("abc"); p.clearAccountData(); assertEquals("", p.pushRegistrationId.first())
  }

  @Test fun ttlCacheExpiresAfterThirtySeconds() = runBlocking {
    var now = 0L
    val c = TtlCache<String, Int>(clock = { now })
    c.put("a", 1)
    now = 29_999; assertEquals(1, c.get("a"))
    now = 30_000; assertNull(c.get("a"))
    var loads = 0
    now = 100_000
    assertEquals(5, c.getOrLoad("b") { loads++; 5 })
    assertEquals(5, c.getOrLoad("b") { loads++; 6 })
    assertEquals(1, loads)
    assertEquals(7, c.getOrLoad("b", force = true) { loads++; 7 })
    assertEquals(2, loads)
  }
}
