package org.zohara.zoai.core

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import com.github.takahirom.roborazzi.captureRoboImage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.zohara.zoai.core.auth.AuthErrors
import org.zohara.zoai.core.auth.ui.CONSENT_POINTS
import org.zohara.zoai.core.auth.ui.ConsentContent
import org.zohara.zoai.core.auth.ui.LoginContent
import org.zohara.zoai.core.auth.ui.LoginMode
import org.zohara.zoai.core.auth.ui.VerifyContent
import org.zohara.zoai.core.design.ZoAiTheme

/** The sign-in, verify and consent screens (AUTH-01..16, AUTH-29) and the sentences of the sign-in errors (AUTH-08, AUTH-09). */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35], qualifiers = "w411dp-h860dp-xhdpi", application = android.app.Application::class)
class AuthUiTest {
  @get:Rule val rule = createComposeRule()

  private var submitted = 0
  private var googled = 0

  @Composable
  private fun Login(start: LoginMode = LoginMode.SignIn, error: String? = null, note: String? = null, google: Boolean = true, notConfigured: Boolean = false, dark: Boolean = true) {
    ZoAiTheme(dark) {
      var mode by remember { mutableStateOf(start) }
      var email by remember { mutableStateOf("") }
      var pass by remember { mutableStateOf("") }
      LoginContent(
        mode, email, pass, busy = false, error = error, note = note, googleEnabled = google, notConfigured = notConfigured,
        onMode = { mode = it }, onEmail = { email = it }, onPassword = { pass = it }, onSubmit = { submitted++ }, onGoogle = { googled++ },
        onTerms = {}, onPrivacy = {}, modifier = Modifier.fillMaxSize(),
      )
    }
  }

  @Test fun loginHasEverythingTheWebHas() { // AUTH-01, AUTH-02, AUTH-03, AUTH-04, AUTH-05, AUTH-07, AUTH-15
    rule.setContent { Login() }
    rule.onNodeWithText("ZO-AI").assertIsDisplayed()
    rule.onNodeWithText("Your AI, anywhere. Sign in to continue and bring your own API key.").assertIsDisplayed()
    rule.onNodeWithText("Sign in with Google").assertIsEnabled().performClick()
    assertEquals(1, googled)
    rule.onNodeWithText("or use your e-mail").assertIsDisplayed()
    rule.onNodeWithText("Create account").assertIsDisplayed()          // the second tab
    rule.onAllNodesWithText("Sign in").assertCountEquals(2)             // the first tab and the submit button
    rule.onNodeWithText("E-mail").assertIsDisplayed()
    rule.onNodeWithText("you@example.com").assertIsDisplayed()
    rule.onNodeWithText("Forgot password?").assertIsDisplayed()
    rule.onNodeWithText("Use at least 6 characters.").assertDoesNotExist()
    rule.onNodeWithText("By signing in you will be asked to agree to the Terms and Privacy notice.", substring = true).assertIsDisplayed()
    rule.onRoot().captureRoboImage("src/test/screenshots/auth_login_dark.png")
  }

  @Test fun theTabsSwitchTheModeAndTheHintIsOnlyForCreate() { // AUTH-04, AUTH-06, AUTH-07
    rule.setContent { Login() }
    rule.onNodeWithText("Create account").performClick()
    rule.onNodeWithText("Use at least 6 characters.").assertIsDisplayed()
    rule.onAllNodesWithText("Create account").assertCountEquals(2)      // tab and submit
    rule.onAllNodesWithText("Sign in")[0].performClick()
    rule.onNodeWithText("Use at least 6 characters.").assertDoesNotExist()
  }

  @Test fun passwordShowAndHide() { // AUTH-06
    rule.setContent { Login() }
    rule.onNodeWithContentDescription("Show password").performClick()
    rule.onNodeWithContentDescription("Hide password").assertIsDisplayed()
    rule.onNodeWithContentDescription("Hide password").performClick()
    rule.onNodeWithContentDescription("Show password").assertIsDisplayed()
  }

  @Test fun forgotPasswordSwitchesToResetMode() { // AUTH-10, AUTH-07
    rule.setContent { Login() }
    rule.onNodeWithText("Forgot password?").performClick()
    rule.onNodeWithText("Send reset link").assertIsDisplayed()
    rule.onNodeWithText("Back to sign in").assertIsDisplayed()
    rule.onNodeWithText("Password").assertDoesNotExist()
    rule.onNodeWithText("Send reset link").performClick(); assertEquals(1, submitted)
    rule.onNodeWithText("Back to sign in").performClick()
    rule.onNodeWithText("Forgot password?").assertIsDisplayed()
  }

  @Test fun theResetNoteAndTheErrorLineAreShown() { // AUTH-08, AUTH-10
    rule.setContent { Login(start = LoginMode.Reset, note = "If there is an account for a@b.c, a link to choose a new password is on its way. Check your inbox.", error = "Type your e-mail address first.") }
    rule.onNodeWithText("If there is an account for a@b.c, a link to choose a new password is on its way. Check your inbox.").assertIsDisplayed()
    rule.onNodeWithText("Type your e-mail address first.").assertIsDisplayed()
  }

  @Test fun withoutGoogleServicesGoogleExplainsAndEmailStillWorks() { // AUTH-02, AUTH-29
    rule.setContent { Login(google = false) }
    rule.onNodeWithText("Google sign-in is not set up in this build of the app yet. Use your e-mail and password.").assertIsDisplayed()
    rule.onNodeWithText("Sign in with Google").assertIsEnabled()
  }

  @Test fun aBuildWithoutFirebaseSaysSoAndDisablesSignIn() { // AUTH-29
    rule.setContent { Login(notConfigured = true) }
    rule.onNodeWithText("Sign-in is not configured in this build.").assertIsDisplayed()
    rule.onNodeWithText("Sign in with Google").assertIsNotEnabled()
  }

  @Test fun typingInTheFields() { // AUTH-05
    rule.setContent { Login() }
    rule.onNodeWithContentDescription("E-mail").performTextInput("a@b.c")
    rule.onNodeWithContentDescription("Password").performTextInput("secret1")
  }

  @Test fun verifyScreenHasTheWebsTextAndButtons() { // AUTH-12
    var verified = 0; var resent = 0; var other = 0
    rule.setContent { ZoAiTheme(true) { VerifyContent("a@b.c", "Not verified yet. Press the link in the e-mail first. Check your spam folder too.", true, false, 0, { verified++ }, { resent++ }, { other++ }) } }
    rule.onNodeWithText("Check your inbox").assertIsDisplayed()
    rule.onNodeWithText("Open the e-mail we sent to a@b.c, press the link in it, then come back here and press I verified.").assertIsDisplayed()
    rule.onNodeWithText("Not verified yet. Press the link in the e-mail first. Check your spam folder too.").assertIsDisplayed()
    rule.onNodeWithText("I verified").performClick(); rule.onNodeWithText("Resend verification e-mail").performClick(); rule.onNodeWithText("Use another account").performClick()
    assertEquals(listOf(1, 1, 1), listOf(verified, resent, other))
    rule.onRoot().captureRoboImage("src/test/screenshots/auth_verify_dark.png")
  }

  @Test fun resendHas30SecondCooldown() { // AUTH-12
    rule.setContent { ZoAiTheme(true) { VerifyContent("a@b.c", "Sent again to a@b.c.", false, false, 29, {}, {}, {}) } }
    rule.onNodeWithText("Resend verification e-mail (29)").assertIsNotEnabled()
  }

  @Test fun consentHasFivePointsAndContinueNeedsTheCheckbox() { // AUTH-16
    var cont = 0; var out = 0
    rule.setContent {
      ZoAiTheme(false) {
        var agreed by remember { mutableStateOf(false) }
        ConsentContent(agreed, false, null, { agreed = it }, { cont++ }, { out++ }, {}, {})
      }
    }
    rule.onNodeWithText("Before you start").assertIsDisplayed()
    assertEquals(5, CONSENT_POINTS.size)
    for ((lead, _) in CONSENT_POINTS) rule.onNodeWithText(lead, substring = true).assertExists()
    rule.onNodeWithText("Continue").assertIsNotEnabled()
    rule.onNodeWithText("I agree to the Terms and the Privacy notice.").performClick()
    rule.onNodeWithText("Continue").assertIsEnabled().performClick()
    rule.onNodeWithText("Sign out").performClick()
    assertEquals(listOf(1, 1), listOf(cont, out))
    rule.onRoot().captureRoboImage("src/test/screenshots/auth_consent_light.png")
  }

  @Test fun consentBulletForTheComputerIsRewordedForThePhone() { // AUTH-16
    val (lead, rest) = CONSENT_POINTS[3]
    assertEquals("No access to your computer.", lead)
    assert(rest.contains("choose My computer")) { rest }
  }

  // ---- the sentences ----

  @Test fun validationMessages() { // AUTH-08
    assertEquals("Type your e-mail address first.", AuthErrors.validate("", "x"))
    assertEquals("Type your password.", AuthErrors.validate("a@b.c", ""))
    assertEquals("Choose a longer password, at least 6 characters.", AuthErrors.validate("a@b.c", "12345", create = true))
    assertNull(AuthErrors.validate("a@b.c", "123456", create = true))
    assertNull("reset mode needs no password", AuthErrors.validate("a@b.c", "", needPassword = false))
  }

  @Test fun firebaseErrorTable() { // AUTH-09
    val notRight = "The e-mail or password is not right."
    for (c in listOf("ERROR_USER_NOT_FOUND", "ERROR_WRONG_PASSWORD", "ERROR_INVALID_CREDENTIAL", "ERROR_INVALID_LOGIN_CREDENTIALS")) assertEquals(c, notRight, AuthErrors.forCode(c))
    assertEquals("That does not look like an e-mail address.", AuthErrors.forCode("ERROR_INVALID_EMAIL"))
    assertEquals("This account has been turned off. Contact the operator of this service.", AuthErrors.forCode("ERROR_USER_DISABLED"))
    assertEquals("Too many tries. Please wait a few minutes and try again.", AuthErrors.forCode("ERROR_TOO_MANY_REQUESTS"))
    assertEquals(AuthErrors.NETWORK, AuthErrors.forCode("ERROR_NETWORK_REQUEST_FAILED"))
    assertEquals("Please sign in again and retry.", AuthErrors.forCode("ERROR_REQUIRES_RECENT_LOGIN"))
    assertEquals(AuthErrors.GENERIC, AuthErrors.forCode("ERROR_SOMETHING_NEW"))
    assertEquals(AuthErrors.GENERIC, AuthErrors.forCode(null))
    assertNull("the person cancelled: silent", AuthErrors.forCode("ERROR_USER_CANCELLED"))
    assert(AuthErrors.forCode("ERROR_EMAIL_ALREADY_IN_USE")!!.startsWith("There is already an account"))
    assert(AuthErrors.forCode("ERROR_OPERATION_NOT_ALLOWED")!!.startsWith("E-mail sign-in is not turned on"))
    assert(AuthErrors.forCode("ERROR_ACCOUNT_EXISTS_WITH_DIFFERENT_CREDENTIAL")!!.startsWith("This e-mail already has an account made another way"))
  }

  // ---- the repository (no Firebase needed for these paths) ----

  private fun repo(web: String = "") = org.zohara.zoai.core.auth.AuthRepository(androidx.test.core.app.ApplicationProvider.getApplicationContext(), false, web)

  @Test fun theRepositoryChecksTheFieldsBeforeAnyCall() = kotlinx.coroutines.runBlocking { // AUTH-08
    val r = repo()
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed("Type your e-mail address first."), r.signIn("", ""))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed("Type your password."), r.signIn("a@b.c", ""))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed("Choose a longer password, at least 6 characters."), r.signUp("a@b.c", "123"))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed("Type your e-mail address first."), r.resetPassword(" "))
  }

  @Test fun withoutFirebaseEveryActionSaysSoAndGoogleExplainsItself() = kotlinx.coroutines.runBlocking { // AUTH-29, AUTH-02
    val r = repo()
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed(AuthErrors.NOT_CONFIGURED), r.signIn("a@b.c", "secret1"))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Failed(AuthErrors.GOOGLE_NOT_SET_UP), r.signInWithGoogle(androidx.test.core.app.ApplicationProvider.getApplicationContext()))
    assertEquals(false, r.configured); assertEquals(false, r.googleConfigured)
    assertEquals(org.zohara.zoai.core.auth.AuthState.NotConfigured, r.state.value)
  }

  @Test fun sdkExceptionsBecomeTheSameSentences() { // AUTH-09
    val r = repo()
    val failed = { m: String -> org.zohara.zoai.core.auth.AuthResult.Failed(m) }
    assertEquals(failed("The e-mail or password is not right."), r.mapError(com.google.firebase.auth.FirebaseAuthInvalidCredentialsException("ERROR_INVALID_CREDENTIAL", "x")))
    assertEquals(failed("The e-mail or password is not right."), r.mapError(com.google.firebase.auth.FirebaseAuthException("ERROR_WRONG_PASSWORD", "x")))
    assertEquals(failed(AuthErrors.NETWORK), r.mapError(com.google.firebase.FirebaseNetworkException("x")))
    assertEquals(failed("Too many tries. Please wait a few minutes and try again."), r.mapError(com.google.firebase.FirebaseTooManyRequestsException("x")))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Cancelled, r.mapError(androidx.credentials.exceptions.GetCredentialCancellationException("x")))
    assertEquals(org.zohara.zoai.core.auth.AuthResult.Cancelled, r.mapError(com.google.firebase.auth.FirebaseAuthException("ERROR_USER_CANCELLED", "x")))
    assertEquals(failed(AuthErrors.GENERIC), r.mapError(RuntimeException("x")))
    assert((r.mapError(androidx.credentials.exceptions.NoCredentialException("x")) as org.zohara.zoai.core.auth.AuthResult.Failed).message.startsWith("No Google account is on this phone"))
  }
}
