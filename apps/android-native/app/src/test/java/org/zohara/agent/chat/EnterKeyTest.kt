package org.zohara.agent.chat

import android.view.KeyEvent
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EnterKeyTest {
  @Test fun hardwareEnterSends() = assertTrue(enterSends(shift = false, keyFlags = 0))

  @Test fun hardwareShiftEnterIsANewLine() = assertFalse(enterSends(shift = true, keyFlags = 0))

  @Test fun theOnScreenKeyboardEnterIsANewLine() {
    assertFalse(enterSends(shift = false, keyFlags = KeyEvent.FLAG_SOFT_KEYBOARD))
    assertFalse(enterSends(shift = false, keyFlags = KeyEvent.FLAG_SOFT_KEYBOARD or KeyEvent.FLAG_KEEP_TOUCH_MODE))
  }
}
