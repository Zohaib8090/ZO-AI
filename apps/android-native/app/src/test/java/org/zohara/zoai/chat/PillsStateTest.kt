package org.zohara.zoai.chat

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.zohara.zoai.core.net.ApiException
import org.zohara.zoai.core.net.ErrorKind
import org.zohara.zoai.core.net.Profile
import org.zohara.zoai.core.net.ProfilesResp
import org.zohara.zoai.core.net.StatusInfo
import org.zohara.zoai.core.store.RunOn

@OptIn(ExperimentalCoroutinesApi::class)
class PillsStateTest {
  @Test fun theModelAndModePillsReadTheStatus() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend()
    be.profilesResult = ProfilesResp("p1", listOf(Profile("p1", "Work", "claude-code", "claude-opus-5-5", pinned = listOf("claude-opus-5-5", "claude-sonnet-5-5"))))
    val p = PillsState(be, backgroundScope, FakeEnv())
    p.refresh(); advanceUntilIdle()
    val ui = p.ui.value
    assertEquals("Opus 5.5", ui.modelText)
    assertEquals(PermMode.Manual, ui.mode)
    assertEquals(1, ui.profiles.size)
    be.statusResult = StatusInfo(mode = "Bypass", ready = false)
    p.refresh(); advanceUntilIdle()
    assertEquals("Choose a model", p.ui.value.modelText)
    assertEquals(PermMode.Bypass, p.ui.value.mode)
  }

  @Test fun pickingAPinnedModelSwitchesProfileThenModel() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend()
    be.profilesResult = ProfilesResp("p1", listOf(Profile("p2", "Home", "openai", "gpt-4o", pinned = listOf("gpt-4o"))))
    val p = PillsState(be, backgroundScope, FakeEnv())
    p.refresh(); advanceUntilIdle()
    var err: String? = "unset"
    p.chooseModel("p2", "gpt-4o") { err = it }; advanceUntilIdle()
    assertEquals(listOf("use p2", "select gpt-4o"), be.calls)
    assertEquals(null, err)
  }

  @Test fun modeChangeIsOptimisticAndRollsBackOnRefusal() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend()
    val p = PillsState(be, backgroundScope, FakeEnv())
    p.refresh(); advanceUntilIdle()
    p.setMode(PermMode.Auto); advanceUntilIdle()
    assertEquals(PermMode.Auto, p.ui.value.mode)
    be.modeError = ApiException(ErrorKind.Rejected, "Bypass mode is not available to this account.")
    var err: String? = null
    p.setMode(PermMode.Bypass) { err = it }; advanceUntilIdle()
    assertEquals(PermMode.Auto, p.ui.value.mode)
    assertEquals("Bypass mode is not available to this account.", err)
  }

  @Test fun bypassIsOnlyOfferedToUnrestrictedAccountsInTheCloud() {
    val env = FakeEnv()
    val p = PillsState(FakeBackend(), kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.Unconfined), env)
    assertTrue(p.bypassAllowed())
    env.restricted = true; assertFalse(p.bypassAllowed())
    env.restricted = false; env.run = RunOn("d"); assertFalse(p.bypassAllowed())
  }

  @Test fun onAComputerThePillsComeFromThatComputerOrSayItIsNotReachable() = runTest(UnconfinedTestDispatcher()) {
    val be = FakeBackend()
    val env = FakeEnv().also { it.run = RunOn("d1") }
    val p = PillsState(be, backgroundScope, env)
    p.refresh(); advanceUntilIdle()
    assertEquals("qwen", p.ui.value.modelText)
    assertEquals(PermMode.Auto, p.ui.value.mode)
    be.computerStatusError = ApiException(ErrorKind.Rejected, "Your computer is not reachable.")
    p.refresh(); advanceUntilIdle()
    assertEquals("Computer not reachable", p.ui.value.modelText)
    p.chooseModel(null, "llama") ; advanceUntilIdle()
    assertTrue(be.calls.contains("cselect d1 llama"))
  }

  @Test fun modeTextsAreTheDesktopWords() {
    assertEquals("ZO-AI asks before every change and every command.", PermMode.Manual.text)
    assertEquals("Looking around runs on its own; changes and other commands ask first.", PermMode.Auto.text)
    assertEquals("Nothing asks. Use with care.", PermMode.Bypass.text)
    assertEquals(PermMode.Auto, PermMode.parse("AUTO"))
    assertEquals(PermMode.Manual, PermMode.parse("nonsense"))
  }
}
