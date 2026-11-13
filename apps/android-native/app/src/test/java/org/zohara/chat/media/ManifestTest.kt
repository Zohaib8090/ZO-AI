package org.zohara.chat.media

import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.zohara.chat.media.notify.NotificationAskText
import org.zohara.chat.media.notify.shouldAskAboutNotifications

/** ATT-22, ATT-25, PHN-08, VOICE-08, NOTIF-14, INTER-15: what the merged manifest promises. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class ManifestTest {
  private val context get() = ApplicationProvider.getApplicationContext<android.app.Application>()
  private val pm get() = context.packageManager

  @Test fun noBroadStoragePermissionAndNoCameraPermission() { // ATT-25, ATT-17
    val asked = pm.getPackageInfo(context.packageName, PackageManager.GET_PERMISSIONS).requestedPermissions.orEmpty().toSet()
    for (bad in listOf("android.permission.READ_EXTERNAL_STORAGE", "android.permission.WRITE_EXTERNAL_STORAGE", "android.permission.READ_MEDIA_IMAGES", "android.permission.MANAGE_EXTERNAL_STORAGE", "android.permission.CAMERA")) {
      assertFalse(bad, bad in asked)
    }
    assertTrue("android.permission.RECORD_AUDIO" in asked) // VOICE-08: asked at run time with an explanation
    assertTrue("android.permission.POST_NOTIFICATIONS" in asked)
    assertTrue("android.permission.INTERNET" in asked)
  }

  private fun sharers(action: String, type: String) = pm.queryIntentActivities(Intent(action).setType(type).addCategory(Intent.CATEGORY_DEFAULT), 0).map { it.activityInfo.name }

  @Test fun otherAppsCanShareTextPicturesAndFilesWithZoAi() { // ATT-22, PHN-08
    val share = ShareActivity::class.java.name
    for (t in listOf("text/plain", "image/png", "image/jpeg", "application/pdf", "application/zip", "application/json", "text/markdown")) {
      assertTrue("SEND $t", share in sharers(Intent.ACTION_SEND, t))
      assertTrue("SEND_MULTIPLE $t", share in sharers(Intent.ACTION_SEND_MULTIPLE, t))
    }
    assertFalse(share in sharers(Intent.ACTION_SEND, "video/mp4"))
  }

  @Test fun theShareActivityIsExportedAndAServiceTakesThePush() { // NOTIF-14
    val a = pm.getActivityInfo(ComponentName(context, ShareActivity::class.java), 0)
    assertTrue(a.exported)
    val svc = pm.queryIntentServices(Intent("com.google.firebase.MESSAGING_EVENT").setPackage(context.packageName), 0)
    assertEquals("org.zohara.chat.media.notify.ZoMessagingService", svc.first().serviceInfo.name) // ours first: the library's own service comes after it
    val app = pm.getApplicationInfo(context.packageName, PackageManager.GET_META_DATA)
    assertEquals("answers", app.metaData.getString("com.google.firebase.messaging.default_notification_channel_id"))
  }

  @Test fun theCameraWritesThroughAFileProvider() { // ATT-17
    val p = pm.resolveContentProvider(context.packageName + ".media.fileprovider", 0)
    assertTrue(p != null && !p.exported && p.grantUriPermissions)
  }

  @Test fun theNotificationQuestionIsAskedOnceAfterSignIn() { // INTER-15
    assertTrue(shouldAskAboutNotifications(alreadyAsked = false, granted = false))
    assertFalse(shouldAskAboutNotifications(alreadyAsked = true, granted = false))
    assertFalse(shouldAskAboutNotifications(alreadyAsked = false, granted = true))
    assertTrue(NotificationAskText.BODY.contains("never contain your messages"))
  }
}
