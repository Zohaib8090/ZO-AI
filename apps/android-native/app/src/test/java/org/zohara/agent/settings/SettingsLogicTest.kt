package org.zohara.agent.settings

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.agent.core.net.AccountDevice
import org.zohara.agent.core.net.Limits
import org.zohara.agent.core.net.McpServer
import org.zohara.agent.core.net.Machine
import org.zohara.agent.core.net.MemoryItem
import org.zohara.agent.core.net.MemoryResp
import org.zohara.agent.core.net.PersonalitiesResp
import org.zohara.agent.core.net.Personality
import org.zohara.agent.core.net.PushSubscription
import org.zohara.agent.core.net.TermsState
import org.zohara.agent.core.net.Whoami
import org.zohara.agent.core.net.DeviceLookup
import org.zohara.agent.media.notify.PushResult
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject

class SettingsLogicTest {
  // ---- SET-01, SET-03
  @Test fun adminSeesTwelvePagesAndRestrictedTen() {
    assertEquals(12, SettingsPages.visible(false).size)
    val r = SettingsPages.visible(true).map { it.id }
    assertEquals(10, r.size)
    assertFalse("apps" in r); assertFalse("machines" in r)
    assertEquals(listOf("model", "personality", "memory", "general"), r.take(4))
  }

  @Test fun anAdminPageFallsBackToModelForRestrictedAndAnUnknownOneToo() {
    assertEquals("model", SettingsPages.resolve("apps", true).id)
    assertEquals("apps", SettingsPages.resolve("apps", false).id)
    assertEquals("model", SettingsPages.resolve("nope", false).id)
  }

  @Test fun everyPageHasItsOwnRoute() {
    assertEquals(SettingsPages.all.size, SettingsPages.all.map { it.route }.toSet().size)
    assertEquals("Model and profiles", SettingsPages.all[0].title)
    assertEquals("My computers", SettingsPages.all.first { it.id == "computers" }.title)
  }

  // ---- formatting
  @Test fun agoLikeTheWeb() {
    val now = 1_000_000_000_000L
    assertEquals("never", formatAgo(0, now))
    assertEquals("just now", formatAgo(now - 20_000, now))
    assertEquals("5 min ago", formatAgo(now - 5 * 60_000, now))
    assertEquals("3 h ago", formatAgo(now - 3 * 3_600_000L, now))
  }

  @Test fun secondsAndMillisBothWork() {
    assertEquals(1_794_509_000_000L, toMillis(1_794_509_000))
    assertEquals(1_794_509_000_000L, toMillis(1_794_509_000_000))
    assertEquals(0, toMillis(0))
  }

  @Test fun humanBytes() {
    assertEquals("0 B", humanBytes(0)); assertEquals("512 B", humanBytes(512)); assertEquals("1.5 KB", humanBytes(1536))
    assertEquals("25 MB", humanBytes(26_214_400)); assertEquals("2.2 KB", humanBytes(2224))
  }

  @Test fun limitsLineUsesTheLabelsOfTheWeb() {
    assertEquals("Requests per minute: 60 · Answers at the same time: 2", limitsText(Limits(60, 2)))
    assertEquals(
      "Requests per minute: 30 · Answers at the same time: 2 · Longest message (characters): 20000 · Storage: 25 MB · Steps per answer: 8 · Longest answer (seconds): 120",
      limitsText(Limits(30, 2, 20000, 26_214_400, 8, 120)),
    )
  }

  @Test fun voiceLanguagesAreTheEighteenOfTheWeb() {
    assertEquals(18, VOICE_LANGS.size)
    assertEquals("" to "Device default", VOICE_LANGS[0])
    assertTrue(VOICE_LANGS.any { it.first == "ur-PK" && it.second == "Urdu" })
    assertTrue(VOICE_LANGS.any { it.first == "hi-IN" })
    assertEquals(VOICE_LANGS.size, VOICE_LANGS.map { it.first }.toSet().size)
  }

  @Test fun permissionModes() {
    assertEquals(listOf("Manual", "Auto"), permissionModes(true))
    assertEquals(listOf("Manual", "Auto", "Bypass"), permissionModes(false))
    assertTrue(permissionModeText(false).contains("Bypass never asks"))
    assertFalse(permissionModeText(true).contains("Bypass"))
  }

  @Test fun claudeLine() {
    assertTrue(claudeLine(false, 0).startsWith("Send your Claude sign-in"))
    assertTrue(claudeLine(true, 0).contains("Sign-in sent earlier."))
    assertTrue(claudeLine(true, 1_794_509_000) { "TODAY" }.contains("Sign-in sent TODAY."))
  }

  // ---- personality
  @Test fun personalityRowsStartWithPlain() {
    val rows = personalityRows(PersonalitiesResp("friend", listOf(Personality("friend", "Friendly buddy", "Sound warm", builtin = true), Personality("p1", "Calm", "Slow", generated = true))))
    assertEquals(listOf("", "friend", "p1"), rows.map { it.id })
    assertFalse(rows[0].inUse); assertTrue(rows[1].inUse)
    assertEquals("Plain", rows[0].name); assertEquals(PLAIN_TEXT, rows[0].sub)
    assertEquals("Calm · written by the AI", rows[2].name)
    assertEquals(listOf(false, false, true), rows.map { it.deletable })
  }

  @Test fun plainIsInUseWhenNothingIsActive() = assertTrue(personalityRows(PersonalitiesResp("", emptyList()))[0].inUse)

  // ---- memory
  private fun mem(id: String, kind: String, updated: Long, pinned: Boolean = false) = MemoryItem(id, kind, "text $id", updated = updated, pinned = pinned, title = if (kind == "episode") "T$id" else "")

  @Test fun memoryGroupsPinnedFirstThenNewest() {
    val g = MemoryLogic.groups(listOf(mem("a", "fact", 1), mem("b", "fact", 5), mem("c", "fact", 2, pinned = true), mem("d", "person", 9), mem("e", "episode", 3)))
    assertEquals(listOf("About you", "Facts", "Chat summaries"), g.map { it.label })
    assertEquals(listOf("c", "b", "a"), g[1].shown.map { it.id })
  }

  @Test fun memoryShowsEightyPerGroupAndCountsTheRest() {
    val items = (1..95).map { mem("f$it", "fact", it.toLong()) } + mem("p", "person", 1)
    val g = MemoryLogic.groups(items)
    assertEquals(80, g.first { it.kind == "fact" }.shown.size)
    assertEquals(15, MemoryLogic.omitted(items))
    assertEquals("f95", g.first { it.kind == "fact" }.shown[0].id)
  }

  @Test fun memoryExplainerCountsNotesAndSummaries() {
    val t = MemoryLogic.explainer(MemoryResp(true, 3, listOf(mem("a", "fact", 1), mem("b", "preference", 1), mem("c", "episode", 1))))
    assertTrue(t.startsWith("2 notes about you and 1 chat summaries, shared with the computer and the phone. After every 3 messages of yours"))
    assertTrue(t.endsWith("Never passwords or keys."))
  }

  // ---- connected apps, machines
  @Test fun mcpFormRules() {
    val ex = listOf(McpServer("exa", "https://exa.ai/mcp"))
    assertEquals("Give it a name and an address or command.", McpLogic.validate("", "https://x.y/mcp", ex))
    assertEquals("A server with that name is already added.", McpLogic.validate("EXA", "https://x.y/mcp", ex))
    assertTrue(McpLogic.validate("local", "npx some-server", ex).contains("https"))
    assertEquals("", McpLogic.validate("new", "https://x.y/mcp", ex))
  }

  @Test fun machineLineAndForm() {
    assertEquals("ubuntu@203.0.113.7:22 · key saved · n", MachineLogic.describe(Machine("vm1", "203.0.113.7", "ubuntu", 22, "n", true)))
    assertEquals("h:2200 · no key", MachineLogic.describe(Machine("a", "h", "", 2200, "", false)))
    assertEquals("", MachineLogic.validate("home-server", "192.168.1.20", "me", "22", "-----BEGIN"))
    assertTrue(MachineLogic.validate("bad name!", "h", "u", "22", "k").contains("letters"))
    assertEquals("The port is a number from 1 to 65535.", MachineLogic.validate("a", "h", "u", "70000", "k"))
    assertEquals("Paste the private key that this machine accepts.", MachineLogic.validate("a", "h", "u", "22", " "))
  }

  // ---- sync
  @Test fun syncCountsKeysAndTakesTheNewestTime() {
    val m = Json.parseToJsonElement("""{"ok":true,"chats":{"a":100,"b":300},"memory":{"m":200},"personalities":{},"profiles":{"p":999999},"now":5}""").jsonObject
    val i = SyncLogic.parse(m, null)
    assertEquals(2, i.chats); assertEquals(1, i.memory); assertEquals(0, i.personalities)
    assertEquals(300, i.newestMs)
    assertNull(i.outward)
    assertEquals("2 chats, 1 memory item and 0 personalities. Last change: 1 min ago.", SyncLogic.storedText(i.copy(newestMs = 1_000_000_000_000L), 1_000_000_000_000L + 60_000))
  }

  @Test fun syncReadsTheOutwardStatus() {
    val st = Json.parseToJsonElement("""{"ok":true,"enabled":true,"url":"https://x.example","last_ok":0,"error":"boom"}""").jsonObject
    val i = SyncLogic.parse(Json.parseToJsonElement("{}").jsonObject, st)
    assertEquals("To https://x.example. Last good sync: never. Problem: boom", SyncLogic.outwardText(i.outward!!))
    assertNull(SyncLogic.parse(Json.parseToJsonElement("{}").jsonObject, Json.parseToJsonElement("""{"enabled":false}""").jsonObject).outward)
  }

  // ---- data, account
  @Test fun dataTexts() {
    assertEquals("1 saved chat, kept on the server.", DataLogic.savedText(1))
    assertEquals("3 saved chats, kept on the server.", DataLogic.savedText(3))
    assertEquals("3 chats deleted", DataLogic.deletedToast(3))
    assertTrue(DataLogic.DELETE_ALL_TEXT.contains("Projects and scheduled tasks stay."))
  }

  @Test fun accountDevicesAndTexts() {
    val d = AccountDevice("1", "My laptop", "desktop", "sync", created = 1_794_509_000, lastUsed = 0)
    assertEquals("My laptop", AccountLogic.deviceName(d))
    assertEquals("Desktop app", AccountLogic.deviceKind(d))
    assertEquals("Terminal", AccountLogic.deviceKind(AccountDevice("2", "", "cli", "full")))
    assertEquals("Terminal", AccountLogic.deviceName(AccountDevice("2", "", "cli", "full")))
    assertEquals("Terminal", AccountLogic.deviceKind(AccountDevice("3", "", "", "full")))
    assertTrue(AccountLogic.deviceLine(d).startsWith("Desktop app · last used never · signed in "))
    val me = Whoami(admin = true, restricted = false, email = "a@b.c", limits = Limits(60, 2, dataBytes = 26_214_400), dataBytes = 2224, terms = TermsState("2026-11-12", true))
    assertEquals("2.2 KB of 25 MB. Chats, memory and personalities.", AccountLogic.dataText(me))
    assertTrue(AccountLogic.consentText(me).contains("accepted the notice and terms of version 2026-11-12"))
  }

  @Test fun exportFileNameAndDeleteTexts() {
    assertEquals("zohara-agent-export-2026-11-13.json", AccountLogic.exportFileName(LocalDate.of(2026, 11, 13)))
    assertEquals("Your account and data were deleted.", AccountLogic.deleteDone(true))
    assertTrue(AccountLogic.deleteDone(false).contains("sign-in record could not be removed"))
    assertTrue(AccountLogic.deleteConfirmed("DELETE"))
    assertFalse(AccountLogic.deleteConfirmed("delete")); assertFalse(AccountLogic.deleteConfirmed("DELETE ")); assertFalse(AccountLogic.deleteConfirmed(""))
  }

  // ---- approve a sign-in
  @Test fun approveCodeFormat() {
    assertEquals("ABCD-EFGH", ApproveLogic.formatCode("abcdefgh"))
    assertEquals("ABCD-12", ApproveLogic.formatCode("abcd 12"))
    assertEquals("ABCD", ApproveLogic.formatCode("ab-cd"))
    assertEquals("ABCD-EFGH", ApproveLogic.formatCode("abcd-efgh-ijkl"))
    assertTrue(ApproveLogic.codeComplete("ABCD-EFGH")); assertFalse(ApproveLogic.codeComplete("ABCD-EFG"))
    assertTrue(ApproveLogic.strict(DeviceLookup(kind = "cli"))); assertTrue(ApproveLogic.strict(DeviceLookup(kind = "desktop", sameNetwork = false)))
    assertFalse(ApproveLogic.strict(DeviceLookup(kind = "desktop")))
    assertEquals("Terminal (full access to your account)", ApproveLogic.kindText("cli"))
  }

  // ---- advanced
  @Test fun serverAddressRules() {
    assertEquals("", AdvancedLogic.validate("https://zo.example.com", false))
    assertEquals(AdvancedLogic.BAD, AdvancedLogic.validate("http://zo.example.com", false))
    assertEquals(AdvancedLogic.BAD, AdvancedLogic.validate("not a url", false))
  }

  // ---- notifications
  @Test fun pushMessages() {
    assertEquals("Background notifications are on", NotifLogic.pushMessage(PushResult.Registered("x"), true))
    assertEquals("Background notifications are off", NotifLogic.pushMessage(PushResult.Off, false))
    assertEquals("Turn on notifications first.", NotifLogic.pushMessage(PushResult.NeedsPermission, true))
    assertEquals("This server cannot send notifications to the app.", NotifLogic.pushMessage(PushResult.ServerCannot, true))
    assertEquals("boom", NotifLogic.pushMessage(PushResult.Failed("boom"), true))
    val s = PushSubscription("i1", "fcm", "Android: Pixel 8", 1_794_509_000)
    assertEquals("Android: Pixel 8 (this device)", NotifLogic.deviceTitle(s, "i1"))
    assertEquals("Android: Pixel 8", NotifLogic.deviceTitle(s, "other"))
    assertEquals("Browser", NotifLogic.deviceTitle(PushSubscription("i2", "webpush", ""), ""))
    assertEquals("Android app", NotifLogic.deviceTitle(PushSubscription("i3", "fcm", ""), ""))
  }

  @Test fun licencesListTheFontsAndLibraries() {
    assertTrue(Licenses.all.any { it.name.startsWith("Inter") && it.license.contains("Open Font") })
    assertTrue(Licenses.all.any { it.name.startsWith("OkHttp") })
    assertEquals("https://x.example/privacy", legalUrl("https://x.example/", "privacy"))
  }
}
