package org.zohara.zoai.pages

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.zohara.zoai.core.net.ChatSummary

@OptIn(ExperimentalCoroutinesApi::class)
class PagesHubRegistryTest {
  @Before fun up() { Dispatchers.setMain(UnconfinedTestDispatcher()) }
  @After fun down() { Dispatchers.resetMain() }

  @Test fun oneHubPerPersonAndNewPersonGetsAFreshOne() {
    val counts = ArrayList<Int>()
    val api = FakeApi().apply { chatList = listOf(ChatSummary("a", title = "mine")) }
    val reg = PagesHubRegistry { s -> PagesHub(api, s) { counts += it } }
    val a1 = reg.get("alice")
    assertSame(a1, reg.get("alice"))
    a1.chats.refresh()
    assertTrue(a1.chats.state.value.value!!.isNotEmpty())
    val b = reg.get("bob")
    assertNotSame(a1, b)
    // the next person never sees the previous person's list
    assertTrue(b.chats.state.value.isFirstLoad)
  }
}
