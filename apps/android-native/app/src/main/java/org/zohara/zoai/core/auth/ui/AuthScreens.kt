package org.zohara.zoai.core.auth.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import org.zohara.zoai.core.design.ErrorLine
import org.zohara.zoai.core.design.PrimaryButton
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ZoLogo
import org.zohara.zoai.core.design.ZoTextField
import org.zohara.zoai.core.design.ZoTheme

/** The three modes of the e-mail card (AUTH-04, AUTH-10). */
enum class LoginMode { SignIn, SignUp, Reset }

/** Page frame of the login, verify and consent screens: centred column, hero row with the mark, serif title (AUTH-01). */
@Composable
fun AuthFrame(title: String, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
  Box(modifier.fillMaxSize().background(ZoTheme.colors.bg).safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
    Column(
      Modifier.widthIn(max = 440.dp).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 28.dp),
      verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
      Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 12.dp)) {
        ZoLogo(size = 38.dp)
        Text(title, style = ZoTheme.type.hero, color = ZoTheme.colors.text, modifier = Modifier.padding(start = 12.dp))
      }
      content()
    }
  }
}

@Composable
private fun LegalLine(prefix: String, onTerms: () -> Unit, onPrivacy: () -> Unit, between: String = " and ") {
  val c = ZoTheme.colors
  val link = TextLinkStyles(SpanStyle(color = c.accent, textDecoration = TextDecoration.Underline))
  val text = buildAnnotatedString {
    append(prefix)
    withLink(LinkAnnotation.Clickable("terms", link) { onTerms() }) { append("Terms") }
    append(between)
    withLink(LinkAnnotation.Clickable("privacy", link) { onPrivacy() }) { append("Privacy notice") }
    append(".")
  }
  Text(text, style = ZoTheme.type.fieldHint.copy(fontSize = androidx.compose.ui.unit.TextUnit(12f, androidx.compose.ui.unit.TextUnitType.Sp)), color = c.dim)
}

/** AUTH-01..10: the sign-in screen. State and actions come from the caller, so the screen can be shown in a screenshot test. */
@Composable
fun LoginContent(
  mode: LoginMode,
  email: String,
  password: String,
  busy: Boolean,
  error: String?,
  note: String?,
  googleEnabled: Boolean,
  notConfigured: Boolean,
  onMode: (LoginMode) -> Unit,
  onEmail: (String) -> Unit,
  onPassword: (String) -> Unit,
  onSubmit: () -> Unit,
  onGoogle: () -> Unit,
  onTerms: () -> Unit,
  onPrivacy: () -> Unit,
  modifier: Modifier = Modifier,
) {
  val c = ZoTheme.colors
  AuthFrame("ZO-AI", modifier) {
    Text("Your AI, anywhere. Sign in to continue and bring your own API key.", style = ZoTheme.type.body, color = c.text3)
    if (notConfigured) {
      Text("Sign-in is not configured in this build.", style = ZoTheme.type.cardMeta, color = c.warn)
    }
    PrimaryButton("Sign in with Google", onGoogle, Modifier.fillMaxWidth(), enabled = !busy && !notConfigured)
    if (!googleEnabled && !notConfigured) {
      Text("Google sign-in is not set up in this build of the app yet. Use your e-mail and password.", style = ZoTheme.type.fieldHint, color = c.dim)
    }
    Text("or use your e-mail", style = ZoTheme.type.fieldHint, color = c.dimmer, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
    Column(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(c.card).border(1.dp, c.line, ZoTheme.shapes.card).padding(16.dp),
      verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
      if (mode != LoginMode.Reset) {
        Row(Modifier.fillMaxWidth().clip(ZoTheme.shapes.field).background(c.input).padding(3.dp), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
          for ((m, label) in listOf(LoginMode.SignIn to "Sign in", LoginMode.SignUp to "Create account")) {
            val sel = mode == m
            Box(
              Modifier.weight(1f).clip(RoundedCornerShape(8.dp)).background(if (sel) c.raise else androidx.compose.ui.graphics.Color.Transparent)
                .clickable(role = Role.Tab) { onMode(m) }.semantics { role = Role.Tab; selected = sel }.padding(vertical = 8.dp),
              contentAlignment = Alignment.Center,
            ) { Text(label, style = ZoTheme.type.button, color = if (sel) c.text else c.dim) }
          }
        }
      } else {
        Text("Choose a new password", style = ZoTheme.type.popTitle, color = c.text)
      }
      ZoTextField(email, onEmail, label = "E-mail", placeholder = "you@example.com", keyboardType = KeyboardType.Email, enabled = !busy)
      if (mode != LoginMode.Reset) {
        ZoTextField(
          password, onPassword, label = "Password", placeholder = "Password", password = true, enabled = !busy,
          hint = if (mode == LoginMode.SignUp) "Use at least 6 characters." else null,
        )
      }
      PrimaryButton(
        when (mode) { LoginMode.SignIn -> "Sign in"; LoginMode.SignUp -> "Create account"; LoginMode.Reset -> "Send reset link" },
        onSubmit, Modifier.fillMaxWidth(), enabled = !notConfigured, loading = busy,
      )
      Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        if (mode == LoginMode.Reset) {
          LinkButton("Back to sign in") { onMode(LoginMode.SignIn) }
        } else {
          LinkButton("Forgot password?") { onMode(LoginMode.Reset) }
        }
      }
      if (!note.isNullOrBlank()) Text(note, style = ZoTheme.type.cardMeta, color = c.text3)
    }
    if (!error.isNullOrBlank()) ErrorLine(error)
    LegalLine("By signing in you will be asked to agree to the ", onTerms, onPrivacy)
  }
}

@Composable
fun LinkButton(text: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
  Text(text, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.accent, modifier = modifier.clip(RoundedCornerShape(6.dp)).clickable(role = Role.Button, onClick = onClick).padding(vertical = 6.dp, horizontal = 2.dp))
}

/** AUTH-12: "Check your inbox". */
@Composable
fun VerifyContent(
  email: String,
  message: String?,
  messageIsError: Boolean,
  busy: Boolean,
  cooldownSeconds: Int,
  onVerified: () -> Unit,
  onResend: () -> Unit,
  onUseAnother: () -> Unit,
  modifier: Modifier = Modifier,
) {
  val c = ZoTheme.colors
  AuthFrame("Check your inbox", modifier) {
    Text(
      "Open the e-mail we sent to $email, press the link in it, then come back here and press I verified.",
      style = ZoTheme.type.body, color = c.text3,
    )
    Column(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(c.card).border(1.dp, c.line, ZoTheme.shapes.card).padding(16.dp),
      verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
      if (!message.isNullOrBlank()) Text(message, style = ZoTheme.type.cardMeta, color = if (messageIsError) c.danger else c.text3)
      PrimaryButton("I verified", onVerified, Modifier.fillMaxWidth(), loading = busy)
      SecondaryButton(
        if (cooldownSeconds > 0) "Resend verification e-mail ($cooldownSeconds)" else "Resend verification e-mail",
        onResend, Modifier.fillMaxWidth(), enabled = cooldownSeconds == 0 && !busy,
      )
      LinkButton("Use another account", Modifier.align(Alignment.CenterHorizontally), onUseAnother)
    }
  }
}

/** AUTH-16: "Before you start". */
@Composable
fun ConsentContent(
  agreed: Boolean,
  busy: Boolean,
  error: String?,
  onAgreed: (Boolean) -> Unit,
  onContinue: () -> Unit,
  onSignOut: () -> Unit,
  onTerms: () -> Unit,
  onPrivacy: () -> Unit,
  modifier: Modifier = Modifier,
) {
  val c = ZoTheme.colors
  AuthFrame("Before you start", modifier) {
    Column(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.card).background(c.card).border(1.dp, c.line, ZoTheme.shapes.card).padding(16.dp),
      verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
      for ((lead, rest) in CONSENT_POINTS) Text(consentLine(lead, rest), style = ZoTheme.type.body, color = c.text2)
    }
    LegalLine("Read the ", onPrivacy, onTerms, between = " and the ")
    Row(
      Modifier.fillMaxWidth().clip(ZoTheme.shapes.field).clickable(role = Role.Checkbox) { onAgreed(!agreed) }.padding(vertical = 6.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      Box(
        Modifier.size(22.dp).clip(RoundedCornerShape(6.dp)).background(if (agreed) c.accent else androidx.compose.ui.graphics.Color.Transparent).border(1.5.dp, if (agreed) c.accent else c.line12, RoundedCornerShape(6.dp)),
        contentAlignment = Alignment.Center,
      ) {
        if (agreed) androidx.compose.material3.Icon(org.zohara.zoai.core.design.ZoIcons.Check, null, Modifier.size(16.dp), tint = c.onAccent)
      }
      Text("I agree to the Terms and the Privacy notice.", style = ZoTheme.type.body, color = c.text, modifier = Modifier.padding(start = 12.dp))
    }
    if (!error.isNullOrBlank()) ErrorLine(error)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End)) {
      SecondaryButton("Sign out", onSignOut)
      PrimaryButton("Continue", onContinue, enabled = agreed, loading = busy)
    }
    Spacer(Modifier.height(8.dp))
  }
}

/** The five points of the consent screen (AUTH-16, reworded for the phone). */
val CONSENT_POINTS: List<Pair<String, String>> = listOf(
  "What this is." to "ZO-AI is the assistant built into Zohara OS. Here it can chat, search the web and keep notes about you (memory).",
  "Where your data goes." to "Your chats and memory are stored in the cloud (Google Firebase) under your own account, so they follow you between the phone, the web, the desktop app and the terminal. The person who runs this service can technically access the database. Do not put sensitive secrets in chats.",
  "Your own API key." to "You bring your own key from an AI provider. It is stored encrypted on our server and only used to call that provider for you.",
  "No access to your computer." to "Computer-control tools run only in your own desktop app. To use your computer from here, choose My computer under the message box.",
  "Export or delete." to "You can export or delete everything at any time in Settings > Account.",
)

private fun consentLine(lead: String, rest: String): AnnotatedString = buildAnnotatedString {
  withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(lead) }
  append(" ")
  append(rest)
}
