package org.zohara.zoai.link

import java.util.WeakHashMap
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.zohara.zoai.AppContainer
import org.zohara.zoai.core.net.ClaudeStatus

/**
 * The link part of the app, made once per [AppContainer] (`LinkModule.of(container)`): the linked computers, the Run on choice, the desktop model controls.
 * Screens that show Run on put [LinkPresence] in their composition (it polls every 20 s while visible). See docs/MEDIA-LINK.md.
 */
class LinkModule private constructor(val container: AppContainer) {
  private val scope = container.scope

  val repo = LinkRepository({ container.api.linkStatus() })

  val runOn = RunOnController(repo, PrefsRunOnStore(container.prefs, container.app), scope) { s -> scope.launch { container.toast.show(s) } }

  /** the model and mode controls of the chosen computer (MODEL-32) */
  val desktop = DesktopOps({ device, inner -> container.api.desktopOp(device, inner) }, { runOn.choice.value.device }) { scope.launch { runOn.refresh() } }

  init {
    // signing out forgets the choice (the stored one is cleared by the session); a plain start must not
    scope.launch {
      var wasIn = false
      container.auth.state.collect { st ->
        val isIn = st is org.zohara.zoai.core.auth.AuthState.SignedIn
        if (wasIn && !isIn) runOn.reset()
        wasIn = isIn
      }
    }
  }

  private val _claude = MutableStateFlow<ClaudeStatus?>(null)
  /** the Claude Code status for admins (LINK-CC); null until loaded or when it could not be read */
  val claude: StateFlow<ClaudeStatus?> = _claude.asStateFlow()

  /** reads `claude_status` (admins only: the card shows only when [ClaudeStatus.admin]) */
  suspend fun loadClaude(): ClaudeStatus? {
    val s = try { container.api.claudeStatus() } catch (_: Exception) { null }
    _claude.value = s
    return s
  }

  companion object {
    private val instances = WeakHashMap<AppContainer, LinkModule>()
    @Synchronized fun of(container: AppContainer): LinkModule = instances.getOrPut(container) { LinkModule(container) }
  }
}
