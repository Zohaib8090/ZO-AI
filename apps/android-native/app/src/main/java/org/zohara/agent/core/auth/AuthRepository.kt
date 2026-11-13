package org.zohara.agent.core.auth

import android.content.Context
import androidx.credentials.CredentialManager
import androidx.credentials.CustomCredential
import androidx.credentials.GetCredentialRequest
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.NoCredentialException
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseNetworkException
import com.google.firebase.FirebaseTooManyRequestsException
import com.google.firebase.auth.FirebaseAuth
import com.google.firebase.auth.FirebaseAuthException
import com.google.firebase.auth.FirebaseUser
import com.google.firebase.auth.GoogleAuthProvider
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.tasks.await
import org.zohara.agent.core.net.TokenSource

data class AuthUser(val uid: String, val email: String, val displayName: String, val emailVerified: Boolean, val passwordAccount: Boolean) {
  /** NAV-18: first word of the display name, else the e-mail's local part, capitalised */
  val firstName: String
    get() {
      val base = displayName.trim().substringBefore(' ').ifBlank { email.substringBefore('@') }
      return base.replaceFirstChar { it.uppercase() }
    }
  val label: String get() = displayName.ifBlank { email }
  val initial: String get() = label.trim().take(1).ifEmpty { "?" }
}

sealed interface AuthState {
  /** the session is being read */
  data object Loading : AuthState
  data object SignedOut : AuthState
  /** an e-mail and password account whose address is not verified yet (AUTH-13) */
  data class NeedsVerification(val user: AuthUser) : AuthState
  data class SignedIn(val user: AuthUser) : AuthState
  /** the build has no google-services.json: no sign-in at all (AUTH-29) */
  data object NotConfigured : AuthState
}

/** Result of an auth action: a sentence for the person when it failed ("" = nothing to show: the person cancelled). */
sealed interface AuthResult {
  data object Ok : AuthResult
  data class Failed(val message: String) : AuthResult
  data object Cancelled : AuthResult
}

/**
 * Firebase Auth behind the app's sign-in screens. FirebaseApp is started lazily: when the build has no google-services.json
 * ([firebaseConfigured] false) nothing touches Firebase and the state is [AuthState.NotConfigured].
 * Also the [TokenSource] of the HTTP client.
 */
class AuthRepository(
  private val context: Context,
  private val firebaseConfigured: Boolean,
  private val googleWebClientId: String,
) : TokenSource {
  private val _state = MutableStateFlow<AuthState>(if (firebaseConfigured) AuthState.Loading else AuthState.NotConfigured)
  val state: StateFlow<AuthState> = _state.asStateFlow()

  private val auth: FirebaseAuth? by lazy {
    if (!firebaseConfigured) null
    else runCatching {
      if (FirebaseApp.getApps(context).isEmpty()) FirebaseApp.initializeApp(context)
      FirebaseAuth.getInstance()
    }.getOrNull()
  }

  /** Tests only: sets the state without Firebase (the whole-app tests sign in this way). */
  @androidx.annotation.VisibleForTesting
  internal fun forceStateForTests(state: AuthState, token: String? = "test-token") { _state.value = state; tokenForTests = token }
  private var tokenForTests: String? = null

  val configured: Boolean get() = auth != null
  val googleConfigured: Boolean get() = auth != null && googleWebClientId.isNotBlank()

  /** Call once at start: follows the Firebase session (stays signed in across launches, AUTH-23). */
  fun start() {
    val a = auth
    if (a == null) { _state.value = AuthState.NotConfigured; return }
    a.addAuthStateListener { fb -> publish(fb.currentUser) }
    publish(a.currentUser)
  }

  private fun publish(u: FirebaseUser?) {
    _state.value = when {
      u == null -> AuthState.SignedOut
      u.toUser().let { it.passwordAccount && !it.emailVerified } -> AuthState.NeedsVerification(u.toUser())
      else -> AuthState.SignedIn(u.toUser())
    }
  }

  private fun FirebaseUser.toUser() = AuthUser(
    uid = uid, email = email.orEmpty(), displayName = displayName.orEmpty(), emailVerified = isEmailVerified,
    passwordAccount = providerData.any { it.providerId == "password" } && providerData.none { it.providerId == "google.com" },
  )

  // ---- token (blocking, for the OkHttp interceptor) ----
  override fun idToken(forceRefresh: Boolean): String? {
    tokenForTests?.let { return it }
    val u = auth?.currentUser ?: return null
    return try {
      com.google.android.gms.tasks.Tasks.await(u.getIdToken(forceRefresh), 20, java.util.concurrent.TimeUnit.SECONDS).token
    } catch (_: Exception) {
      null
    }
  }

  // ---- actions ----
  private suspend fun guarded(block: suspend (FirebaseAuth) -> Unit): AuthResult {
    val a = auth ?: return AuthResult.Failed(AuthErrors.NOT_CONFIGURED)
    return try {
      block(a)
      AuthResult.Ok
    } catch (e: kotlinx.coroutines.CancellationException) {
      throw e
    } catch (e: Exception) {
      mapError(e)
    }
  }

  fun mapError(e: Exception): AuthResult = when (e) {
    is FirebaseAuthException -> AuthErrors.forCode(e.errorCode)?.let { AuthResult.Failed(it) } ?: AuthResult.Cancelled
    is FirebaseTooManyRequestsException -> AuthResult.Failed(AuthErrors.forCode("ERROR_TOO_MANY_REQUESTS")!!)
    is FirebaseNetworkException -> AuthResult.Failed(AuthErrors.NETWORK)
    is GetCredentialCancellationException -> AuthResult.Cancelled
    is NoCredentialException -> AuthResult.Failed("No Google account is on this phone yet. Add one in Settings, or use your e-mail and password.")
    is GetCredentialException -> AuthResult.Failed(AuthErrors.GENERIC)
    else -> AuthResult.Failed(AuthErrors.GENERIC)
  }

  suspend fun signIn(email: String, password: String): AuthResult =
    AuthErrors.validate(email, password)?.let { AuthResult.Failed(it) } ?: guarded { it.signInWithEmailAndPassword(email.trim(), password).await() }

  /** Creates the account and sends the verification e-mail (AUTH-11). */
  suspend fun signUp(email: String, password: String): AuthResult =
    AuthErrors.validate(email, password, create = true)?.let { AuthResult.Failed(it) } ?: guarded {
      val r = it.createUserWithEmailAndPassword(email.trim(), password).await()
      r.user?.sendEmailVerification()?.await()
    }

  suspend fun resetPassword(email: String): AuthResult =
    AuthErrors.validate(email, "", needPassword = false)?.let { AuthResult.Failed(it) } ?: guarded { it.sendPasswordResetEmail(email.trim()).await() }

  suspend fun resendVerification(): AuthResult = guarded { a -> a.currentUser?.sendEmailVerification()?.await() }

  /** "I verified": reloads the user and forces a new ID token (AUTH-12). Failed(NOT_VERIFIED) when the address is still not verified. */
  suspend fun checkVerified(): AuthResult {
    val a = auth ?: return AuthResult.Failed(AuthErrors.NOT_CONFIGURED)
    val u = a.currentUser ?: return AuthResult.Failed(AuthErrors.GENERIC)
    return try {
      u.reload().await()
      val fresh = a.currentUser ?: return AuthResult.Failed(AuthErrors.GENERIC)
      if (!fresh.isEmailVerified) return AuthResult.Failed(AuthErrors.NOT_VERIFIED)
      fresh.getIdToken(true).await()
      publish(fresh)
      AuthResult.Ok
    } catch (e: kotlinx.coroutines.CancellationException) {
      throw e
    } catch (e: Exception) {
      mapError(e)
    }
  }

  /** Google through Credential Manager, then Firebase `signInWithCredential` (AUTH-02). [activityContext] must be an Activity. */
  suspend fun signInWithGoogle(activityContext: Context): AuthResult {
    if (googleWebClientId.isBlank()) return AuthResult.Failed(AuthErrors.GOOGLE_NOT_SET_UP)
    return guarded { a ->
      val option = GetGoogleIdOption.Builder().setServerClientId(googleWebClientId).setFilterByAuthorizedAccounts(false).setAutoSelectEnabled(false).build()
      val request = GetCredentialRequest.Builder().addCredentialOption(option).build()
      val result = CredentialManager.create(activityContext).getCredential(activityContext, request)
      val cred = result.credential
      if (cred is CustomCredential && cred.type == GoogleIdTokenCredential.TYPE_GOOGLE_ID_TOKEN_CREDENTIAL) {
        val g = GoogleIdTokenCredential.createFrom(cred.data)
        a.signInWithCredential(GoogleAuthProvider.getCredential(g.idToken, null)).await()
      } else {
        throw IllegalStateException("unsupported credential")
      }
    }
  }

  fun signOut() {
    auth?.signOut()
    if (auth == null) _state.value = if (firebaseConfigured) AuthState.SignedOut else AuthState.NotConfigured
  }
}
