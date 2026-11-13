package org.zohara.zoai.nav

import android.app.Activity
import androidx.activity.compose.LocalActivity
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import kotlinx.coroutines.flow.MutableStateFlow
import org.zohara.zoai.AppContainer
import org.zohara.zoai.LocalAppContainer
import org.zohara.zoai.core.auth.AuthState
import org.zohara.zoai.core.auth.ui.ConsentScreen
import org.zohara.zoai.core.auth.ui.LoginScreen
import org.zohara.zoai.core.auth.ui.VerifyScreen
import org.zohara.zoai.core.design.SecondaryButton
import org.zohara.zoai.core.design.ToastHost
import org.zohara.zoai.core.design.ZoAiTheme
import org.zohara.zoai.core.design.ZoLogo
import org.zohara.zoai.core.design.ZoTheme
import org.zohara.zoai.core.design.isDarkFor
import org.zohara.zoai.core.session.Gate
import org.zohara.zoai.core.session.NetBanner
import org.zohara.zoai.core.design.ThemeChoice
import org.zohara.zoai.core.design.LightZoColors
import org.zohara.zoai.core.design.DarkZoColors

/** Root of the UI: theme, then sign-in, consent, or the app with its drawer and navigation graph. */
@Composable
fun ZoApp(container: AppContainer, pendingChat: MutableStateFlow<String?>) {
  val theme by container.prefs.theme.collectAsState(initial = ThemeChoice.System)
  val dark = isDarkFor(theme)
  // media-link: the attachment strip, the "+" button, the microphone, Run on and read aloud in the chat's composer slots (once)
  remember(container) { org.zohara.zoai.media.ChatPlug.install(container); true }
  CompositionLocalProvider(LocalAppContainer provides container) {
    ZoAiTheme(dark) {
      SystemBars(dark)
      Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg)) {
        val auth by container.auth.state.collectAsState()
        when (val a = auth) {
          AuthState.Loading -> Splash()
          AuthState.SignedOut, AuthState.NotConfigured -> LoginScreen()
          is AuthState.NeedsVerification -> VerifyScreen(a.user.email)
          is AuthState.SignedIn -> {
            val gate by container.session.gate.collectAsState()
            when (val g = gate) {
              Gate.Loading -> Splash()
              is Gate.Consent -> ConsentScreen(g)
              is Gate.Ready -> MainShell(container, a, pendingChat)
            }
          }
        }
        ToastHost(container.toast, Modifier.align(Alignment.BottomCenter))
      }
    }
  }
}

/** PHN-07: the mark on the app background while the session loads. */
@Composable
fun Splash() {
  Box(Modifier.fillMaxSize().background(ZoTheme.colors.bg), contentAlignment = Alignment.Center) { ZoLogo(size = 64.dp) }
}

/** THEME-11: status and navigation bars follow the theme (edge to edge). */
@Composable
private fun SystemBars(dark: Boolean) {
  val activity = LocalActivity.current as? ComponentActivity ?: return
  val bg = (if (dark) DarkZoColors.bg else LightZoColors.bg).toArgb()
  SideEffect {
    val style = if (dark) SystemBarStyle.dark(android.graphics.Color.TRANSPARENT) else SystemBarStyle.light(android.graphics.Color.TRANSPARENT, android.graphics.Color.argb(0x40, 0, 0, 0))
    activity.enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
    activity.window.decorView.setBackgroundColor(bg)
  }
}

@Composable
private fun MainShell(container: AppContainer, auth: AuthState.SignedIn, pendingChat: MutableStateFlow<String?>) {
  val nav = rememberNavController()
  val drawer = rememberZoDrawerState()
  val navigator = remember(nav, drawer) { ZoNavigator(nav, drawer) }
  val topBar = remember { TopBarController() }
  val backEntry by nav.currentBackStackEntryAsState()
  val route = backEntry?.destination?.route
  val running by container.runningCount.collectAsState()
  val banner by container.session.banner.collectAsState()
  val activity = LocalActivity.current

  // NAV-20: a notification or link opens a chat
  val pending by pendingChat.collectAsState()
  LaunchedEffect(pending) {
    val id = DeepLinks.validChatId(pending)
    if (id != null) { navigator.openChat(id); pendingChat.value = null }
  }
  // ATT-22: something shared from another app opens a new message
  org.zohara.zoai.media.ShareNavigation { navigator.home() }
  // INTER-15: asked once, with an explanation, whether to tell the person when an answer is ready
  org.zohara.zoai.media.notify.NotificationAskOnce()
  // NAV-21: back at the first screen minimizes the app, it never ends a running answer
  BackHandler(enabled = route == Routes.HOME && !drawer.isOpen) { activity?.moveTaskToBack(true) }

  CompositionLocalProvider(LocalNav provides navigator, LocalTopBar provides topBar) {
    ZoDrawerLayout(
      state = drawer,
      drawerContent = {
        DrawerContent(
          currentRoute = route,
          restricted = container.session.restricted,
          userLabel = auth.user.label,
          userInitial = auth.user.initial,
          runningCount = running,
          onNavigate = { r -> if (r == Routes.HOME) navigator.home() else navigator.navigate(r) },
          onSettings = { navigator.settings() },
          onSignOut = { container.session.signOut() },
          onClose = if (drawer.isOpen) ({ drawer.close() }) else null,
          activeChatId = backEntry?.arguments?.getString(Routes.ARG_ID),
          onOpenChat = { navigator.openChat(it) },
        )
      },
    ) { permanent ->
      Column(Modifier.fillMaxSize().background(ZoTheme.colors.bg)) {
        ZoTopBar(routeTitle = Routes.titleFor(route), showMenuButton = !permanent, onMenu = { drawer.open() })
        NetBannerRow(banner, onSignInAgain = { container.session.signOut() })
        Box(Modifier.weight(1f).fillMaxWidth().imePadding()) {
          NavHost(
            navController = nav,
            startDestination = Routes.HOME,
            enterTransition = { fadeIn(tween(120)) },
            exitTransition = { fadeOut(tween(120)) },
            popEnterTransition = { fadeIn(tween(120)) },
            popExitTransition = { fadeOut(tween(120)) },
          ) {
            for (def in Routes.all) {
              composable(
                def.pattern,
                arguments = buildList {
                  if (def.pattern.contains("{${Routes.ARG_ID}}")) add(navArgument(Routes.ARG_ID) { type = NavType.StringType })
                  if (def.pattern.contains("{${Routes.ARG_INDEX}}")) add(navArgument(Routes.ARG_INDEX) { type = NavType.IntType })
                },
              ) { entry -> ScreenRegistry.screens.getValue(def.pattern)(entry) }
            }
          }
        }
      }
    }
  }
}

/** NET-03: "Offline. Retrying." and "Signed out or not allowed." with "Sign in again". */
@Composable
fun NetBannerRow(banner: NetBanner?, onSignInAgain: () -> Unit, modifier: Modifier = Modifier) {
  if (banner == null) return
  val c = ZoTheme.colors
  Row(modifier.fillMaxWidth().background(c.card2).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
    Text(
      if (banner == NetBanner.Offline) "Offline. Retrying." else "Signed out or not allowed.",
      style = ZoTheme.type.cardMeta, color = c.text2, modifier = Modifier.weight(1f),
    )
    if (banner == NetBanner.SignedOut) SecondaryButton("Sign in again", onSignInAgain)
  }
}
