package org.zohara.zoai.notification

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationManagerCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.firstOrNull
import kotlinx.coroutines.launch
import org.zohara.zoai.data.ApiClient
import org.zohara.zoai.data.ZoaiPreferences

class ApprovalActionReceiver : BroadcastReceiver() {
    companion object {
        const val ACTION_ALLOW = "org.zohara.zoai.ACTION_ALLOW"
        const val ACTION_DENY = "org.zohara.zoai.ACTION_DENY"
        const val EXTRA_STREAM_ID = "stream_id"
        const val EXTRA_APPROVAL_ID = "approval_id"
        const val EXTRA_NOTIFICATION_ID = "notif_id"
    }

    override fun onReceive(context: Context, intent: Intent) {
        val streamId = intent.getLongExtra(EXTRA_STREAM_ID, 0L)
        val approvalId = intent.getLongExtra(EXTRA_APPROVAL_ID, 0L)
        val notifId = intent.getIntExtra(EXTRA_NOTIFICATION_ID, 0)
        val allow = intent.action == ACTION_ALLOW

        if (notifId != 0) {
            NotificationManagerCompat.from(context).cancel(notifId)
        }

        if (streamId == 0L || approvalId == 0L) return

        val prefs = ZoaiPreferences(context)
        CoroutineScope(Dispatchers.IO).launch {
            val server = prefs.serverUrl.firstOrNull() ?: ZoaiPreferences.DEFAULT_SERVER
            val token = prefs.authToken.firstOrNull()
            val api = ApiClient(server) { token }
            api.replyApproval(streamId, approvalId, allow)
        }
    }
}
