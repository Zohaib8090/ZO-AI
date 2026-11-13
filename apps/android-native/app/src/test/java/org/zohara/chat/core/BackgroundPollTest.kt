package org.zohara.chat.core

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.zohara.chat.pages.RefreshWhileVisible

/** Timers pause while the app is in the background (NET-10, BG-04): the lists, the activity badge and the link status poll all use this rule. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class BackgroundPollTest {
  @get:Rule val rule = createComposeRule()

  private class Owner : LifecycleOwner {
    val registry = LifecycleRegistry.createUnsafe(this)
    override val lifecycle: Lifecycle get() = registry
  }

  @Test fun noPollsWhileTheAppIsInTheBackgroundAndOneOnReturn() {
    val owner = Owner()
    owner.registry.currentState = Lifecycle.State.RESUMED
    var n = 0
    rule.mainClock.autoAdvance = false
    rule.setContent { CompositionLocalProvider(LocalLifecycleOwner provides owner) { RefreshWhileVisible(15_000) { n++ } } }
    rule.mainClock.advanceTimeByFrame()
    assertEquals("once on entry", 1, n)
    rule.mainClock.advanceTimeBy(15_001)
    assertEquals("then every 15 s", 2, n)

    rule.runOnUiThread { owner.registry.currentState = Lifecycle.State.CREATED } // the app goes to the background
    rule.mainClock.advanceTimeBy(120_000)
    assertEquals("nothing while it is away", 2, n)

    rule.runOnUiThread { owner.registry.currentState = Lifecycle.State.RESUMED } // and comes back
    rule.mainClock.advanceTimeByFrame()
    assertEquals("one at once on return", 3, n)
    rule.mainClock.advanceTimeBy(15_001)
    assertEquals(4, n)
  }
}
