package org.zohara.agent.update

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.zohara.agent.core.store.ZoPrefs

/** The state of the "Check for updates" button on the About page. */
sealed interface CheckState {
  data object Idle : CheckState
  data object Checking : CheckState
  data class Done(val outcome: UpdateOutcome) : CheckState
  data class Failed(val message: String) : CheckState
}

/** Runs the manual check of the About page and the "automatically" switch. A manual check never posts a notification (the result is on screen). */
class UpdateController(
  private val scope: CoroutineScope,
  private val checker: UpdateChecker,
  private val prefs: ZoPrefs,
) {
  private val _state = MutableStateFlow<CheckState>(CheckState.Idle)
  val state: StateFlow<CheckState> = _state

  fun check() {
    if (_state.value == CheckState.Checking) return
    _state.value = CheckState.Checking
    scope.launch {
      _state.value = try {
        CheckState.Done(checker.check())
      } catch (e: UpdateCheckException) {
        CheckState.Failed("Could not check for updates: ${e.message}.")
      }
    }
  }

  /** Saves the switch. The schedule follows the saved value ([watchUpdateSchedule]), so there is one place that starts and stops the work. */
  fun setAuto(on: Boolean) {
    scope.launch { prefs.setUpdateAuto(on) }
  }
}

/** Keeps the periodic check in step with the saved switch: at start and whenever it changes. Never throws: a broken WorkManager must not stop the app. */
fun watchUpdateSchedule(scope: CoroutineScope, context: Context, prefs: ZoPrefs, apply: (Context, Boolean) -> Unit = UpdateScheduler::apply) {
  scope.launch {
    prefs.updateAuto.collect { on -> try { apply(context, on) } catch (_: Exception) { } }
  }
}
