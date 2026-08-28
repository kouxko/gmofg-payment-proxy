package com.interceptproxy.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ControlLeaseWatchdogTest {
    @Test
    fun leaseDoesNotExpireBeforeFiveSecondsAndExpiresAtBoundary() {
        val watchdog = ControlLeaseWatchdog(timeoutMillis = 5_000)
        watchdog.activate(generation = 7, ownerEpoch = "epoch-7", nowMillis = 100)

        assertNull(watchdog.expiredGeneration(nowMillis = 5_099))
        assertEquals(7L, watchdog.expiredGeneration(nowMillis = 5_100))
        assertNull(watchdog.expiredGeneration(nowMillis = 6_000))
    }

    @Test
    fun disabledPolicyNeverArmsLease() {
        val watchdog = ControlLeaseWatchdog(timeoutMillis = 5_000)

        watchdog.configure(
            generation = 3,
            ownerEpoch = "epoch-3",
            enabled = false,
            nowMillis = 0,
        )

        assertNull(watchdog.expiredGeneration(nowMillis = 50_000))
    }

    @Test
    fun staleHeartbeatCannotRenewNewGeneration() {
        val watchdog = ControlLeaseWatchdog(timeoutMillis = 5_000)
        watchdog.activate(generation = 1, ownerEpoch = "epoch-1", nowMillis = 0)
        watchdog.activate(generation = 2, ownerEpoch = "epoch-2", nowMillis = 1_000)

        assertFalse(watchdog.renew("epoch-1", nowMillis = 4_000))
        assertTrue(watchdog.renew("epoch-2", nowMillis = 4_000))
        assertNull(watchdog.expiredGeneration(nowMillis = 8_999))
        assertEquals(2L, watchdog.expiredGeneration(nowMillis = 9_000))
    }

    @Test
    fun staleTimeoutCannotStopReplacementGeneration() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val stale = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val replacement = VpnRuntimeRegistry.startRequested("profile-2", runtime)

        assertNull(VpnRuntimeRegistry.stopRequestedIfCurrent(stale))
        assertTrue(VpnRuntimeRegistry.canStart(replacement))

        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun claimedOldLeaseStopCannotReleaseTunAfterInterleavedNewStart() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val oldStop = VpnRuntimeRegistry.stopRequestedIfCurrent(oldGeneration)!!
        val replacement = VpnRuntimeRegistry.startRequested("profile-2", runtime)
        var tunReleased = false

        assertFalse(
            VpnExternalStopCoordinator.completeActiveServiceStop(
                oldStop,
                "stale lease timeout",
            ) { tunReleased = true },
        )
        assertFalse(tunReleased)
        assertTrue(VpnRuntimeRegistry.canStart(replacement))

        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun claimedOldLeaseStopWithoutServiceCannotStopInterleavedNewGeneration() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        val oldStop = VpnRuntimeRegistry.stopRequestedIfCurrent(oldGeneration)!!
        val replacement = VpnRuntimeRegistry.startRequested("profile-2", runtime)
        var nativeDataPlaneStopped = false
        var queuedServiceStopped = false

        VpnExternalStopCoordinator.completeWithoutActiveService(
            oldStop,
            "stale lease timeout",
            stopNativeDataPlane = { nativeDataPlaneStopped = true },
            stopQueuedService = { queuedServiceStopped = true },
        )

        assertFalse(nativeDataPlaneStopped)
        assertFalse(queuedServiceStopped)
        assertTrue(VpnRuntimeRegistry.canStart(replacement))
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun replacementEnqueueFailureRestoresRunningGenerationAndOldLease() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val oldGeneration = VpnRuntimeRegistry.startRequested("profile-1", runtime)
        assertTrue(VpnRuntimeRegistry.running("profile-1", runtime, oldGeneration))
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        coordinator.configure(oldGeneration, "epoch-old", enabled = true, nowMillis = 0)

        val result = coordinator.start(
            profileId = "profile-2",
            runtime = runtime,
            ownerEpoch = "epoch-new",
            enabled = true,
            nowMillis = { 1_000 },
        ) { error("enqueue rejected") }

        assertTrue(result.isFailure)
        assertEquals(oldGeneration, VpnRuntimeRegistry.snapshot().generation)
        assertEquals("running", VpnRuntimeRegistry.snapshot().state)
        assertTrue(coordinator.heartbeat("epoch-old", nowMillis = 4_999))
        assertFalse(coordinator.heartbeat("epoch-new", nowMillis = 4_999))
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun successfulEnqueueArmsLeaseFromCompletionTime() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        var nowMillis = 1_000L

        val result = coordinator.start(
            profileId = "profile-2",
            runtime = runtime,
            ownerEpoch = "epoch-new",
            enabled = true,
            nowMillis = { nowMillis },
        ) {
            nowMillis = 6_000L
        }

        assertTrue(result.isSuccess)
        assertNull(coordinator.claimExpiredStop(nowMillis = 6_000))
        VpnRuntimeRegistry.resetForTest()
    }

    @Test
    fun enqueueFailureAfterNewGenerationPublishesClaimsGenerationAwareFailOpenStop() {
        VpnRuntimeRegistry.resetForTest()
        val runtime = ProxyRuntimeFacts("profile", "routes", 1)
        val coordinator = ControlLeaseCoordinator(timeoutMillis = 5_000)
        var claimedStop: VpnRuntimeRegistry.StopRequest? = null

        val result = coordinator.start(
            profileId = "profile-2",
            runtime = runtime,
            ownerEpoch = "epoch-new",
            enabled = true,
            nowMillis = { 1_000 },
            onRollbackConflict = { claimedStop = it },
        ) { generation ->
            assertTrue(VpnRuntimeRegistry.running("profile-2", runtime, generation))
            error("enqueue result became ambiguous")
        }

        assertTrue(result.isFailure)
        assertEquals(claimedStop?.generation, VpnRuntimeRegistry.snapshot().generation)
        assertEquals("stop_requested", VpnRuntimeRegistry.snapshot().state)
        VpnRuntimeRegistry.resetForTest()
    }
}
