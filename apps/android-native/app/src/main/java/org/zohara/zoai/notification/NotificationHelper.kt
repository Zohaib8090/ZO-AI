package org.zohara.zoai.notification

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import org.zohara.zoai.MainActivity
import org.zohara.zoai.R

object NotificationHelper {
    const val CHANNEL_NEEDS_YOU = "needs_you"
    const val CHANNEL_ANSWERS = "answers"

    fun createChannels(context: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager

            val needsYouChannel = NotificationChannel(
                CHANNEL_NEEDS_YOU,
                context.getString(R.string.channel_needs_you),
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = context.getString(R.string.channel_needs_you_desc)
                enableVibration(true)
            }

            val answersChannel = NotificationChannel(
                CHANNEL_ANSWERS,
                context.getString(R.string.channel_answers),
                NotificationManager.IMPORTANCE_DEFAULT
            ).apply {
                description = context.getString(R.string.channel_answers_desc)
            }

            manager.createNotificationChannel(needsYouChannel)
            manager.createNotificationChannel(answersChannel)
        }
    }

    fun showApprovalNotification(
        context: Context,
        notificationId: Int,
        streamId: Long,
        approvalId: Long,
        summary: String
    ) {
        val tapIntent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val tapPending = PendingIntent.getActivity(
            context,
            notificationId,
            tapIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Allow action
        val allowIntent = Intent(context, ApprovalActionReceiver::class.java).apply {
            action = ApprovalActionReceiver.ACTION_ALLOW
            putExtra(ApprovalActionReceiver.EXTRA_STREAM_ID, streamId)
            putExtra(ApprovalActionReceiver.EXTRA_APPROVAL_ID, approvalId)
            putExtra(ApprovalActionReceiver.EXTRA_NOTIFICATION_ID, notificationId)
        }
        val allowPending = PendingIntent.getBroadcast(
            context,
            notificationId * 10 + 1,
            allowIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Deny action
        val denyIntent = Intent(context, ApprovalActionReceiver::class.java).apply {
            action = ApprovalActionReceiver.ACTION_DENY
            putExtra(ApprovalActionReceiver.EXTRA_STREAM_ID, streamId)
            putExtra(ApprovalActionReceiver.EXTRA_APPROVAL_ID, approvalId)
            putExtra(ApprovalActionReceiver.EXTRA_NOTIFICATION_ID, notificationId)
        }
        val denyPending = PendingIntent.getBroadcast(
            context,
            notificationId * 10 + 2,
            denyIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notif = NotificationCompat.Builder(context, CHANNEL_NEEDS_YOU)
            .setSmallIcon(R.drawable.ic_stat_zoai)
            .setContentTitle("ZO-AI needs your approval")
            .setContentText(summary.ifEmpty { "A command needs your decision." })
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setContentIntent(tapPending)
            .setAutoCancel(true)
            .addAction(0, "Allow", allowPending)
            .addAction(0, "Deny", denyPending)
            .build()

        NotificationManagerCompat.from(context).notify(notificationId, notif)
    }

    fun showQuestionNotification(
        context: Context,
        notificationId: Int,
        question: String
    ) {
        val tapIntent = Intent(context, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val tapPending = PendingIntent.getActivity(
            context,
            notificationId,
            tapIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val notif = NotificationCompat.Builder(context, CHANNEL_NEEDS_YOU)
            .setSmallIcon(R.drawable.ic_stat_zoai)
            .setContentTitle("ZO-AI needs your answer")
            .setContentText(question.ifEmpty { "ZO-AI is asking you a question." })
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setContentIntent(tapPending)
            .setAutoCancel(true)
            .addAction(0, "Answer", tapPending)
            .build()

        NotificationManagerCompat.from(context).notify(notificationId, notif)
    }
}
