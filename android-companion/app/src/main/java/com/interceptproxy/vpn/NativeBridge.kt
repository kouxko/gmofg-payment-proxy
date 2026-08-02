package com.interceptproxy.vpn

import android.net.VpnService

/**
 * Rust cdylib 的唯一 Kotlin 入口。
 *
 * so 缺失、ABI 不匹配或 JNI 初始化失败时一律返回失败。VpnService 会在建立 TUN 前
 * 检查可用性，并在启动失败时立即关闭 TUN，以 fail-open 方式恢复系统原网络。
 */
object NativeBridge {
    private val libraryLoaded = runCatching {
        System.loadLibrary("intercept_proxy_android_engine")
    }.isSuccess

    fun isDataPlaneAvailable(): Boolean = libraryLoaded && nativeIsDataPlaneAvailable()

    fun validateProfile(profileJson: String, inventoryJson: String): String =
        if (libraryLoaded) nativeValidateProfile(profileJson, inventoryJson)
        else "Rust 数据面库未安装或 ABI 不匹配"

    fun start(
        tunFd: Int,
        profileJson: String,
        inventoryJson: String,
        protector: NativeSocketProtector,
    ): Boolean = libraryLoaded && nativeStart(tunFd, profileJson, inventoryJson, protector)

    fun stop() {
        if (libraryLoaded) nativeStop()
    }

    /** 只返回包数、字节数和连接阶段计数，不包含任何应用 Payload。 */
    fun statsJson(): String = if (libraryLoaded) nativeStatsJson() else "{}"

    private external fun nativeValidateProfile(profileJson: String, inventoryJson: String): String
    private external fun nativeStart(
        tunFd: Int,
        profileJson: String,
        inventoryJson: String,
        socketProtector: NativeSocketProtector,
    ): Boolean
    private external fun nativeStop()
    private external fun nativeIsDataPlaneAvailable(): Boolean
    private external fun nativeStatsJson(): String
}

/** Rust 打开的 TCP/UDP 转发 socket 必须通过此对象绕过 VPN，防止递归进入 TUN。 */
class NativeSocketProtector(private val service: VpnService) {
    @Suppress("unused") // 由 JNI 按方法名调用。
    fun protectSocket(fileDescriptor: Int): Boolean = service.protect(fileDescriptor)

    /** 原生线程异常退出时切回主线程关闭 TUN，保证目标应用 fail-open。 */
    @Suppress("unused") // 由 JNI 按方法名调用。
    fun onNativeFailure(message: String) {
        (service as? InterceptVpnService)?.onNativeDataPlaneFailure(message)
    }
}
