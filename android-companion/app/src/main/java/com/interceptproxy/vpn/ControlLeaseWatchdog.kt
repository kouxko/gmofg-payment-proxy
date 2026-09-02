package com.interceptproxy.vpn

/** 可控时钟驱动的 generation-aware 控制租约状态机。 */
internal class ControlLeaseWatchdog(private val timeoutMillis: Long) {
    private data class Lease(
        val generation: Long,
        val ownerEpoch: String,
        val deadlineMillis: Long,
    )

    private var lease: Lease? = null

    @Synchronized
    fun configure(generation: Long, ownerEpoch: String, enabled: Boolean, nowMillis: Long) {
        lease = if (enabled) {
            Lease(generation, ownerEpoch, nowMillis + timeoutMillis)
        } else {
            null
        }
    }

    fun activate(generation: Long, ownerEpoch: String, nowMillis: Long) =
        configure(generation, ownerEpoch, enabled = true, nowMillis)

    @Synchronized
    fun renew(ownerEpoch: String, nowMillis: Long): Boolean {
        val current = lease ?: return false
        if (current.ownerEpoch != ownerEpoch || nowMillis >= current.deadlineMillis) return false
        lease = current.copy(deadlineMillis = nowMillis + timeoutMillis)
        return true
    }

    @Synchronized
    fun expiredGeneration(nowMillis: Long): Long? {
        val current = lease ?: return null
        if (nowMillis < current.deadlineMillis) return null
        lease = null
        return current.generation
    }

    @Synchronized
    fun clear() {
        lease = null
    }
}
