package org.zohara.agent.core.auth.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.zohara.agent.LocalAppContainer
import org.zohara.agent.core.auth.AuthResult
import org.zohara.agent.core.session.Gate
import org.zohara.agent.core.util.openInBrowser

private fun Context.findActivity(): Activity? = when (this) {
  is Activity -> this
  is ContextWrapper -> baseContext.findActivity()
  else -> null
}

/** Stateful login screen (AUTH-01..10, AUTH-14, AUTH-29). */
@Composable
fun LoginScreen() {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  val scope = rememberCoroutineScope()
  var mode by rememberSaveable { mutableStateOf(LoginMode.SignIn) }
  var email by rememberSaveable { mutableStateOf("") }
  var password by rememberSaveable { mutableStateOf("") }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  var note by remember { mutableStateOf<String?>(null) }
  val refused by container.session.loginMessage.collectAsState()
  val notConfigured = !container.auth.configured

  fun finish(r: AuthResult, onOk: () -> Unit = {}) {
    busy = false
    when (r) {
      AuthResult.Ok -> onOk()
      is AuthResult.Failed -> error = r.message
      AuthResult.Cancelled -> Unit
    }
  }

  LoginContent(
    mode = mode, email = email, password = password, busy = busy, error = error ?: refused, note = note,
    googleEnabled = container.auth.googleConfigured, notConfigured = notConfigured,
    onMode = { mode = it; error = null; note = null; container.session.clearLoginMessage() },
    onEmail = { email = it },
    onPassword = { password = it },
    onSubmit = {
      error = null; note = null; busy = true; container.session.clearLoginMessage()
      scope.launch {
        when (mode) {
          LoginMode.SignIn -> finish(container.auth.signIn(email, password))
          LoginMode.SignUp -> finish(container.auth.signUp(email, password))
          LoginMode.Reset -> finish(container.auth.resetPassword(email)) {
            note = "If there is an account for ${email.trim()}, a link to choose a new password is on its way. Check your inbox."
          }
        }
      }
    },
    onGoogle = {
      error = null; container.session.clearLoginMessage()
      val a = context.findActivity()
      if (a == null) { error = "Something went wrong. Please try again." } else {
        busy = true
        scope.launch { finish(container.auth.signInWithGoogle(a)) }
      }
    },
    onTerms = { openInBrowser(context, container.serverUrl + "/terms") },
    onPrivacy = { openInBrowser(context, container.serverUrl + "/privacy") },
  )
}

/** Stateful "Check your inbox" screen (AUTH-12, AUTH-13). */
@Composable
fun VerifyScreen(email: String) {
  val container = LocalAppContainer.current
  val scope = rememberCoroutineScope()
  var busy by remember { mutableStateOf(false) }
  var message by remember { mutableStateOf<String?>(null) }
  var isError by remember { mutableStateOf(false) }
  var cooldown by remember { mutableIntStateOf(0) }
  LaunchedEffect(cooldown) { if (cooldown > 0) { delay(1000); cooldown-- } }
  VerifyContent(
    email = email, message = message, messageIsError = isError, busy = busy, cooldownSeconds = cooldown,
    onVerified = {
      busy = true; message = null
      scope.launch {
        val r = container.auth.checkVerified()
        busy = false
        if (r is AuthResult.Failed) { message = r.message; isError = true }
      }
    },
    onResend = {
      scope.launch {
        val r = container.auth.resendVerification()
        if (r is AuthResult.Failed) { message = r.message; isError = true } else { message = "Sent again to $email."; isError = false; cooldown = 30 }
      }
    },
    onUseAnother = { container.session.signOut() },
  )
}

/** Stateful consent screen (AUTH-16, AUTH-17). */
@Composable
fun ConsentScreen(gate: Gate.Consent) {
  val container = LocalAppContainer.current
  val context = LocalContext.current
  var agreed by rememberSaveable { mutableStateOf(false) }
  var busy by remember { mutableStateOf(false) }
  var error by remember { mutableStateOf<String?>(null) }
  ConsentContent(
    agreed = agreed, busy = busy, error = error ?: gate.error,
    onAgreed = { agreed = it },
    onContinue = {
      busy = true; error = null
      container.session.acceptTerms(gate.version) { msg -> busy = false; error = msg }
    },
    onSignOut = { container.session.signOut() },
    onTerms = { openInBrowser(context, container.serverUrl + "/terms") },
    onPrivacy = { openInBrowser(context, container.serverUrl + "/privacy") },
  )
}
