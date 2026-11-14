package org.zohara.agent.update

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.CoroutineWorker
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import org.zohara.agent.BuildConfig
import org.zohara.agent.ZoApplication
import org.zohara.agent.core.store.ZoPrefs

/** What the worker needs; tests replace [UpdateWorkerEnv.provider]. */
class UpdateDeps(val prefs: ZoPrefs, val checker: UpdateChecker)

object UpdateWorkerEnv {
  @Volatile var provider: (Context) -> UpdateDeps? = { c ->
    (c.applicationContext as? ZoApplication)?.container?.let { UpdateDeps(it.prefs, UpdateChecker(UpdateApi(serverUrl = { it.serverUrl }), BuildConfig.VERSION_NAME)) }
  }
}

/**
 * The periodic check: asks the server for the newest release and posts ONE notification per new version. It never downloads or installs
 * anything. When "Check for updates automatically" is off it does nothing (and the scheduler normally has cancelled it already).
 */
class UpdateWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
  override suspend fun doWork(): Result {
    val deps = UpdateWorkerEnv.provider(applicationContext) ?: return Result.success()
    if (!deps.prefs.updateAutoNow()) return Result.success()
    val outcome = try {
      deps.checker.check()
    } catch (e: UpdateCheckException) {
      // a failed check is tried again soon, twice at most; after that it simply waits for the next period
      return if (runAttemptCount < MAX_RETRIES) Result.retry() else Result.success()
    }
    UpdateNotifier.announce(applicationContext, deps.prefs, outcome)
    return Result.success()
  }

  companion object { const val MAX_RETRIES = 2 }
}

/** Starts and stops the periodic check. Work survives a restart of the phone (WorkManager keeps it in its own database and re-schedules it after boot). */
object UpdateScheduler {
  const val UNIQUE_NAME = "zohara-agent-update-check"
  const val PERIOD_HOURS = 12L
  const val FLEX_HOURS = 3L

  /** Every ~12 hours, only with a network and a battery that is not low; the system picks the moment (Doze friendly). */
  fun request() = PeriodicWorkRequestBuilder<UpdateWorker>(PERIOD_HOURS, TimeUnit.HOURS, FLEX_HOURS, TimeUnit.HOURS)
    .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).setRequiresBatteryNotLow(true).build())
    .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.MINUTES)
    .addTag(UNIQUE_NAME)
    .build()

  /** On: keep the existing schedule (do not push it back at every start); off: cancel it. */
  fun apply(context: Context, enabled: Boolean) {
    val wm = WorkManager.getInstance(context)
    if (enabled) wm.enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request()) else wm.cancelUniqueWork(UNIQUE_NAME)
  }
}
