package com.interceptproxy.vpn

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.os.Build

internal object AndroidPlatformCompatibility {
    fun usesApi26ForegroundContract(sdkInt: Int): Boolean =
        sdkInt >= Build.VERSION_CODES.O

    fun startVpnService(context: Context, intent: Intent) {
        if (usesApi26ForegroundContract(Build.VERSION.SDK_INT)) {
            startForegroundServiceApi26(context, intent)
        } else {
            context.startService(intent)
        }
    }

    @SuppressLint("NewApi")
    private fun startForegroundServiceApi26(context: Context, intent: Intent) {
        context.startForegroundService(intent)
    }
}
