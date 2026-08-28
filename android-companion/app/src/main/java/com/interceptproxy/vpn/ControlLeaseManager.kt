package com.interceptproxy.vpn

import android.content.Context
import android.os.SystemClock
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** 进程级租约调度器；不依赖 Activity 或当前页面生命周期。 */
internal object ControlLeaseManager {
    private const val TIMEOUT_MILLIS = 5_000L
    private const val CHECK_INTERVAL_MILLIS = 100L
    private val coordinator = ControlLeaseCoordinator(TIMEOUT_MILLIS)
    private val executor = Executors.newSingleThreadScheduledExecutor { runnable ->
        Thread(runnable, "intercept-control-lease").apply { isDaemon = true }
    }
    @Volatile private var applicationContext: Context? = null
    @Volatile private var started = false

    @Synchronized
    fun configure(context: Context, generation: Long, ownerEpoch: String, enabled: Boolean) {
        ensureStarted(context)
        coordinator.configure(generation, ownerEpoch, enabled, SystemClock.elapsedRealtime())
    }

    fun start(
        context: Context,
        profileId: String,
        runtime: ProxyRuntimeFacts,
        ownerEpoch: String,
        enabled: Boolean,
        enqueue: (Long) -> Unit,
    ): Result<Long> {
        ensureStarted(context)
        return coordinator.start(
            profileId,
            runtime,
            ownerEpoch,
            enabled,
            SystemClock::elapsedRealtime,
            onRollbackConflict = { stopRequest ->
                InterceptVpnServiceControl.stopFromExpiredControlLease(context, stopRequest)
            },
            enqueue = enqueue,
        )
    }

    @Synchronized
    private fun ensureStarted(context: Context) {
        applicationContext = context.applicationContext
        if (!started) {
            started = true
            executor.scheduleAtFixedRate(
                ::checkExpiry,
                CHECK_INTERVAL_MILLIS,
                CHECK_INTERVAL_MILLIS,
                TimeUnit.MILLISECONDS,
            )
        }
    }

    fun configureUnmanagedRecovery(context: Context, generation: Long, enabled: Boolean) {
        configure(
            context,
            generation,
            "unmanaged-recovery-$generation-${java.util.UUID.randomUUID()}",
            enabled,
        )
    }

    fun heartbeat(ownerEpoch: String): Boolean =
        coordinator.heartbeat(ownerEpoch, SystemClock.elapsedRealtime())

    fun clear() = coordinator.clear()

    private fun checkExpiry() {
        val stopRequest = coordinator.claimExpiredStop(SystemClock.elapsedRealtime()) ?: return
        applicationContext?.let { context ->
            InterceptVpnServiceControl.stopFromExpiredControlLease(context, stopRequest)
        }
    }
}
