package com.interceptproxy.vpn

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build

internal class TargetPackageReceiverRegistration(
    private val service: InterceptVpnService,
) {
    private var receiver: TargetPackageChangeReceiver? = null

    fun register(targetPackages: Set<String>) {
        unregister()
        receiver = TargetPackageChangeReceiver(targetPackages).also { packageReceiver ->
            val filter = IntentFilter().apply {
                addAction(Intent.ACTION_PACKAGE_ADDED)
                addAction(Intent.ACTION_PACKAGE_REMOVED)
                addAction(Intent.ACTION_PACKAGE_REPLACED)
                addDataScheme("package")
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                service.registerReceiver(packageReceiver, filter, Context.RECEIVER_EXPORTED)
            } else {
                @Suppress("DEPRECATION")
                service.registerReceiver(packageReceiver, filter)
            }
        }
    }

    fun unregister() {
        receiver?.let { packageReceiver ->
            runCatching { service.unregisterReceiver(packageReceiver) }
        }
        receiver = null
    }
}
