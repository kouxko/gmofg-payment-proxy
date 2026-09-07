package com.interceptproxy.vpn

internal sealed class RecoveryAdmission {
    object WaitingForDesktopControl : RecoveryAdmission()
    data class Started(val generation: Long) : RecoveryAdmission()
}

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
    ): Result<Long> = enqueueStart(
        profileId = profileId,
        runtime = runtime,
        onRollbackConflict = onRollbackConflict,
        enqueue = enqueue,
    ) { generation ->
        watchdog.configure(generation, ownerEpoch, enabled, nowMillis())
        generation
    }

    @Synchronized
    fun recover(
        profileId: String,
        runtime: ProxyRuntimeFacts,
        stopVpnOnControlLoss: Boolean,
        onRollbackConflict: (VpnRuntimeRegistry.StopRequest) -> Unit = {},
        enqueue: (Long) -> Unit,
    ): Result<RecoveryAdmission> {
        if (stopVpnOnControlLoss) {
            return Result.success(RecoveryAdmission.WaitingForDesktopControl)
        }
        return enqueueStart(
            profileId = profileId,
            runtime = runtime,
            onRollbackConflict = onRollbackConflict,
            enqueue = enqueue,
        ) { generation ->
            watchdog.clear()
            RecoveryAdmission.Started(generation)
        }
    }

    private fun <T> enqueueStart(
        profileId: String,
        runtime: ProxyRuntimeFacts,
        onRollbackConflict: (VpnRuntimeRegistry.StopRequest) -> Unit,
        enqueue: (Long) -> Unit,
        afterEnqueue: (Long) -> T,
    ): Result<T> {
        val request = VpnRuntimeRegistry.beginStart(profileId, runtime)
        return runCatching {
            enqueue(request.generation)
            afterEnqueue(request.generation)
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
