package com.interceptproxy.vpn

/**
 * 集中声明 VpnService 的资源释放顺序，避免重构时把 fail-open 变成先等 JNI 再撤销路由。
 */
internal object VpnServiceResourceOrder {
    fun startService(
        attachExternalControl: () -> Unit,
        startForeground: () -> Unit,
    ) {
        attachExternalControl()
        startForeground()
    }

    inline fun revokeService(
        stopVpn: () -> Unit,
        revokeSystemPermission: () -> Unit,
    ) {
        stopVpn()
        revokeSystemPermission()
    }

    fun closeCurrentDataPlane(
        releaseTun: () -> Unit,
        clearTunConfiguration: () -> Unit,
        clearActiveGeneration: () -> Unit,
        stopNativeDataPlane: () -> Unit,
    ) {
        releaseTun()
        clearTunConfiguration()
        clearActiveGeneration()
        stopNativeDataPlane()
    }

    fun destroyService(
        unregisterPackageReceiver: () -> Unit,
        releaseTun: () -> Unit,
        stopNativeDataPlane: () -> Unit,
        clearActiveGeneration: () -> Unit,
    ) {
        unregisterPackageReceiver()
        releaseTun()
        stopNativeDataPlane()
        clearActiveGeneration()
    }

    fun finishStopping(
        stopForeground: () -> Unit,
        stopService: () -> Unit,
    ) {
        stopForeground()
        stopService()
    }
}
