package org.zohara.agent.nav

import android.content.Intent
import android.net.Uri
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** NAV-20, NAV-22, AUTH-24: what an intent from a notification, a link or the launcher shortcut asks for. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], application = android.app.Application::class)
class DeepLinksTest {
  private fun view(uri: String) = Intent(Intent.ACTION_VIEW, Uri.parse(uri))

  @Test fun chatLinkAndExtra() {
    assertEquals("c1794509120577", DeepLinks.pendingFrom(view("zohara-agent://chat/c1794509120577")))
    assertEquals("abc-1", DeepLinks.pendingFrom(Intent().putExtra(DeepLinks.EXTRA_CHAT, "abc-1")))
  }

  @Test fun invalidChatIdsAreIgnored() {
    assertNull(DeepLinks.pendingFrom(view("zohara-agent://chat/..%2Fetc")))
    assertNull(DeepLinks.pendingFrom(view("zohara-agent://chat/" + "a".repeat(65))))
    assertNull(DeepLinks.pendingFrom(Intent().putExtra(DeepLinks.EXTRA_CHAT, "a b")))
    assertNull(DeepLinks.pendingFrom(view("https://evil.example/chat/c1")))
    assertNull(DeepLinks.pendingFrom(view("zohara-agent://other/c1")))
    assertNull(DeepLinks.pendingFrom(Intent()))
    assertNull(DeepLinks.pendingFrom(null))
  }

  @Test fun theShortcutAndTheApproveLinkAreNotChatIds() { // NAV-22, AUTH-24
    assertEquals(DeepLinks.NEW_CHAT, DeepLinks.pendingFrom(view("zohara-agent://new")))
    assertEquals(DeepLinks.APPROVE, DeepLinks.pendingFrom(view("zohara-agent://device")))
    assertNull("the special values never pass as a chat id", DeepLinks.validChatId(DeepLinks.NEW_CHAT))
    assertNull(DeepLinks.validChatId(DeepLinks.APPROVE))
    assertNull(DeepLinks.chatIdFrom(view("zohara-agent://new")))
  }

  @Test fun theZoharaChatSchemeStillWorks() {
    assertEquals("c1", DeepLinks.pendingFrom(view("zohara-chat://chat/c1")))
    assertEquals(DeepLinks.NEW_CHAT, DeepLinks.pendingFrom(view("zohara-chat://new")))
    assertEquals(DeepLinks.APPROVE, DeepLinks.pendingFrom(view("zohara-chat://device")))
  }

  @Test fun theOldZoaiSchemeStillWorks() {
    assertEquals("c1", DeepLinks.pendingFrom(view("zoai://chat/c1")))
    assertEquals(DeepLinks.NEW_CHAT, DeepLinks.pendingFrom(view("zoai://new")))
    assertEquals(DeepLinks.APPROVE, DeepLinks.pendingFrom(view("zoai://device")))
    assertNull(DeepLinks.pendingFrom(view("https://chat/c1")))
  }
}
