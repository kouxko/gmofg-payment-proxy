package com.interceptproxy.vpn

import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.util.Log

internal object VpnTunFactory {
    private const val TAG = "InterceptVpnService"

    fun establish(
        builder: VpnService.Builder,
        profile: CompanionProfile,
    ): ParcelFileDescriptor? = runCatching {
        builder
            .setSession(profile.targetPackages.joinToString { it.packageName })
            .setMtu(profile.mtu)
            .addAddress("10.254.0.2", 32)
            .addAddress("fd00:6970:7670::2", 128)
            .addRoute("0.0.0.0", 0)
            .addRoute("::", 0)
            // DNS 只用于把查询送入 TUN。Rust/tun2proxy 在本机用 Fake-IP 回答，
            // 随后的原始域名通过 SOCKS5 和 adb reverse 交给桌面代理处理。
            .let(VpnLocalDns::addTo)

        for (target in profile.targetPackages) {
            builder.addAllowedApplication(target.packageName)
        }
        builder.establish()
    }.getOrElse { error ->
        Log.e(TAG, "建立 Android TUN 失败", error)
        null
    }
}
