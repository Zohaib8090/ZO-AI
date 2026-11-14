package org.zohara.agent.update

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import org.zohara.agent.core.util.openInBrowser

/**
 * Where this install came from decides what "Download" opens: Google Play's own listing for a Play install, the GitHub release page for a
 * sideloaded APK. The app never downloads or installs anything itself.
 */
object InstallSource {
  const val PLAY_STORE = "com.android.vending"

  fun isPlayInstaller(installer: String?): Boolean = installer == PLAY_STORE

  /** The package that installed this app, or null (sideloaded with adb or a file manager that leaves no record, or unknown). */
  fun installerOf(context: Context): String? = try {
    val pm = context.packageManager
    if (Build.VERSION.SDK_INT >= 30) pm.getInstallSourceInfo(context.packageName).installingPackageName
    else @Suppress("DEPRECATION") pm.getInstallerPackageName(context.packageName)
  } catch (_: Exception) { null }

  fun fromPlay(context: Context): Boolean = isPlayInstaller(installerOf(context))

  fun playMarketUri(packageName: String): Uri = Uri.parse("market://details?id=$packageName")
  fun playWebUrl(packageName: String): String = "https://play.google.com/store/apps/details?id=$packageName"

  /** What the Download button opens. */
  sealed interface Target {
    data class Play(val packageName: String) : Target
    data class ReleasePage(val url: String) : Target
  }

  fun targetFor(installer: String?, packageName: String, latest: LatestRelease?): Target =
    if (isPlayInstaller(installer)) Target.Play(packageName) else Target.ReleasePage(UpdateOutcomes.releasePage(latest))

  /** Opens the target: the Play app (or its web page when the Play app is missing), or the release page in a browser tab. */
  fun open(context: Context, target: Target) {
    when (target) {
      is Target.ReleasePage -> openInBrowser(context, target.url)
      is Target.Play -> try {
        context.startActivity(Intent(Intent.ACTION_VIEW, playMarketUri(target.packageName)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
      } catch (_: ActivityNotFoundException) {
        openInBrowser(context, playWebUrl(target.packageName))
      }
    }
  }
}
