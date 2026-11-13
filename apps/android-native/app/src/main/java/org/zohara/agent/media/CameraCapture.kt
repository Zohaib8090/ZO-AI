package org.zohara.agent.media

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.MediaStore
import androidx.core.content.FileProvider
import java.io.File

/** Take a photo with the camera app (ATT-17): the result goes into a file of the app's cache through a FileProvider, so no camera permission is asked (ATT-25). */
object CameraCapture {
  fun authority(context: Context) = "${context.packageName}.media.fileprovider"

  /** the phone has a camera app (ATT-28: "Take a photo" is only offered then) */
  fun available(context: Context): Boolean {
    val pm = context.packageManager
    if (!pm.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)) return false
    return Intent(MediaStore.ACTION_IMAGE_CAPTURE).resolveActivity(pm) != null
  }

  /** a new empty file in `cache/camera` and its content uri */
  fun newTarget(context: Context): Pair<File, Uri> {
    val dir = File(context.cacheDir, "camera").apply { mkdirs() }
    dir.listFiles()?.filter { System.currentTimeMillis() - it.lastModified() > 24 * 3600 * 1000L }?.forEach { it.delete() }
    val f = File.createTempFile("zo-photo-", ".jpg", dir)
    return f to FileProvider.getUriForFile(context, authority(context), f)
  }
}
