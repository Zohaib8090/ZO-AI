package org.zohara.zoai.media.notify

import android.app.Notification
import android.app.NotificationManager
import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.util.NotificationChannels

/** NOTIF-01, NOTIF-04, NOTIF-06, NOTIF-11, NOTIF-14, INTER-12: what is shown, when, and what the push payload may carry. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class NotifyTest {
  private val context get() = ApplicationProvider.getApplicationContext<android.app.Application>()
  private class Posted(val tag: String, val id: Int, val n: Notification)
  private val posted = ArrayList<Posted>()
  private val settings = MutableStateFlow(NotifySettings())
  private var fg = false
  private var perm = true

  private fun notifier() = ZoNotifier(context, settings, { fg }, { perm }, { tag, id, n -> posted.add(Posted(tag, id, n)) })
  private fun Notification.title() = extras.getString(Notification.EXTRA_TITLE)
  private fun Notification.text() = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString()

  @Before fun channels() { NotificationChannels.create(context) }

  // ---------------------------------------------------------------- text

  @Test fun theDefaultTextsSayNothingAboutTheContent() {
    assertEquals("ZO-AI" to "Your answer is ready", NotifyText.answer(false, "Secret plans").let { it.title to it.body })
    assertEquals("Secret plans" to "ZO-AI: your answer is ready", NotifyText.answer(true, "Secret plans").let { it.title to it.body })
    assertEquals("ZO-AI" to "Your answer is ready", NotifyText.answer(true, "").let { it.title to it.body })
    assertEquals("ZO-AI needs your decision", NotifyText.approval(false, "rm -rf /").body)
    assertEquals("ZO-AI needs your decision", NotifyText.question(false, "What is your password?").body)
    assertEquals("rm -rf /", NotifyText.approval(true, "rm -rf /").body)
    assertEquals("ZO-AI needs you", NotifyText.approval(true, "x").title)
    assertEquals("Which one?", NotifyText.question(true, "Which one?").body)
  }

  @Test fun titlesAreCutAndCleaned() {
    val t = NotifyText.answer(true, "a".repeat(100) + "\u0007")
    assertTrue(t.title.length <= 60 && t.title.endsWith("…"))
  }

  // ---------------------------------------------------------------- answers

  @Test fun anAnswerFinishedInTheBackgroundShowsOneNoticePerChat() {
    val n = notifier()
    assertTrue(n.answerFinished("c123", "My chat"))
    assertTrue(n.answerFinished("c123", "My chat"))
    assertEquals(2, posted.size)
    assertEquals(posted[0].tag, posted[1].tag) // the same tag and id: the second replaces the first
    assertEquals("zo-chat-c123", posted[0].tag)
    assertEquals(posted[0].id, posted[1].id)
    val note = posted[0].n
    assertEquals(NotificationChannels.ANSWERS, note.channelId)
    assertEquals("ZO-AI", note.title())
    assertEquals("Your answer is ready", note.text())
    assertNotNull(note.contentIntent)
  }

  @Test fun theChatTitleIsOnlyUsedWhenTheChoiceIsOn() {
    settings.value = NotifySettings(showTitle = true)
    notifier().answerFinished("c1", "Trip to Lahore")
    assertEquals("Trip to Lahore", posted.single().n.title())
    assertEquals("ZO-AI: your answer is ready", posted.single().n.text())
  }

  @Test fun nothingIsShownWhileTheAppIsVisibleOrTheChoiceIsOffOrThereIsNoPermission() {
    fg = true; assertFalse(notifier().answerFinished("c1", null))
    fg = false; perm = false; assertFalse(notifier().answerFinished("c1", null))
    perm = true; settings.value = NotifySettings(answers = false); assertFalse(notifier().answerFinished("c1", null))
    assertTrue(posted.isEmpty())
  }

  @Test fun anInvalidChatIdIsDropped() {
    assertFalse(notifier().answerFinished("../etc", null))
    assertFalse(notifier().answerFinished("", null))
    assertTrue(posted.isEmpty())
  }

  @Test fun silentWhenSoundIsOff() {
    settings.value = NotifySettings(sound = false)
    notifier().answerFinished("c1", null)
    assertTrue(posted.single().n.flags and Notification.FLAG_ONLY_ALERT_ONCE != 0)
  }

  // ---------------------------------------------------------------- needs you

  @Test fun anApprovalHasAllowAndDenyAndOnlyTheDefaultText() {
    notifier().approval("c9", 77, 5, "Run: rm -rf /tmp/x")
    val note = posted.single().n
    assertEquals(NotificationChannels.NEEDS_YOU, note.channelId)
    assertEquals("ZO-AI needs your decision", note.text())
    assertEquals(listOf("Allow", "Deny"), note.actions.map { it.title.toString() })
    assertEquals("zo-need-c9", posted.single().tag)
  }

  @Test fun withDetailsTheSummaryIsShown() {
    settings.value = NotifySettings(details = true)
    notifier().approval("c9", 77, 5, "Run: ls")
    assertEquals("Run: ls", posted.single().n.text())
    posted.clear()
    notifier().question("c9", 77, 6, "Which file?")
    assertEquals("Which file?", posted.single().n.text())
    assertEquals(listOf("Answer"), posted.single().n.actions.map { it.title.toString() })
  }

  @Test fun theQuestionNoticeOpensTheChatWithTheQuestion() {
    notifier().question("c9", 77, 6, "Which file?")
    val pi = posted.single().n.contentIntent
    val intent = org.robolectric.Shadows.shadowOf(pi).savedIntent
    assertEquals("zoai://chat/c9", intent.data.toString())
    assertEquals("c9", intent.getStringExtra("chat"))
    assertEquals("6", intent.getStringExtra(ZoNotifier.EXTRA_QUESTION))
    assertEquals(org.zohara.zoai.MainActivity::class.java.name, intent.component?.className)
  }

  @Test fun theAllowButtonCarriesTheStreamAndTheApprovalId() {
    notifier().approval("c9", 77, 5, "x")
    val allow = posted.single().n.actions[0].actionIntent
    val i = org.robolectric.Shadows.shadowOf(allow).savedIntent
    assertEquals(ApprovalReceiver.ACTION, i.action)
    assertEquals(77L, i.getLongExtra("stream", 0)); assertEquals(5L, i.getLongExtra("id", -1)); assertTrue(i.getBooleanExtra("yes", false))
    val deny = org.robolectric.Shadows.shadowOf(posted.single().n.actions[1].actionIntent).savedIntent
    assertFalse(deny.getBooleanExtra("yes", true))
    assertEquals(ApprovalReceiver::class.java.name, i.component?.className)
  }

  @Test fun needsYouRespectsItsOwnSwitchAndTheForeground() {
    settings.value = NotifySettings(needsYou = false); assertFalse(notifier().approval("c1", 1, 1, "x"))
    settings.value = NotifySettings(); fg = true; assertFalse(notifier().question("c1", 1, 1, "x"))
    assertTrue(posted.isEmpty())
  }

  // ---------------------------------------------------------------- FCM data

  @Test fun anAnswerPushBuildsTheNoticeFromTheDataOnly() {
    val n = notifier()
    assertTrue(n.fromPush(mapOf("type" to "answer", "chat" to "c55", "title" to "Hidden unless on")))
    assertEquals("ZO-AI", posted.single().n.title()) // the title choice is off
    posted.clear()
    settings.value = NotifySettings(showTitle = true)
    n.fromPush(mapOf("type" to "answer", "chat" to "c55", "title" to "Plan"))
    assertEquals("Plan", posted.single().n.title())
  }

  @Test fun badPushDataIsDropped() {
    val n = notifier()
    assertFalse(n.fromPush(mapOf("type" to "answer", "chat" to "bad id!")))
    assertFalse(n.fromPush(mapOf("type" to "mystery", "chat" to "c1")))
    assertFalse(n.fromPush(mapOf("type" to "answer")))
    assertTrue(posted.isEmpty())
  }

  @Test fun aNeedsYouPushIsGenericWithTheChatOnly() {
    notifier().fromPush(mapOf("type" to "needs_you", "chat" to "c8", "title" to "secret"))
    assertEquals("ZO-AI needs your decision", posted.single().n.text())
    assertEquals(NotificationChannels.NEEDS_YOU, posted.single().n.channelId)
  }

  @Test fun theTestNoticeShowsEvenInTheForeground() {
    fg = true
    assertTrue(notifier().test())
    perm = false
    assertFalse(notifier().test())
  }

  @Test fun theChannelsAreCreatedWithTheRightImportance() {
    val nm = context.getSystemService(NotificationManager::class.java)
    assertEquals(NotificationManager.IMPORTANCE_DEFAULT, nm.getNotificationChannel("answers").importance)
    assertEquals(NotificationManager.IMPORTANCE_HIGH, nm.getNotificationChannel("needs_you").importance)
    assertEquals("Answers", nm.getNotificationChannel("answers").name.toString())
    assertEquals("Needs you", nm.getNotificationChannel("needs_you").name.toString())
  }

  // ---------------------------------------------------------------- the buttons

  @Test fun allowAndDenyAreSentToTheServerAndTheNoticeSaysWhatHappened() = runBlocking {
    val calls = ArrayList<Triple<Long, Long, Boolean>>()
    assertEquals("Allowed.", ApprovalActions.send({ s, i, y -> calls.add(Triple(s, i, y)); true }, 77, 5, true))
    assertEquals("Denied.", ApprovalActions.send({ _, _, _ -> true }, 77, 5, false))
    assertEquals(listOf(Triple(77L, 5L, true)), calls)
    assertEquals(ApprovalActions.NOT_WAITING, ApprovalActions.send({ _, _, _ -> false }, 1, 1, true))
    assertEquals(ApprovalActions.ON_COMPUTER, ApprovalActions.send({ _, _, _ -> throw ApiException(ErrorKind.Rejected, "approvals_on_computer", 409) }, 1, 1, true))
    assertEquals("Could not send your answer: Offline. Retrying.", ApprovalActions.send({ _, _, _ -> throw ApiException(ErrorKind.Offline, "Offline. Retrying.") }, 1, 1, true))
    assertNull(null)
  }
}

/** NOTIF-02: a finished answer leaves a notification (the launcher shows its dot) until the app is back on screen, then it is closed. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class NotifyBadgeTest {
  private val context get() = ApplicationProvider.getApplicationContext<android.app.Application>()

  @Test fun theNoticeStaysUntilTheAppComesBackAndThenIsClosed() {
    NotificationChannels.create(context)
    val nm = context.getSystemService(NotificationManager::class.java)
    val n = ZoNotifier(context, MutableStateFlow(NotifySettings()), { false }, { true })
    assertTrue(n.answerFinished("c1", null))
    assertEquals(1, org.robolectric.Shadows.shadowOf(nm).allNotifications.size)
    n.cancelAll() // what the module does when the app becomes visible
    assertEquals(0, org.robolectric.Shadows.shadowOf(nm).allNotifications.size)
  }

  @Test fun comingBackToTheAppClosesOpenNotices() {
    NotificationChannels.create(context)
    org.robolectric.Shadows.shadowOf(context).grantPermissions(android.Manifest.permission.POST_NOTIFICATIONS)
    val container = org.zohara.zoai.AppContainer(context)
    val module = org.zohara.zoai.media.MediaModule.of(container)
    val nm = context.getSystemService(NotificationManager::class.java)
    AppVisibility.set(false)
    module.install()
    org.robolectric.shadows.ShadowLooper.idleMainLooper()
    assertTrue(module.notifier.answerFinished("c2", null))
    assertEquals(1, org.robolectric.Shadows.shadowOf(nm).allNotifications.size)
    AppVisibility.set(true)
    org.robolectric.shadows.ShadowLooper.idleMainLooper()
    assertEquals(0, org.robolectric.Shadows.shadowOf(nm).allNotifications.size)
  }
}
