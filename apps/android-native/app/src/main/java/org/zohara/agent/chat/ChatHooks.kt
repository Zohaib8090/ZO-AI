package org.zohara.agent.chat

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
  /**
   * Wraps the whole composer card once: gets the text of the box, a function that replaces it, send, and whether an answer is being written, and draws [content].
   * The media module makes its one controller here (attachments, dictation, sheets) and hands it to the other slots, which are drawn inside [content].
   */
  var composerHost by mutableStateOf<(@Composable (text: String, setText: (String) -> Unit, send: () -> Unit, busy: Boolean, content: @Composable () -> Unit) -> Unit)?>(null)
  /** called by the box on every edit with the text before and after; returns the text to show (a pasted address becomes a chip and is taken out, typing stops dictation) */
  var textEdited: ((old: String, new: String) -> String)? = null
  /** extra modifier for the text box: keyboard pictures and paste, drag and drop, Esc and Ctrl+Shift+M on a hardware keyboard */
  var fieldModifier by mutableStateOf<(@Composable () -> androidx.compose.ui.Modifier)?>(null)
  /** the speaker button under an answer (draws Read aloud / Stop reading); when set it is used instead of the plain [readAloud] action */
  var readAloudButton by mutableStateOf<(@Composable (text: String) -> Unit)?>(null)
  var readAloud by mutableStateOf<((String) -> Unit)?>(null)
  val chatsChanged = MutableSharedFlow<Long>(extraBufferCapacity = 8)
  val prefill = MutableStateFlow<String?>(null)
  /** the route on screen and its chat id argument (set by the app shell); VOICE-11 reads an answer aloud only when its chat is still the one shown */
  val screen = MutableStateFlow<Pair<String?, String?>>(null to null)
}
