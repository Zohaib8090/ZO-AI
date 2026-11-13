package org.zohara.agent.nav

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import org.zohara.agent.core.design.ZoTheme

const val DRAWER_SLIDE_MS = 180
/** NAV-07: from this width the sidebar stays beside the content (tablets, folded phones opened up) */
val PermanentDrawerMinWidth = 900.dp

/** The modal navigation drawer's state: progress 0 (closed) to 1 (open), slid in [DRAWER_SLIDE_MS] ms. */
@Stable
class ZoDrawerState(private val scope: CoroutineScope, initiallyOpen: Boolean = false) : DrawerControl {
  val progress = Animatable(if (initiallyOpen) 1f else 0f)
  private var target by mutableStateOf(initiallyOpen)
  override val isOpen: Boolean get() = target
  override fun open() { target = true; scope.launch { progress.animateTo(1f, tween(DRAWER_SLIDE_MS)) } }
  override fun close() { target = false; scope.launch { progress.animateTo(0f, tween(DRAWER_SLIDE_MS)) } }
  suspend fun dragTo(p: Float) { progress.snapTo(p.coerceIn(0f, 1f)) }
  fun settle(velocityOpen: Boolean?) {
    val shouldOpen = velocityOpen ?: (progress.value > 0.5f)
    if (shouldOpen) open() else close()
  }
}

@Composable
fun rememberZoDrawerState(): ZoDrawerState {
  val scope = rememberCoroutineScope()
  return remember { ZoDrawerState(scope) }
}

/**
 * The sidebar as a modal drawer (NAV-05): slides in 180 ms, scrim behind it, swipe from the left edge to open and swipe back or tap the scrim to
 * close, and the back button closes it first. At 900 dp and wider it is a permanent pane beside the content (NAV-07).
 */
@Composable
fun ZoDrawerLayout(
  state: ZoDrawerState,
  drawerContent: @Composable () -> Unit,
  modifier: Modifier = Modifier,
  content: @Composable (permanent: Boolean) -> Unit,
) {
  val c = ZoTheme.colors
  val density = LocalDensity.current
  BoxWithConstraints(modifier.fillMaxSize().background(c.bg)) {
    val permanent = maxWidth >= PermanentDrawerMinWidth
    val panelWidth: Dp = if (permanent) 300.dp else minOf(304.dp, maxWidth * 0.86f)
    if (permanent) {
      Row(Modifier.fillMaxSize()) {
        Box(Modifier.width(panelWidth).fillMaxHeight().background(c.bg)) { drawerContent() }
        Box(Modifier.width(1.dp).fillMaxHeight().background(c.border))
        Box(Modifier.weight(1f).fillMaxHeight()) { content(true) }
      }
    } else {
      val widthPx = with(density) { panelWidth.toPx() }
      val scope = rememberCoroutineScope()
      val p = state.progress.value
      BackHandler(enabled = state.isOpen) { state.close() }
      Box(Modifier.fillMaxSize()) {
        content(false)
        // edge strip: swipe from the left edge to open
        run {
          Box(
            Modifier.fillMaxHeight().width(18.dp).pointerInput(Unit) {
              detectHorizontalDragGestures(
                onDragEnd = { state.settle(null) },
                onDragCancel = { state.settle(null) },
              ) { _, dx -> scope.launch { state.dragTo(state.progress.value + dx / widthPx) } }
            },
          )
        }
        if (p > 0f) {
          Box(
            Modifier.fillMaxSize().graphicsLayer { alpha = p }.background(c.scrim)
              .semantics { contentDescription = "Close the sidebar" }
              .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, role = androidx.compose.ui.semantics.Role.Button) { state.close() },
          )
          Box(
            Modifier.fillMaxHeight().width(panelWidth)
              .offset { IntOffset((-(1f - p) * widthPx).roundToInt(), 0) }
              .background(c.bg)
              .pointerInput(Unit) {
                detectHorizontalDragGestures(onDragEnd = { state.settle(null) }, onDragCancel = { state.settle(null) }) { _, dx ->
                  scope.launch { state.dragTo(state.progress.value + dx / widthPx) }
                }
              },
          ) {
            Row(Modifier.fillMaxSize()) {
              Box(Modifier.weight(1f).fillMaxHeight()) { drawerContent() }
              Box(Modifier.width(1.dp).fillMaxHeight().background(c.border))
            }
          }
        }
      }
    }
  }
}
