package org.zohara.chat.core

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.chat.core.net.ZoHttp
import org.zohara.chat.core.util.ZoLog

/** No secret reaches the log (PHN-14) and the network timeouts (NET-08). */
class SecretsAndTimeoutsTest {
  @Test fun tokensKeysAndPrivateKeysAreRedacted() {
    val jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefghijklmnop"
    assertEquals("sent Bearer ***", ZoLog.redact("sent Bearer $jwt"))
    assertFalse(ZoLog.redact("key sk-abcdefgh12345678 end").contains("abcdefgh12345678"))
    assertFalse(ZoLog.redact("""{"key":"sk-proj-0123456789abcdef"}""").contains("0123456789abcdef"))
    assertFalse(ZoLog.redact("""{"password":"hunter2hunter2"}""").contains("hunter2"))
    assertFalse(ZoLog.redact("-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkq\n-----END PRIVATE KEY-----").contains("MIIEvQ"))
    assertFalse(ZoLog.redact("x " + "A".repeat(300)).contains("AAAAAAAAAA"))
    assertEquals("nothing secret here", ZoLog.redact("nothing secret here"))
  }

  @Test fun noCodeWritesToTheAndroidLogExceptThroughZoLog() {
    val root = File("src/main/java")
    assertTrue("run from the module directory", root.isDirectory)
    val offenders = root.walkTopDown().filter { it.isFile && it.extension == "kt" && it.name != "ZoLog.kt" }
      .filter { f -> f.readLines().any { Regex("""\bLog\.(v|d|i|w|e|wtf)\(""").containsMatchIn(it) || it.contains("println(") || it.contains("printStackTrace") } }
      .map { it.path }.toList()
    assertEquals("these files log directly: $offenders", emptyList<String>(), offenders)
  }

  @Test fun noWindowIsMarkedSecureAndNothingIsBackedUp() {
    val main = File("src/main")
    assertFalse("FLAG_SECURE is not used", main.walkTopDown().filter { it.extension == "kt" }.any { it.readText().contains("FLAG_SECURE") })
    val manifest = File(main, "AndroidManifest.xml").readText()
    assertTrue(manifest.contains("""android:allowBackup="false""""))
    assertTrue(manifest.contains("""android:usesCleartextTraffic="false""""))
    val rules = File(main, "res/xml/data_extraction_rules.xml").readText()
    assertTrue(rules.contains("<cloud-backup>") && rules.contains("<device-transfer>") && rules.contains("""domain="sharedpref""""))
    assertFalse("debug-only settings stay out of the main manifest", manifest.contains("networkSecurityConfig"))
    assertTrue(File("src/debug/res/xml/network_security_config_debug.xml").readText().contains("""<base-config cleartextTrafficPermitted="false""""))
  }

  @Test fun timeoutsAreTheOnesOfTheMatrix() { // NET-08
    assertEquals(15L, ZoHttp.CONNECT_SECONDS); assertEquals(190L, ZoHttp.OP_READ_SECONDS); assertEquals(90L, ZoHttp.STREAM_SILENCE_SECONDS)
  }
}
