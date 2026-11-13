package org.zohara.chat.media.notify

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.chat.core.net.ApiException
import org.zohara.chat.core.net.ErrorKind
import org.zohara.chat.core.net.PushConfig
import org.zohara.chat.core.net.PushSubscription

/** NOTIF-03, NOTIF-15, NOTIF-16, NOTIF-17, NOTIF-05, BG-05: registering this phone for push. */
class PushRegistrarTest {
  private class Backend(var fcm: Boolean = true) : PushBackend {
    val subs = LinkedHashMap<String, String>() // id -> token
    val calls = ArrayList<String>()
    var next = 1
    var limit = false
    override suspend fun config() = PushConfig("", fcm)
    override suspend fun subscribe(token: String, label: String, showTitle: Boolean): String {
      calls.add("subscribe:$token:$label:$showTitle")
      if (limit) throw ApiException(ErrorKind.Rejected, "too many", 409)
      val existing = subs.entries.firstOrNull { it.value == token }?.key // the same token replaces its entry and keeps the id
      val id = existing ?: "id${next++}"
      subs[id] = token
      return id
    }
    override suspend fun unsubscribe(id: String) { calls.add("unsubscribe:$id"); subs.remove(id) }
    override suspend fun list() = subs.keys.map { PushSubscription(it, "fcm", "x", 0) }
  }

  private class Tokens(var token: String? = "tok1") : TokenSource {
    var deleted = 0
    override suspend fun token() = token
    override suspend fun deleteToken() { deleted++; token = "tok-renewed-$deleted" }
  }

  private fun rig(perm: Boolean = true, enabled: Boolean = true, backend: Backend = Backend(), tokens: Tokens = Tokens()): Triple<PushRegistrar, Backend, Pair<Tokens, MemoryPushStore>> {
    val store = MemoryPushStore().also { it.enabled = enabled }
    return Triple(PushRegistrar(backend, tokens, store, { perm }, "Android: Pixel"), backend, tokens to store)
  }

  @Test fun registersAfterSignInWithTheLabelAndTheTitleChoice() = runBlocking {
    val (r, b, s) = rig()
    val res = r.sync("uid1", showTitle = true)
    assertEquals(PushResult.Registered("id1"), res)
    assertEquals(listOf("subscribe:tok1:Android: Pixel:true"), b.calls)
    assertEquals("id1", s.second.id); assertEquals("uid1", s.second.uid)
    assertTrue(r.registered.value) // keep_running goes on the sends now
  }

  @Test fun anUnchangedRegistrationIsNotSentAgain() = runBlocking {
    val (r, b, _) = rig()
    r.sync("u", false); r.sync("u", false); r.sync("u", false)
    assertEquals(1, b.calls.count { it.startsWith("subscribe") })
  }

  @Test fun aRegistrationTheServerForgotIsDoneAgain() = runBlocking {
    val (r, b, _) = rig()
    r.sync("u", false)
    b.subs.clear()
    assertEquals(PushResult.Registered("id2"), r.sync("u", false))
  }

  @Test fun changingTheTitleChoiceRegistersAgainAndReplacesTheEntry() = runBlocking {
    val (r, b, _) = rig()
    r.sync("u", false)
    assertEquals(PushResult.Registered("id1"), r.sync("u", true))
    assertEquals(1, b.subs.size)
    assertEquals("subscribe:tok1:Android: Pixel:true", b.calls.last())
  }

  @Test fun nothingHappensWithoutPermissionOrWhenTheSwitchIsOff() = runBlocking {
    val a = rig(perm = false)
    assertEquals(PushResult.NeedsPermission, a.first.sync("u", false)); assertTrue(a.second.calls.isEmpty())
    val b = rig(enabled = false)
    assertEquals(PushResult.Off, b.first.sync("u", false)); assertTrue(b.second.calls.isEmpty())
    assertFalse(b.first.registered.value)
  }

  @Test fun aServerThatCannotSendToTheAppIsRespected() = runBlocking {
    val (r, b, _) = rig(backend = Backend(fcm = false))
    assertEquals(PushResult.ServerCannot, r.sync("u", false))
    assertTrue(b.calls.isEmpty())
  }

  @Test fun noTokenMeansNoRegistration() = runBlocking {
    val (r, _, _) = rig(tokens = Tokens(null))
    assertEquals(PushResult.NoToken, r.sync("u", false))
  }

  @Test fun theFiveDeviceLimitHasASentence() = runBlocking {
    val (r, _, _) = rig(backend = Backend().also { it.limit = true })
    val res = r.sync("u", false) as PushResult.Failed
    assertTrue(res.message, res.message.contains("5 devices"))
  }

  @Test fun anotherPersonOnTheSamePhoneDropsTheOldRegistrationAndRenewsTheToken() = runBlocking {
    val (r, b, s) = rig()
    r.sync("alice", false)
    val res = r.sync("bob", false)
    assertEquals(1, s.first.deleted) // the token that alice registered can not reach bob's phone any more
    assertEquals("bob", s.second.uid)
    assertTrue(res is PushResult.Registered)
    assertTrue(b.calls.last().startsWith("subscribe:tok-renewed-1"))
  }

  @Test fun aNewTokenReplacesTheOldRegistration() = runBlocking {
    val (r, b, s) = rig()
    r.sync("u", false)
    val res = r.onNewToken("u", "tok2", false)
    assertEquals(listOf("subscribe:tok1:Android: Pixel:false", "unsubscribe:id1", "subscribe:tok2:Android: Pixel:false"), b.calls)
    assertTrue(res is PushResult.Registered)
    assertEquals("tok2", s.second.token)
  }

  @Test fun aNewTokenWhileSignedOutOrOffRegistersNothing() = runBlocking {
    val (r, b, _) = rig()
    assertEquals(PushResult.Off, r.onNewToken(null, "tok2", false))
    val c = rig(enabled = false)
    assertEquals(PushResult.Off, c.first.onNewToken("u", "tok2", false))
    assertTrue(b.calls.isEmpty() && c.second.calls.isEmpty())
  }

  @Test fun signOutRemovesTheRegistration() = runBlocking {
    val (r, b, s) = rig()
    r.sync("u", false)
    r.unregister()
    assertEquals("unsubscribe:id1", b.calls.last())
    assertEquals("", s.second.id)
    assertFalse(r.registered.value)
  }

  @Test fun switchingOffRemovesItAndTheChoiceIsKept() = runBlocking {
    val (r, b, s) = rig()
    r.sync("u", false)
    r.setEnabled(false)
    assertEquals(PushResult.Off, r.sync("u", false))
    assertTrue(b.subs.isEmpty())
    assertFalse(s.second.enabled)
    r.setEnabled(true)
    assertEquals(PushResult.Registered("id2"), r.sync("u", false))
  }

  @Test fun removingThisPhoneFromTheListAlsoSwitchesPushOff() = runBlocking {
    val (r, b, s) = rig()
    r.sync("u", false)
    b.subs["other"] = "tokX"
    r.removeDevice("other")
    assertTrue(s.second.enabled)
    r.removeDevice("id1")
    assertFalse(s.second.enabled)
    assertTrue(b.subs.isEmpty())
  }

  @Test fun theDeviceLabelIsAtMost60Characters() {
    assertEquals("Android: Pixel 8", PushRegistrar.deviceLabel("Pixel 8"))
    assertEquals(60, PushRegistrar.deviceLabel("x".repeat(100)).length)
    assertEquals("Android", PushRegistrar.deviceLabel(""))
  }
}
