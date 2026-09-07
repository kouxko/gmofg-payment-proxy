package com.interceptproxy.vpn

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.os.Build

internal object VpnForegroundNotification {
    private const val CHANNEL_ID = "intercept_proxy_vpn"
    private const val NOTIFICATION_ID = 41001

    fun start(service: InterceptVpnService) {
        createChannel(service)
        service.startForeground(NOTIFICATION_ID, create(service))
    }

    private fun createChannel(service: InterceptVpnService) {
        if (!AndroidPlatformCompatibility.usesApi26ForegroundContract(Build.VERSION.SDK_INT)) return
        createChannelApi26(service)
    }

    @SuppressLint("NewApi")
    private fun createChannelApi26(service: InterceptVpnService) {
        val manager = service.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                service.getString(R.string.vpn_notification_channel),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    @Suppress("DEPRECATION")
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
        val builder = if (AndroidPlatformCompatibility.usesApi26ForegroundContract(Build.VERSION.SDK_INT)) {
            createBuilderApi26(service)
        } else {
            Notification.Builder(service).setPriority(Notification.PRIORITY_LOW)
        }
        return builder
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setContentTitle(service.getString(R.string.vpn_notification_title))
            .setContentText(service.getString(R.string.vpn_notification_text))
            .setContentIntent(openIntent)
            .setOngoing(true)
            .addAction(stopAction)
            .build()
    }

    @SuppressLint("NewApi")
    private fun createBuilderApi26(service: InterceptVpnService): Notification.Builder =
        Notification.Builder(service, CHANNEL_ID)
}
