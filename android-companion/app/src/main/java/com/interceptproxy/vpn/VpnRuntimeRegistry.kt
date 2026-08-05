package com.interceptproxy.vpn

/**
 * 当前进程内可验证的 VPN 状态与启动代次。
 *
 * 每次 start/stop 都推进 generation。排队中的旧 start Intent 即使在 stop 后才送达，
 * 也不能重新建立 TUN。
 */
object VpnRuntimeRegistry {
    data class StopRequest(
        val generation: Long,
        val requiresTeardownConfirmation: Boolean,
    )

    data class Snapshot(
        val state: String,
        val activeProfileId: String?,
        val activeProfileFingerprint: String?,
        val activeRouteFingerprint: String?,
        val activeRouteCount: Int,
        val message: String,
        val generation: Long,
    ) {
        val verified: Boolean
            get() = state == "running" || state == "stopped"
    }

    @Volatile
    private var snapshot = stoppedSnapshot(0, "VPN 当前未运行。")

    @Volatile
    private var desiredRunning = false

    @Synchronized
    fun snapshot(): Snapshot = snapshot

    @Synchronized
    fun startRequested(profileId: String, runtime: ProxyRuntimeFacts): Long {
        val generation = snapshot.generation + 1
        desiredRunning = true
        snapshot = Snapshot(
            "start_requested",
            profileId,
            runtime.profileFingerprint,
            runtime.routeFingerprint,
            runtime.routeCount,
            "已请求启动目标应用 VPN。",
            generation,
        )
        return generation
    }

    @Synchronized
    fun canStart(generation: Long): Boolean =
        desiredRunning && snapshot.generation == generation

    @Synchronized
    fun running(profileId: String, runtime: ProxyRuntimeFacts, generation: Long): Boolean {
        if (!canStart(generation)) return false
        snapshot = Snapshot(
            "running",
            profileId,
            runtime.profileFingerprint,
            runtime.routeFingerprint,
            runtime.routeCount,
            "目标应用 VPN 与 Rust 数据面正在运行。",
            generation,
        )
        return true
    }

    /** 返回本次 stop generation；只有 Service/TUN 清理完成后才能调用 [confirmStopped]。 */
    @Synchronized
    fun stopRequested(): StopRequest {
        val requiresConfirmation = snapshot.state == "start_requested" ||
            snapshot.state == "running" || snapshot.state == "stop_requested"
        val generation = snapshot.generation + 1
        desiredRunning = false
        snapshot = snapshot.copy(
            state = "stop_requested",
            message = "已请求停止 VPN，正在等待 TUN 释放。",
            generation = generation,
        )
        return StopRequest(generation, requiresConfirmation)
    }

    @Synchronized
    fun confirmStopped(generation: Long, message: String = "VPN 已停止，目标应用恢复系统网络。"): Boolean {
        if (desiredRunning || snapshot.generation != generation) return false
        snapshot = stoppedSnapshot(generation, message)
        return true
    }

    @Synchronized
    fun faulted(message: String) {
        desiredRunning = false
        snapshot = snapshot.copy(
            state = "faulted",
            message = message,
            generation = snapshot.generation + 1,
        )
    }

    @Synchronized
    fun faultedIfCurrent(generation: Long, message: String): Boolean {
        if (!desiredRunning || snapshot.generation != generation) return false
        faulted(message)
        return true
    }

    @Synchronized
    internal fun resetForTest() {
        desiredRunning = false
        snapshot = stoppedSnapshot(0, "VPN 当前未运行。")
    }

    private fun stoppedSnapshot(generation: Long, message: String) = Snapshot(
        "stopped",
        null,
        null,
        null,
        0,
        message,
        generation,
    )
}

/**
 * 完成“启动 Intent 已排队，但 Service 实例尚未创建”时的外部停止。
 *
 * stop generation 已经使排队中的 start 失效；在停止原生数据面并撤销系统中的
 * Service 启动请求后，不再等待一个可能永远不会创建的 Service 回调确认。
 */
internal object VpnExternalStopCoordinator {
    fun completeWithoutActiveService(
        stopRequest: VpnRuntimeRegistry.StopRequest,
        message: String,
        stopNativeDataPlane: () -> Unit,
        stopQueuedService: () -> Unit,
    ) {
        stopNativeDataPlane()
        stopQueuedService()
        VpnRuntimeRegistry.confirmStopped(stopRequest.generation, message)
    }
}
