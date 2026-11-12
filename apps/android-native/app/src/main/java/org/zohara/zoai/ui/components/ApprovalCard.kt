package org.zohara.zoai.ui.components

import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.zohara.zoai.theme.*

@Composable
fun ApprovalCard(
    tool: String,
    summary: String,
    onAllow: () -> Unit,
    onDeny: () -> Unit,
    modifier: Modifier = Modifier
) {
    var resolvedResult by remember { mutableStateOf<Boolean?>(null) }

    Card(
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = CardDark),
        modifier = modifier
            .fillMaxWidth()
            .border(1.dp, Line2Dark, RoundedCornerShape(12.dp))
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            if (resolvedResult != null) {
                val ok = resolvedResult == true
                Text(
                    text = "${if (ok) "Allowed" else "Denied"}: ${summary.ifEmpty { tool }}",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (ok) AccentGreen else AccentRed
                )
            } else {
                Text(
                    text = "ZO-AI wants to: ${summary.ifEmpty { tool }}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextDark
                )
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Button(
                        onClick = {
                            resolvedResult = true
                            onAllow()
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = BgDark)
                    ) {
                        Text("Allow")
                    }
                    OutlinedButton(
                        onClick = {
                            resolvedResult = false
                            onDeny()
                        },
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = TextDark)
                    ) {
                        Text("Deny")
                    }
                }
            }
        }
    }
}
