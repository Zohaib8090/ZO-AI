package org.zohara.agent.core.auth

/** Firebase sign-in errors in the web's sentences (index.html `FBERR`). Keys are Firebase `errorCode`s of the Android SDK. */
object AuthErrors {
  const val GENERIC = "Something went wrong. Please try again."
  const val NOT_CONFIGURED = "Sign-in is not configured in this build."
  const val GOOGLE_NOT_SET_UP = "Google sign-in is not set up in this build of the app yet. Use your e-mail and password."
  const val NOT_RIGHT = "The e-mail or password is not right."
  const val NETWORK = "Cannot reach the sign-in service. Check your connection and try again."
  const val NOT_VERIFIED = "Not verified yet. Press the link in the e-mail first. Check your spam folder too."

  private val byCode = mapOf(
    "ERROR_INVALID_EMAIL" to "That does not look like an e-mail address.",
    "ERROR_MISSING_EMAIL" to "Type your e-mail address first.",
    "ERROR_MISSING_PASSWORD" to "Type your password.",
    "ERROR_USER_DISABLED" to "This account has been turned off. Contact the operator of this service.",
    "ERROR_USER_NOT_FOUND" to NOT_RIGHT,
    "ERROR_WRONG_PASSWORD" to NOT_RIGHT,
    "ERROR_INVALID_CREDENTIAL" to NOT_RIGHT,
    "ERROR_INVALID_LOGIN_CREDENTIALS" to NOT_RIGHT,
    "ERROR_EMAIL_ALREADY_IN_USE" to "There is already an account with this e-mail. Use Sign in, or Forgot password if you do not remember it.",
    "ERROR_WEAK_PASSWORD" to "Choose a longer password, at least 6 characters.",
    "ERROR_TOO_MANY_REQUESTS" to "Too many tries. Please wait a few minutes and try again.",
    "ERROR_NETWORK_REQUEST_FAILED" to NETWORK,
    "ERROR_OPERATION_NOT_ALLOWED" to "E-mail sign-in is not turned on for this service yet. Use Google instead.",
    "ERROR_ACCOUNT_EXISTS_WITH_DIFFERENT_CREDENTIAL" to "This e-mail already has an account made another way. Sign in the way you used before.",
    "ERROR_REQUIRES_RECENT_LOGIN" to "Please sign in again and retry.",
  )

  /** Codes the web stays silent about (the person closed or cancelled the sign-in). */
  val silent = setOf("ERROR_CANCELLED", "ERROR_USER_CANCELLED")

  /** Sentence for a Firebase error code; null when the code is silent. */
  fun forCode(code: String?): String? = when {
    code == null -> GENERIC
    code in silent -> null
    else -> byCode[code] ?: GENERIC
  }

  /** The local checks before a call (AUTH-08). Returns a sentence or null. */
  fun validate(email: String, password: String, needPassword: Boolean = true, create: Boolean = false): String? = when {
    email.isBlank() -> "Type your e-mail address first."
    !email.trim().let { it.contains('@') && it.substringAfter('@').contains('.') } -> "That does not look like an e-mail address."
    needPassword && password.isEmpty() -> "Type your password."
    needPassword && create && password.length < 6 -> "Choose a longer password, at least 6 characters."
    else -> null
  }
}
