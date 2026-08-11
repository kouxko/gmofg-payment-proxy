package com.interceptproxy.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent

internal object VpnForegroundNotification {
    private const val CHANNEL_ID = "intercept_proxy_vpn"
    private const val NOTIFICATION_ID = 41001

    fun start(service: InterceptVpnService) {
        createChannel(service)
        service.startForeground(NOTIFICATION_ID, create(service))
    }

    private fun createChannel(service: InterceptVpnService) {
        val manager = service.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                service.getString(R.string.vpn_notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    private fun create(service: InterceptVpnService): Notification {
        val stopIntent = PendingIntent.getService(
            service,
            0,
            InterceptVpnService.stopIntent(service),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val openIntent = PendingIntent.getActivity(
            service,
            1,
            Intent(service, VpnConsentActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val stopAction = Notification.Action.Builder(
            null,
            service.getString(R.string.vpn_stop),
            stopIntent,
        ).build()
        return Notification.Builder(service, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setContentTitle(service.getString(R.string.vpn_notification_title))
            .setContentText(service.getString(R.string.vpn_notification_text))
            .setContentIntent(openIntent)
            .setOngoing(true)
            .addAction(stopAction)
            .build()
    }
}
