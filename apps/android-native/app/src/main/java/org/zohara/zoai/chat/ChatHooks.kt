package org.zohara.zoai.chat

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * The places where other modules plug into the chat screen. Each is set once (for example in the module's own init or composition) and
 * read by the composer; nothing here is needed for the chat to work.
 *
 *  - media module: the attachments themselves are read straight from `MediaModule.of(container).attachments` (take at send, restore on a
 *    failed send); [attachmentChips] draws the chips above the text, [attachButton] the "+" button in the tool row.
 *  - link module: [runOnPill] is the "Run on" control (Cloud / My computer) in the pills row. The chosen computer is `prefs.runOn`.
 *  - voice: [micButton] gets a function that appends text to the box and one that sends.
 *  - [readAloud] when set, AI messages get a speaker button.
 *  - pages module: collect [chatsChanged] to refresh the chat lists; the value is a delay in milliseconds already waited.
 *  - [prefill] text (shared in from another app) that the home composer takes once.
 */
object ChatHooks {
  var attachmentChips by mutableStateOf<(@Composable () -> Unit)?>(null)
  var attachButton by mutableStateOf<(@Composable () -> Unit)?>(null)
  var runOnPill by mutableStateOf<(@Composable () -> Unit)?>(null)
  var micButton by mutableStateOf<(@Composable (appendText: (String) -> Unit, send: () -> Unit) -> Unit)?>(null)
  var readAloud by mutableStateOf<((String) -> Unit)?>(null)
  val chatsChanged = MutableSharedFlow<Long>(extraBufferCapacity = 8)
  val prefill = MutableStateFlow<String?>(null)
}
