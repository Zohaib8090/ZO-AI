package org.zohara.zoai.core.design

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// ---------------------------------------------------------------- icons

@Composable
fun ZoIcon(icon: ImageVector, modifier: Modifier = Modifier, tint: Color = ZoTheme.colors.icon, size: Dp = 20.dp, contentDescription: String? = null) {
  Icon(icon, contentDescription, modifier.size(size), tint = tint)
}

/** Round, 34 dp icon button with the hover colour as pressed state (the `.icon-btn` of the stylesheet). */
@Composable
fun ZoIconButton(
  icon: ImageVector,
  contentDescription: String,
  onClick: () -> Unit,
  modifier: Modifier = Modifier,
  tint: Color = ZoTheme.colors.icon,
  size: Dp = 40.dp,
  iconSize: Dp = 20.dp,
  enabled: Boolean = true,
  background: Color = Color.Transparent,
) {
  Box(
    modifier
      .size(size)
      .clip(ZoTheme.shapes.button)
      .background(background)
      .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
      .semantics { this.contentDescription = contentDescription }
      .alpha(if (enabled) 1f else 0.4f),
    contentAlignment = Alignment.Center,
  ) { Icon(icon, null, Modifier.size(iconSize), tint = tint) }
}

// ---------------------------------------------------------------- buttons

@Composable
private fun ButtonBase(
  text: String,
  onClick: () -> Unit,
  modifier: Modifier,
  container: Color,
  content: Color,
  border: Color?,
  enabled: Boolean,
  loading: Boolean,
  icon: ImageVector?,
  shape: androidx.compose.ui.graphics.Shape,
  weight600: Boolean,
) {
  val t = ZoTheme.type
  Row(
    modifier
      .heightIn(min = 40.dp)
      .clip(shape)
      .background(container, shape)
      .then(if (border != null) Modifier.border(1.dp, border, shape) else Modifier)
      .clickable(enabled = enabled && !loading, role = Role.Button, onClick = onClick)
      .alpha(if (enabled) 1f else 0.45f)
      .padding(horizontal = 16.dp, vertical = 8.dp),
    horizontalArrangement = Arrangement.Center,
    verticalAlignment = Alignment.CenterVertically,
  ) {
    if (loading) {
      ZoSpinner(size = 16.dp, color = content)
      Spacer(Modifier.width(8.dp))
    } else if (icon != null) {
      Icon(icon, null, Modifier.size(16.dp), tint = content)
      Spacer(Modifier.width(8.dp))
    }
    Text(text, style = t.button.let { if (weight600) it.copy(fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold) else it }, color = content, maxLines = 1, overflow = TextOverflow.Ellipsis)
  }
}

/** The filled button: light on dark, dark on light (`.page-btn.primary`, --btn / --btnfg). */
@Composable
fun PrimaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, loading: Boolean = false, icon: ImageVector? = null) {
  val c = ZoTheme.colors
  ButtonBase(text, onClick, modifier, c.btn, c.btnFg, null, enabled, loading, icon, ZoTheme.shapes.pill, true)
}

/** The outlined pill (`.page-btn`). */
@Composable
fun SecondaryButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, loading: Boolean = false, icon: ImageVector? = null) {
  val c = ZoTheme.colors
  ButtonBase(text, onClick, modifier, Color.Transparent, c.text, c.line12, enabled, loading, icon, ZoTheme.shapes.pill, false)
}

/** Same pill as [SecondaryButton]; the name the matrix uses for small actions beside a row. */
@Composable
fun PillButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, accent: Boolean = false, icon: ImageVector? = null) {
  val c = ZoTheme.colors
  ButtonBase(text, onClick, modifier.defaultMinSize(minHeight = 34.dp), Color.Transparent, if (accent) c.accent else c.text, if (accent) c.accent.copy(alpha = 0.5f) else c.line12, enabled, false, icon, ZoTheme.shapes.pill, false)
}

/** Destructive action: red fill (#E5484D like the recording mic) with white text. */
@Composable
fun DangerButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, loading: Boolean = false, outlined: Boolean = false) {
  val c = ZoTheme.colors
  if (outlined) ButtonBase(text, onClick, modifier, Color.Transparent, c.danger, c.danger.copy(alpha = 0.5f), enabled, loading, null, ZoTheme.shapes.pill, false)
  else ButtonBase(text, onClick, modifier, c.mic, Color.White, null, enabled, loading, null, ZoTheme.shapes.pill, true)
}

// ---------------------------------------------------------------- switch, spinner

@Composable
fun ZoSwitch(checked: Boolean, onCheckedChange: ((Boolean) -> Unit)?, modifier: Modifier = Modifier, enabled: Boolean = true) {
  val c = ZoTheme.colors
  val track by animateColorAsState(if (checked) c.accent else c.raise, tween(120), label = "track")
  val knob by animateColorAsState(if (checked) c.onAccent else c.text3, tween(120), label = "knob")
  val x by animateDpAsState(if (checked) 20.dp else 2.dp, tween(120), label = "x")
  Box(
    modifier
      .size(width = 42.dp, height = 24.dp)
      .clip(CircleShape)
      .background(track)
      .then(if (!checked) Modifier.border(1.dp, c.line12, CircleShape) else Modifier)
      .semantics { role = Role.Switch; stateDescription = if (checked) "On" else "Off" }
      .then(if (onCheckedChange != null) Modifier.clickable(enabled = enabled, role = Role.Switch) { onCheckedChange(!checked) } else Modifier)
      .alpha(if (enabled) 1f else 0.45f),
    contentAlignment = Alignment.CenterStart,
  ) {
    Box(Modifier.offset(x = x).size(20.dp).clip(CircleShape).background(knob))
  }
}

@Composable
fun ZoSpinner(modifier: Modifier = Modifier, size: Dp = 20.dp, color: Color = ZoTheme.colors.accent) {
  CircularProgressIndicator(modifier.size(size), color = color, strokeWidth = 2.dp, trackColor = Color.Transparent)
}

// ---------------------------------------------------------------- cards and rows

/** `.settings-card`: card colour, 1 dp border at 6 percent, radius 12. */
@Composable
fun SettingsCard(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
  val c = ZoTheme.colors
  Column(
    modifier
      .fillMaxWidth()
      .clip(ZoTheme.shapes.card)
      .background(c.card)
      .border(1.dp, c.line, ZoTheme.shapes.card),
    content = content,
  )
}

/** A plain card for lists (project cards, search hits). */
@Composable
fun ZoCard(modifier: Modifier = Modifier, onClick: (() -> Unit)? = null, content: @Composable ColumnScope.() -> Unit) {
  val c = ZoTheme.colors
  Column(
    modifier
      .fillMaxWidth()
      .clip(ZoTheme.shapes.card)
      .background(c.card)
      .border(1.dp, c.line, ZoTheme.shapes.card)
      .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier),
    content = content,
  )
}

/** One row of a settings card: title and optional subtitle at the start, a control (switch, button, value, chevron) at the end. */
@Composable
fun SettingsRow(
  title: String,
  modifier: Modifier = Modifier,
  subtitle: String? = null,
  onClick: (() -> Unit)? = null,
  divider: Boolean = true,
  titleColor: Color = ZoTheme.colors.text,
  leading: (@Composable () -> Unit)? = null,
  control: (@Composable RowScope.() -> Unit)? = null,
) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  Column(modifier) {
    Row(
      Modifier
        .fillMaxWidth()
        .then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
        .padding(horizontal = 16.dp, vertical = 12.dp)
        .heightIn(min = 28.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      if (leading != null) {
        leading()
        Spacer(Modifier.width(12.dp))
      }
      Column(Modifier.weight(1f)) {
        Text(title, style = t.cardName, color = titleColor)
        if (!subtitle.isNullOrBlank()) Text(subtitle, style = t.cardMeta, color = c.dim, modifier = Modifier.padding(top = 2.dp))
      }
      if (control != null) {
        Spacer(Modifier.width(12.dp))
        control()
      }
    }
    if (divider) ZoDivider(Modifier.padding(start = 16.dp))
  }
}

/** Small uppercase-free section label above a card (`.section-title`). */
@Composable
fun SectionHeader(text: String, modifier: Modifier = Modifier) {
  Text(text, style = ZoTheme.type.sectionHeader, color = ZoTheme.colors.dimmer, modifier = modifier.padding(start = 6.dp, end = 6.dp, top = 18.dp, bottom = 8.dp))
}

// ---------------------------------------------------------------- text field

/** The `.field-input` look: input colour, 1 dp border (focus: the stronger border), radius 10, 13 sp text. */
@Composable
fun ZoTextField(
  value: String,
  onValueChange: (String) -> Unit,
  modifier: Modifier = Modifier,
  label: String? = null,
  hint: String? = null,
  placeholder: String = "",
  password: Boolean = false,
  singleLine: Boolean = true,
  minLines: Int = 1,
  enabled: Boolean = true,
  isError: Boolean = false,
  keyboardType: KeyboardType = KeyboardType.Text,
  keyboardOptions: KeyboardOptions? = null,
  trailing: (@Composable () -> Unit)? = null,
  mono: Boolean = false,
) {
  val c = ZoTheme.colors
  val t = ZoTheme.type
  val src = remember { MutableInteractionSource() }
  val focused by src.collectIsFocusedAsState()
  var shown by remember { mutableStateOf(false) }
  Column(modifier) {
    if (label != null) Text(label, style = t.fieldLabel, color = c.text, modifier = Modifier.padding(bottom = 6.dp))
    val textStyle = (if (mono) t.code else t.body.copy(fontSize = 14.sp)).copy(color = c.text)
    Row(
      Modifier
        .fillMaxWidth()
        .clip(ZoTheme.shapes.field)
        .background(c.input)
        .border(1.dp, if (isError) c.danger else if (focused) c.borderFocus else c.border, ZoTheme.shapes.field)
        .padding(horizontal = 12.dp, vertical = 11.dp),
      verticalAlignment = Alignment.CenterVertically,
    ) {
      Box(Modifier.weight(1f)) {
        BasicTextField(
          value = value,
          onValueChange = onValueChange,
          enabled = enabled,
          singleLine = singleLine,
          minLines = minLines,
          textStyle = textStyle,
          cursorBrush = SolidColor(c.accent),
          interactionSource = src,
          visualTransformation = if (password && !shown) PasswordVisualTransformation() else VisualTransformation.None,
          keyboardOptions = keyboardOptions ?: KeyboardOptions(keyboardType = if (password) KeyboardType.Password else keyboardType),
          modifier = Modifier.fillMaxWidth(),
        )
        if (value.isEmpty() && placeholder.isNotEmpty()) Text(placeholder, style = textStyle, color = c.dimmer)
      }
      if (password) {
        ZoIconButton(
          if (shown) ZoIcons.EyeOff else ZoIcons.Eye, if (shown) "Hide password" else "Show password", { shown = !shown },
          size = 28.dp, iconSize = 18.dp,
        )
      }
      trailing?.invoke()
    }
    if (hint != null) Text(hint, style = t.fieldHint, color = if (isError) c.danger else c.dimmer, modifier = Modifier.padding(top = 5.dp))
  }
}

// ---------------------------------------------------------------- sheets

/** The content of a bottom sheet (`.sheet-head`, `.sheet-body`, `.sheet-foot`): serif title with a close button, a body, an optional footer. */
@Composable
fun SheetContent(title: String, onClose: (() -> Unit)?, modifier: Modifier = Modifier, footer: (@Composable RowScope.() -> Unit)? = null, body: @Composable ColumnScope.() -> Unit) {
  val c = ZoTheme.colors
  Column(modifier.fillMaxWidth().navigationBarsPadding()) {
    Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 10.dp, top = 6.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
      Text(title, style = ZoTheme.type.sheetTitle, color = c.text, modifier = Modifier.weight(1f))
      if (onClose != null) ZoIconButton(ZoIcons.Close, "Close", onClose)
    }
    Column(Modifier.padding(horizontal = 18.dp, vertical = 6.dp), content = body)
    if (footer != null) Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 18.dp, top = 14.dp, bottom = 18.dp), horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.End), content = footer)
    else Spacer(Modifier.size(12.dp))
  }
}

/** A modal bottom sheet in the app's look: sheet colour, top radius 18, scrim 55 percent (dark) or 35 percent (light). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ZoBottomSheet(
  title: String,
  onDismiss: () -> Unit,
  footer: (@Composable RowScope.() -> Unit)? = null,
  skipPartiallyExpanded: Boolean = true,
  body: @Composable ColumnScope.() -> Unit,
) {
  val c = ZoTheme.colors
  ModalBottomSheet(
    onDismissRequest = onDismiss,
    sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = skipPartiallyExpanded),
    containerColor = c.sheet,
    contentColor = c.text,
    scrimColor = c.scrim,
    shape = ZoTheme.shapes.sheet,
    dragHandle = {
      Box(Modifier.padding(top = 10.dp, bottom = 4.dp).size(width = 36.dp, height = 4.dp).clip(CircleShape).background(c.line12))
    },
  ) { SheetContent(title, onDismiss, footer = footer, body = body) }
}

// ---------------------------------------------------------------- toast

/** One toast per distinct text, shown for 2.6 s (NET-11). Put [ToastHost] once near the root and call [ToastState.show] from anywhere. */
@Stable
class ToastState {
  val host = SnackbarHostState()
  private var last: String = ""
  private var lastAt = 0L

  suspend fun show(text: String, nowMs: Long = System.currentTimeMillis()) {
    if (text.isBlank()) return
    if (text == last && nowMs - lastAt < 2600) return
    last = text
    lastAt = nowMs
    host.currentSnackbarData?.dismiss()
    host.showSnackbar(text, duration = SnackbarDuration.Indefinite)
  }
}

@Composable
fun rememberToastState() = remember { ToastState() }

@Composable
fun ToastHost(state: ToastState, modifier: Modifier = Modifier) {
  val c = ZoTheme.colors
  val data = state.host.currentSnackbarData
  LaunchedEffect(data) {
    if (data != null) {
      delay(2600)
      data.dismiss()
    }
  }
  SnackbarHost(state.host, modifier.navigationBarsPadding()) { d ->
    Box(Modifier.padding(16.dp).clip(ZoTheme.shapes.popover).background(c.popover).border(1.dp, c.line12, ZoTheme.shapes.popover).padding(horizontal = 16.dp, vertical = 12.dp)) {
      Text(d.visuals.message, style = ZoTheme.type.body, color = c.text)
    }
  }
}

/** Handy for screens: `val toast = rememberToast()` gives a lambda that shows a toast. */
@Composable
fun rememberToast(state: ToastState): (String) -> Unit {
  val scope = rememberCoroutineScope()
  return remember(state, scope) { { text -> scope.launch { state.show(text) } } }
}

// ---------------------------------------------------------------- empty state, avatar, chips

/** A calm empty or error state: an optional icon, a title and a sentence (`.sheet-empty`). */
@Composable
fun EmptyBox(title: String, modifier: Modifier = Modifier, subtitle: String? = null, icon: ImageVector? = null, action: (@Composable () -> Unit)? = null) {
  val c = ZoTheme.colors
  Column(modifier.fillMaxWidth().padding(horizontal = 24.dp, vertical = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
    if (icon != null) {
      Icon(icon, null, Modifier.size(28.dp), tint = c.dimmer)
      Spacer(Modifier.size(12.dp))
    }
    Text(title, style = ZoTheme.type.popTitle, color = c.text2)
    if (subtitle != null) Text(subtitle, style = ZoTheme.type.cardMeta, color = c.dim, modifier = Modifier.padding(top = 4.dp), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
    if (action != null) {
      Spacer(Modifier.size(14.dp))
      action()
    }
  }
}

/** The round avatar with the person's initial, and an optional count badge for running items (NAV-15: 9+ above nine). */
@Composable
fun Avatar(initial: String, modifier: Modifier = Modifier, size: Dp = 30.dp, badge: Int = 0) {
  val c = ZoTheme.colors
  Box(modifier.size(size)) {
    Box(Modifier.size(size).clip(CircleShape).background(c.accent), contentAlignment = Alignment.Center) {
      Text(initial.take(1).uppercase(), style = ZoTheme.type.body.copy(fontSize = (size.value * 0.46f).sp, fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold), color = c.onAccent)
    }
    if (badge > 0) {
      Box(
        Modifier.align(Alignment.TopEnd).offset(x = 4.dp, y = (-4).dp).defaultMinSize(minWidth = 16.dp, minHeight = 16.dp).clip(CircleShape).background(c.btn).padding(horizontal = 4.dp)
          .semantics { contentDescription = if (badge == 1) "1 thing is running" else "$badge things are running" },
        contentAlignment = Alignment.Center,
      ) { Text(if (badge > 9) "9+" else badge.toString(), style = ZoTheme.type.fieldHint.copy(fontSize = 10.sp, fontWeight = androidx.compose.ui.text.font.FontWeight.Bold), color = c.btnFg) }
    }
  }
}

/** A small rounded tag (`.chip`): neutral, ok (green) or accent. */
@Composable
fun ZoChip(text: String, modifier: Modifier = Modifier, color: Color = ZoTheme.colors.dim) {
  Text(text, style = ZoTheme.type.fieldHint.copy(fontSize = 11.sp), color = color,
    modifier = modifier.clip(ZoTheme.shapes.pill).border(BorderStroke(1.dp, color.copy(alpha = 0.4f)), ZoTheme.shapes.pill).padding(horizontal = 8.dp, vertical = 2.dp))
}

/** A thin 1 dp divider line. */
@Composable
fun ZoDivider(modifier: Modifier = Modifier) {
  Box(modifier.fillMaxWidth().height(1.dp).background(ZoTheme.colors.line))
}

/** The red error line (login, forms). */
@Composable
fun ErrorLine(text: String, modifier: Modifier = Modifier) {
  Text(text, style = ZoTheme.type.cardMeta, color = ZoTheme.colors.danger, modifier = modifier)
}
