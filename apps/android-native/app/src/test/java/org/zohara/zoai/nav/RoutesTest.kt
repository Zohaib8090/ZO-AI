package org.zohara.zoai.nav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RoutesTest {
  @Test fun everyRouteHasAScreenAndEveryScreenARoute() {
    assertEquals(Routes.all.map { it.pattern }.toSet(), ScreenRegistry.screens.keys)
    assertEquals(Routes.all.size, Routes.all.map { it.pattern }.toSet().size)
  }

  @Test fun chatIdsAreValidated() {
    assertEquals("abc-123_x", DeepLinks.validChatId("abc-123_x"))
    assertNull(DeepLinks.validChatId("../etc/passwd"))
    assertNull(DeepLinks.validChatId("a".repeat(65)))
    assertNull(DeepLinks.validChatId(null))
  }
}
