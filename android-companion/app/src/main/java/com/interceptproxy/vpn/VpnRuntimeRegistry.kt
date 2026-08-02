package com.interceptproxy.vpn

/**
 * 当前进程内可验证的 VPN 状态。
 *
 * 控制 socket 只能声称它亲自观察到的状态；不能把“进程存在”伪装成“VPN 正在运行”。
 */
object VpnRuntimeRegistry {
    data class Snapshot(
        val state: String,
        val activeProfileId: String?,
        val message: String,
    )

    @Volatile
    private var snapshot = Snapshot("stopped", null, "VPN 当前未运行。")

    fun snapshot(): Snapshot = snapshot

    fun startRequested(profileId: String) {
        snapshot = Snapshot("start_requested", profileId, "已请求启动目标应用 VPN。")
    }

    fun running(profileId: String) {
        snapshot = Snapshot("running", profileId, "目标应用 VPN 与 Rust 数据面正在运行。")
    }

    fun stopRequested() {
        snapshot = snapshot.copy(state = "stop_requested", message = "已请求停止 VPN。")
    }

    fun stopped(message: String = "VPN 已停止，目标应用恢复系统网络。") {
        snapshot = Snapshot("stopped", null, message)
    }

    fun faulted(message: String) {
        snapshot = Snapshot("faulted", snapshot.activeProfileId, message)
    }
}
