package com.interceptproxy.vpn

/** 将租约替换、start generation 与到期 stop claim 放在同一把进程内锁下。 */
internal class ControlLeaseCoordinator(timeoutMillis: Long) {
    private val watchdog = ControlLeaseWatchdog(timeoutMillis)

    @Synchronized
    fun configure(
        generation: Long,
        ownerEpoch: String,
        enabled: Boolean,
        nowMillis: Long,
    ) = watchdog.configure(generation, ownerEpoch, enabled, nowMillis)

    @Synchronized
    fun start(
        profileId: String,
        runtime: ProxyRuntimeFacts,
        ownerEpoch: String,
        enabled: Boolean,
        nowMillis: () -> Long,
        onRollbackConflict: (VpnRuntimeRegistry.StopRequest) -> Unit = {},
        enqueue: (Long) -> Unit,
    ): Result<Long> {
        val request = VpnRuntimeRegistry.beginStart(profileId, runtime)
        return runCatching {
            enqueue(request.generation)
            watchdog.configure(request.generation, ownerEpoch, enabled, nowMillis())
            request.generation
        }.onFailure {
            if (!VpnRuntimeRegistry.rollbackStart(request)) {
                watchdog.clear()
                VpnRuntimeRegistry.stopRequestedIfCurrent(request.generation)
                    ?.let(onRollbackConflict)
            }
        }
    }

    @Synchronized
    fun heartbeat(ownerEpoch: String, nowMillis: Long): Boolean =
        watchdog.renew(ownerEpoch, nowMillis)

    @Synchronized
    fun claimExpiredStop(nowMillis: Long): VpnRuntimeRegistry.StopRequest? {
        val generation = watchdog.expiredGeneration(nowMillis) ?: return null
        return VpnRuntimeRegistry.stopRequestedIfCurrent(generation)
    }

    @Synchronized
    fun clear() = watchdog.clear()
}
