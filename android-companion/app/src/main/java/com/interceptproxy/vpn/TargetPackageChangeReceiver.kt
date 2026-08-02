package com.interceptproxy.vpn

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** 目标 APK 升级、卸载或替换后强制重新校验签名、UID 与 shared UID 组。 */
class TargetPackageChangeReceiver(
    private val targetPackages: Set<String>,
) : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val changedPackage = intent.data?.schemeSpecificPart ?: return
        if (changedPackage !in targetPackages) return
        context.startService(
            Intent(context, InterceptVpnService::class.java)
                .setAction(InterceptVpnService.ACTION_REVALIDATE),
        )
    }
}
